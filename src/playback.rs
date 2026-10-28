//! MP4 playback through the Windows media player.
//!
//! The player owns decode, the clock, and the speakers. Each presented frame is
//! copied out of a D3D11 surface into RGBA so the existing textured quad can
//! zoom and pan it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use windows::core::Interface;
use winit::event_loop::EventLoopProxy;

use crate::decode::{DecodedImage, UserEvent};

const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;

pub struct VideoSession {
    path: PathBuf,
    generation: u64,
    player: windows::Media::Playback::MediaPlayer,
    /// Kept alive for as long as the player is using it.
    _source: windows::Media::Core::MediaSource,
    shared: Arc<Shared>,
    frame_token: i64,
    opened_token: i64,
    failed_token: i64,
    ended_token: i64,
    resume_after_overlay: bool,
    ended: bool,
}

struct Shared {
    alive: AtomicBool,
    generation: u64,
    max_edge: u32,
    frames: Mutex<Option<DecodedImage>>,
    copier: Mutex<Copier>,
    proxy: EventLoopProxy<UserEvent>,
    failed: AtomicBool,
}

struct Copier {
    device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    target: Option<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>,
    staging: Option<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>,
    surface: Option<windows::Graphics::DirectX::Direct3D11::IDirect3DSurface>,
    size: Option<(u32, u32)>,
}

// Direct3D pointers are not marked Send. This device is created with
// multithread protection, and every use takes the copier mutex: the frame
// callback, and drop on the UI thread after that callback has left the lock.
unsafe impl Send for Copier {}

