//! The fixed edit pipeline. Each run starts from the baseline.

use rayon::prelude::*;

use crate::edit::image::{decode, encode, Image, CHUNK};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aspect {
    Original,
    Free,
    R3x2,
    R2x3,
    R4x3,
    R3x4,
    R1x1,
    R4x5,
    R5x4,
    R16x9,
    R9x16,
}

impl Aspect {
    pub const CYCLE: [Aspect; 11] = [
        Aspect::Original,
        Aspect::Free,
        Aspect::R3x2,
        Aspect::R2x3,
        Aspect::R4x3,
        Aspect::R3x4,
        Aspect::R1x1,
        Aspect::R4x5,
        Aspect::R5x4,
        Aspect::R16x9,
        Aspect::R9x16,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Aspect::Original => "original",
            Aspect::Free => "free",
            Aspect::R3x2 => "3:2",
            Aspect::R2x3 => "2:3",
            Aspect::R4x3 => "4:3",
            Aspect::R3x4 => "3:4",
            Aspect::R1x1 => "1:1",
            Aspect::R4x5 => "4:5",
            Aspect::R5x4 => "5:4",
            Aspect::R16x9 => "16:9",
            Aspect::R9x16 => "9:16",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::CYCLE.into_iter().find(|aspect| aspect.label() == text)
    }

    pub fn next(self) -> Self {
        let i = Self::CYCLE
            .iter()
            .position(|aspect| *aspect == self)
            .unwrap_or(0);
        Self::CYCLE[(i + 1) % Self::CYCLE.len()]
    }

    /// Numbered ratios swap sides under a clockwise quarter turn.
    pub fn turned_clockwise(self) -> Self {
        match self {
            Aspect::R3x2 => Aspect::R2x3,
            Aspect::R2x3 => Aspect::R3x2,
            Aspect::R4x3 => Aspect::R3x4,
            Aspect::R3x4 => Aspect::R4x3,
            Aspect::R4x5 => Aspect::R5x4,
            Aspect::R5x4 => Aspect::R4x5,
            Aspect::R16x9 => Aspect::R9x16,
            Aspect::R9x16 => Aspect::R16x9,
            other => other,
        }
    }

