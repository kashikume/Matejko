//! Baseline copy, adjustment JSON, and the JPEG writer.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};

use serde::{Deserialize, Serialize};

use crate::edit::image::{decode, Image};
use crate::edit::pipeline::{Adjustments, Aspect, Frame};

const VERSION: u32 = 1;
const SOFTWARE_NOTE: &str = "edited with Matejko";

#[derive(Debug)]
pub struct SessionPaths {
    pub open: PathBuf,
    pub baseline: PathBuf,
    pub adj: PathBuf,
    pub original_name: String,
}

impl SessionPaths {
    pub fn for_open(path: &Path) -> Self {
        let original_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image.jpg".into());
        let folder = path.parent().unwrap_or(Path::new("."));
        let matejko = folder.join(".matejko");
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        Self {
            open: path.to_path_buf(),
            baseline: matejko.join(&original_name),
            adj: matejko.join(format!("{stem}.matejko_adj")),
            original_name,
        }
    }

    /// A new version at `copy`. In the session's own folder it shares the
    /// session's baseline, which may belong to an earlier file than the one
    /// that is open. In another folder it gets a baseline of its own in that
    /// folder's `.matejko`, with the same bytes. A different photo's baseline
    /// already there under that name is left alone: the new one takes the
    /// next free name, `<stem>_2.jpg` and so on, and the adj records it.
    pub fn for_copy(session: &SessionPaths, copy: &Path, baseline_bytes: &[u8]) -> Self {
        let folder = copy.parent().unwrap_or(Path::new("."));
        let matejko = folder.join(".matejko");
        let stem = copy
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        let adj = matejko.join(format!("{stem}.matejko_adj"));
        let session_folder = session.open.parent().unwrap_or(Path::new("."));
        if same_folder(folder, session_folder) {
            return Self {
                open: copy.to_path_buf(),
                baseline: session.baseline.clone(),
                adj,
                original_name: session.original_name.clone(),
            };
        }
        let original = Path::new(&session.original_name);
        let original_stem = original
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        let extension = original
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let mut name = session.original_name.clone();
        for n in 2.. {
            let candidate = matejko.join(&name);
            let free = match fs::read(&candidate) {
                Ok(bytes) => bytes == baseline_bytes,
                Err(_) => !candidate.exists(),
            };
            if free {
                break;
            }
            name = format!("{original_stem}_{n}{extension}");
        }
        Self {
            open: copy.to_path_buf(),
            baseline: matejko.join(&name),
            adj,
            original_name: name,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AdjFileRaw {
    version: u32,
    original: String,
    adjustments: AdjBody,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdjBody {
    quarter_turns: u8,
    mirror_horizontal: bool,
    mirror_vertical: bool,
    temperature: f32,
    tint: f32,
    exposure_ev: f32,
    blackpoint: f32,
    shadow: i32,
    highlight: i32,
    brightness: i32,
    contrast: i32,
    vibrance: i32,
    saturation: i32,
    clarity: i32,
    rotation: f32,
    /// Keystone. Older files have none: 0.
    #[serde(default)]
    vertical: i32,
    #[serde(default)]
    horizontal: i32,
    #[serde(default)]
    distortion: i32,
    aspect: String,
    frame: FrameBody,
    jpeg_quality: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct FrameBody {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

pub struct AdjFile {
    pub original: String,
    pub adjustments: Adjustments,
}

impl AdjFile {
    pub fn to_json(&self) -> String {
        let body = AdjFileRaw {
            version: VERSION,
            original: self.original.clone(),
            adjustments: AdjBody {
                quarter_turns: self.adjustments.quarter_turns,
                mirror_horizontal: self.adjustments.mirror_horizontal,
                mirror_vertical: self.adjustments.mirror_vertical,
                temperature: self.adjustments.temperature,
                tint: self.adjustments.tint,
                exposure_ev: self.adjustments.exposure_ev,
                blackpoint: self.adjustments.blackpoint,
                shadow: self.adjustments.shadow.round() as i32,
                highlight: self.adjustments.highlight.round() as i32,
                brightness: self.adjustments.brightness.round() as i32,
                contrast: self.adjustments.contrast.round() as i32,
                vibrance: self.adjustments.vibrance.round() as i32,
                saturation: self.adjustments.saturation.round() as i32,
                clarity: self.adjustments.clarity.round() as i32,
                rotation: self.adjustments.rotation,
                vertical: self.adjustments.vertical.round() as i32,
                horizontal: self.adjustments.horizontal.round() as i32,
                distortion: self.adjustments.distortion.round() as i32,
                aspect: self.adjustments.aspect.label().into(),
                frame: FrameBody {
                    x: self.adjustments.frame.x,
                    y: self.adjustments.frame.y,
                    w: self.adjustments.frame.w,
                    h: self.adjustments.frame.h,
                },
                jpeg_quality: self.adjustments.jpeg_quality,
            },
        };
        serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into())
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: AdjFileRaw = serde_json::from_str(text).map_err(|err| err.to_string())?;
        if raw.version != VERSION {
            return Err(format!("Unknown adjustment version {}.", raw.version));
        }
        let aspect = Aspect::parse(&raw.adjustments.aspect)
            .ok_or_else(|| format!("Unknown aspect {}.", raw.adjustments.aspect))?;
        Ok(Self {
            original: raw.original,
            adjustments: Adjustments {
                quarter_turns: raw.adjustments.quarter_turns % 4,
                mirror_horizontal: raw.adjustments.mirror_horizontal,
                mirror_vertical: raw.adjustments.mirror_vertical,
                temperature: raw.adjustments.temperature,
                tint: raw.adjustments.tint,
                exposure_ev: raw.adjustments.exposure_ev,
                blackpoint: raw.adjustments.blackpoint,
                shadow: raw.adjustments.shadow as f32,
                highlight: raw.adjustments.highlight as f32,
                brightness: raw.adjustments.brightness as f32,
                contrast: raw.adjustments.contrast as f32,
                vibrance: raw.adjustments.vibrance as f32,
                saturation: raw.adjustments.saturation as f32,
                clarity: raw.adjustments.clarity as f32,
                rotation: raw.adjustments.rotation,
                vertical: raw.adjustments.vertical as f32,
                horizontal: raw.adjustments.horizontal as f32,
                distortion: raw.adjustments.distortion as f32,
                aspect,
                frame: Frame {
                    x: raw.adjustments.frame.x,
                    y: raw.adjustments.frame.y,
                    w: raw.adjustments.frame.w,
                    h: raw.adjustments.frame.h,
                },
                jpeg_quality: raw.adjustments.jpeg_quality.clamp(1, 100),
            },
        })
    }
}

pub struct LoadedSession {
    pub paths: SessionPaths,
    pub baseline_bytes: Vec<u8>,
    pub baseline: Image,
    pub adjustments: Adjustments,
    pub estimated_quality: u8,
    pub had_adj: bool,
}

pub fn load_session(open: &Path) -> Result<LoadedSession, String> {
    let bytes = fs::read(open).map_err(|err| err.to_string())?;
    let mut paths = SessionPaths::for_open(open);
    let (baseline_bytes, original_name) = if let Ok(adj_text) = fs::read_to_string(&paths.adj) {
        let adj = AdjFile::parse(&adj_text)?;
        let baseline_path = open
            .parent()
            .unwrap_or(Path::new("."))
            .join(".matejko")
            .join(&adj.original);
        let baseline_bytes = fs::read(&baseline_path).map_err(|_| {
            "This version points at a baseline that is not in .matejko.".to_string()
        })?;
        paths.baseline = baseline_path;
        paths.original_name = adj.original.clone();
        (baseline_bytes, Some(adj))
    } else if paths.baseline.is_file() {
        let baseline_bytes = fs::read(&paths.baseline).map_err(|err| err.to_string())?;
        (baseline_bytes, None)
    } else {
        (bytes.clone(), None)
    };
    let estimated_quality = estimate_jpeg_quality(&baseline_bytes).unwrap_or(90);
    let baseline = decode_jpeg_linear(&baseline_bytes)?;
    let (adjustments, had_adj) = if let Some(adj) = original_name {
        (adj.adjustments, true)
    } else {
        (Adjustments::defaults(estimated_quality), false)
    };
    Ok(LoadedSession {
        paths,
        baseline_bytes,
        baseline,
        adjustments,
        estimated_quality,
        had_adj,
    })
}

pub fn save_over(session: &LoadedSession, adjustments: &Adjustments) -> Result<(), String> {
    write_render(
        &session.paths.open,
        &session.paths.baseline,
        &session.paths.adj,
        &session.paths.original_name,
        &session.baseline_bytes,
        &session.baseline,
        adjustments,
        session.estimated_quality,
    )
}

/// Two paths name the same folder, however they are spelled.
fn same_folder(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

pub fn save_copy(
    session: &LoadedSession,
    copy: &Path,
    adjustments: &Adjustments,
) -> Result<PathBuf, String> {
    let paths = SessionPaths::for_copy(&session.paths, copy, &session.baseline_bytes);
    if let Some(parent) = paths.baseline.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if !paths.baseline.exists() {
        write_atomic(&paths.baseline, &session.baseline_bytes)?;
    }
    write_render(
        &paths.open,
        &paths.baseline,
        &paths.adj,
        &paths.original_name,
        &session.baseline_bytes,
        &session.baseline,
        adjustments,
        session.estimated_quality,
    )?;
    Ok(paths.open)
}

fn write_render(
    dest: &Path,
    baseline_path: &Path,
    adj_path: &Path,
    original_name: &str,
    baseline_bytes: &[u8],
    baseline: &Image,
    adjustments: &Adjustments,
    estimated_quality: u8,
) -> Result<(), String> {
    if let Some(parent) = baseline_path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if !baseline_path.exists() {
        write_atomic(baseline_path, baseline_bytes)?;
    }
    let rendered = if adjustments.is_identity(estimated_quality) {
        baseline_bytes.to_vec()
    } else {
        let image = crate::edit::pipeline::run(
            baseline,
            adjustments,
            &crate::edit::pipeline::RunOptions {
                radius_scale: 1.0,
                dither: true,
                crop: true,
            },
        );
        let encoded = encode_jpeg_444(&image, adjustments.jpeg_quality)?;
        patch_exif(&encoded, baseline_bytes, image.width, image.height)
    };
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    write_atomic(dest, &rendered)?;
    let adj = AdjFile {
        original: original_name.to_string(),
        adjustments: adjustments.clone(),
    };
    write_atomic(adj_path, adj.to_json().as_bytes())?;
    Ok(())
}

/// Writes `bytes` to a temporary file beside `path`, flushes it to disk, then
/// renames it over `path`. A crash or a full disk leaves either the old file
/// or the new one, never half of it, and the folder watcher never sees a
/// partly written photo. The temporary name ends in `.matejko-tmp`, which
/// the browsing list does not show.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let temp = path.with_file_name(format!(".{name}.{}.matejko-tmp", std::process::id()));
    let written = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    written.map_err(|err| {
        let _ = fs::remove_file(&temp);
        err.to_string()
    })
}

/// Decodes with EXIF orientation applied, as the viewer does, so the
/// baseline is upright. The rendered file is written with orientation 1.
pub fn decode_jpeg_linear(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| err.to_string())?
        .into_decoder()
        .map_err(|err| format!("Couldn't decode this file. {err}"))?;
    decoder
        .set_limits(Limits::no_limits())
        .map_err(|err| err.to_string())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|err| format!("Couldn't decode this file. {err}"))?;
    image.apply_orientation(orientation);
    let rgba = image.into_rgba8();
    Ok(Image::from_srgb_rgba(rgba.width(), rgba.height(), rgba.as_raw()))
}

pub fn encode_jpeg_444(image: &Image, quality: u8) -> Result<Vec<u8>, String> {
    let rgba = image.to_rgba8(true);
    let mut rgb = Vec::with_capacity((rgba.len() / 4) * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    let mut output = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut output, quality.clamp(1, 100));
    encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_4_4);
    encoder
        .encode(
            &rgb,
            image.width as u16,
            image.height as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|err| err.to_string())?;
    Ok(output)
}

/// IJG 1–100 quality whose luminance table is closest to the file's.
pub fn estimate_jpeg_quality(bytes: &[u8]) -> Option<u8> {
    let table = luminance_dqt(bytes)?;
    let mut best_q = 90u8;
    let mut best_sum = u32::MAX;
    for q in 1..=100u8 {
        let candidate = ijg_luma_table(q);
        let sum: u32 = table
            .iter()
            .zip(candidate.iter())
            .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs())
            .sum();
        if sum < best_sum || (sum == best_sum && q > best_q) {
            best_sum = sum;
            best_q = q;
        }
    }
    Some(best_q)
}

fn ijg_luma_table(quality: u8) -> [u8; 64] {
    const STD: [u8; 64] = [
        16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69,
        56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81,
        104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
    ];
    let scale = if quality < 50 {
        5000 / quality as i32
    } else {
        200 - 2 * quality as i32
    };
    let mut table = [0u8; 64];
    for (slot, value) in table.iter_mut().zip(STD) {
        let entry = (value as i32 * scale + 50) / 100;
        *slot = entry.clamp(1, 255) as u8;
    }
    table
}

fn luminance_dqt(bytes: &[u8]) -> Option<[u8; 64]> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 4 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let marker = bytes[i];
        i += 1;
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if i + 1 >= bytes.len() {
            return None;
        }
        let len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
        if len < 2 || i + len > bytes.len() {
            return None;
        }
        if marker == 0xDB {
            let mut cursor = i + 2;
            let end = i + len;
            while cursor < end {
                let pq = bytes[cursor];
                cursor += 1;
                let precision = pq >> 4;
                let table_id = pq & 0x0F;
                let count = if precision == 0 { 64 } else { 128 };
                if cursor + count > end {
                    break;
                }
                if table_id == 0 && precision == 0 {
                    let mut table = [0u8; 64];
                    table.copy_from_slice(&bytes[cursor..cursor + 64]);
                    return Some(table);
                }
                cursor += count;
            }
        }
        if marker == 0xDA {
            return None;
        }
        i += len;
    }
    None
}

