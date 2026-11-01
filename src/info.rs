//! File and EXIF summary for the information overlay.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use exif::{Error as ExifError, Exif, In, Reader as ExifReader, Tag, Value};

use crate::catalog::is_video;
use crate::transport::format_clock;

/// Build the text shown when the user presses I.
///
/// `duration` is the playing length of a video. Photos pass `None`.
pub fn format_image_info(
    path: &Path,
    pixels: Option<(u32, u32)>,
    duration: Option<Duration>,
) -> String {
    let mut lines = Vec::new();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("?");
    lines.push(name.to_string());

    if let Some(parent) = path.parent() {
        let folder = parent.to_string_lossy();
        if !folder.is_empty() {
            lines.push(folder.into_owned());
        }
    }

    match path.metadata() {
        Ok(meta) => {
            lines.push(format!("Size {}", format_bytes(meta.len())));
            if let Ok(modified) = meta.modified() {
                lines.push(format!("Modified {}", format_time(modified)));
            }
            if let Ok(created) = meta.created() {
                lines.push(format!("Created {}", format_time(created)));
            }
        }
        Err(err) => lines.push(format!("Couldn't read the file. {err}")),
    }

    if is_video(path) {
        lines.push("Format MP4".into());
    } else {
        let format = match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("jpg" | "jpeg") => "JPEG",
            Some("png") => "PNG",
            _ => "Image",
        };
        lines.push(format!("Format {format}"));
    }

    if let Some((width, height)) = pixels {
        lines.push(format!("Pixels {width} × {height}"));
    } else {
        lines.push("Pixels (not loaded yet)".into());
    }

    if is_video(path) {
        match duration {
            Some(duration) if !duration.is_zero() => {
                lines.push(format!("Duration {}", format_clock(duration)));
            }
            _ => lines.push("Duration unknown".into()),
        }
    } else {
        match read_exif(path) {
            Ok(exif) => append_exif(&mut lines, &exif),
            Err(ExifStatus::Missing) => lines.push("No EXIF data".into()),
            Err(ExifStatus::Error(err)) => lines.push(format!("EXIF unavailable. {err}")),
        }
    }

    lines.push(String::new());
    if gps_coordinates(path).is_some() {
        lines.push("M to show on Google Maps".into());
    }
    lines.push("Esc or I to close".into());
    lines.join("\n")
}