pub fn file_uri(path: &Path) -> Result<String, String> {
    let absolute =
        std::fs::canonicalize(path).map_err(|err| format!("Couldn't open this video. {err}"))?;
    let raw = absolute.to_string_lossy();
    let (prefix, body) = if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        ("file://", rest)
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        ("file:///", rest)
    } else if let Some(rest) = raw.strip_prefix(r"\\") {
        ("file://", rest)
    } else {
        ("file:///", raw.as_ref())
    };
    let mut uri = String::from(prefix);
    for ch in body.chars() {
        match ch {
            '\\' => uri.push('/'),
            'A'..='Z' | 'a'..='z' | '0'..='9' | '/' | ':' | '-' | '_' | '.' | '~' => uri.push(ch),
            other => {
                let mut buf = [0u8; 4];
                for byte in other.encode_utf8(&mut buf).as_bytes() {
                    uri.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    Ok(uri)
}

impl VideoSession {
    pub fn open(
        path: &Path,
        proxy: EventLoopProxy<UserEvent>,
        max_edge: u32,
    ) -> Result<Self, String> {
        ensure_winrt()?;
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let (device, context) = create_device()?;
        let shared = Arc::new(Shared {
            alive: AtomicBool::new(true),
            generation,
            max_edge,
            frames: Mutex::new(None),
            copier: Mutex::new(Copier {
                device,
                context,
                target: None,
                staging: None,
                surface: None,
                size: None,
            }),
            proxy,
            failed: AtomicBool::new(false),
        });

        let player = windows::Media::Playback::MediaPlayer::new()
            .map_err(|err| format!("Couldn't open the video player. {err}"))?;
        if let Ok(commands) = player.CommandManager() {
            let _ = commands.SetIsEnabled(false);
        }
        player
            .SetIsVideoFrameServerEnabled(true)
            .map_err(|err| format!("Couldn't read video frames. {err}"))?;
        player
            .SetIsMuted(false)
            .map_err(|err| format!("Couldn't play audio. {err}"))?;

        let frame_shared = shared.clone();
        let frame_token = player
            .VideoFrameAvailable(&on::<
                windows::Media::Playback::MediaPlayer,
                windows::core::IInspectable,
            >(move |player, _| {
                on_frame(player.as_ref(), &frame_shared);
                Ok(())
            }))
            .map_err(|err| format!("Couldn't read video frames. {err}"))?;

        let opened_shared = shared.clone();
        let opened_token = player
            .MediaOpened(&on::<
                windows::Media::Playback::MediaPlayer,
                windows::core::IInspectable,
            >(move |player, _| {
                let Some(player) = player.as_ref() else {
                    return Ok(());
                };
                let message = match presented_size(player) {
                    Ok(Some(_)) => return Ok(()),
                    Ok(None) => "This file has no video frames.".to_string(),
                    Err(err) => err,
                };
                if !opened_shared.failed.swap(true, Ordering::AcqRel) {
                    let _ = opened_shared.proxy.send_event(UserEvent::VideoFailed {
                        generation: opened_shared.generation,
                        message,
                    });
                }
                Ok(())
            }))
            .map_err(|err| format!("Couldn't open this video. {err}"))?;

        let failed_shared = shared.clone();
        let failed_token = player
            .MediaFailed(&on::<
                windows::Media::Playback::MediaPlayer,
                windows::Media::Playback::MediaPlayerFailedEventArgs,
            >(move |_, args| {
                if failed_shared.failed.swap(true, Ordering::AcqRel) {
                    return Ok(());
                }
                let message = args
                    .as_ref()
                    .map(failure_message)
                    .unwrap_or_else(|| "Couldn't play this video.".into());
                let _ = failed_shared.proxy.send_event(UserEvent::VideoFailed {
                    generation: failed_shared.generation,
                    message,
                });
                Ok(())
            }))
            .map_err(|err| format!("Couldn't open this video. {err}"))?;

        let ended_shared = shared.clone();
        let ended_token = player
            .MediaEnded(&on::<
                windows::Media::Playback::MediaPlayer,
                windows::core::IInspectable,
            >(move |_, _| {
                let _ = ended_shared.proxy.send_event(UserEvent::VideoEnded {
                    generation: ended_shared.generation,
                });
                Ok(())
            }))
            .map_err(|err| format!("Couldn't open this video. {err}"))?;

        let uri = file_uri(path)?;
        let winrt_uri = windows::Foundation::Uri::CreateUri(&windows::core::HSTRING::from(uri))
            .map_err(|err| format!("Couldn't open this video. {err}"))?;
        let source = windows::Media::Core::MediaSource::CreateFromUri(&winrt_uri)
            .map_err(|err| format!("Couldn't open this video. {err}"))?;
        player
            .SetSource(&source)
            .map_err(|err| format!("Couldn't open this video. {err}"))?;
        player
            .Play()
            .map_err(|err| format!("Couldn't play this video. {err}"))?;

        Ok(Self {
            path: path.to_path_buf(),
            generation,
            player,
            _source: source,
            shared,
            frame_token,
            opened_token,
            failed_token,
            ended_token,
            resume_after_overlay: false,
            ended: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn take_frame(&self) -> Option<DecodedImage> {
        lock(&self.shared.frames).take()
    }

    pub fn mark_ended(&mut self) {
        self.ended = true;
        self.resume_after_overlay = false;
    }

    pub fn show_play_icon(&self) -> bool {
        self.ended || !self.is_playing()
    }

    /// Keep the bar on screen while the picture is frozen.
    pub fn hold_controls(&self) -> bool {
        if self.ended {
            return true;
        }
        matches!(
            self.player
                .PlaybackSession()
                .and_then(|session| session.PlaybackState()),
            Ok(windows::Media::Playback::MediaPlaybackState::Paused)
        )
    }

    pub fn timeline(&self) -> (Duration, Duration) {
        let Ok(session) = self.player.PlaybackSession() else {
            return (Duration::ZERO, Duration::ZERO);
        };
        let position = session
            .Position()
            .map(span_to_duration)
            .unwrap_or(Duration::ZERO);
        let duration = session
            .NaturalDuration()
            .map(span_to_duration)
            .unwrap_or(Duration::ZERO);
        (position, duration)
    }

    pub fn duration(&self) -> Option<Duration> {
        let (_, duration) = self.timeline();
        (duration > Duration::ZERO).then_some(duration)
    }

    pub fn toggle_pause(&mut self) {
        if self.ended {
            self.seek_fraction(0.0);
            self.ended = false;
            let _ = self.player.Play();
            return;
        }
        if self.is_playing() {
            let _ = self.player.Pause();
        } else {
            let _ = self.player.Play();
        }
    }

    pub fn seek_fraction(&mut self, fraction: f32) {
        let Ok(session) = self.player.PlaybackSession() else {
            return;
        };
        let Ok(duration) = session.NaturalDuration() else {
            return;
        };
        if duration.Duration <= 0 {
            return;
        }
        let fraction = (fraction as f64).clamp(0.0, 1.0);
        let ticks = (duration.Duration as f64 * fraction) as i64;
        let _ = session.SetPosition(windows::Foundation::TimeSpan { Duration: ticks });
        if self.ended && fraction < 0.999 {
            self.ended = false;
        }
    }

    pub fn pause_for_overlay(&mut self) {
        self.resume_after_overlay = self.is_playing();
        if self.resume_after_overlay {
            let _ = self.player.Pause();
        }
    }

    /// Pause, and play again when the overlay closes, even if playback had not
    /// reported itself as playing yet.
    pub fn hold_for_overlay(&mut self) {
        self.resume_after_overlay = !self.ended;
        let _ = self.player.Pause();
    }

    pub fn resume_after_overlay(&mut self) {
        let resume = self.resume_after_overlay;
        self.resume_after_overlay = false;
        if resume && !self.ended {
            let _ = self.player.Play();
        }
    }

    fn is_playing(&self) -> bool {
        let Ok(session) = self.player.PlaybackSession() else {
            return false;
        };
        matches!(
            session.PlaybackState(),
            Ok(windows::Media::Playback::MediaPlaybackState::Playing
                | windows::Media::Playback::MediaPlaybackState::Opening
                | windows::Media::Playback::MediaPlaybackState::Buffering)
        )
    }
}

impl Drop for VideoSession {
    fn drop(&mut self) {
        self.shared.alive.store(false, Ordering::Release);
        {
            let _guard = lock(&self.shared.copier);
        }
        let _ = self.player.RemoveVideoFrameAvailable(self.frame_token);
        let _ = self.player.RemoveMediaOpened(self.opened_token);
        let _ = self.player.RemoveMediaFailed(self.failed_token);
        let _ = self.player.RemoveMediaEnded(self.ended_token);
        let _ = self.player.Pause();
        if let Ok(closable) = self.player.cast::<windows::Foundation::IClosable>() {
            let _ = closable.Close();
        }
    }
}

fn on_frame(player: Option<&windows::Media::Playback::MediaPlayer>, shared: &Shared) {
    if !shared.alive.load(Ordering::Acquire) {
        return;
    }
    let Some(player) = player else {
        return;
    };
    let image = {
        let mut copier = lock(&shared.copier);
        if !shared.alive.load(Ordering::Acquire) {
            return;
        }
        match copier.copy(player, shared.max_edge) {
            Ok(None) => return,
            Ok(Some(image)) => image,
            Err(err) => {
                if !shared.failed.swap(true, Ordering::AcqRel) {
                    let _ = shared.proxy.send_event(UserEvent::VideoFailed {
                        generation: shared.generation,
                        message: err,
                    });
                }
                return;
            }
        }
    };
    *lock(&shared.frames) = Some(image);
    let _ = shared.proxy.send_event(UserEvent::VideoFrame {
        generation: shared.generation,
    });
}

impl Copier {
    fn copy(
        &mut self,
        player: &windows::Media::Playback::MediaPlayer,
        max_edge: u32,
    ) -> Result<Option<DecodedImage>, String> {
        if self.surface.is_none() {
            let Some((width, height)) = presented_size(player)? else {
                return Ok(None);
            };
            self.allocate(width, height, max_edge)?;
            if player
                .CopyFrameToVideoSurface(self.surface.as_ref().unwrap())
                .is_err()
            {
                if width == height {
                    return Err("Couldn't read a video frame.".into());
                }
                self.allocate(height, width, max_edge)?;
                player
                    .CopyFrameToVideoSurface(self.surface.as_ref().unwrap())
                    .map_err(|err| format!("Couldn't read a video frame. {err}"))?;
            }
        } else {
            player
                .CopyFrameToVideoSurface(self.surface.as_ref().unwrap())
                .map_err(|err| format!("Couldn't read a video frame. {err}"))?;
        }
        self.read_rgba().map(Some)
    }

    fn allocate(&mut self, width: u32, height: u32, max_edge: u32) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Err("This file has no video frames.".into());
        }
        if width > max_edge || height > max_edge {
            return Err(format!(
                "This video is too large to display ({width} × {height})."
            ));
        }
        unsafe {
            let target = texture(
                &self.device,
                width,
                height,
                windows::Win32::Graphics::Direct3D11::D3D11_USAGE_DEFAULT,
                windows::Win32::Graphics::Direct3D11::D3D11_BIND_RENDER_TARGET
                    | windows::Win32::Graphics::Direct3D11::D3D11_BIND_SHADER_RESOURCE,
                Default::default(),
            )?;
            let staging = texture(
                &self.device,
                width,
                height,
                windows::Win32::Graphics::Direct3D11::D3D11_USAGE_STAGING,
                Default::default(),
                windows::Win32::Graphics::Direct3D11::D3D11_CPU_ACCESS_READ,
            )?;
            let dxgi = target
                .cast::<windows::Win32::Graphics::Dxgi::IDXGISurface>()
                .map_err(|err| format!("Couldn't read a video frame. {err}"))?;
            let created =
                windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11SurfaceFromDXGISurface(
                    &dxgi,
                )
                .map_err(|err| format!("Couldn't read a video frame. {err}"))?;
            let surface = created
                .cast::<windows::Graphics::DirectX::Direct3D11::IDirect3DSurface>()
                .map_err(|err| format!("Couldn't read a video frame. {err}"))?;
            self.target = Some(target);
            self.staging = Some(staging);
            self.surface = Some(surface);
            self.size = Some((width, height));
        }
        Ok(())
    }

    fn read_rgba(&self) -> Result<DecodedImage, String> {
        let (width, height) = self.size.ok_or("This file has no video frames.")?;
        let target = self.target.as_ref().unwrap();
        let staging = self.staging.as_ref().unwrap();
        unsafe {
            self.context.CopyResource(staging, target);
            let mut mapped =
                windows::Win32::Graphics::Direct3D11::D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(
                    staging,
                    0,
                    windows::Win32::Graphics::Direct3D11::D3D11_MAP_READ,
                    Default::default(),
                    Some(&mut mapped),
                )
                .map_err(|err| format!("Couldn't read a video frame. {err}"))?;
            let image = read_bgra(&mapped, width, height);
            self.context.Unmap(staging, 0);
            image
        }
    }
}

fn read_bgra(
    mapped: &windows::Win32::Graphics::Direct3D11::D3D11_MAPPED_SUBRESOURCE,
    width: u32,
    height: u32,
) -> Result<DecodedImage, String> {
    let row_bytes = width as usize * 4;
    let pitch = mapped.RowPitch as usize;
    if mapped.pData.is_null() || pitch < row_bytes {
        return Err("Couldn't read a video frame.".into());
    }
    let mut rgba = vec![0u8; row_bytes * height as usize];
    unsafe {
        let src = mapped.pData as *const u8;
        for y in 0..height as usize {
            let row = std::slice::from_raw_parts(src.add(y * pitch), row_bytes);
            let dest = &mut rgba[y * row_bytes..(y + 1) * row_bytes];
            for x in 0..width as usize {
                let i = x * 4;
                dest[i] = row[i + 2];
                dest[i + 1] = row[i + 1];
                dest[i + 2] = row[i];
                dest[i + 3] = 255;
            }
        }
    }
    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

fn presented_size(
    player: &windows::Media::Playback::MediaPlayer,
) -> Result<Option<(u32, u32)>, String> {
    let session = player
        .PlaybackSession()
        .map_err(|err| format!("Couldn't open this video. {err}"))?;
    let width = session
        .NaturalVideoWidth()
        .map_err(|err| format!("Couldn't open this video. {err}"))?;
    let height = session
        .NaturalVideoHeight()
        .map_err(|err| format!("Couldn't open this video. {err}"))?;
    if width == 0 || height == 0 {
        return Ok(None);
    }
    let swap = matches!(
        session.PlaybackRotation(),
        Ok(
            windows::Media::MediaProperties::MediaRotation::Clockwise90Degrees
                | windows::Media::MediaProperties::MediaRotation::Clockwise270Degrees
        )
    );
    Ok(Some(if swap {
        (height, width)
    } else {
        (width, height)
    }))
}

unsafe fn texture(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
    usage: windows::Win32::Graphics::Direct3D11::D3D11_USAGE,
    bind: windows::Win32::Graphics::Direct3D11::D3D11_BIND_FLAG,
    cpu: windows::Win32::Graphics::Direct3D11::D3D11_CPU_ACCESS_FLAG,
) -> Result<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D, String> {
    let desc = windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: usage,
        BindFlags: bind.0 as u32,
        CPUAccessFlags: cpu.0 as u32,
        MiscFlags: Default::default(),
    };
    let mut texture = None;
    device
        .CreateTexture2D(&desc, None, Some(&mut texture))
        .map_err(|err| format!("Couldn't read a video frame. {err}"))?;
    texture.ok_or_else(|| "Couldn't read a video frame.".into())
}

fn create_device() -> Result<
    (
        windows::Win32::Graphics::Direct3D11::ID3D11Device,
        windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    ),
    String,
> {
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
    use windows::Win32::Graphics::Direct3D11::D3D11CreateDevice;
    use windows::Win32::Graphics::Direct3D11::ID3D11Multithread;
    use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    use windows::Win32::Graphics::Direct3D11::D3D11_SDK_VERSION;

    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let mut context = None;
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                windows::Win32::Foundation::HMODULE(std::ptr::null_mut()),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        if created.is_ok() {
            if let (Some(device), Some(context)) = (device, context) {
                if let Ok(multithread) = device.cast::<ID3D11Multithread>() {
                    unsafe {
                        let _ = multithread.SetMultithreadProtected(true);
                    }
                }
                return Ok((device, context));
            }
        }
    }
    Err("Couldn't open a video device.".into())
}

fn ensure_winrt() -> Result<(), String> {
    match unsafe {
        windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_SINGLETHREADED,
        )
    } {
        Ok(()) => Ok(()),
        Err(err) if err.code().0 == RPC_E_CHANGED_MODE => Ok(()),
        Err(err) => Err(format!("Couldn't start Windows media. {err}")),
    }
}

fn failure_message(args: &windows::Media::Playback::MediaPlayerFailedEventArgs) -> String {
    use windows::Media::Playback::MediaPlayerError;
    let reason = match args.Error() {
        Ok(MediaPlayerError::DecodingError) => "Couldn't decode this video.",
        Ok(MediaPlayerError::SourceNotSupported) => "This video format isn't supported.",
        Ok(MediaPlayerError::NetworkError) => "Couldn't open this video.",
        Ok(MediaPlayerError::Aborted) => "Playback was interrupted.",
        _ => "Couldn't play this video.",
    };
    match args.ExtendedErrorCode() {
        Ok(hr) if !hr.is_ok() => format!("{reason} ({hr})"),
        _ => reason.to_string(),
    }
}

fn span_to_duration(span: windows::Foundation::TimeSpan) -> Duration {
    if span.Duration <= 0 {
        Duration::ZERO
    } else {
        Duration::from_nanos((span.Duration as u64).saturating_mul(100))
    }
}

fn on<T, A>(
    invoke: impl FnMut(windows::core::Ref<'_, T>, windows::core::Ref<'_, A>) -> windows::core::Result<()>
        + Send
        + 'static,
) -> windows::Foundation::TypedEventHandler<T, A>
where
    T: windows::core::RuntimeType + 'static,
    A: windows::core::RuntimeType + 'static,
{
    windows::Foundation::TypedEventHandler::new(invoke)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_uses_forward_slashes_and_escapes_spaces() {
        let dir = std::env::temp_dir().join(format!("matejko uri {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        std::fs::write(&path, b"x").unwrap();
        let uri = file_uri(&path).unwrap();
        assert!(uri.starts_with("file:///"), "{uri}");
        assert!(uri.contains("matejko%20uri"), "{uri}");
        assert!(uri.ends_with("/clip.mp4"), "{uri}");
        assert!(!uri.contains('\\'), "{uri}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opening_a_bogus_mp4_does_not_panic() {
        let dir = std::env::temp_dir().join(format!("matejko-bogus-mp4-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.mp4");
        std::fs::write(&path, b"not a video").unwrap();
        use winit::platform::windows::EventLoopBuilderExtWindows;
        let event_loop =
            winit::event_loop::EventLoop::<crate::decode::UserEvent>::with_user_event()
                .with_any_thread(true)
                .build()
                .expect("event loop");
        let opened = VideoSession::open(&path, event_loop.create_proxy(), 4096);
        drop(opened);
        drop(event_loop);
        let _ = std::fs::remove_dir_all(dir);
    }
}
