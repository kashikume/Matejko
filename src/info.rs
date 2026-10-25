//! File and EXIF summary for the information overlay.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use exif::{Error as ExifError, Exif, In, Reader as ExifReader, Tag};

/// Build the text shown when the user presses I.
pub fn format_image_info(path: &Path, pixels: Option<(u32, u32)>) -> String {
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

    if let Some((width, height)) = pixels {
        lines.push(format!("Pixels {width} × {height}"));
    } else {
        lines.push("Pixels (not loaded yet)".into());
    }

    match read_exif(path) {
        Ok(exif) => append_exif(&mut lines, &exif),
        Err(ExifStatus::Missing) => lines.push("No EXIF data".into()),
        Err(ExifStatus::Error(err)) => lines.push(format!("EXIF unavailable. {err}")),
    }

    lines.push(String::new());
    lines.push("Esc or I to close".into());
    lines.join("\n")
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
        (false, false) if model.to_ascii_lowercase().contains(&make.to_ascii_lowercase()) => {
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

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "matejko-info-{label}-{}",
            std::process::id()
        ));
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

        let text = format_image_info(&path, Some((3, 2)));
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
        let text = format_image_info(&path, Some((1, 2)));
        assert!(text.contains("phone.jpg"), "{text}");
        assert!(text.contains("Format JPEG"), "{text}");
        assert!(text.contains("Orientation"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
