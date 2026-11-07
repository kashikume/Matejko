//! Background JPEG and PNG decode. EXIF orientation is applied to the pixels.

use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::thread::{self, JoinHandle};

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use winit::event_loop::EventLoopProxy;

/// The byte order of `DecodedImage::pixels`, four bytes a pixel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PixelOrder {
    #[default]
    Rgba,
    /// A video frame as Direct3D hands it over. The fourth byte is not
    /// alpha and is not used.
    Bgrx,
}

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub order: PixelOrder,
}

/// A file's size and modified time: when either moves, the file changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    len: u64,
    modified: std::time::SystemTime,
}

pub fn read_stamp(path: &Path) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileStamp {
        len: meta.len(),
        modified: meta.modified().ok()?,
    })
}

pub enum UserEvent {
    Decoded {
        path: PathBuf,
        /// Read just before the file was, so it describes what was decoded.
        stamp: Option<FileStamp>,
        result: Result<DecodedImage, String>,
    },
    /// The JPEG baseline for edit mode, decoded on this thread.
    BaselineReady {
        path: PathBuf,
        result: Result<EditBaseline, String>,
    },
    /// A render or a save finished on the edit worker.
    Edit(crate::edit::WorkerEvent),
    /// The watched folder changed. The app debounces before rescanning.
    FolderDirty,
    /// A new video frame is waiting in the player. `generation` identifies the
    /// session that produced it.
    VideoFrame {
        generation: u64,
    },
    VideoFailed {
        generation: u64,
        message: String,
    },
    VideoEnded {
        generation: u64,
    },
}

enum Job {
    View(PathBuf),
    Edit(PathBuf),
}

pub struct EditBaseline {
    pub loaded: crate::edit::LoadedSession,
    pub drag: crate::edit::Image,
}

/// The waiting work. The edit request shares the queue's lock, so a request
/// can never land between the thread's last look and its wait.
#[derive(Default)]
struct Waiting {
    queue: std::collections::VecDeque<PathBuf>,
    edit: Option<PathBuf>,
}

struct Jobs {
    waiting: Mutex<Waiting>,
    ready: Condvar,
}