fn patch_exif(encoded: &[u8], baseline: &[u8], width: u32, height: u32) -> Vec<u8> {
    let Some(app1) = exif_app1(baseline) else {
        return encoded.to_vec();
    };
    let mut app1 = app1.to_vec();
    patch_tiff(&mut app1, width, height);
    let stripped = strip_app1(encoded);
    let mut out = Vec::with_capacity(stripped.len() + app1.len());
    out.extend_from_slice(&stripped[..2]);
    out.extend_from_slice(&app1);
    out.extend_from_slice(&stripped[2..]);
    out
}

fn exif_app1(bytes: &[u8]) -> Option<&[u8]> {
    let mut i = 2;
    while i + 4 < bytes.len() && bytes[i] == 0xFF {
        let marker = bytes[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            break;
        }
        if marker == 0xE1 && bytes[i + 4..].starts_with(b"Exif\0\0") {
            return Some(&bytes[i..i + 2 + len]);
        }
        i += 2 + len;
    }
    None
}

fn strip_app1(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() < 2 {
        return bytes.to_vec();
    }
    let mut out = bytes[..2].to_vec();
    let mut i = 2;
    while i + 4 < bytes.len() && bytes[i] == 0xFF {
        let marker = bytes[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            out.extend_from_slice(&bytes[i..]);
            return out;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            break;
        }
        let keep = marker != 0xE1 || !bytes[i + 4..].starts_with(b"Exif\0\0");
        if keep {
            out.extend_from_slice(&bytes[i..i + 2 + len]);
        }
        i += 2 + len;
    }
    if i < bytes.len() {
        out.extend_from_slice(&bytes[i..]);
    }
    out
}

