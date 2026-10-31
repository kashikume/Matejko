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
    /// Keystone, −100…+100. See `Warp::new`.
    pub vertical: f32,
    pub horizontal: f32,
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
            vertical: 0.0,
            horizontal: 0.0,
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
            && self.vertical == 0.0
            && self.horizontal == 0.0
            && self.aspect == Aspect::Original
            && self.frame == Frame::default()
    }

    pub fn is_identity(&self, estimated_quality: u8) -> bool {
        self.tone_is_default()
            && self.geometry_is_default()
            && self.jpeg_quality == estimated_quality
    }

    /// The geometry for a photo of this size in the chosen orientation.
    pub fn warp(&self, oriented_w: f32, oriented_h: f32) -> Warp {
        Warp::new(self.rotation, self.vertical, self.horizontal, oriented_w, oriented_h)
    }

    /// Mirrors the result as it is on screen: the turn and the left-right
    /// keystone change sign, and the frame flips with the picture.
    pub fn mirror_horizontally(&mut self) {
        self.mirror_horizontal = !self.mirror_horizontal;
        self.rotation = -self.rotation;
        self.horizontal = -self.horizontal;
        self.frame.x = 1.0 - self.frame.x - self.frame.w;
    }

    pub fn mirror_vertically(&mut self) {
        self.mirror_vertical = !self.mirror_vertical;
        self.rotation = -self.rotation;
        self.vertical = -self.vertical;
        self.frame.y = 1.0 - self.frame.y - self.frame.h;
    }

    pub fn turn_clockwise(&mut self, photo_w: f32, photo_h: f32) {
        self.quarter_turns = (self.quarter_turns + 1) % 4;
        self.aspect = self.aspect.turned_clockwise();
        // The old top is now on the right, and the old right at the bottom.
        let (vertical, horizontal) = (self.vertical, self.horizontal);
        self.vertical = -horizontal;
        self.horizontal = vertical;
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
        self.frame = fit_aspect(self.aspect, &self.warp(oriented_w, oriented_h));
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
    /// Save crops to the frame. The on-screen picture keeps the whole
    /// rotated photo, so the margin and the empty corners can be dimmed.
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
    // Read before any tone stage moves the ceiling.
    let clipped = (adj.shadow != 0.0 || adj.highlight != 0.0).then(|| clip_mask(&image));
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
    if let Some(clipped) = clipped {
        image = shadows_highlights(
            image,
            &clipped,
            adj.shadow,
            adj.highlight,
            options.radius_scale,
        );
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
    let warp = adj.warp(image.width as f32, image.height as f32);
    if options.crop {
        if !warp.is_identity() || adj.frame != Frame::default() {
            image = crop_frame(&image, &warp, adj.frame);
        }
    } else if !warp.is_identity() {
        image = warp_canvas(&image, &warp);
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

/// `clipped` marks, per pixel, the channels that were on the ceiling in the
/// file (bit 0 red, 1 green, 2 blue). See `clip_mask`.
fn shadows_highlights(
    mut image: Image,
    clipped: &[u8],
    shadow: f32,
    highlight: f32,
    radius_scale: f32,
) -> Image {
    let tile = ((REPAIR_TILE as f32 * radius_scale).round() as usize).max(4);
    repair_clipped(&mut image, clipped, tile);
    // 10% of the short side of the image this run is on. A preview is
    // smaller, so its radius shrinks with it.
    let short = image.width.min(image.height) as f32;
    let radius = (0.10 * short).clamp(1.0, short);
    let luma = linear_luma(&image);
    let base = guided_filter(&luma, image.width, image.height, radius as u32, 0.02 * 0.02);
    let mut out = image;
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

/// Tile side for the clipped-channel ratios. The median is taken over a tile
/// and its eight neighbours, about 32 px either side.
const REPAIR_TILE: usize = 16;
/// A sibling channel below this carries no usable ratio.
const REPAIR_FLOOR: f32 = 0.01;

/// Per pixel, bit `k` is set when channel `k` was on the ceiling in the
/// file. A JPEG has already clipped, so codes 254 and 255 are the ceiling.
fn clip_mask(image: &Image) -> Vec<u8> {
    let ceiling = decode(253.5 / 255.0);
    image
        .rgb
        .par_chunks_exact(3)
        .map(|px| {
            (px[0] >= ceiling) as u8
                | ((px[1] >= ceiling) as u8) << 1
                | ((px[2] >= ceiling) as u8) << 2
        })
        .collect()
}

/// Rebuilds a clipped channel `k` from an unclipped sibling `j`:
/// `k = j · median(k / j)` over nearby pixels where `k` was not clipped and
/// `j` is above the floor. A pixel with all three clipped, or with no such
/// neighbours, stays. A repair never lowers a channel below its clipped value.
fn repair_clipped(image: &mut Image, clipped: &[u8], tile: usize) {
    let partial = |m: u8| m != 0 && m != 0b111;
    if !clipped.par_iter().any(|m| partial(*m)) {
        return;
    }
    let w = image.width as usize;
    let h = image.height as usize;
    let tiles_w = w.div_ceil(tile);
    let tiles_h = h.div_ceil(tile);
    let needs: Vec<bool> = (0..tiles_w * tiles_h)
        .into_par_iter()
        .map(|t| {
            let (tx, ty) = (t % tiles_w, t / tiles_w);
            (ty * tile..((ty + 1) * tile).min(h)).any(|y| {
                (tx * tile..((tx + 1) * tile).min(w)).any(|x| partial(clipped[y * w + x]))
            })
        })
        .collect();
    let rgb = &image.rgb;
    // medians[t][k * 3 + j] is the median of k / j around tile t, or NaN.
    let medians: Vec<[f32; 9]> = (0..tiles_w * tiles_h)
        .into_par_iter()
        .map(|t| {
            let mut out = [f32::NAN; 9];
            if !needs[t] {
                return out;
            }
            let (tx, ty) = (t % tiles_w, t / tiles_w);
            let x0 = tx.saturating_sub(1) * tile;
            let x1 = ((tx + 2) * tile).min(w);
            let y0 = ty.saturating_sub(1) * tile;
            let y1 = ((ty + 2) * tile).min(h);
            let mut samples: [Vec<f32>; 9] = Default::default();
            // Every second pixel each way is plenty for a median.
            for y in (y0..y1).step_by(2) {
                for x in (x0..x1).step_by(2) {
                    let i = y * w + x;
                    let px = &rgb[i * 3..i * 3 + 3];
                    for k in 0..3 {
                        if clipped[i] & (1 << k) != 0 {
                            continue;
                        }
                        for j in 0..3 {
                            if j != k && px[j] > REPAIR_FLOOR {
                                samples[k * 3 + j].push(px[k] / px[j]);
                            }
                        }
                    }
                }
            }
            for (slot, values) in out.iter_mut().zip(samples.iter_mut()) {
                if !values.is_empty() {
                    let mid = values.len() / 2;
                    *slot = *values.select_nth_unstable_by(mid, f32::total_cmp).1;
                }
            }
            out
        })
        .collect();
    image
        .rgb
        .par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                let m = clipped[y * w + x];
                if !partial(m) {
                    continue;
                }
                // The larger unclipped channel is the reference.
                let j = (0..3)
                    .filter(|c| m & (1 << c) == 0)
                    .max_by(|a, b| px[*a].total_cmp(&px[*b]))
                    .unwrap_or(0);
                let tile_index = (y / tile) * tiles_w + x / tile;
                for k in (0..3).filter(|c| m & (1 << c) != 0) {
                    let ratio = medians[tile_index][k * 3 + j];
                    let rebuilt = px[j] * ratio;
                    if rebuilt.is_finite() {
                        px[k] = px[k].max(rebuilt);
                    }
                }
            }
        });
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
/// Reference tones the remap is sampled at, evenly over [0, 1]. Their
/// spacing, 1/7, is about the detail amplitude `τ`.
const CLARITY_REFERENCES: usize = 8;

/// Fast local Laplacian (Aubry et al. 2014) on encoded luma. For each
/// reference tone γ the luma is remapped with
/// `r(i) = i + a · (i − γ) · exp(−(i − γ)² / 2τ²)` and its band-pass levels
/// are built. Each output coefficient takes the two references around its
/// own Gaussian level and interpolates between them. Chroma goes back as
/// the per-channel offset from luma.
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
    let gauss = gaussian_pyramid(luma.clone(), width, height, levels);
    let n = gauss.len();
    if n < 3 || first_band >= n - 1 {
        return image.clone();
    }
    // Bands below `first_band` are the input's own.
    let mut bands: Vec<Vec<f32>> = (0..n - 1)
        .map(|level| {
            if level < first_band {
                laplacian_band(&gauss, level)
            } else {
                vec![0.0; gauss[level].0.len()]
            }
        })
        .collect();
    let spacing = 1.0 / (CLARITY_REFERENCES - 1) as f32;
    for j in 0..CLARITY_REFERENCES {
        let gamma = j as f32 * spacing;
        let remapped: Vec<f32> = luma
            .par_iter()
            .map(|i| {
                let delta = i - gamma;
                i + a * delta * (-(delta * delta) / (2.0 * tau * tau)).exp()
            })
            .collect();
        let remapped = gaussian_pyramid(remapped, width, height, n);
        for (level, band) in bands.iter_mut().enumerate().skip(first_band) {
            let coefficients = laplacian_band(&remapped, level);
            band.par_iter_mut()
                .zip(coefficients.par_iter().zip(gauss[level].0.par_iter()))
                .for_each(|(out, (coefficient, g))| {
                    // Hat weight: 1 at γ, 0 one spacing away.
                    let weight = 1.0 - ((g - gamma).abs() / spacing);
                    if weight > 0.0 {
                        *out += weight * coefficient;
                    }
                });
        }
    }
    let (mut current, mut current_w, mut current_h) = gauss.last().cloned().unwrap();
    for level in (0..n - 1).rev() {
        let (_, gauss_w, gauss_h) = gauss[level];
        let mut up = upsample_sized(&current, current_w, current_h, gauss_w, gauss_h);
        up.par_iter_mut()
            .zip(bands[level].par_iter())
            .for_each(|(upsampled, band)| *upsampled += band);
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
                let offset = current[i].clamp(0.0, 1.0) - luma[i];
                for channel in px.iter_mut() {
                    *channel = (*channel + offset).clamp(0.0, 1.0);
                }
            }
        });
    out
}