/// Latitude and longitude in signed decimal degrees, when a photo's EXIF has
/// a usable GPS position. South and west are negative.
pub fn gps_coordinates(path: &Path) -> Option<(f64, f64)> {
    if is_video(path) {
        return None;
    }
    let exif = read_exif(path).ok()?;
    let lat = gps_degrees(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S')?;
    let lon = gps_degrees(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W')?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some((lat, lon))
}

/// The Google Maps address that drops a pin on the coordinates.
pub fn maps_url((lat, lon): (f64, f64)) -> String {
    format!("https://www.google.com/maps/search/?api=1&query={lat:.6},{lon:.6}")
}

fn gps_degrees(exif: &Exif, tag: Tag, ref_tag: Tag, negative: u8) -> Option<f64> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    // Degrees, minutes, seconds. Some writers leave out the seconds.
    let Value::Rational(parts) = &field.value else {
        return None;
    };
    if parts.is_empty() || parts.iter().any(|part| part.denom == 0) {
        return None;
    }
    let degrees: f64 = parts
        .iter()
        .zip([1.0, 60.0, 3600.0])
        .map(|(part, scale)| part.to_f64() / scale)
        .sum();
    let south_or_west = exif
        .get_field(ref_tag, In::PRIMARY)
        .and_then(|field| match &field.value {
            Value::Ascii(strings) => strings.first().and_then(|s| s.first().copied()),
            _ => None,
        })
        .is_some_and(|letter| letter.eq_ignore_ascii_case(&negative));
    Some(if south_or_west { -degrees } else { degrees })
}

enum ExifStatus {
    Missing,
    Error(String),
}

fn read_exif(path: &Path) -> Result<Exif, ExifStatus> {
    let file = File::open(path).map_err(|err| ExifStatus::Error(err.to_string()))?;
    let mut reader = BufReader::new(file);
    match ExifReader::new().read_from_container(&mut reader) {
        Ok(exif) => {
            if exif.fields().next().is_none() {
                Err(ExifStatus::Missing)
            } else {
                Ok(exif)
            }
        }
        Err(ExifError::NotFound(_)) => Err(ExifStatus::Missing),
        Err(err) => Err(ExifStatus::Error(err.to_string())),
    }
}

fn append_exif(lines: &mut Vec<String>, exif: &Exif) {
    lines.push(String::new());
    lines.push("EXIF".into());

    let mut any = false;
    any |= push_tag(lines, exif, "Camera", Tag::Make, Tag::Model);
    any |= push_field(lines, exif, "Lens", Tag::LensModel);
    any |= push_field(lines, exif, "Taken", Tag::DateTimeOriginal);
    if !lines.iter().any(|line| line.starts_with("Taken ")) {
        any |= push_field(lines, exif, "Taken", Tag::DateTime);
    }
    any |= push_exposure(lines, exif);
    any |= push_field(lines, exif, "Focal length", Tag::FocalLength);
    any |= push_field(lines, exif, "35mm equiv.", Tag::FocalLengthIn35mmFilm);
    any |= push_field(lines, exif, "Flash", Tag::Flash);
    any |= push_field(lines, exif, "White balance", Tag::WhiteBalance);
    any |= push_field(lines, exif, "Orientation", Tag::Orientation);
    any |= push_field(lines, exif, "Software", Tag::Software);
    any |= push_field(lines, exif, "Artist", Tag::Artist);
    any |= push_field(lines, exif, "Copyright", Tag::Copyright);
    any |= push_gps(lines, exif);

    if !any {
        // Keep a short dump of whatever is present when none of the usual tags are.
        let mut count = 0;
        for field in exif.fields() {
            if count >= 8 {
                lines.push("…".into());
                break;
            }
            lines.push(format!(
                "{} {}",
                field.tag,
                field.display_value().with_unit(exif)
            ));
            count += 1;
            any = true;
        }
    }

    if !any {
        lines.push("No useful tags".into());
    }
}

fn push_field(lines: &mut Vec<String>, exif: &Exif, label: &str, tag: Tag) -> bool {
    let Some(field) = exif.get_field(tag, In::PRIMARY) else {
        return false;
    };
    let value = field.display_value().with_unit(exif).to_string();
    if value.trim().is_empty() {
        return false;
    }
    lines.push(format!("{label} {value}"));
    true
}

fn push_tag(lines: &mut Vec<String>, exif: &Exif, label: &str, make: Tag, model: Tag) -> bool {
    let make = exif
        .get_field(make, In::PRIMARY)
        .map(|field| field.display_value().to_string())
        .unwrap_or_default();
    let model = exif
        .get_field(model, In::PRIMARY)
        .map(|field| field.display_value().to_string())
        .unwrap_or_default();
    let make = make.trim().trim_matches('"');
    let model = model.trim().trim_matches('"');
    let value = match (make.is_empty(), model.is_empty()) {
        (true, true) => return false,
        (false, true) => make.to_string(),
        (true, false) => model.to_string(),
        (false, false)
            if model
                .to_ascii_lowercase()
                .contains(&make.to_ascii_lowercase()) =>
        {
            model.to_string()
        }
        (false, false) => format!("{make} {model}"),
    };
    lines.push(format!("{label} {value}"));
    true
}

fn push_exposure(lines: &mut Vec<String>, exif: &Exif) -> bool {
    let mut parts = Vec::new();
    for tag in [Tag::ExposureTime, Tag::FNumber] {
        if let Some(field) = exif.get_field(tag, In::PRIMARY) {
            parts.push(field.display_value().with_unit(exif).to_string());
        }
    }
    let iso = iso_value(exif);
    if let Some(iso) = iso {
        parts.push(format!("ISO {iso}"));
    }
    if parts.is_empty() {
        return false;
    }
    lines.push(format!("Exposure {}", parts.join("  ")));
    true
}

fn iso_value(exif: &Exif) -> Option<String> {
    for tag in [
        Tag::PhotographicSensitivity,
        Tag::ISOSpeed,
        Tag::StandardOutputSensitivity,
    ] {
        if let Some(field) = exif.get_field(tag, In::PRIMARY) {
            return Some(field.display_value().to_string());
        }
    }
    None
}

fn push_gps(lines: &mut Vec<String>, exif: &Exif) -> bool {
    let (Some(lat), Some(lon)) = (
        exif.get_field(Tag::GPSLatitude, In::PRIMARY),
        exif.get_field(Tag::GPSLongitude, In::PRIMARY),
    ) else {
        return false;
    };
    let lat_ref = exif
        .get_field(Tag::GPSLatitudeRef, In::PRIMARY)
        .map(|field| field.display_value().to_string())
        .unwrap_or_default();
    let lon_ref = exif
        .get_field(Tag::GPSLongitudeRef, In::PRIMARY)
        .map(|field| field.display_value().to_string())
        .unwrap_or_default();
    lines.push(format!(
        "GPS {} {}  {} {}",
        lat.display_value().with_unit(exif),
        lat_ref.trim_matches('"'),
        lon.display_value().with_unit(exif),
        lon_ref.trim_matches('"'),
    ));
    true
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if n < MB {
        format!("{:.1} KB ({bytes} B)", n / KB)
    } else if n < GB {
        format!("{:.1} MB ({bytes} B)", n / MB)
    } else {
        format!("{:.2} GB ({bytes} B)", n / GB)
    }
}

fn format_time(time: SystemTime) -> String {
    let local: DateTime<Local> = time.into();
    local.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::DynamicImage;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::Duration;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("matejko-info-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_jpeg_with_orientation(path: &Path, orientation: u16) {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode(
                &[255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128],
                2,
                2,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        std::fs::write(path, inject_exif_orientation(&jpeg, orientation)).unwrap();
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
    fn info_lists_file_basics_for_a_png() {
        let dir = temp_dir("png");
        let path = dir.join("shot.png");
        let image =
            DynamicImage::ImageRgba8(image::RgbaImage::from_raw(3, 2, vec![0; 24]).unwrap());
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, bytes).unwrap();

        let text = format_image_info(&path, Some((3, 2)), None);
        assert!(text.contains("shot.png"), "{text}");
        assert!(text.contains("Format PNG"), "{text}");
        assert!(text.contains("Pixels 3 × 2"), "{text}");
        assert!(text.contains("No EXIF data"), "{text}");
        assert!(text.contains("Esc or I to close"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn info_reads_exif_orientation_from_a_jpeg() {
        let dir = temp_dir("jpeg");
        let path = dir.join("phone.jpg");
        write_jpeg_with_orientation(&path, 6);
        let text = format_image_info(&path, Some((1, 2)), None);
        assert!(text.contains("phone.jpg"), "{text}");
        assert!(text.contains("Format JPEG"), "{text}");
        assert!(text.contains("Orientation"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn write_jpeg_with_gps(path: &Path, lat: (&[u8], [u32; 3]), lon: (&[u8], [u32; 3])) {
        use exif::experimental::Writer;
        use exif::{Field, Rational};

        let dms = |parts: [u32; 3]| {
            Value::Rational(
                parts
                    .iter()
                    .map(|&n| Rational::from((n, 1)))
                    .collect(),
            )
        };
        let fields = [
            Field {
                tag: Tag::GPSLatitudeRef,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![lat.0.to_vec()]),
            },
            Field {
                tag: Tag::GPSLatitude,
                ifd_num: In::PRIMARY,
                value: dms(lat.1),
            },
            Field {
                tag: Tag::GPSLongitudeRef,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![lon.0.to_vec()]),
            },
            Field {
                tag: Tag::GPSLongitude,
                ifd_num: In::PRIMARY,
                value: dms(lon.1),
            },
        ];
        let mut writer = Writer::new();
        for field in &fields {
            writer.push_field(field);
        }
        let mut tiff = Cursor::new(Vec::new());
        writer.write(&mut tiff, true).unwrap();
        let tiff = tiff.into_inner();

        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode(&[0; 12], 2, 2, image::ExtendedColorType::Rgb8)
            .unwrap();
        let length = (tiff.len() + 8) as u16;
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(b"Exif\0\0");
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&jpeg[2..]);
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn gps_coordinates_are_signed_decimal_degrees() {
        let dir = temp_dir("gps");
        let path = dir.join("trip.jpg");
        write_jpeg_with_gps(&path, (b"S", [33, 51, 36]), (b"W", [70, 30, 0]));
        let (lat, lon) = gps_coordinates(&path).unwrap();
        assert!((lat + 33.86).abs() < 1e-9, "{lat}");
        assert!((lon + 70.5).abs() < 1e-9, "{lon}");
        assert_eq!(
            maps_url((lat, lon)),
            "https://www.google.com/maps/search/?api=1&query=-33.860000,-70.500000"
        );
        let text = format_image_info(&path, Some((2, 2)), None);
        assert!(text.contains("M to show on Google Maps"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn photos_without_gps_have_no_coordinates() {
        let dir = temp_dir("nogps");
        let path = dir.join("phone.jpg");
        write_jpeg_with_orientation(&path, 1);
        assert_eq!(gps_coordinates(&path), None);
        let text = format_image_info(&path, Some((2, 2)), None);
        assert!(!text.contains("Google Maps"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn info_describes_an_mp4_without_exif() {
        let dir = temp_dir("mp4");
        let path = dir.join("clip.mp4");
        std::fs::write(&path, b"not a real video").unwrap();
        let text = format_image_info(&path, Some((1920, 1080)), Some(Duration::from_secs(65)));
        assert!(text.contains("clip.mp4"), "{text}");
        assert!(text.contains("Format MP4"), "{text}");
        assert!(text.contains("Pixels 1920 × 1080"), "{text}");
        assert!(text.contains("Duration 1:05"), "{text}");
        assert!(!text.contains("EXIF"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