fn patch_tiff(app1: &mut Vec<u8>, width: u32, height: u32) {
    if app1.len() < 16 || &app1[4..10] != b"Exif\0\0" {
        return;
    }
    let tiff = 10usize;
    if app1.len() < tiff + 8 {
        return;
    }
    let le = app1[tiff] == b'I';
    let read_u16 = |buf: &[u8], at: usize| -> u16 {
        if le {
            u16::from_le_bytes([buf[at], buf[at + 1]])
        } else {
            u16::from_be_bytes([buf[at], buf[at + 1]])
        }
    };
    let read_u32 = |buf: &[u8], at: usize| -> u32 {
        if le {
            u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
        } else {
            u32::from_be_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
        }
    };
    let write_u16 = |buf: &mut [u8], at: usize, value: u16| {
        let bytes = if le { value.to_le_bytes() } else { value.to_be_bytes() };
        buf[at] = bytes[0];
        buf[at + 1] = bytes[1];
    };
    let ifd = tiff + read_u32(app1, tiff + 4) as usize;
    if ifd + 2 > app1.len() {
        return;
    }
    let count = read_u16(app1, ifd) as usize;
    let mut exif_offset = None;
    for entry in 0..count {
        let at = ifd + 2 + entry * 12;
        if at + 12 > app1.len() {
            return;
        }
        let tag = read_u16(app1, at);
        let typ = read_u16(app1, at + 2);
        let n = read_u32(app1, at + 4);
        match tag {
            0x0112 if typ == 3 => write_u16(app1, at + 8, 1),
            0x0100 | 0xA002 => write_dimension(app1, at, typ, width, le),
            0x0101 | 0xA003 => write_dimension(app1, at, typ, height, le),
            0x8769 => exif_offset = Some(read_u32(app1, at + 8)),
            0x0131 => {
                let note = software_value(app1, tiff, at, typ, n, le);
                write_software(app1, tiff, at, &note, le);
            }
            _ => {}
        }
    }
    if let Some(offset) = exif_offset {
        let sub = tiff + offset as usize;
        if sub + 2 <= app1.len() {
            let sub_count = read_u16(app1, sub) as usize;
            for entry in 0..sub_count {
                let at = sub + 2 + entry * 12;
                if at + 12 > app1.len() {
                    break;
                }
                let tag = read_u16(app1, at);
                let typ = read_u16(app1, at + 2);
                match tag {
                    0xA002 => write_dimension(app1, at, typ, width, le),
                    0xA003 => write_dimension(app1, at, typ, height, le),
                    _ => {}
                }
            }
        }
    }
}