    fn ratio(self, photo_w: f32, photo_h: f32) -> Option<f32> {
        match self {
            Aspect::Original => Some(photo_w / photo_h.max(1.0)),
            Aspect::Free => None,
            Aspect::R3x2 => Some(3.0 / 2.0),
            Aspect::R2x3 => Some(2.0 / 3.0),
            Aspect::R4x3 => Some(4.0 / 3.0),
            Aspect::R3x4 => Some(3.0 / 4.0),
            Aspect::R1x1 => Some(1.0),
            Aspect::R4x5 => Some(4.0 / 5.0),
            Aspect::R5x4 => Some(5.0 / 4.0),
            Aspect::R16x9 => Some(16.0 / 9.0),
            Aspect::R9x16 => Some(9.0 / 16.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Adjustments {
    pub quarter_turns: u8,
    pub mirror_horizontal: bool,
    pub mirror_vertical: bool,
    pub temperature: f32,
    pub tint: f32,
    pub exposure_ev: f32,
    pub blackpoint: f32,
    pub shadow: f32,
    pub highlight: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub vibrance: f32,
    pub saturation: f32,
    pub clarity: f32,
    pub rotation: f32,
    pub aspect: Aspect,
    pub frame: Frame,
    pub jpeg_quality: u8,
}

impl Adjustments {
    pub fn defaults(jpeg_quality: u8) -> Self {
        Self {
            quarter_turns: 0,
            mirror_horizontal: false,
            mirror_vertical: false,
            temperature: 0.0,
            tint: 0.0,
            exposure_ev: 0.0,
            blackpoint: 0.0,
            shadow: 0.0,
            highlight: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            clarity: 0.0,
            rotation: 0.0,
            aspect: Aspect::Original,
            frame: Frame::default(),
            jpeg_quality,
        }
    }

    pub fn tone_is_default(&self) -> bool {
        self.temperature == 0.0
            && self.tint == 0.0
            && self.exposure_ev == 0.0
            && self.blackpoint == 0.0
            && self.shadow == 0.0
            && self.highlight == 0.0
            && self.brightness == 0.0
            && self.contrast == 0.0
            && self.vibrance == 0.0
            && self.saturation == 0.0
            && self.clarity == 0.0
    }

    pub fn geometry_is_default(&self) -> bool {
        self.quarter_turns == 0
            && !self.mirror_horizontal
            && !self.mirror_vertical
            && self.rotation == 0.0
            && self.aspect == Aspect::Original
            && self.frame == Frame::default()
    }

    pub fn is_identity(&self, estimated_quality: u8) -> bool {
        self.tone_is_default()
            && self.geometry_is_default()
            && self.jpeg_quality == estimated_quality
    }

    pub fn turn_clockwise(&mut self, photo_w: f32, photo_h: f32) {
        self.quarter_turns = (self.quarter_turns + 1) % 4;
        self.aspect = self.aspect.turned_clockwise();
        let frame = self.frame;
        self.frame = Frame {
            x: 1.0 - frame.y - frame.h,
            y: frame.x,
            w: frame.h,
            h: frame.w,
        };
        let _ = (photo_w, photo_h);
    }

    pub fn cycle_aspect(&mut self, oriented_w: f32, oriented_h: f32) {
        self.aspect = self.aspect.next();
        self.frame = fit_aspect(self.aspect, self.rotation, oriented_w, oriented_h);
    }
}

/// Which controls a compare hold substitutes. Geometry always stays current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageControls {
    #[allow(dead_code)]
    Current,
    /// All defaults.
    Original,
    /// Original tone, current geometry.
    OriginalFramed,
    /// Last save, tone and geometry.
    Saved,
    /// Last-save tone, current geometry.
    SavedFramed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guide {
    None,
    Thirds,
    Center,
    Pointer,
}

impl Guide {
    pub fn next(self) -> Self {
        match self {
            Guide::None => Guide::Thirds,
            Guide::Thirds => Guide::Center,
            Guide::Center => Guide::Pointer,
            Guide::Pointer => Guide::None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Guide::None => "None",
            Guide::Thirds => "Thirds",
            Guide::Center => "Center",
            Guide::Pointer => "Pointer",
        }
    }
}

pub struct RunOptions {
    /// Multiply shadow, highlight, and clarity radii by this. The drag image
    /// uses drag width / full width. A full run uses 1.
    pub radius_scale: f32,
    pub dither: bool,
    /// Save crops to the frame. The on-screen picture keeps the usable
    /// rectangle so the margin outside the frame can be dimmed.
    pub crop: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            radius_scale: 1.0,
            dither: true,
            crop: true,
        }
    }
}

pub fn run(baseline: &Image, adj: &Adjustments, options: &RunOptions) -> Image {
    let mut image = orient(baseline, adj);
    if adj.temperature != 0.0 || adj.tint != 0.0 {
        white_balance(&mut image, adj.temperature, adj.tint);
    }
    if adj.exposure_ev != 0.0 {
        let gain = 2.0_f32.powf(adj.exposure_ev);
        image.map_channels(|v| v * gain);
    }
    if adj.blackpoint != 0.0 {
        blackpoint(&mut image, adj.blackpoint);
    }
    if adj.shadow != 0.0 || adj.highlight != 0.0 {
        image = shadows_highlights(&image, adj.shadow, adj.highlight);
    }
    let encoded = adj.brightness != 0.0
        || adj.contrast != 0.0
        || adj.vibrance != 0.0
        || adj.saturation != 0.0
        || adj.clarity != 0.0;
    if encoded {
        encode_inplace(&mut image);
        if adj.brightness != 0.0 {
            brightness(&mut image, adj.brightness);
        }
        if adj.contrast != 0.0 {
            contrast(&mut image, adj.contrast);
        }
        if adj.vibrance != 0.0 {
            vibrance(&mut image, adj.vibrance);
        }
        if adj.saturation != 0.0 {
            saturation(&mut image, adj.saturation);
        }
        if adj.clarity != 0.0 {
            image = clarity(&image, adj.clarity, options.radius_scale);
        }
        decode_inplace(&mut image);
    }
    let frame = if options.crop {
        adj.frame
    } else {
        Frame::default()
    };
    if adj.rotation != 0.0 || frame != Frame::default() {
        image = resample_frame(&image, adj.rotation, frame);
    }
    let _ = options.dither;
    image
}

fn encode_inplace(image: &mut Image) {
    image.map_channels(|v| {
        let encoded = encode(v);
        if encoded < 0.0 {
            0.0
        } else {
            encoded
        }
    });
}

fn decode_inplace(image: &mut Image) {
    image.map_channels(decode);
}

pub fn orient(baseline: &Image, adj: &Adjustments) -> Image {
    let turns = adj.quarter_turns % 4;
    if turns == 0 && !adj.mirror_horizontal && !adj.mirror_vertical {
        return baseline.clone();
    }
    let (width, height) = if turns % 2 == 1 {
        (baseline.height, baseline.width)
    } else {
        (baseline.width, baseline.height)
    };
    let mut out = Image {
        width,
        height,
        rgb: vec![0.0; (width as usize) * (height as usize) * 3],
    };
    let bw = baseline.width;
    let bh = baseline.height;
    out.rgb
        .par_chunks_mut(width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let y = y as u32;
            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                let x = x as u32;
                let (u, v) = match turns {
                    0 => (x, y),
                    1 => (y, bh - 1 - x),
                    2 => (bw - 1 - x, bh - 1 - y),
                    _ => (bw - 1 - y, x),
                };
                let sx = if adj.mirror_horizontal { bw - 1 - u } else { u };
                let sy = if adj.mirror_vertical { bh - 1 - v } else { v };
                px.copy_from_slice(&baseline.pixel(sx, sy));
            }
        });
    out
}

fn white_balance(image: &mut Image, temperature: f32, tint: f32) {
    let matrix = white_balance_matrix(temperature, tint);
    image.for_each_pixel(|px| {
        let r = px[0];
        let g = px[1];
        let b = px[2];
        px[0] = finite_positive(matrix[0] * r + matrix[1] * g + matrix[2] * b);
        px[1] = finite_positive(matrix[3] * r + matrix[4] * g + matrix[5] * b);
        px[2] = finite_positive(matrix[6] * r + matrix[7] * g + matrix[8] * b);
    });
}

fn finite_positive(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        0.0
    }
}

/// Bradford CAT from D65 toward the daylight locus shifted by temperature and tint.
pub fn white_balance_matrix(temperature: f32, tint: f32) -> [f32; 9] {
    let src = xyz_d65();
    let dst = shifted_white(temperature, tint);
    let adapt = bradford_adapt(src, dst);
    let xyz_from_rgb = SRGB_TO_XYZ;
    let rgb_from_xyz = XYZ_TO_SRGB;
    mul3(&rgb_from_xyz, &mul3(&adapt, &xyz_from_rgb))
}

fn xyz_d65() -> [f32; 3] {
    let x = 0.3127;
    let y = 0.3290;
    [x / y, 1.0, (1.0 - x - y) / y]
}

