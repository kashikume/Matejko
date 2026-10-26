//! Background JPEG and PNG decode. EXIF orientation is applied to the pixels.

use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::thread::{self, JoinHandle};

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use winit::event_loop::EventLoopProxy;

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub enum UserEvent {
    Decoded {
        path: PathBuf,
        result: Result<DecodedImage, String>,
    },
    /// The watched folder changed. The app debounces before rescanning.
    FolderDirty,
}

struct Jobs {
    queue: Mutex<std::collections::VecDeque<PathBuf>>,
    ready: Condvar,
}

impl Jobs {
    fn new() -> Self {
        Self {
            queue: Mutex::new(std::collections::VecDeque::new()),
            ready: Condvar::new(),
        }
    }

    fn enqueue(&self, path: PathBuf, front: bool) {
        let mut queue = self.queue.lock().expect("decode queue");
        queue.retain(|queued| queued != &path);
        if front {
            queue.push_front(path);
        } else {
            queue.push_back(path);
        }
        self.ready.notify_one();
    }

    fn promote(&self, path: &Path) {
        let mut queue = self.queue.lock().expect("decode queue");
        if let Some(index) = queue.iter().position(|queued| queued == path) {
            let path = queue.remove(index).expect("queued path");
            queue.push_front(path);
        }
    }

    fn pop(&self) -> PathBuf {
        let mut queue = self.queue.lock().expect("decode queue");
        loop {
            if let Some(path) = queue.pop_front() {
                return path;
            }
            queue = self.ready.wait(queue).expect("decode queue");
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
                let path = thread_jobs.pop();
                let result = decode_file(&path, max_edge);
                if proxy
                    .send_event(UserEvent::Decoded { path, result })
                    .is_err()
                {
                    break;
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
}

pub fn decode_file(path: &Path, max_edge: u32) -> Result<DecodedImage, String> {
    let reader = ImageReader::open(path).map_err(|err| err.to_string())?;
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
        rgba: pixels,
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
            &decoded.rgba
        );
        let top = &decoded.rgba[0..3];
        let bottom = &decoded.rgba[4..7];
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
}