fn write_dimension(buf: &mut [u8], at: usize, typ: u16, value: u32, le: bool) {
    if typ == 3 {
        let bytes = if le {
            (value as u16).to_le_bytes()
        } else {
            (value as u16).to_be_bytes()
        };
        buf[at + 8] = bytes[0];
        buf[at + 9] = bytes[1];
    } else if typ == 4 {
        let bytes = if le { value.to_le_bytes() } else { value.to_be_bytes() };
        buf[at + 8..at + 12].copy_from_slice(&bytes);
    }
}

fn software_value(buf: &[u8], tiff: usize, at: usize, typ: u16, count: u32, le: bool) -> String {
    if typ != 2 {
        return SOFTWARE_NOTE.into();
    }
    let bytes = if count <= 4 {
        &buf[at + 8..at + 8 + count as usize]
    } else {
        let offset = if le {
            u32::from_le_bytes([buf[at + 8], buf[at + 9], buf[at + 10], buf[at + 11]])
        } else {
            u32::from_be_bytes([buf[at + 8], buf[at + 9], buf[at + 10], buf[at + 11]])
        } as usize;
        let start = tiff + offset;
        let end = (start + count as usize).min(buf.len());
        if start >= buf.len() {
            return SOFTWARE_NOTE.into();
        }
        &buf[start..end]
    };
    let text = String::from_utf8_lossy(bytes).trim_end_matches('\0').to_string();
    if text.contains(SOFTWARE_NOTE) {
        text
    } else if text.is_empty() {
        SOFTWARE_NOTE.into()
    } else {
        format!("{text}, {SOFTWARE_NOTE}")
    }
}