fn shifted_white(temperature: f32, tint: f32) -> [f32; 3] {
    let mired = 153.75 + 50.0 * (temperature / 100.0);
    let kelvin = 1.0e6 / mired;
    let (x0, y0) = daylight_xy(kelvin);
    let (x1, y1) = daylight_xy(kelvin + 50.0);
    let mut dx = x1 - x0;
    let mut dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt().max(1.0e-8);
    dx /= len;
    dy /= len;
    // Rotate 90° so the y component points toward negative y (magenta).
    let (px, py) = if -dx < 0.0 { (dy, -dx) } else { (-dy, dx) };
    let shift = 0.02 * (tint / 100.0);
    let x = x0 + px * shift;
    let y = (y0 + py * shift).max(1.0e-4);
    [x / y, 1.0, (1.0 - x - y) / y]
}

fn daylight_xy(kelvin: f32) -> (f32, f32) {
    let t = kelvin.clamp(4000.0, 25000.0);
    let t2 = t * t;
    let t3 = t2 * t;
    let x = if t <= 7000.0 {
        -4.6070e9 / t3 + 2.9678e6 / t2 + 0.09911e3 / t + 0.244063
    } else {
        -2.0064e9 / t3 + 1.9018e6 / t2 + 0.24748e3 / t + 0.237040
    };
    let y = -3.0 * x * x + 2.870 * x - 0.275;
    (x, y)
}

const BRADFORD: [f32; 9] = [
    0.8951, 0.2664, -0.1614, -0.7502, 1.7135, 0.0367, 0.0389, -0.0685, 1.0296,
];
const BRADFORD_INV: [f32; 9] = [
    0.9869929, -0.1470543, 0.1599627, 0.4323053, 0.5183603, 0.0492912, -0.0085287, 0.0400428,
    0.9684867,
];

fn bradford_adapt(src: [f32; 3], dst: [f32; 3]) -> [f32; 9] {
    let rho_s = mul_vec(&BRADFORD, src);
    let rho_d = mul_vec(&BRADFORD, dst);
    let scale = [
        rho_d[0] / rho_s[0],
        0.0,
        0.0,
        0.0,
        rho_d[1] / rho_s[1],
        0.0,
        0.0,
        0.0,
        rho_d[2] / rho_s[2],
    ];
    mul3(&BRADFORD_INV, &mul3(&scale, &BRADFORD))
}

const SRGB_TO_XYZ: [f32; 9] = [
    0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522, 0.0721750, 0.0193339, 0.1191920,
    0.9503041,
];
const XYZ_TO_SRGB: [f32; 9] = [
    3.2404542, -1.5371385, -0.4985314, -0.9692660, 1.8760108, 0.0415560, 0.0556434, -0.2040259,
    1.0572252,
];

fn mul3(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
    let mut out = [0.0; 9];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] = a[row * 3] * b[col]
                + a[row * 3 + 1] * b[3 + col]
                + a[row * 3 + 2] * b[6 + col];
        }
    }
    out
}

fn mul_vec(m: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

fn blackpoint(image: &mut Image, slider: f32) {
    let b = 0.05 * (slider / 100.0);
    if b > 0.0 {
        let scale = 1.0 / (1.0 - b);
        image.map_channels(|v| (v - b).max(0.0) * scale);
    } else if b < 0.0 {
        let q = -b;
        image.map_channels(|v| q + (1.0 - q) * v);
    }
}

fn brightness(image: &mut Image, slider: f32) {
    let gamma = 2.0_f32.powf(-(slider / 100.0));
    image.map_channels(|x| x.clamp(0.0, 1.0).powf(gamma));
}

fn contrast(image: &mut Image, slider: f32) {
    let m = 1.0 + 0.5 * (slider / 100.0);
    image.map_channels(|x| {
        let x = x.clamp(0.0, 1.0);
        let t = x - 0.5;
        (0.5 + t * (m + 4.0 * (1.0 - m) * t * t)).clamp(0.0, 1.0)
    });
}

fn saturation(image: &mut Image, slider: f32) {
    let a = slider / 100.0;
    image.for_each_pixel(|px| {
        let y = 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2];
        for channel in px.iter_mut() {
            *channel = ((*channel - y) * (1.0 + a) + y).clamp(0.0, 1.0);
        }
    });
}

fn vibrance(image: &mut Image, slider: f32) {
    let a = slider / 100.0;
    image.for_each_pixel(|px| {
        let r = px[0].clamp(0.0, 1.0);
        let g = px[1].clamp(0.0, 1.0);
        let b = px[2].clamp(0.0, 1.0);
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let max_c = r.max(g).max(b);
        let min_c = r.min(g).min(b);
        let chroma = max_c - min_c;
        let skin = skin_mask(r, g, b, chroma);
        let weight = if a >= 0.0 {
            (1.0 - chroma) * (1.0 - skin)
        } else {
            chroma * (1.0 - skin)
        };
        let gain = 1.0 + a * weight;
        px[0] = ((r - y) * gain + y).clamp(0.0, 1.0);
        px[1] = ((g - y) * gain + y).clamp(0.0, 1.0);
        px[2] = ((b - y) * gain + y).clamp(0.0, 1.0);
    });
}

fn skin_mask(r: f32, g: f32, b: f32, chroma: f32) -> f32 {
    if chroma == 0.0 {
        return 0.0;
    }
    let h = if r >= g && r >= b {
        (g - b) / chroma
    } else if g >= r && g >= b {
        2.0 + (b - r) / chroma
    } else {
        4.0 + (r - g) / chroma
    };
    let mut h = h.rem_euclid(6.0);
    if h < 0.0 {
        h += 6.0;
    }
    let mut hue = 60.0 * h;
    if hue > 180.0 {
        hue -= 360.0;
    }
    if hue <= -10.0 || hue >= 60.0 {
        return 0.0;
    }
    let t = (hue - (-10.0)) / (60.0 - (-10.0));
    chroma * (0.5 - 0.5 * (std::f32::consts::TAU * t).cos())
}