/// Level `level` of the Laplacian pyramid: the Gaussian level minus the
/// next one brought back up to its size.
fn laplacian_band(gauss: &[(Vec<f32>, u32, u32)], level: usize) -> Vec<f32> {
    let (fine, w, h) = &gauss[level];
    let (coarse, cw, ch) = &gauss[level + 1];
    let mut band = upsample_sized(coarse, *cw, *ch, *w, *h);
    band.par_iter_mut()
        .zip(fine.par_iter())
        .for_each(|(out, pixel)| *out = pixel - *out);
    band
}

fn gaussian_pyramid(
    src: Vec<f32>,
    width: u32,
    height: u32,
    max_levels: usize,
) -> Vec<(Vec<f32>, u32, u32)> {
    let mut levels = vec![(src, width, height)];
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

/// A 3×3 projective map, row-major, acting on `(x, y, 1)`.
type Homography = [f64; 9];

/// Full keystone at ±100: one edge 50% longer, the opposite 50% shorter.
const KEYSTONE: f32 = 0.5;

/// The geometry after the tone stages: perspective, then the horizon turn.
/// Offsets are from the photo's centre, in pixels of the oriented photo,
/// y down. The photo's centre stays where it is.
#[derive(Clone, Copy, Debug)]
pub struct Warp {
    pub width: f32,
    pub height: f32,
    theta: f32,
    cos: f32,
    sin: f32,
    /// Photo to corrected, and back. `None` when both sliders are 0.
    perspective: Option<(Homography, Homography)>,
}

impl Warp {
    /// `vertical` and `horizontal` are the sliders, −100…+100. Positive
    /// vertical widens the top, which straightens verticals that lean in
    /// toward the top. Positive horizontal makes the right side taller.
    pub fn new(rotation_deg: f32, vertical: f32, horizontal: f32, width: f32, height: f32) -> Self {
        let theta = rotation_deg.to_radians();
        let perspective = (vertical != 0.0 || horizontal != 0.0).then(|| {
            let forward = keystone(vertical, horizontal, width, height);
            (forward, invert(&forward))
        });
        Self {
            width,
            height,
            theta,
            cos: theta.cos(),
            sin: theta.sin(),
            perspective,
        }
    }

    /// A turn alone, with no perspective.
    #[cfg(test)]
    pub fn turn(rotation_deg: f32, width: f32, height: f32) -> Self {
        Self::new(rotation_deg, 0.0, 0.0, width, height)
    }

    pub fn is_identity(&self) -> bool {
        self.theta.abs() < 1.0e-6 && self.perspective.is_none()
    }

    /// Where the photo point shown at an output offset came from.
    fn source(&self, dx: f32, dy: f32) -> (f32, f32) {
        // Undo the counterclockwise turn by turning clockwise.
        let x = self.cos * dx - self.sin * dy;
        let y = self.sin * dx + self.cos * dy;
        match &self.perspective {
            Some((_, back)) => apply(back, x, y),
            None => (x, y),
        }
    }

    /// Where a photo point is shown.
    fn forward(&self, sx: f32, sy: f32) -> (f32, f32) {
        let (x, y) = match &self.perspective {
            Some((there, _)) => apply(there, sx, sy),
            None => (sx, sy),
        };
        (self.cos * x + self.sin * y, -self.sin * x + self.cos * y)
    }

    /// An output offset shows a point of the photo.
    fn on_photo(&self, dx: f32, dy: f32) -> bool {
        let (sx, sy) = self.source(dx, dy);
        // A hundredth of a pixel of slack keeps a point on the border in.
        sx.is_finite()
            && sy.is_finite()
            && sx.abs() <= self.width * 0.5 + 0.01
            && sy.abs() <= self.height * 0.5 + 0.01
    }

    /// The box around the whole corrected photo: min x, min y, max x, max y.
    fn bounds(&self) -> [f32; 4] {
        let (a, b) = (self.width * 0.5, self.height * 0.5);
        let mut out = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for (sx, sy) in [(-a, -b), (a, -b), (a, b), (-a, b)] {
            let (x, y) = self.forward(sx, sy);
            out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
        }
        out
    }

    /// Pixel size of the box around the whole corrected photo.
    pub fn bounds_size(&self) -> (u32, u32) {
        if self.is_identity() {
            return (self.width as u32, self.height as u32);
        }
        let [x0, y0, x1, y1] = self.bounds();
        // The small allowance keeps float noise, as at 90°, from adding a column.
        (
            (x1 - x0 - 1.0e-3).ceil().max(1.0) as u32,
            (y1 - y0 - 1.0e-3).ceil().max(1.0) as u32,
        )
    }

    /// The photo's centre as fractions of the box from `bounds_size`.
    pub fn origin_fraction(&self) -> [f32; 2] {
        if self.is_identity() {
            return [0.5, 0.5];
        }
        let [x0, y0, ..] = self.bounds();
        let (bw, bh) = self.bounds_size();
        [-x0 / bw as f32, -y0 / bh as f32]
    }

    /// The largest upright rectangle centred on the photo's centre that lies
    /// on the corrected photo: width, height. A turn alone has a closed form.
    pub fn usable(&self) -> (f32, f32) {
        let Some(_) = self.perspective else {
            let (_, _, uw, uh) = usable_rect(self.width, self.height, self.theta);
            return (uw, uh);
        };
        let fits = |a: f32, b: f32| {
            [(-a, -b), (a, -b), (a, b), (-a, b)]
                .iter()
                .all(|&(x, y)| self.on_photo(x, y))
        };
        // For a ratio r of half-width to half-height, the largest half-height.
        let reach = self.width.max(self.height) * 2.0;
        let largest = |r: f32| {
            let (mut lo, mut hi) = (0.0f32, reach / r.max(1.0));
            for _ in 0..32 {
                let mid = 0.5 * (lo + hi);
                if fits(mid * r, mid) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            lo
        };
        // Golden-section search for the ratio with the largest area, around
        // the photo's own ratio.
        let area = |log_r: f32| {
            let r = log_r.exp();
            let b = largest(r);
            r * b * b
        };
        let centre = (self.width / self.height).ln();
        let (mut lo, mut hi) = (centre - 1.5, centre + 1.5);
        let golden = 0.618_034f32;
        let mut x1 = hi - golden * (hi - lo);
        let mut x2 = lo + golden * (hi - lo);
        let (mut f1, mut f2) = (area(x1), area(x2));
        for _ in 0..40 {
            if f1 < f2 {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + golden * (hi - lo);
                f2 = area(x2);
            } else {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - golden * (hi - lo);
                f1 = area(x1);
            }
        }
        let r = (0.5 * (lo + hi)).exp();
        let b = largest(r);
        ((2.0 * b * r).max(1.0), (2.0 * b).max(1.0))
    }
}

/// The map that takes the photo's rectangle to the keystoned quad: the top
/// and bottom edges scale by `1 ± k·v`, the right and left by `1 ± k·h`,
/// and the centre is moved back to the centre.
fn keystone(vertical: f32, horizontal: f32, width: f32, height: f32) -> Homography {
    let kv = (KEYSTONE * vertical / 100.0) as f64;
    let kh = (KEYSTONE * horizontal / 100.0) as f64;
    let (a, b) = (width as f64 * 0.5, height as f64 * 0.5);
    let from = [(-a, -b), (a, -b), (a, b), (-a, b)];
    let to = [
        (-a * (1.0 + kv), -b * (1.0 - kh)),
        (a * (1.0 + kv), -b * (1.0 + kh)),
        (a * (1.0 - kv), b * (1.0 + kh)),
        (-a * (1.0 - kv), b * (1.0 - kh)),
    ];
    let h = homography(from, to);
    // The centre lands where the quad's diagonals cross. Move it back.
    let (tx, ty) = apply(&h, 0.0, 0.0);
    let (tx, ty) = (tx as f64, ty as f64);
    [
        h[0] - tx * h[6],
        h[1] - tx * h[7],
        h[2] - tx * h[8],
        h[3] - ty * h[6],
        h[4] - ty * h[7],
        h[5] - ty * h[8],
        h[6],
        h[7],
        h[8],
    ]
}

/// The projective map that sends four points to four points.
fn homography(from: [(f64, f64); 4], to: [(f64, f64); 4]) -> Homography {
    // u = (h0 x + h1 y + h2) / (h6 x + h7 y + 1), v likewise with h3..h5.
    let mut m = [[0.0f64; 9]; 8];
    for (i, ((x, y), (u, v))) in from.iter().zip(to.iter()).enumerate() {
        m[2 * i] = [*x, *y, 1.0, 0.0, 0.0, 0.0, -x * u, -y * u, *u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, *x, *y, 1.0, -x * v, -y * v, *v];
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..8 {
        let pivot = (col..8)
            .max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))
            .unwrap();
        m.swap(col, pivot);
        let p = m[col][col];
        for k in col..9 {
            m[col][k] /= p;
        }
        for row in 0..8 {
            if row != col {
                let f = m[row][col];
                for k in col..9 {
                    m[row][k] -= f * m[col][k];
                }
            }
        }
    }
    let mut h = [0.0; 9];
    for (i, slot) in h.iter_mut().take(8).enumerate() {
        *slot = m[i][8];
    }
    h[8] = 1.0;
    h
}

fn invert(h: &Homography) -> Homography {
    let [a, b, c, d, e, f, g, i, j] = *h;
    let co = [
        e * j - f * i,
        c * i - b * j,
        b * f - c * e,
        f * g - d * j,
        a * j - c * g,
        c * d - a * f,
        d * i - e * g,
        b * g - a * i,
        a * e - b * d,
    ];
    let det = a * co[0] + b * co[3] + c * co[6];
    co.map(|value| value / det)
}

fn apply(h: &Homography, x: f32, y: f32) -> (f32, f32) {
    let (x, y) = (x as f64, y as f64);
    let w = h[6] * x + h[7] * y + h[8];
    (
        ((h[0] * x + h[1] * y + h[2]) / w) as f32,
        ((h[3] * x + h[4] * y + h[5]) / w) as f32,
    )
}

/// The kept rectangle in canvas pixels, where the canvas has the photo's
/// size and shares its centre: left, top, width, height.
fn frame_pixels(warp: &Warp, frame: Frame) -> (f32, f32, f32, f32) {
    // The frame may reach past the usable rectangle where the corrected
    // photo does. Drags and slider changes keep its corners on the photo.
    let (uw, uh) = warp.usable();
    let ux = (warp.width - uw) * 0.5;
    let uy = (warp.height - uh) * 0.5;
    let (x, y, w, h) = if warp.is_identity() {
        // Upright and square on, the photo is the usable rectangle.
        let w = frame.w.clamp(0.0, 1.0);
        let h = frame.h.clamp(0.0, 1.0);
        (frame.x.clamp(0.0, 1.0 - w), frame.y.clamp(0.0, 1.0 - h), w, h)
    } else {
        (frame.x, frame.y, frame.w, frame.h)
    };
    (ux + x * uw, uy + y * uh, (w * uw).max(1.0), (h * uh).max(1.0))
}

/// Perspective, horizon, then frame: one resample into an image the size of
/// the frame.
fn crop_frame(image: &Image, warp: &Warp, frame: Frame) -> Image {
    let (x0, y0, fw, fh) = frame_pixels(warp, frame);
    if warp.is_identity() {
        // No turn, no keystone: whole pixels, copied without a kernel.
        let left = (x0.round() as u32).min(image.width - 1);
        let top = (y0.round() as u32).min(image.height - 1);
        let width = (fw.round() as u32).clamp(1, image.width - left);
        let height = (fh.round() as u32).clamp(1, image.height - top);
        return copy_region(image, left, top, width, height);
    }
    let out_w = fw.round().max(1.0) as u32;
    let out_h = fh.round().max(1.0) as u32;
    resample_region(image, warp, x0, y0, fw, fh, out_w, out_h)
}

/// The whole corrected photo in the box that just holds it. What lies
/// outside the photo is 0.
fn warp_canvas(image: &Image, warp: &Warp) -> Image {
    let [min_x, min_y, ..] = warp.bounds();
    let (out_w, out_h) = warp.bounds_size();
    resample_region(
        image,
        warp,
        image.width as f32 * 0.5 + min_x,
        image.height as f32 * 0.5 + min_y,
        out_w as f32,
        out_h as f32,
        out_w,
        out_h,
    )
}

fn copy_region(image: &Image, left: u32, top: u32, width: u32, height: u32) -> Image {
    let mut out = Image {
        width,
        height,
        rgb: vec![0.0; (width as usize) * (height as usize) * 3],
    };
    let src_stride = image.width as usize * 3;
    out.rgb
        .par_chunks_mut(width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let start = (top as usize + y) * src_stride + left as usize * 3;
            row.copy_from_slice(&image.rgb[start..start + row.len()]);
        });
    out
}