impl Jobs {
    fn new() -> Self {
        Self {
            waiting: Mutex::new(Waiting::default()),
            ready: Condvar::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Waiting> {
        self.waiting.lock().expect("decode queue")
    }

    fn enqueue(&self, path: PathBuf, front: bool) {
        let mut waiting = self.lock();
        waiting.queue.retain(|queued| queued != &path);
        if front {
            waiting.queue.push_front(path);
        } else {
            waiting.queue.push_back(path);
        }
        self.ready.notify_one();
    }

    fn promote(&self, path: &Path) {
        let mut waiting = self.lock();
        if let Some(index) = waiting.queue.iter().position(|queued| queued == path) {
            let path = waiting.queue.remove(index).expect("queued path");
            waiting.queue.push_front(path);
        }
    }

    /// Drops every waiting decode not in `keep`, and returns the paths it
    /// dropped. The decode already running finishes.
    fn retain(&self, keep: &[PathBuf]) -> Vec<PathBuf> {
        let mut waiting = self.lock();
        let mut dropped = Vec::new();
        waiting.queue.retain(|queued| {
            let kept = keep.contains(queued);
            if !kept {
                dropped.push(queued.clone());
            }
            kept
        });
        dropped
    }

    fn request_edit(&self, path: PathBuf) {
        self.lock().edit = Some(path);
        self.ready.notify_one();
    }

    fn pop(&self) -> Job {
        let mut waiting = self.lock();
        loop {
            if let Some(path) = waiting.edit.take() {
                return Job::Edit(path);
            }
            if let Some(path) = waiting.queue.pop_front() {
                return Job::View(path);
            }
            waiting = self.ready.wait(waiting).expect("decode queue");
        }
    }
}

pub struct Decoder {
    jobs: std::sync::Arc<Jobs>,
    _thread: JoinHandle<()>,
}

impl Decoder {
    pub fn spawn(proxy: EventLoopProxy<UserEvent>, max_edge: u32) -> Self {
        let jobs = std::sync::Arc::new(Jobs::new());
        let thread_jobs = jobs.clone();
        let thread = thread::Builder::new()
            .name("matejko-decode".into())
            .spawn(move || loop {
                match thread_jobs.pop() {
                    Job::View(path) => {
                        let stamp = read_stamp(&path);
                        let result = decode_file(&path, max_edge);
                        if proxy
                            .send_event(UserEvent::Decoded {
                                path,
                                stamp,
                                result,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Job::Edit(path) => {
                        let result = load_edit_baseline(&path);
                        if proxy
                            .send_event(UserEvent::BaselineReady { path, result })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            })
            .expect("decode thread");
        Self {
            jobs,
            _thread: thread,
        }
    }

    pub fn enqueue(&self, path: PathBuf, front: bool) {
        self.jobs.enqueue(path, front);
    }

    pub fn promote(&self, path: &Path) {
        self.jobs.promote(path);
    }

    pub fn request_edit(&self, path: PathBuf) {
        self.jobs.request_edit(path);
    }

    /// See `Jobs::retain`.
    pub fn retain(&self, keep: &[PathBuf]) -> Vec<PathBuf> {
        self.jobs.retain(keep)
    }
}

fn load_edit_baseline(path: &Path) -> Result<EditBaseline, String> {
    let loaded = crate::edit::load_session(path)?;
    let drag = loaded
        .baseline
        .downsample_long_side(crate::edit::DRAG_LONG_SIDE);
    Ok(EditBaseline { loaded, drag })
}

pub fn decode_file(path: &Path, max_edge: u32) -> Result<DecodedImage, String> {
    // The bytes name the format, not the extension: a PNG saved as .jpg
    // still opens. Bytes that name nothing fall back to the extension.
    let reader = ImageReader::open(path)
        .and_then(ImageReader::with_guessed_format)
        .map_err(|err| err.to_string())?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|err| format!("Couldn't decode this file. {err}"))?;
    decoder
        .set_limits(Limits::no_limits())
        .map_err(|err| err.to_string())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (width, height) = decoder.dimensions();
    let (oriented_width, oriented_height) = oriented_dimensions(width, height, orientation);
    if oriented_width > max_edge || oriented_height > max_edge {
        return Err(format!(
            "This image is too large to display ({oriented_width} × {oriented_height})."
        ));
    }
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|err| format!("Couldn't decode this file. {err}"))?;
    image.apply_orientation(orientation);
    let rgba = image.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let pixels = rgba.into_raw();
    if pixels.len() != width as usize * height as usize * 4 {
        return Err("Couldn't decode this file. The pixel buffer has an unexpected size.".into());
    }
    Ok(DecodedImage {
        width,
        height,
        pixels,
        order: PixelOrder::Rgba,
    })
}

fn oriented_dimensions(width: u32, height: u32, orientation: Orientation) -> (u32, u32) {
    match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        _ => (width, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use std::io::Cursor;

    fn jpeg_with_orientation(orientation: u16) -> Vec<u8> {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode(
                &[255, 0, 0, 0, 255, 0],
                2,
                1,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        inject_exif_orientation(&jpeg, orientation)
    }

    fn inject_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let mut payload = Vec::new();
        payload.extend_from_slice(b"Exif\0\0");
        payload.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&0x0112u16.to_le_bytes());
        payload.extend_from_slice(&3u16.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&orientation.to_le_bytes());
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&[0, 0, 0, 0]);
        let length = (payload.len() + 2) as u16;
        let mut out = Vec::with_capacity(jpeg.len() + payload.len() + 4);
        out.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE1]);
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn retain_drops_the_decodes_that_are_no_longer_wanted() {
        let jobs = Jobs::new();
        for name in ["a.jpg", "b.jpg", "c.jpg", "d.jpg"] {
            jobs.enqueue(PathBuf::from(name), false);
        }
        let dropped = jobs.retain(&[PathBuf::from("c.jpg"), PathBuf::from("x.jpg")]);
        assert_eq!(dropped, ["a.jpg", "b.jpg", "d.jpg"].map(PathBuf::from));
        assert!(matches!(jobs.pop(), Job::View(path) if path == Path::new("c.jpg")));
        assert!(jobs.lock().queue.is_empty());
    }

    #[test]
    fn an_edit_request_wakes_a_waiting_thread_and_goes_first() {
        let jobs = std::sync::Arc::new(Jobs::new());
        jobs.enqueue(PathBuf::from("view.jpg"), false);
        jobs.request_edit(PathBuf::from("edit.jpg"));
        assert!(matches!(jobs.pop(), Job::Edit(path) if path == Path::new("edit.jpg")));
        assert!(matches!(jobs.pop(), Job::View(path) if path == Path::new("view.jpg")));

        // A thread already waiting on an empty queue is woken by the request.
        let waiter = {
            let jobs = jobs.clone();
            thread::spawn(move || matches!(jobs.pop(), Job::Edit(_)))
        };
        thread::sleep(std::time::Duration::from_millis(50));
        jobs.request_edit(PathBuf::from("later.jpg"));
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn orientation_six_rotates_a_jpeg_upright() {
        let dir = std::env::temp_dir().join(format!("matejko-exif-{}", std::process::id()));
        fs_create(&dir);
        let path = dir.join("phone.jpg");
        std::fs::write(&path, jpeg_with_orientation(6)).unwrap();
        let decoded = decode_file(&path, 4096).unwrap();
        assert_eq!(
            (decoded.width, decoded.height),
            (1, 2),
            "pixels {:?}",
            &decoded.pixels
        );
        let top = &decoded.pixels[0..3];
        let bottom = &decoded.pixels[4..7];
        assert!(
            top[0] > top[1] + 40 && bottom[1] > bottom[0] + 40,
            "expected red above green, got top {top:?} bottom {bottom:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn fs_create(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
    }

    #[test]
    fn png_round_trip_keeps_size() {
        let image =
            DynamicImage::ImageRgba8(image::RgbaImage::from_raw(2, 2, vec![0; 16]).unwrap());
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let dir = std::env::temp_dir().join(format!("matejko-png-{}", std::process::id()));
        fs_create(&dir);
        let path = dir.join("square.png");
        std::fs::write(&path, bytes).unwrap();
        let decoded = decode_file(&path, 64).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_png_named_jpg_still_decodes() {
        let image =
            DynamicImage::ImageRgba8(image::RgbaImage::from_raw(3, 1, vec![9; 12]).unwrap());
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let dir = std::env::temp_dir().join(format!("matejko-misnamed-{}", std::process::id()));
        fs_create(&dir);
        let path = dir.join("actually_png.jpg");
        std::fs::write(&path, bytes).unwrap();
        let decoded = decode_file(&path, 64).unwrap();
        assert_eq!((decoded.width, decoded.height), (3, 1));
        let _ = std::fs::remove_dir_all(dir);
    }
}