fn shadows_highlights(image: &Image, shadow: f32, highlight: f32) -> Image {
    // 10% of the short side of the image this run is on. A preview is
    // smaller, so its radius shrinks with it.
    let short = image.width.min(image.height) as f32;
    let radius = (0.10 * short).clamp(1.0, short);
    let luma = linear_luma(image);
    let base = guided_filter(&luma, image.width, image.height, radius as u32, 0.02 * 0.02);
    let mut out = image.clone();
    let t_s = 0.18;
    let tau_s = 0.02;
    let a_s = 0.05;
    let k_s = 0.5;
    let t_h = 0.70;
    let c_h = 1.0;
    let g_h = 0.5;
    let t_n = 0.01;
    let s = (shadow.max(0.0)) / 100.0;
    let s_down = ((-shadow).max(0.0)) / 100.0;
    let h = ((-highlight).max(0.0)) / 100.0;
    let h_up = (highlight.max(0.0)) / 100.0;
    out.rgb
        .par_chunks_mut(CHUNK)
        .enumerate()
        .for_each(|(chunk, pixels)| {
            let first = chunk * CHUNK / 3;
            for (j, px) in pixels.chunks_exact_mut(3).enumerate() {
                let i = first + j;
                let l = luma[i];
                let detail = l - base[i];
                let u = (base[i] / t_s).clamp(0.0, 1.0);
                let w_s = (1.0 - u) * (1.0 - u) * (1.0 + 2.0 * u);
                let phi = (base[i] / tau_s).clamp(0.0, 1.0);
                let toe = phi * phi;
                let base_s = base[i] + (s * a_s * toe - s_down * k_s * base[i]) * w_s;
                let excess = (base_s - t_h).max(0.0);
                let v = (excess / (1.0 - t_h)).clamp(0.0, 1.0);
                let w_h = v * v * (3.0 - 2.0 * v);
                let excess_out = excess / (1.0 + h * excess / c_h);
                let base_out = base_s + w_h * (excess_out - excess + h_up * g_h * excess);
                let l_out = (base_out + detail).max(0.0);
                if l <= 1.0e-6 {
                    px.fill(l_out);
                } else {
                    let n = (l / t_n).clamp(0.0, 1.0);
                    for channel in px.iter_mut() {
                        let ratio = *channel / l;
                        let ratio = 1.0 + (ratio - 1.0) * n;
                        *channel = (ratio * l_out).max(0.0);
                    }
                }
            }
        });
    out
}

fn linear_luma(image: &Image) -> Vec<f32> {
    image
        .rgb
        .par_chunks_exact(3)
        .map(|px| 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2])
        .collect()
}

/// Short side of the grid the guided filter's coefficients are solved on.
const GUIDED_GRID: u32 = 512;

/// Self-guided filter. Above `GUIDED_GRID` it is the fast guided filter
/// (He and Sun, 2015): `a` and `b` come from a box-averaged copy of the
/// luma, are upsampled, and are applied to the full-resolution luma, so
/// edges still follow the full-resolution guide.
fn guided_filter(luma: &[f32], width: u32, height: u32, radius: u32, eps: f32) -> Vec<f32> {
    let step = (width.min(height) / GUIDED_GRID).max(1);
    if step == 1 {
        return guided_filter_exact(luma, width, height, radius, eps);
    }
    let (small, small_w, small_h) = box_downsample(luma, width, height, step);
    let small_radius = (radius / step).max(1);
    let (mean_a, mean_b) = guided_coefficients(&small, small_w, small_h, small_radius, eps);
    let mean_a = upsample_sized(&mean_a, small_w, small_h, width, height);
    let mean_b = upsample_sized(&mean_b, small_w, small_h, width, height);
    luma.par_iter()
        .zip(mean_a.par_iter().zip(mean_b.par_iter()))
        .map(|(l, (ma, mb))| ma * l + mb)
        .collect()
}

fn box_downsample(src: &[f32], width: u32, height: u32, step: u32) -> (Vec<f32>, u32, u32) {
    let w = width as usize;
    let h = height as usize;
    let s = step as usize;
    let small_w = w.div_ceil(s);
    let small_h = h.div_ceil(s);
    let mut out = vec![0.0; small_w * small_h];
    out.par_chunks_mut(small_w).enumerate().for_each(|(y, row)| {
        let y0 = y * s;
        let y1 = (y0 + s).min(h);
        for (x, dst) in row.iter_mut().enumerate() {
            let x0 = x * s;
            let x1 = (x0 + s).min(w);
            let mut acc = 0.0;
            for yy in y0..y1 {
                acc += src[yy * w + x0..yy * w + x1].iter().sum::<f32>();
            }
            *dst = acc / ((x1 - x0) * (y1 - y0)) as f32;
        }
    });
    (out, small_w as u32, small_h as u32)
}

fn guided_filter_exact(luma: &[f32], width: u32, height: u32, radius: u32, eps: f32) -> Vec<f32> {
    let (mean_a, mean_b) = guided_coefficients(luma, width, height, radius, eps);
    luma.par_iter()
        .zip(mean_a.par_iter().zip(mean_b.par_iter()))
        .map(|(l, (ma, mb))| ma * l + mb)
        .collect()
}

/// Window means of `a` and `b` for a self-guided filter.
fn guided_coefficients(
    luma: &[f32],
    width: u32,
    height: u32,
    radius: u32,
    eps: f32,
) -> (Vec<f32>, Vec<f32>) {
    let mean = box_mean(luma, width, height, radius);
    let sq: Vec<f32> = luma.par_iter().map(|v| v * v).collect();
    let mean_sq = box_mean(&sq, width, height, radius);
    drop(sq);
    let (a, b): (Vec<f32>, Vec<f32>) = mean
        .par_iter()
        .zip(mean_sq.par_iter())
        .map(|(m, m2)| {
            let var = (m2 - m * m).max(0.0);
            let ai = var / (var + eps);
            (ai, (1.0 - ai) * m)
        })
        .unzip();
    (
        box_mean(&a, width, height, radius),
        box_mean(&b, width, height, radius),
    )
}