/// Samples the canvas region `x0, y0, fw, fh` into `out_w × out_h` pixels.
/// Coordinates are pixel edges: pixel `i` covers `[i, i + 1)`, and the
/// canvas shares the photo's centre. Each output point reads backward
/// through the warp, so the output has no holes.
#[allow(clippy::too_many_arguments)]
fn resample_region(
    image: &Image,
    warp: &Warp,
    x0: f32,
    y0: f32,
    fw: f32,
    fh: f32,
    out_w: u32,
    out_h: u32,
) -> Image {
    let mut out = Image {
        width: out_w,
        height: out_h,
        rgb: vec![0.0; (out_w as usize) * (out_h as usize) * 3],
    };
    let width = image.width as f32;
    let height = image.height as f32;
    let cx = width * 0.5;
    let cy = height * 0.5;
    out.rgb
        .par_chunks_mut(out_w as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let dy = y0 + (y as f32 + 0.5) * fh / out_h as f32 - cy;
            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                let dx = x0 + (x as f32 + 0.5) * fw / out_w as f32 - cx;
                let (sx, sy) = warp.source(dx, dy);
                let (sx, sy) = (sx + cx, sy + cy);
                if !(sx >= 0.0 && sy >= 0.0 && sx <= width && sy <= height) {
                    continue;
                }
                // Pixel centres sit at half-integers.
                let sample = sample_catmull(image, sx - 0.5, sy - 0.5);
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

/// Taps past the edge repeat the edge pixel, so a sample just inside the
/// photo is not darkened by the empty canvas around it.
fn tap(image: &Image, x: i32, y: i32) -> [f32; 3] {
    let x = x.clamp(0, image.width as i32 - 1) as u32;
    let y = y.clamp(0, image.height as i32 - 1) as u32;
    image.pixel(x, y)
}

fn catmull(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// The largest upright rectangle inside the photo turned by `theta`, centred
/// on the canvas: left, top, width, height.
pub fn usable_rect(width: f32, height: f32, theta: f32) -> (f32, f32, f32, f32) {
    let angle = theta.abs();
    if angle < 1.0e-6 {
        return (0.0, 0.0, width, height);
    }
    let sin_a = angle.sin();
    let cos_a = angle.cos();
    let width_is_longer = width >= height;
    let (side_long, side_short) = if width_is_longer {
        (width, height)
    } else {
        (height, width)
    };
    let (uw, uh) = if side_short <= 2.0 * sin_a * cos_a * side_long
        || (sin_a - cos_a).abs() < 1.0e-6
    {
        // Constrained by the short side: two corners touch the long sides.
        let x = 0.5 * side_short;
        if width_is_longer {
            (x / sin_a, x / cos_a)
        } else {
            (x / cos_a, x / sin_a)
        }
    } else {
        // All four corners touch the photo's sides.
        let cos_2a = cos_a * cos_a - sin_a * sin_a;
        (
            (width * cos_a - height * sin_a) / cos_2a,
            (height * cos_a - width * sin_a) / cos_2a,
        )
    };
    let uw = uw.clamp(1.0, width);
    let uh = uh.clamp(1.0, height);
    ((width - uw) * 0.5, (height - uh) * 0.5, uw, uh)
}

/// The usable rectangle as fractions of the on-screen picture, which is the
/// whole corrected photo in the box from `Warp::bounds_size`: left, top,
/// width, height.
pub fn usable_fraction(warp: &Warp) -> [f32; 4] {
    let (uw, uh) = warp.usable();
    let (bw, bh) = warp.bounds_size();
    let (bw, bh) = (bw as f32, bh as f32);
    let [ox, oy] = warp.origin_fraction();
    [ox - uw * 0.5 / bw, oy - uh * 0.5 / bh, uw / bw, uh / bh]
}

pub fn fit_aspect(aspect: Aspect, warp: &Warp) -> Frame {
    let (uw, uh) = warp.usable();
    match aspect.ratio(warp.width, warp.height) {
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

/// Smallest frame side, as a fraction of the usable rectangle.
const MIN_FRAME: f32 = 0.02;

/// Where a stored frame may sit. The frame is kept in fractions of the
/// usable rectangle, but it may reach past it wherever the turned photo
/// does: the rule is only that all four corners lie on the photo.
struct FrameSpace {
    uw: f32,
    uh: f32,
    warp: Warp,
}

impl FrameSpace {
    fn new(warp: &Warp) -> Self {
        let (uw, uh) = warp.usable();
        Self {
            uw,
            uh,
            warp: *warp,
        }
    }

    /// A point given in fractions of the usable rectangle lies on the photo.
    fn on_photo(&self, fx: f32, fy: f32) -> bool {
        // Offsets from the centre, which the usable rectangle shares.
        self.warp
            .on_photo((fx - 0.5) * self.uw, (fy - 0.5) * self.uh)
    }

    fn fits(&self, frame: Frame) -> bool {
        frame.w >= MIN_FRAME * 0.999
            && frame.h >= MIN_FRAME * 0.999
            && self.on_photo(frame.x, frame.y)
            && self.on_photo(frame.x + frame.w, frame.y)
            && self.on_photo(frame.x, frame.y + frame.h)
            && self.on_photo(frame.x + frame.w, frame.y + frame.h)
    }

    /// The frame furthest along `make(0) → make(1)` that still fits.
    /// `make(0)` is where the drag started and is taken to fit.
    fn furthest(&self, make: impl Fn(f32) -> Frame) -> Frame {
        if self.fits(make(1.0)) {
            return make(1.0);
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if self.fits(make(mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        make(lo)
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_frame(a: Frame, b: Frame, t: f32) -> Frame {
    Frame {
        x: lerp(a.x, b.x, t),
        y: lerp(a.y, b.y, t),
        w: lerp(a.w, b.w, t),
        h: lerp(a.h, b.h, t),
    }
}

/// Puts a stored frame back on the photo, for example after the rotation
/// changed. A locked ratio keeps the frame's centre and width, and its height
/// is recomputed in pixels of the new usable rectangle. A frame that no
/// longer fits is drawn toward the centred fit until its corners are back on
/// the photo, so a full frame stays the full usable rectangle.
pub fn clamp_frame_to_photo(frame: Frame, aspect: Aspect, warp: &Warp) -> Frame {
    let space = FrameSpace::new(warp);
    let w = frame.w.max(MIN_FRAME);
    let mut h = frame.h.max(MIN_FRAME);
    if let Some(ratio) = aspect.ratio(warp.width, warp.height) {
        h = w * space.uw / (space.uh * ratio);
    }
    let wanted = Frame {
        x: frame.x + (frame.w - w) * 0.5,
        y: frame.y + (frame.h - h) * 0.5,
        w,
        h,
    };
    if space.fits(wanted) {
        return wanted;
    }
    // The centred fit lies inside the usable rectangle, so it is on the
    // photo. Every frame between the two has its corners between two points
    // on the photo, which is convex, so the search below is sound.
    let safe = fit_aspect(aspect, warp);
    space.furthest(|t| lerp_frame(safe, wanted, t))
}

/// The part of the frame a pointer drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Move,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Handle {
    fn moves_left(self) -> bool {
        matches!(self, Handle::Left | Handle::TopLeft | Handle::BottomLeft)
    }

    fn moves_right(self) -> bool {
        matches!(self, Handle::Right | Handle::TopRight | Handle::BottomRight)
    }

    fn moves_top(self) -> bool {
        matches!(self, Handle::Top | Handle::TopLeft | Handle::TopRight)
    }

    fn moves_bottom(self) -> bool {
        matches!(self, Handle::Bottom | Handle::BottomLeft | Handle::BottomRight)
    }
}

/// The frame after dragging `handle` by `dx`, `dy`, in fractions of the
/// usable rectangle, from where it was at the press. The frame may go
/// wherever the turned photo is. An edge pulled past the photo's border
/// stops on it and stays there, which is the snap. A moved frame slides
/// along the border. A locked ratio holds in pixels.
#[allow(clippy::too_many_arguments)]
pub fn drag_frame(
    start: Frame,
    handle: Handle,
    dx: f32,
    dy: f32,
    aspect: Aspect,
    warp: &Warp,
) -> Frame {
    let space = FrameSpace::new(warp);
    // A locked frame whose shape does not match the ratio at this angle, or
    // a frame off the photo, is first put right, so the drag starts from a
    // frame that fits.
    let start = if aspect.ratio(warp.width, warp.height).is_some() || !space.fits(start) {
        clamp_frame_to_photo(start, aspect, warp)
    } else {
        start
    };
    let left = start.x;
    let top = start.y;
    let right = start.x + start.w;
    let bottom = start.y + start.h;
    if handle == Handle::Move {
        // One axis at a time, so a frame stopped by the border on one side
        // still follows the pointer along it.
        let moved = space.furthest(|t| Frame {
            x: left + dx * t,
            ..start
        });
        return space.furthest(|t| Frame {
            y: top + dy * t,
            ..moved
        });
    }
    let Some(ratio) = aspect.ratio(warp.width, warp.height) else {
        let target_left = (left + dx).min(right - MIN_FRAME);
        let target_right = (right + dx).max(left + MIN_FRAME);
        let target_top = (top + dy).min(bottom - MIN_FRAME);
        let target_bottom = (bottom + dy).max(top + MIN_FRAME);
        let across = space.furthest(|t| {
            let mut frame = start;
            if handle.moves_left() {
                frame.x = lerp(left, target_left, t);
                frame.w = right - frame.x;
            }
            if handle.moves_right() {
                frame.w = lerp(right, target_right, t) - left;
            }
            frame
        });
        return space.furthest(|t| {
            let mut frame = across;
            if handle.moves_top() {
                frame.y = lerp(top, target_top, t);
                frame.h = bottom - frame.y;
            }
            if handle.moves_bottom() {
                frame.h = lerp(bottom, target_bottom, t) - top;
            }
            frame
        });
    };
    // Locked: h = w · k, in fractions.
    let k = space.uw / (space.uh * ratio);
    let horizontal = handle.moves_left() || handle.moves_right();
    let vertical = handle.moves_top() || handle.moves_bottom();
    let w_from_x = if handle.moves_left() {
        start.w - dx
    } else {
        start.w + dx
    };
    let h_from_y = if handle.moves_top() {
        start.h - dy
    } else {
        start.h + dy
    };
    let target_w = match (horizontal, vertical) {
        // A corner follows whichever axis the pointer pulled further.
        (true, true) => w_from_x.max(h_from_y / k),
        (true, false) => w_from_x,
        _ => h_from_y / k,
    }
    .max(MIN_FRAME);
    // The anchored sides stay. An edge keeps the other axis centred.
    let sized = |w: f32| {
        let h = w * k;
        let x = if handle.moves_left() {
            right - w
        } else if handle.moves_right() {
            left
        } else {
            left + start.w * 0.5 - w * 0.5
        };
        let y = if handle.moves_top() {
            bottom - h
        } else if handle.moves_bottom() {
            top
        } else {
            top + start.h * 0.5 - h * 0.5
        };
        Frame { x, y, w, h }
    };
    space.furthest(|t| sized(lerp(start.w, target_w, t)))
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
        let clipped = clip_mask(&image);
        let out = shadows_highlights(image, &clipped, 100.0, -100.0, 1.0);
        let grey = out.pixel(w / 4, h / 2)[0];
        assert!((grey - 0.18).abs() < 2.0e-3, "mid grey moved to {grey}");
        // No bright line on the dark side next to the step.
        let near = out.pixel(w / 2 - 2, h / 2)[0];
        assert!(near < 0.2, "halo on the dark side: {near}");
    }

    fn grey_image(w: u32, h: u32, f: impl Fn(u32, u32) -> f32) -> Image {
        let mut rgb = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = f(x, y);
                rgb.extend_from_slice(&[v, v, v]);
            }
        }
        Image::new(w, h, rgb)
    }

    #[test]
    fn clarity_lifts_texture_and_leaves_a_hard_edge_alone() {
        let texture = grey_image(128, 128, |x, _| 0.5 + 0.02 * (x as f32 * 0.4).sin());
        let same = clarity(&texture, 0.0, 1.0);
        let worst = same
            .rgb
            .iter()
            .zip(&texture.rgb)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(worst < 1.0e-4, "clarity 0 moved a pixel by {worst}");

        let amplitude = |image: &Image| {
            let row: Vec<f32> = (32..96).map(|x| image.pixel(x, 64)[0]).collect();
            row.iter().cloned().fold(f32::MIN, f32::max) - row.iter().cloned().fold(f32::MAX, f32::min)
        };
        let lifted = clarity(&texture, 100.0, 1.0);
        assert!(amplitude(&lifted) > 1.3 * amplitude(&texture), "texture not lifted");

        let edge = grey_image(128, 128, |x, _| if x < 64 { 0.1 } else { 0.9 });
        let out = clarity(&edge, 100.0, 1.0);
        let (low, high) = out
            .rgb
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(low > 0.08 && high < 0.92, "halo at the edge: {low} to {high}");
    }

    #[test]
    fn a_clipped_blue_comes_back_from_its_neighbours() {
        // Left: blue sky, b / g = 1.8. Middle: brighter sky with blue clipped
        // in the file. Right: a white disk with all three clipped.
        let (w, h) = (96u32, 32u32);
        let mut rgb = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                let px = if x < 32 {
                    [0.3, 0.5, 0.9]
                } else if x < 80 {
                    [0.4, 0.6, 1.0]
                } else {
                    [1.0, 1.0, 1.0]
                };
                rgb.extend_from_slice(&px);
            }
        }
        let mut image = Image::new(w, h, rgb);
        let clipped = clip_mask(&image);
        repair_clipped(&mut image, &clipped, 16);
        let near = image.pixel(36, 16);
        assert!((near[2] - 1.08).abs() < 1.0e-3, "blue not rebuilt: {near:?}");
        assert_eq!((near[0], near[1]), (0.4, 0.6));
        // Tiles 3 to 5 hold no unclipped blue: nothing to copy.
        assert_eq!(image.pixel(70, 16)[2], 1.0);
        assert_eq!(image.pixel(90, 16), [1.0, 1.0, 1.0]);
        // Nothing clipped: the stage leaves the picture alone.
        let mut plain = flat(8, 8, [0.2, 0.3, 0.4]);
        let before = plain.rgb.clone();
        let mask = clip_mask(&plain);
        repair_clipped(&mut plain, &mask, 16);
        assert_eq!(plain.rgb, before);
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
        for (w, h, deg) in [(80u32, 60u32, 10.0f32), (80, 60, 30.0), (60, 80, -25.0), (64, 64, 45.0)] {
            let image = flat(w, h, [0.2, 0.2, 0.2]);
            let out = crop_frame(&image, &Warp::turn(deg, w as f32, h as f32), Frame::default());
            for px in out.rgb.chunks_exact(3) {
                assert!((px[0] - 0.2).abs() < 1.0e-3, "{w}x{h} at {deg}°: {px:?}");
            }
        }
    }

    /// Turns a canvas point (pixel edges) clockwise about the centre: where
    /// that point of the upright output came from in the photo.
    fn source_of(w: f32, h: f32, theta: f32, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = (x - w * 0.5, y - h * 0.5);
        (
            theta.cos() * dx - theta.sin() * dy + w * 0.5,
            theta.sin() * dx + theta.cos() * dy + h * 0.5,
        )
    }

    #[test]
    fn usable_rect_matches_the_closed_form_and_touches_the_photo() {
        // 4:3 at 10°: all four sides touch. At 30°: limited by the short side.
        let (_, _, uw, uh) = usable_rect(400.0, 300.0, 10f32.to_radians());
        assert!((uw - 363.7).abs() < 0.5 && (uh - 240.4).abs() < 0.5, "{uw} {uh}");
        let (_, _, uw, uh) = usable_rect(400.0, 300.0, 30f32.to_radians());
        assert!((uw - 300.0).abs() < 0.5 && (uh - 173.2).abs() < 0.5, "{uw} {uh}");
        for deg in [5.0f32, 10.0, 30.0, -20.0, 45.0] {
            let theta = deg.to_radians();
            let (x, y, w, h) = usable_rect(400.0, 300.0, theta);
            let mut slack = f32::MAX;
            for (cx, cy) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
                let (sx, sy) = source_of(400.0, 300.0, theta, cx, cy);
                assert!(sx > -0.01 && sy > -0.01 && sx < 400.01 && sy < 300.01, "{deg}° corner outside");
                slack = slack.min(sx).min(sy).min(400.0 - sx).min(300.0 - sy);
            }
            // At least one corner is on the photo's border: the snap line.
            assert!(slack < 0.05, "{deg}°: rectangle stops {slack} px short");
        }
    }

    #[test]
    fn positive_rotation_turns_counterclockwise() {
        // A dot right of centre moves up under a counterclockwise turn.
        let (w, h) = (101u32, 101u32);
        let mut rgb = vec![0.0; (w * h * 3) as usize];
        let i = ((50 * w + 80) * 3) as usize;
        rgb[i..i + 3].copy_from_slice(&[1.0, 1.0, 1.0]);
        let image = Image::new(w, h, rgb);
        let out = warp_canvas(&image, &Warp::turn(90.0, w as f32, h as f32));
        let brightest = out
            .rgb
            .chunks_exact(3)
            .enumerate()
            .max_by(|a, b| a.1[0].total_cmp(&b.1[0]))
            .map(|(index, _)| (index as u32 % out.width, index as u32 / out.width))
            .unwrap();
        assert_eq!((out.width, out.height), (w, h));
        assert_eq!(brightest, (50, 20));
    }

    #[test]
    fn the_turned_photo_keeps_its_corners_on_screen() {
        let image = flat(80, 40, [0.5, 0.5, 0.5]);
        let warp = Warp::turn(20.0, 80.0, 40.0);
        let out = warp_canvas(&image, &warp);
        assert_eq!((out.width, out.height), warp.bounds_size());
        assert!(out.width > 80 && out.height > 40);
        // Total light is the photo's: nothing was cut off at the sides.
        let total: f32 = out.rgb.iter().step_by(3).sum();
        assert!((total - 0.5 * 80.0 * 40.0).abs() < 0.02 * 0.5 * 80.0 * 40.0, "{total}");
        // The usable rectangle sits at the centre of the box.
        let [x, y, w, h] = usable_fraction(&warp);
        // Within the pixel the box was rounded up by.
        let (px, py) = (1.0 / out.width as f32, 1.0 / out.height as f32);
        assert!((x + w * 0.5 - 0.5).abs() < px && (y + h * 0.5 - 0.5).abs() < py);
        assert_eq!(usable_fraction(&Warp::turn(0.0, 80.0, 40.0)), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn vertical_keystone_widens_the_top_and_keeps_lines_straight() {
        let warp = Warp::new(0.0, 40.0, 0.0, 400.0, 300.0);
        let (tl, _) = warp.forward(-200.0, -150.0);
        let (bl, _) = warp.forward(-200.0, 150.0);
        // The top edge spreads to 1 + 0.5 · 0.4 of its width, the bottom
        // shrinks to 1 − 0.2, about the centre the warp keeps still.
        let top_width = 2.0 * -tl;
        let bottom_width = 2.0 * -bl;
        assert!((top_width / bottom_width - 1.2 / 0.8).abs() < 1.0e-3, "{top_width} {bottom_width}");
        let centre = warp.forward(0.0, 0.0);
        assert!(centre.0.abs() < 1.0e-3 && centre.1.abs() < 1.0e-3, "{centre:?}");
        // Three points on a slanted line stay on one line.
        let points: Vec<(f32, f32)> = [-0.8f32, 0.1, 0.9]
            .iter()
            .map(|t| warp.forward(150.0 * t, 120.0 * t - 30.0))
            .collect();
        let cross = (points[1].0 - points[0].0) * (points[2].1 - points[0].1)
            - (points[1].1 - points[0].1) * (points[2].0 - points[0].0);
        assert!(cross.abs() < 1.0e-2, "bent line: {cross}");
        // And back again.
        for (x, y) in [(-200.0f32, -150.0f32), (37.0, 81.0), (199.0, 149.0)] {
            let (fx, fy) = warp.forward(x, y);
            let (sx, sy) = warp.source(fx, fy);
            assert!((sx - x).abs() < 1.0e-2 && (sy - y).abs() < 1.0e-2);
        }
    }

    #[test]
    fn the_usable_rectangle_of_a_keystone_lies_on_the_photo_and_touches_it() {
        for (rotation, vertical, horizontal) in [(0.0, 30.0, 0.0), (0.0, 0.0, -45.0), (6.0, -20.0, 15.0)] {
            let warp = Warp::new(rotation, vertical, horizontal, 400.0, 300.0);
            let (uw, uh) = warp.usable();
            let corners = |a: f32, b: f32| [(-a, -b), (a, -b), (a, b), (-a, b)];
            for (x, y) in corners(uw * 0.5, uh * 0.5) {
                assert!(warp.on_photo(x, y), "{rotation} {vertical} {horizontal}: corner off the photo");
            }
            // One percent bigger no longer fits: it reaches the border.
            let grown = corners(uw * 0.505, uh * 0.505);
            assert!(!grown.iter().all(|&(x, y)| warp.on_photo(x, y)));
            // Square on, the full frame is the whole photo, and a crop has
            // only photo pixels.
            let image = flat(400, 300, [0.3, 0.3, 0.3]);
            let out = crop_frame(&image, &warp, Frame::default());
            assert!(out.rgb.iter().all(|v| (v - 0.3).abs() < 1.0e-3), "empty pixel in the crop");
        }
    }

    #[test]
    fn mirrors_and_turns_carry_the_corrections_with_the_picture() {
        let mut adj = Adjustments::defaults(90);
        adj.rotation = 5.0;
        adj.vertical = 10.0;
        adj.horizontal = 20.0;
        adj.frame = Frame { x: 0.1, y: 0.2, w: 0.3, h: 0.4 };
        adj.mirror_horizontally();
        assert_eq!((adj.rotation, adj.vertical, adj.horizontal), (-5.0, 10.0, -20.0));
        assert!((adj.frame.x - 0.6).abs() < 1.0e-6 && adj.frame.y == 0.2);
        adj.mirror_horizontally();
        assert_eq!((adj.rotation, adj.horizontal), (5.0, 20.0));
        adj.mirror_vertically();
        assert_eq!((adj.rotation, adj.vertical), (-5.0, -10.0));
        assert!((adj.frame.y - 0.4).abs() < 1.0e-6);
        adj.turn_clockwise(0.0, 0.0);
        // The old top is now the right side; the old right is the bottom.
        assert_eq!((adj.vertical, adj.horizontal), (-20.0, -10.0));
    }

    #[test]
    fn a_crop_at_zero_degrees_copies_pixels_exactly() {
        let rgb: Vec<f32> = (0..20 * 10 * 3).map(|i| (i % 97) as f32 / 97.0).collect();
        let image = Image::new(20, 10, rgb);
        let frame = Frame { x: 0.25, y: 0.2, w: 0.5, h: 0.5 };
        let out = crop_frame(&image, &Warp::turn(0.0, 20.0, 10.0), frame);
        assert_eq!((out.width, out.height), (10, 5));
        assert_eq!(out.pixel(0, 0), image.pixel(5, 2));
        assert_eq!(out.pixel(9, 4), image.pixel(14, 6));
    }

    #[test]
    fn dragging_an_edge_outward_stops_on_the_border() {
        let start = Frame { x: 0.2, y: 0.2, w: 0.5, h: 0.5 };
        let out = drag_frame(start, Handle::Right, 0.9, 0.0, Aspect::Free, &Warp::turn(0.0, 400.0, 300.0));
        assert!((out.x + out.w - 1.0).abs() < 1.0e-4 && out.x == 0.2, "{out:?}");
        let out = drag_frame(start, Handle::TopLeft, -0.5, -0.5, Aspect::Free, &Warp::turn(0.0, 400.0, 300.0));
        assert!(out.x.abs() < 1.0e-4 && out.y.abs() < 1.0e-4, "{out:?}");
        assert!((out.w - 0.7).abs() < 1.0e-4 && (out.h - 0.7).abs() < 1.0e-4);
    }

    #[test]
    fn a_turned_photo_lets_the_frame_reach_past_the_usable_rectangle() {
        // At 10°, the photo stands above the usable rectangle at its middle
        // and reaches past it at the sides halfway down.
        let space = FrameSpace::new(&Warp::turn(10.0, 400.0, 300.0));
        let narrow = Frame { x: 0.45, y: 0.2, w: 0.1, h: 0.6 };
        let up = drag_frame(narrow, Handle::Top, 0.0, -1.0, Aspect::Free, &Warp::turn(10.0, 400.0, 300.0));
        assert!(up.y < -0.02, "the top edge stayed on the usable rectangle: {up:?}");
        assert!(space.fits(up));
        // It stopped on the border: a little further is off the photo.
        assert!(!space.fits(Frame { y: up.y - 0.002, h: up.h + 0.002, ..up }));

        let small = Frame { x: 0.4, y: 0.45, w: 0.1, h: 0.1 };
        let left = drag_frame(small, Handle::Move, -1.0, 0.0, Aspect::Free, &Warp::turn(10.0, 400.0, 300.0));
        assert!(left.x < -0.01 && space.fits(left), "{left:?}");
        assert_eq!((left.w, left.h, left.y), (small.w, small.h, small.y));
    }

    #[test]
    fn a_locked_ratio_holds_in_pixels_while_dragging() {
        let (pw, ph) = (300.0, 200.0);
        let start = fit_aspect(Aspect::R1x1, &Warp::turn(0.0, pw, ph));
        let out = drag_frame(start, Handle::BottomRight, -0.2, -0.05, Aspect::R1x1, &Warp::turn(0.0, pw, ph));
        assert!((out.w * pw - out.h * ph).abs() < 1.0e-3, "{out:?}");
        assert!(out.x + out.w <= 1.0 + 1.0e-4 && out.y + out.h <= 1.0 + 1.0e-4);
        let grown = drag_frame(out, Handle::Left, -2.0, 0.0, Aspect::R1x1, &Warp::turn(0.0, pw, ph));
        assert!((grown.w * pw - grown.h * ph).abs() < 1.0e-2 && grown.h <= 1.0 + 1.0e-4);
        // Turned, the ratio still holds in pixels of the usable rectangle.
        let space = FrameSpace::new(&Warp::turn(12.0, pw, ph));
        let turned = drag_frame(start, Handle::TopRight, 1.0, -1.0, Aspect::R1x1, &Warp::turn(12.0, pw, ph));
        assert!(space.fits(turned));
        assert!((turned.w * space.uw - turned.h * space.uh).abs() < 1.0e-2, "{turned:?}");
    }

    #[test]
    fn a_frame_is_pulled_back_onto_the_photo_when_the_angle_grows() {
        let frame = fit_aspect(Aspect::R1x1, &Warp::turn(0.0, 400.0, 300.0));
        let turned = clamp_frame_to_photo(frame, Aspect::R1x1, &Warp::turn(20.0, 400.0, 300.0));
        let space = FrameSpace::new(&Warp::turn(20.0, 400.0, 300.0));
        assert!(space.fits(turned), "{turned:?}");
        assert!((turned.w * space.uw - turned.h * space.uh).abs() < 1.0e-2, "{turned:?}");
        // A full frame stays the full usable rectangle at any angle.
        assert_eq!(
            clamp_frame_to_photo(Frame::default(), Aspect::Free, &Warp::turn(20.0, 400.0, 300.0)),
            Frame::default()
        );
    }
}
