//! Scene-linear RGB, tightly packed binary32.

use rayon::prelude::*;

/// Floats per parallel work item: 4096 pixels.
pub const CHUNK: usize = 3 * 4096;

#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<f32>,
}

impl Image {
    #[cfg(test)]
    pub fn new(width: u32, height: u32, rgb: Vec<f32>) -> Self {
        let mut image = Self { width, height, rgb };
        image.sanitize();
        image
    }

    /// `rgba` holds `width × height` pixels. Alpha is ignored.
    pub fn from_srgb_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        const PIXELS: usize = CHUNK / 3;
        let table = decode_table();
        let mut rgb = vec![0.0; (width as usize) * (height as usize) * 3];
        rgb.par_chunks_mut(CHUNK)
            .zip(rgba.par_chunks(PIXELS * 4))
            .for_each(|(out, src)| {
                for (dst, px) in out.chunks_exact_mut(3).zip(src.chunks_exact(4)) {
                    dst[0] = table[px[0] as usize];
                    dst[1] = table[px[1] as usize];
                    dst[2] = table[px[2] as usize];
                }
            });
        Self { width, height, rgb }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [f32; 3] {
        let i = ((y as usize) * (self.width as usize) + x as usize) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }

    #[cfg(test)]
    fn sanitize(&mut self) {
        for v in &mut self.rgb {
            *v = finite_or_zero(*v);
        }
    }

    pub fn map_channels(&mut self, f: impl Fn(f32) -> f32 + Sync) {
        self.rgb.par_chunks_mut(CHUNK).for_each(|chunk| {
            for v in chunk {
                *v = finite_or_zero(f(*v));
            }
        });
    }

    /// Runs `f` on each `[r, g, b]` in place. `f` writes finite values.
    pub fn for_each_pixel(&mut self, f: impl Fn(&mut [f32]) + Sync) {
        self.rgb.par_chunks_mut(CHUNK).for_each(|chunk| {
            for px in chunk.chunks_exact_mut(3) {
                f(px);
            }
        });
    }

    /// Area-average in linear light so the long side is at most `long_side`.
    pub fn downsample_long_side(&self, long_side: u32) -> Self {
        let long = self.width.max(self.height);
        if long <= long_side || long_side == 0 {
            return self.clone();
        }
        let scale = long_side as f32 / long as f32;
        let width = ((self.width as f32) * scale).round().max(1.0) as u32;
        let height = ((self.height as f32) * scale).round().max(1.0) as u32;
        let mut rgb = vec![0.0; (width as usize) * (height as usize) * 3];
        rgb.par_chunks_mut(width as usize * 3)
            .enumerate()
            .for_each(|(y, row)| {
                let y0 = (y as f32) / height as f32 * self.height as f32;
                let y1 = ((y + 1) as f32) / height as f32 * self.height as f32;
                for (x, px) in row.chunks_exact_mut(3).enumerate() {
                    let x0 = (x as f32) / width as f32 * self.width as f32;
                    let x1 = ((x + 1) as f32) / width as f32 * self.width as f32;
                    px.copy_from_slice(&self.area_average(x0, x1, y0, y1));
                }
            });
        Self { width, height, rgb }
    }

    fn area_average(&self, x0: f32, x1: f32, y0: f32, y1: f32) -> [f32; 3] {
        let mut acc = [0.0; 3];
        let mut weight = 0.0;
        let x_start = x0.floor().max(0.0) as u32;
        let y_start = y0.floor().max(0.0) as u32;
        let x_end = x1.ceil().min(self.width as f32) as u32;
        let y_end = y1.ceil().min(self.height as f32) as u32;
        for y in y_start..y_end {
            let yb0 = (y as f32).max(y0);
            let yb1 = ((y + 1) as f32).min(y1);
            if yb1 <= yb0 {
                continue;
            }
            for x in x_start..x_end {
                let xb0 = (x as f32).max(x0);
                let xb1 = ((x + 1) as f32).min(x1);
                if xb1 <= xb0 {
                    continue;
                }
                let w = (xb1 - xb0) * (yb1 - yb0);
                let px = self.pixel(x, y);
                acc[0] += px[0] * w;
                acc[1] += px[1] * w;
                acc[2] += px[2] * w;
                weight += w;
            }
        }
        if weight <= 0.0 {
            return [0.0; 3];
        }
        [acc[0] / weight, acc[1] / weight, acc[2] / weight]
    }

    pub fn to_rgba8(&self, dither: bool) -> Vec<u8> {
        const PIXELS: usize = CHUNK / 3;
        let mut rgba = vec![0u8; self.rgb.len() / 3 * 4];
        rgba.par_chunks_mut(PIXELS * 4)
            .zip(self.rgb.par_chunks(PIXELS * 3))
            .enumerate()
            .for_each(|(chunk, (out, src))| {
                for (j, (dst, px)) in out.chunks_exact_mut(4).zip(src.chunks_exact(3)).enumerate() {
                    let noise = if dither { triangular(chunk * PIXELS + j) } else { 0.0 };
                    dst[0] = quantize(px[0], noise);
                    dst[1] = quantize(px[1], noise);
                    dst[2] = quantize(px[2], noise);
                    dst[3] = 255;
                }
            });
        rgba
    }