/// Mean over a (2r+1)² window clipped to the image. Separable: a running
/// sum along each row, then column prefix sums. The clipped counts of the
/// two passes multiply to the clipped area, so the result is the 2D mean.
fn box_mean(src: &[f32], width: u32, height: u32, radius: u32) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let r = radius as usize;
    let mut rows = vec![0.0f32; w * h];
    rows.par_chunks_mut(w)
        .zip(src.par_chunks(w))
        .for_each(|(out, row)| {
            let mut prefix = Vec::with_capacity(w + 1);
            let mut acc = 0.0f64;
            prefix.push(0.0);
            for v in row {
                acc += *v as f64;
                prefix.push(acc);
            }
            for (x, dst) in out.iter_mut().enumerate() {
                let x0 = x.saturating_sub(r);
                let x1 = (x + r + 1).min(w);
                *dst = ((prefix[x1] - prefix[x0]) / (x1 - x0) as f64) as f32;
            }
        });
    // Column prefix sums, one row of w at a time.
    let mut prefix = vec![0.0f32; w * (h + 1)];
    for y in 0..h {
        let (done, rest) = prefix.split_at_mut((y + 1) * w);
        let above = &done[y * w..];
        let next = &mut rest[..w];
        let row = &rows[y * w..(y + 1) * w];
        for x in 0..w {
            next[x] = above[x] + row[x];
        }
    }
    drop(rows);
    let mut out = vec![0.0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, dst)| {
        let y0 = y.saturating_sub(r);
        let y1 = (y + r + 1).min(h);
        let count = (y1 - y0) as f32;
        let top = &prefix[y0 * w..(y0 + 1) * w];
        let bottom = &prefix[y1 * w..(y1 + 1) * w];
        for x in 0..w {
            dst[x] = (bottom[x] - top[x]) / count;
        }
    });
    out
}

/// Pyramid levels for a full-resolution run. Bands 1..LEVELS-1 are the
/// mid frequencies clarity owns.
const CLARITY_LEVELS: usize = 6;

fn clarity(image: &Image, slider: f32, radius_scale: f32) -> Image {
    let a = slider / 100.0;
    let tau = 0.12_f32;
    let width = image.width;
    let height = image.height;
    // A preview at 1/2^k of full width sees the same bands k levels finer.
    let shift = (-radius_scale.max(1.0e-3).log2()).round().clamp(0.0, 2.0) as usize;
    let levels = CLARITY_LEVELS - shift;
    let first_band = 1usize.saturating_sub(shift);
    let luma: Vec<f32> = image
        .rgb
        .par_chunks_exact(3)
        .map(|px| (0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2]).clamp(0.0, 1.0))
        .collect();
    let pyramid = gaussian_pyramid(&luma, width, height, levels);
    if pyramid.len() < 3 {
        return image.clone();
    }
    let mut laplacian: Vec<Vec<f32>> = Vec::new();
    for i in 0..pyramid.len() - 1 {
        let up = upsample_sized(
            &pyramid[i + 1].0,
            pyramid[i + 1].1,
            pyramid[i + 1].2,
            pyramid[i].1,
            pyramid[i].2,
        );
        laplacian.push(
            pyramid[i]
                .0
                .par_iter()
                .zip(up.par_iter())
                .map(|(pixel, upsampled)| pixel - upsampled)
                .collect(),
        );
    }
    laplacian.push(pyramid.last().unwrap().0.clone());
    let last = laplacian.len() - 1;
    for level in first_band..last {
        laplacian[level].par_iter_mut().for_each(|coef| {
            let delta = *coef;
            *coef = delta * (1.0 + a * (-(delta * delta) / (2.0 * tau * tau)).exp());
        });
    }
    let mut current = laplacian.pop().unwrap();
    let mut current_w = pyramid.last().unwrap().1;
    let mut current_h = pyramid.last().unwrap().2;
    for level in (0..laplacian.len()).rev() {
        let gauss_w = pyramid[level].1;
        let gauss_h = pyramid[level].2;
        let mut up = upsample_sized(&current, current_w, current_h, gauss_w, gauss_h);
        up.par_iter_mut()
            .zip(laplacian[level].par_iter())
            .for_each(|(upsampled, lap)| *upsampled += lap);
        current = up;
        current_w = gauss_w;
        current_h = gauss_h;
    }
    let mut out = image.clone();
    out.rgb
        .par_chunks_mut(CHUNK)
        .enumerate()
        .for_each(|(chunk, pixels)| {
            let first = chunk * CHUNK / 3;
            for (j, px) in pixels.chunks_exact_mut(3).enumerate() {
                let i = first + j;
                let old = luma[i].max(1.0e-6);
                let scale = current[i].clamp(0.0, 1.0) / old;
                for channel in px.iter_mut() {
                    *channel = (*channel * scale).clamp(0.0, 1.0);
                }
            }
        });
    out
}

fn gaussian_pyramid(
    src: &[f32],
    width: u32,
    height: u32,
    max_levels: usize,
) -> Vec<(Vec<f32>, u32, u32)> {
    let mut levels = vec![(src.to_vec(), width, height)];
    let mut w = width;
    let mut h = height;
    while w > 2 && h > 2 && levels.len() < max_levels {
        let blurred = blur3(&levels.last().unwrap().0, w, h);
        let nw = (w / 2).max(1);
        let nh = (h / 2).max(1);
        let mut next = vec![0.0; (nw as usize) * (nh as usize)];
        next.par_chunks_mut(nw as usize)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, dst) in row.iter_mut().enumerate() {
                    *dst = blurred[y * 2 * w as usize + x * 2];
                }
            });
        w = nw;
        h = nh;
        levels.push((next, w, h));
    }
    levels
}