fn write_software(app1: &mut Vec<u8>, tiff: usize, at: usize, text: &str, le: bool) {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    let count = bytes.len() as u32;
    let write_u16 = |buf: &mut Vec<u8>, pos: usize, value: u16| {
        let raw = if le { value.to_le_bytes() } else { value.to_be_bytes() };
        buf[pos] = raw[0];
        buf[pos + 1] = raw[1];
    };
    let write_u32 = |buf: &mut Vec<u8>, pos: usize, value: u32| {
        let raw = if le { value.to_le_bytes() } else { value.to_be_bytes() };
        buf[pos..pos + 4].copy_from_slice(&raw);
    };
    write_u16(app1, at + 2, 2);
    write_u32(app1, at + 4, count);
    if count <= 4 {
        for i in 0..4 {
            app1[at + 8 + i] = bytes.get(i).copied().unwrap_or(0);
        }
        return;
    }
    let offset = app1.len() - tiff;
    write_u32(app1, at + 8, offset as u32);
    app1.extend_from_slice(&bytes);
    let segment_len = (app1.len() - 2) as u16;
    app1[2] = (segment_len >> 8) as u8;
    app1[3] = (segment_len & 0xFF) as u8;
}

#[allow(dead_code)]
fn _decode_used(c: f32) -> f32 {
    decode(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::{ExtendedColorType, ImageEncoder};

    #[test]
    fn an_atomic_write_replaces_the_file_and_leaves_no_temporary_behind() {
        let dir = std::env::temp_dir().join(format!("matejko-atomic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo.jpg");
        fs::write(&path, b"old bytes").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        // A new file works too.
        write_atomic(&dir.join("fresh.matejko_adj"), b"{}").unwrap();
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|name| !name.ends_with(".matejko-tmp")), "{names:?}");
        assert_eq!(names.len(), 2, "{names:?}");
        // A folder that is not there fails cleanly.
        assert!(write_atomic(&dir.join("missing").join("x.jpg"), b"x").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    fn tagged_jpeg() -> Vec<u8> {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .write_image(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255], 2, 2, ExtendedColorType::Rgb8)
            .unwrap();
        inject_exif(&jpeg)
    }

    fn inject_exif(jpeg: &[u8]) -> Vec<u8> {
        // Little-endian TIFF, IFD with Orientation, Software, GPSLatitude, and pixel size.
        let software = b"TestCam\0";
        let mut tiff = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        let entries: u16 = 5;
        tiff.extend_from_slice(&entries.to_le_bytes());
        // ImageWidth
        push_entry(&mut tiff, 0x0100, 3, 1, 2);
        // ImageLength
        push_entry(&mut tiff, 0x0101, 3, 1, 2);
        // Orientation = 6
        push_entry(&mut tiff, 0x0112, 3, 1, 6);
        // Software, offset filled after the IFD
        let software_entry_at = tiff.len();
        push_entry(&mut tiff, 0x0131, 2, software.len() as u32, 0);
        // GPS IFD pointer
        let gps_entry_at = tiff.len();
        push_entry(&mut tiff, 0x8825, 4, 1, 0);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let software_offset = tiff.len() as u32;
        tiff.extend_from_slice(software);
        let gps_offset = tiff.len() as u32;
        tiff.extend_from_slice(&1u16.to_le_bytes());
        push_entry(&mut tiff, 0x0002, 5, 3, gps_offset + 2 + 12 + 4);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        for value in [50u32, 1, 0, 1, 0, 1] {
            tiff.extend_from_slice(&value.to_le_bytes());
        }
        write_u32_at(&mut tiff, software_entry_at + 8, software_offset);
        write_u32_at(&mut tiff, gps_entry_at + 8, gps_offset);
        let mut app1 = vec![0xFF, 0xE1, 0, 0];
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&tiff);
        let len = (app1.len() - 2) as u16;
        app1[2] = (len >> 8) as u8;
        app1[3] = (len & 0xFF) as u8;
        let mut out = Vec::new();
        out.extend_from_slice(&jpeg[..2]);
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    fn push_entry(buf: &mut Vec<u8>, tag: u16, typ: u16, count: u32, value: u32) {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u32_at(buf: &mut [u8], at: usize, value: u32) {
        buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn quality_estimate_matches_the_writer() {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 85)
            .write_image(&[10, 20, 30], 1, 1, ExtendedColorType::Rgb8)
            .unwrap();
        assert_eq!(estimate_jpeg_quality(&jpeg), Some(85));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("matejko-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_baseline_is_decoded_upright() {
        // 4 × 2 pixels stored, orientation 6: upright it is 2 × 4.
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .write_image(&[128; 4 * 2 * 3], 4, 2, ExtendedColorType::Rgb8)
            .unwrap();
        let image = decode_jpeg_linear(&inject_exif(&jpeg)).unwrap();
        assert_eq!((image.width, image.height), (2, 4));
    }

    #[test]
    fn a_copy_of_a_copy_shares_the_one_baseline() {
        let dir = temp_dir("copies");
        let path = dir.join("photo.jpg");
        fs::write(&path, tagged_jpeg()).unwrap();
        let mut changed = load_session(&path).unwrap().adjustments;
        changed.exposure_ev = 0.5;
        save_copy(&load_session(&path).unwrap(), &dir.join("photo_v2.jpg"), &changed).unwrap();
        let second = load_session(&dir.join("photo_v2.jpg")).unwrap();
        assert_eq!(second.paths.original_name, "photo.jpg");
        save_copy(&second, &dir.join("photo_v3.jpg"), &changed).unwrap();
        let mut jpegs: Vec<String> = fs::read_dir(dir.join(".matejko"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".jpg"))
            .collect();
        jpegs.sort();
        assert_eq!(jpegs, ["photo.jpg"]);
        let third = AdjFile::parse(
            &fs::read_to_string(dir.join(".matejko").join("photo_v3.matejko_adj")).unwrap(),
        )
        .unwrap();
        assert_eq!(third.original, "photo.jpg");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_adj_from_before_keystone_still_opens() {
        let mut adj = AdjFile {
            original: "a.jpg".into(),
            adjustments: Adjustments::defaults(90),
        };
        adj.adjustments.vertical = 12.0;
        adj.adjustments.horizontal = -7.0;
        let back = AdjFile::parse(&adj.to_json()).unwrap();
        assert_eq!((back.adjustments.vertical, back.adjustments.horizontal), (12.0, -7.0));
        let old = adj
            .to_json()
            .lines()
            .filter(|line| !line.contains("\"vertical\"") && !line.contains("\"horizontal\""))
            .collect::<Vec<_>>()
            .join("\n");
        let back = AdjFile::parse(&old).unwrap();
        assert_eq!((back.adjustments.vertical, back.adjustments.horizontal), (0.0, 0.0));
    }

    #[test]
    fn a_copy_in_another_folder_takes_its_baseline_along() {
        let dir = temp_dir("elsewhere");
        let home = dir.join("holiday");
        let away = dir.join("best");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&away).unwrap();
        let path = home.join("photo.jpg");
        let original = tagged_jpeg();
        fs::write(&path, &original).unwrap();
        let mut changed = load_session(&path).unwrap().adjustments;
        changed.exposure_ev = 0.5;
        save_copy(&load_session(&path).unwrap(), &away.join("pick.jpg"), &changed).unwrap();
        // The copy's folder holds everything a later session needs.
        assert_eq!(fs::read(away.join(".matejko").join("photo.jpg")).unwrap(), original);
        let later = load_session(&away.join("pick.jpg")).unwrap();
        assert_eq!(later.paths.original_name, "photo.jpg");
        assert_eq!(later.adjustments.exposure_ev, 0.5);
        // A second copy there reuses the same baseline.
        save_copy(&later, &away.join("pick_2.jpg"), &changed).unwrap();
        let baselines = |folder: &Path| {
            let mut names: Vec<String> = fs::read_dir(folder.join(".matejko"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".jpg"))
                .collect();
            names.sort();
            names
        };
        assert_eq!(baselines(&away), ["photo.jpg"]);

        // Another photo's baseline under the same name is not touched.
        let crowded = dir.join("crowded");
        fs::create_dir_all(crowded.join(".matejko")).unwrap();
        fs::write(crowded.join(".matejko").join("photo.jpg"), b"someone else").unwrap();
        save_copy(&load_session(&path).unwrap(), &crowded.join("pick.jpg"), &changed).unwrap();
        assert_eq!(fs::read(crowded.join(".matejko").join("photo.jpg")).unwrap(), b"someone else");
        assert_eq!(fs::read(crowded.join(".matejko").join("photo_2.jpg")).unwrap(), original);
        let later = load_session(&crowded.join("pick.jpg")).unwrap();
        assert_eq!(later.paths.original_name, "photo_2.jpg");
        // Copies saved elsewhere write nothing beside the photo itself.
        assert!(!home.join(".matejko").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_version_is_refused() {
        let text = r#"{"version":2,"original":"a.jpg","adjustments":{}}"#;
        assert!(AdjFile::parse(text).is_err());
    }

    #[test]
    fn identity_copies_bytes_and_a_change_keeps_gps_and_notes_software() {
        let dir = std::env::temp_dir().join(format!("matejko-edit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo.jpg");
        let original = tagged_jpeg();
        fs::write(&path, &original).unwrap();
        let session = load_session(&path).unwrap();
        save_over(&session, &session.adjustments).unwrap();
        let copied = fs::read(&path).unwrap();
        assert_eq!(copied, original);
        assert!(dir.join(".matejko").join("photo.jpg").is_file());

        let mut changed = session.adjustments.clone();
        changed.exposure_ev = 0.5;
        save_over(&session, &changed).unwrap();
        let rendered = fs::read(&path).unwrap();
        assert_ne!(rendered, original);
        let exif = exif::Reader::new()
            .read_from_container(&mut Cursor::new(&rendered))
            .unwrap();
        let orientation = exif
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .unwrap();
        assert_eq!(orientation.value.get_uint(0), Some(1));
        let software = exif
            .get_field(exif::Tag::Software, exif::In::PRIMARY)
            .unwrap()
            .display_value()
            .to_string();
        assert!(software.contains("edited with Matejko"), "{software}");
        assert!(exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY).is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