    pub fn histogram_region(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> [Vec<u32>; 3] {
        let x1 = x1.min(self.width);
        let y1 = y1.min(self.height);
        let empty = || [vec![0u32; 256], vec![0u32; 256], vec![0u32; 256]];
        (y0..y1)
            .into_par_iter()
            .fold(empty, |mut bins, y| {
                for x in x0..x1 {
                    let px = self.pixel(x, y);
                    for (channel, bin) in px.iter().zip(bins.iter_mut()) {
                        bin[code_of(*channel)] += 1;
                    }
                }
                bins
            })
            .reduce(empty, |mut a, b| {
                for (left, right) in a.iter_mut().zip(b.iter()) {
                    for (l, r) in left.iter_mut().zip(right) {
                        *l += r;
                    }
                }
                a
            })
    }
}

fn finite_or_zero(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        0.0
    }
}

pub fn decode(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn encode(l: f32) -> f32 {
    if l <= 0.0031308 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// The linear value of each 8-bit sRGB code.
fn decode_table() -> &'static [f32; 256] {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|code| decode(code as f32 / 255.0)))
}

/// `thresholds[c]` is the smallest linear value `quantize(_, 0.0)` puts
/// above code `c`. Found by bisecting on the float's bits, so it matches
/// `quantize` exactly, rounding and all.
fn code_thresholds() -> &'static [f32; 255] {
    static TABLE: std::sync::OnceLock<[f32; 255]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|code| {
            // Positive floats sort like their bits.
            let (mut lo, mut hi) = (0u32, 1.0f32.to_bits());
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if quantize(f32::from_bits(mid), 0.0) as usize > code {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            f32::from_bits(hi)
        })
    })
}

/// Buckets over linear 0…1 for `code_of`. The steepest part of the sRGB
/// curve, at black, has 12.92 × 255 ≈ 3294 codes per unit, fewer than the
/// buckets, so a bucket never spans more than one code boundary.
const CODE_BUCKETS: usize = 4096;

/// The code at the start of each bucket.
fn bucket_codes() -> &'static [u8; CODE_BUCKETS] {
    static TABLE: std::sync::OnceLock<[u8; CODE_BUCKETS]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let thresholds = code_thresholds();
        std::array::from_fn(|bucket| {
            let start = bucket as f32 / CODE_BUCKETS as f32;
            thresholds.partition_point(|threshold| *threshold <= start) as u8
        })
    })
}

/// `quantize(linear, 0.0)` without the `powf`: a table read, then a step
/// past each code boundary inside the bucket.
fn code_of(linear: f32) -> usize {
    let thresholds = code_thresholds();
    // A negative or NaN value lands in bucket 0, and above 1 in the last.
    let bucket = ((linear * CODE_BUCKETS as f32) as usize).min(CODE_BUCKETS - 1);
    let mut code = bucket_codes()[bucket] as usize;
    while code < thresholds.len() && linear >= thresholds[code] {
        code += 1;
    }
    code
}

pub fn quantize(linear: f32, noise: f32) -> u8 {
    let encoded = encode(linear.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let code = (encoded * 255.0 + noise).round();
    code.clamp(0.0, 255.0) as u8
}

/// Sum of two draws from −0.5 to +0.5. Deterministic so tests can ignore it
/// by passing `dither: false`.
fn triangular(index: usize) -> f32 {
    let a = hash(index) - 0.5;
    let b = hash(index.wrapping_mul(3).wrapping_add(1)) - 0.5;
    a + b
}

fn hash(n: usize) -> f32 {
    let mut x = n.wrapping_mul(0x9E3779B1) ^ 0xA5A5A5A5;
    x ^= x >> 16;
    (x & 0xFFFF) as f32 / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decode_table_is_decode() {
        let rgba: Vec<u8> = (0..=255u8).flat_map(|c| [c, c, c, 9]).collect();
        let image = Image::from_srgb_rgba(256, 1, &rgba);
        for code in 0..256 {
            assert_eq!(image.rgb[code * 3], decode(code as f32 / 255.0), "code {code}");
        }
    }

    #[test]
    fn code_of_matches_quantize_everywhere() {
        // Every threshold and its neighbours, where rounding decides.
        for threshold in code_thresholds() {
            let bits = threshold.to_bits();
            for v in [bits - 1, bits, bits + 1].map(f32::from_bits) {
                assert_eq!(code_of(v), quantize(v, 0.0) as usize, "at {v}");
            }
        }
        // A dense sweep, and values outside 0 to 1.
        for i in 0..=200_000 {
            let v = i as f32 / 200_000.0;
            assert_eq!(code_of(v), quantize(v, 0.0) as usize, "at {v}");
        }
        for v in [-1.0, -0.0, 1.0, 1.5, f32::INFINITY, f32::NAN] {
            assert_eq!(code_of(v), quantize(v, 0.0) as usize, "at {v}");
        }
    }
}