/// 1-2-1 binomial in each direction, renormalized at the borders.
fn blur3(src: &[f32], width: u32, height: u32) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let mut rows = vec![0.0; src.len()];
    rows.par_chunks_mut(w)
        .zip(src.par_chunks(w))
        .for_each(|(dst, row)| {
            for x in 0..w {
                let mut acc = 2.0 * row[x];
                let mut weight = 2.0;
                if x > 0 {
                    acc += row[x - 1];
                    weight += 1.0;
                }
                if x + 1 < w {
                    acc += row[x + 1];
                    weight += 1.0;
                }
                dst[x] = acc / weight;
            }
        });
    let mut out = vec![0.0; src.len()];
    out.par_chunks_mut(w).enumerate().for_each(|(y, dst)| {
        let mid = &rows[y * w..(y + 1) * w];
        let up = (y > 0).then(|| &rows[(y - 1) * w..y * w]);
        let down = (y + 1 < h).then(|| &rows[(y + 1) * w..(y + 2) * w]);
        let weight = 2.0 + up.is_some() as u8 as f32 + down.is_some() as u8 as f32;
        for x in 0..w {
            let mut acc = 2.0 * mid[x];
            if let Some(up) = up {
                acc += up[x];
            }
            if let Some(down) = down {
                acc += down[x];
            }
            dst[x] = acc / weight;
        }
    });
    out
}

fn upsample_sized(src: &[f32], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<f32> {
    let mut out = vec![0.0; (dst_w as usize) * (dst_h as usize)];
    out.par_chunks_mut(dst_w as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let sy = ((y as f32 + 0.5) * src_h as f32 / dst_h as f32 - 0.5)
                .clamp(0.0, (src_h - 1) as f32);
            let y0 = sy.floor() as u32;
            let y1 = (y0 + 1).min(src_h - 1);
            let fy = sy - y0 as f32;
            for (x, dst) in row.iter_mut().enumerate() {
                let sx = ((x as f32 + 0.5) * src_w as f32 / dst_w as f32 - 0.5)
                    .clamp(0.0, (src_w - 1) as f32);
                let x0 = sx.floor() as u32;
                let x1 = (x0 + 1).min(src_w - 1);
                let fx = sx - x0 as f32;
                let p00 = src[(y0 * src_w + x0) as usize];
                let p10 = src[(y0 * src_w + x1) as usize];
                let p01 = src[(y1 * src_w + x0) as usize];
                let p11 = src[(y1 * src_w + x1) as usize];
                let top = p00 + (p10 - p00) * fx;
                let bot = p01 + (p11 - p01) * fx;
                *dst = top + (bot - top) * fy;
            }
        });
    out
}

fn resample_frame(image: &Image, rotation_deg: f32, frame: Frame) -> Image {
    let theta = rotation_deg.to_radians();
    let (ux, uy, uw, uh) = usable_rect(image.width as f32, image.height as f32, theta);
    let x0 = ux + frame.x.clamp(0.0, 1.0) * uw;
    let y0 = uy + frame.y.clamp(0.0, 1.0) * uh;
    let fw = (frame.w.clamp(0.0, 1.0) * uw).max(1.0);
    let fh = (frame.h.clamp(0.0, 1.0) * uh).max(1.0);
    let out_w = fw.round().max(1.0) as u32;
    let out_h = fh.round().max(1.0) as u32;
    if theta.abs() < 1.0e-6 && frame == Frame::default() {
        return image.clone();
    }
    let mut out = Image {
        width: out_w,
        height: out_h,
        rgb: vec![0.0; (out_w as usize) * (out_h as usize) * 3],
    };
    let cx = (image.width as f32 - 1.0) * 0.5;
    let cy = (image.height as f32 - 1.0) * 0.5;
    let cos = theta.cos();
    let sin = theta.sin();
    out.rgb
        .par_chunks_mut(out_w as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let py = y0 + (y as f32 + 0.5) * fh / out_h as f32;
            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                let sample_x = x0 + (x as f32 + 0.5) * fw / out_w as f32;
                let dx = sample_x - cx;
                let dy = py - cy;
                let sx = cos * dx + sin * dy + cx;
                let sy = -sin * dx + cos * dy + cy;
                let sample = sample_catmull(image, sx, sy);
                for (dst, v) in px.iter_mut().zip(sample) {
                    *dst = if v.is_finite() { v } else { 0.0 };
                }
            }
        });
    out
}

fn sample_catmull(image: &Image, x: f32, y: f32) -> [f32; 3] {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let mut cols = [[0.0; 3]; 4];
    for (i, dy) in (-1..=2).enumerate() {
        let mut row = [[0.0; 3]; 4];
        for (j, dx) in (-1..=2).enumerate() {
            row[j] = tap(image, x0 + dx, y0 + dy);
        }
        cols[i] = [
            catmull(row[0][0], row[1][0], row[2][0], row[3][0], tx),
            catmull(row[0][1], row[1][1], row[2][1], row[3][1], tx),
            catmull(row[0][2], row[1][2], row[2][2], row[3][2], tx),
        ];
    }
    [
        catmull(cols[0][0], cols[1][0], cols[2][0], cols[3][0], ty).max(0.0),
        catmull(cols[0][1], cols[1][1], cols[2][1], cols[3][1], ty).max(0.0),
        catmull(cols[0][2], cols[1][2], cols[2][2], cols[3][2], ty).max(0.0),
    ]
}

fn tap(image: &Image, x: i32, y: i32) -> [f32; 3] {
    if x < 0 || y < 0 || x >= image.width as i32 || y >= image.height as i32 {
        [0.0, 0.0, 0.0]
    } else {
        image.pixel(x as u32, y as u32)
    }
}

fn catmull(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

pub fn usable_rect(width: f32, height: f32, theta: f32) -> (f32, f32, f32, f32) {
    if theta.abs() < 1.0e-6 {
        return (0.0, 0.0, width, height);
    }
    let cos = theta.cos().abs();
    let sin = theta.sin().abs();
    let w_limit = width * cos - height * sin;
    let h_limit = height * cos - width * sin;
    let (uw, uh) = if w_limit > 0.0 && h_limit > 0.0 {
        (w_limit, h_limit)
    } else if width * sin >= height * cos {
        let uh = width * cos + height * sin;
        let uw = width * height / uh.max(1.0);
        (uw, height.min(uh))
    } else {
        let uw = width * cos + height * sin;
        let uh = width * height / uw.max(1.0);
        (width.min(uw), uh)
    };
    let uw = uw.clamp(1.0, width);
    let uh = uh.clamp(1.0, height);
    ((width - uw) * 0.5, (height - uh) * 0.5, uw, uh)
}

pub fn fit_aspect(aspect: Aspect, rotation_deg: f32, width: f32, height: f32) -> Frame {
    let theta = rotation_deg.to_radians();
    let (_, _, uw, uh) = usable_rect(width, height, theta);
    match aspect.ratio(width, height) {
        None => Frame::default(),
        Some(ratio) => {
            let (fw, fh) = if uw / uh > ratio {
                (uh * ratio, uh)
            } else {
                (uw, uw / ratio)
            };
            Frame {
                x: ((uw - fw) * 0.5 / uw).max(0.0),
                y: ((uh - fh) * 0.5 / uh).max(0.0),
                w: (fw / uw).min(1.0),
                h: (fh / uh).min(1.0),
            }
        }
    }
}

pub fn clamp_frame_to_photo(frame: Frame, aspect: Aspect, rotation_deg: f32, width: f32, height: f32) -> Frame {
    let fitted = fit_aspect(aspect, rotation_deg, width, height);
    if aspect == Aspect::Free {
        Frame {
            x: frame.x.clamp(0.0, 1.0 - frame.w.max(0.01)),
            y: frame.y.clamp(0.0, 1.0 - frame.h.max(0.01)),
            w: frame.w.clamp(0.01, 1.0),
            h: frame.h.clamp(0.01, 1.0),
        }
    } else if frame_inside(frame) && frame.w <= fitted.w + 0.001 && frame.h <= fitted.h + 0.001 {
        frame
    } else {
        fitted
    }
}

fn frame_inside(frame: Frame) -> bool {
    frame.x >= 0.0
        && frame.y >= 0.0
        && frame.w > 0.0
        && frame.h > 0.0
        && frame.x + frame.w <= 1.0 + 1.0e-4
        && frame.y + frame.h <= 1.0 + 1.0e-4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, rgb: [f32; 3]) -> Image {
        let mut data = Vec::new();
        for _ in 0..width * height {
            data.extend_from_slice(&rgb);
        }
        Image::new(width, height, data)
    }

    #[test]
    fn encode_decode_round_trips_and_extends_past_one() {
        let linear = decode(0.5);
        let back = encode(linear);
        assert!((back - 0.5).abs() < 1.0e-5);
        let above = encode(2.0);
        assert!(above > 1.0);
        assert!(decode(1.5) > 1.0);
    }

    #[test]
    fn negatives_and_non_finite_become_zero() {
        let image = Image::new(1, 1, vec![f32::NAN, -0.2, f32::INFINITY]);
        assert_eq!(image.pixel(0, 0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn exposure_keeps_grey_neutral_and_temperature_does_not() {
        let grey = flat(2, 2, [0.18, 0.18, 0.18]);
        let mut adj = Adjustments::defaults(90);
        adj.exposure_ev = 1.0;
        let exposed = run(&grey, &adj, &RunOptions { radius_scale: 1.0, dither: false, crop: true });
        let px = exposed.pixel(0, 0);
        assert!((px[0] - 0.36).abs() < 1.0e-5);
        assert!((px[0] - px[1]).abs() < 1.0e-5 && (px[1] - px[2]).abs() < 1.0e-5);

        adj.exposure_ev = 0.0;
        adj.temperature = 40.0;
        let warmed = run(&grey, &adj, &RunOptions { radius_scale: 1.0, dither: false, crop: true });
        let px = warmed.pixel(0, 0);
        let spread = (px[0] - px[1]).abs() + (px[1] - px[2]).abs();
        assert!(spread > 0.01, "temperature should move a grey off neutral, got {px:?}");
    }

    #[test]
    fn blackpoint_crushes_and_lifts() {
        let mut image = flat(1, 1, [0.18, 0.18, 0.18]);
        blackpoint(&mut image, 100.0);
        let y = image.pixel(0, 0)[0];
        assert!((y - 0.136842).abs() < 1.0e-3, "{y}");

        let mut white = flat(1, 1, [1.0, 1.0, 1.0]);
        blackpoint(&mut white, 100.0);
        assert!((white.pixel(0, 0)[0] - 1.0).abs() < 1.0e-5);

        let mut black = flat(1, 1, [0.0, 0.0, 0.0]);
        blackpoint(&mut black, -100.0);
        assert!((black.pixel(0, 0)[0] - 0.05).abs() < 1.0e-5);

        let mut mixed = flat(1, 1, [0.02, 0.08, 0.08]);
        blackpoint(&mut mixed, 100.0);
        let px = mixed.pixel(0, 0);
        assert_eq!(px[0], 0.0);
        assert!(px[1] > 0.0 && (px[1] - px[2]).abs() < 1.0e-6);
    }

    #[test]
    fn zero_sliders_leave_the_image() {
        let image = flat(2, 2, [0.2, 0.4, 0.6]);
        let out = run(&image, &Adjustments::defaults(90), &RunOptions { radius_scale: 1.0, dither: false, crop: true });
        assert_eq!(out.pixel(1, 1), image.pixel(1, 1));
    }

    #[test]
    fn orient_quarter_turn_swaps_axes() {
        let mut rgb = vec![0.0; 6 * 3];
        rgb[0] = 1.0;
        let image = Image::new(2, 3, rgb);
        let mut adj = Adjustments::defaults(90);
        adj.quarter_turns = 1;
        let out = orient(&image, &adj);
        assert_eq!((out.width, out.height), (3, 2));
        // 90° clockwise puts the baseline origin at the top-right.
        assert_eq!(out.pixel(2, 0)[0], 1.0);
    }

    #[test]
    fn brightness_moves_mid_grey_and_pins_ends() {
        let mut encoded = Image::new(1, 1, vec![0.5, 0.5, 0.5]);
        brightness(&mut encoded, 100.0);
        assert!((encoded.pixel(0, 0)[0] - 0.7071).abs() < 1.0e-3);
        encoded = Image::new(1, 1, vec![0.0, 1.0, 0.5]);
        brightness(&mut encoded, 100.0);
        assert_eq!(encoded.pixel(0, 0)[0], 0.0);
        assert!((encoded.pixel(0, 0)[1] - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn contrast_pins_mid_grey() {
        let mut image = Image::new(1, 1, vec![0.5, 0.25, 1.0]);
        contrast(&mut image, 100.0);
        let px = image.pixel(0, 0);
        assert!((px[0] - 0.5).abs() < 1.0e-5);
        assert!((px[1] - 0.15625).abs() < 1.0e-3);
        assert!((px[2] - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn saturation_minus_one_is_grey() {
        let mut image = Image::new(1, 1, vec![1.0, 0.0, 0.0]);
        saturation(&mut image, -100.0);
        let px = image.pixel(0, 0);
        assert!((px[0] - 0.2126).abs() < 1.0e-4);
        assert!((px[1] - 0.2126).abs() < 1.0e-4);
    }

    #[test]
    fn quarter_turn_swaps_a_three_two_frame() {
        let mut adj = Adjustments::defaults(90);
        adj.aspect = Aspect::R3x2;
        adj.frame = Frame { x: 0.1, y: 0.2, w: 0.5, h: 0.4 };
        adj.turn_clockwise(30.0, 20.0);
        assert_eq!(adj.aspect, Aspect::R2x3);
        assert!((adj.frame.x - (1.0 - 0.2 - 0.4)).abs() < 1.0e-5);
        assert!((adj.frame.y - 0.1).abs() < 1.0e-5);
        assert!((adj.frame.w - 0.4).abs() < 1.0e-5);
        assert!((adj.frame.h - 0.5).abs() < 1.0e-5);
    }

    #[test]
    fn subsampled_guided_filter_keeps_mid_grey_and_the_edge() {
        // 1200 px short side solves the filter on a half-size grid.
        let (w, h) = (1200u32, 1200u32);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for _y in 0..h {
            for x in 0..w {
                let v = if x < w / 2 { 0.18 } else { 0.9 };
                rgb.extend_from_slice(&[v, v, v]);
            }
        }
        let image = Image::new(w, h, rgb);
        let out = shadows_highlights(&image, 100.0, -100.0);
        let grey = out.pixel(w / 4, h / 2)[0];
        assert!((grey - 0.18).abs() < 2.0e-3, "mid grey moved to {grey}");
        // No bright line on the dark side next to the step.
        let near = out.pixel(w / 2 - 2, h / 2)[0];
        assert!(near < 0.2, "halo on the dark side: {near}");
    }

    #[test]
    #[ignore]
    fn bench_stages() {
        for (w, h) in [(4032u32, 3024u32), (1600, 1200)] {
            let mut rgb = Vec::with_capacity((w * h * 3) as usize);
            for i in 0..(w * h * 3) as usize {
                rgb.push(((i * 2654435761) % 1000) as f32 / 1000.0);
            }
            let image = Image::new(w, h, rgb);
            let time = |name: &str, f: &dyn Fn()| {
                let t = std::time::Instant::now();
                f();
                eprintln!("{w}x{h} {name}: {:?}", t.elapsed());
            };
            let opts = RunOptions { radius_scale: 1.0, dither: false, crop: false };
            let mut adj = Adjustments::defaults(90);
            time("identity", &|| { run(&image, &adj, &opts); });
            adj.exposure_ev = 0.5;
            time("exposure", &|| { run(&image, &adj, &opts); });
            adj.exposure_ev = 0.0;
            adj.temperature = 20.0;
            time("wb", &|| { run(&image, &adj, &opts); });
            adj.temperature = 0.0;
            adj.shadow = 30.0;
            time("shadows", &|| { run(&image, &adj, &opts); });
            adj.shadow = 0.0;
            adj.contrast = 30.0;
            time("contrast", &|| { run(&image, &adj, &opts); });
            adj.contrast = 0.0;
            adj.clarity = 30.0;
            time("clarity", &|| { run(&image, &adj, &opts); });
            adj.clarity = 0.0;
            adj.rotation = 5.0;
            time("rotate", &|| { run(&image, &adj, &opts); });
            time("rgba8", &|| { image.to_rgba8(true); });
            time("histogram", &|| { image.histogram_region(0, 0, w, h); });
        }
    }

    #[test]
    fn rotation_frame_does_not_sample_an_empty_corner() {
        let mut rgb = vec![0.2; 8 * 8 * 3];
        rgb[0] = 1.0;
        let image = Image::new(8, 8, rgb);
        let frame = fit_aspect(Aspect::R1x1, 20.0, 8.0, 8.0);
        let out = resample_frame(&image, 20.0, frame);
        for px in out.rgb.chunks_exact(3) {
            assert!(px[0] > 0.05, "empty corner leaked into the frame: {px:?}");
        }
    }
}
