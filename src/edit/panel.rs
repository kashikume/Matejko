//! Right-hand edit panel. Rows are shared by the text and the pointer.

use crate::edit::pipeline::{Adjustments, Guide, ImageControls};

pub const DRAG_LONG_SIDE: u32 = 1600;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slider {
    Exposure,
    Blackpoint,
    Shadow,
    Highlight,
    Brightness,
    Contrast,
    Temperature,
    Tint,
    Vibrance,
    Saturation,
    Clarity,
    Rotation,
    Quality,
}

impl Slider {
    pub fn range(self) -> (f32, f32) {
        match self {
            Slider::Exposure => (-5.0, 5.0),
            Slider::Rotation => (-45.0, 45.0),
            Slider::Quality => (1.0, 100.0),
            _ => (-100.0, 100.0),
        }
    }

    pub fn get(self, adj: &Adjustments) -> f32 {
        match self {
            Slider::Exposure => adj.exposure_ev,
            Slider::Blackpoint => adj.blackpoint,
            Slider::Shadow => adj.shadow,
            Slider::Highlight => adj.highlight,
            Slider::Brightness => adj.brightness,
            Slider::Contrast => adj.contrast,
            Slider::Temperature => adj.temperature,
            Slider::Tint => adj.tint,
            Slider::Vibrance => adj.vibrance,
            Slider::Saturation => adj.saturation,
            Slider::Clarity => adj.clarity,
            Slider::Rotation => adj.rotation,
            Slider::Quality => adj.jpeg_quality as f32,
        }
    }

    pub fn set(self, adj: &mut Adjustments, value: f32) {
        let (min, max) = self.range();
        let value = value.clamp(min, max);
        match self {
            Slider::Exposure => adj.exposure_ev = value,
            Slider::Blackpoint => adj.blackpoint = value,
            Slider::Shadow => adj.shadow = value,
            Slider::Highlight => adj.highlight = value,
            Slider::Brightness => adj.brightness = value,
            Slider::Contrast => adj.contrast = value,
            Slider::Temperature => adj.temperature = value,
            Slider::Tint => adj.tint = value,
            Slider::Vibrance => adj.vibrance = value,
            Slider::Saturation => adj.saturation = value,
            Slider::Clarity => adj.clarity = value,
            Slider::Rotation => adj.rotation = value,
            Slider::Quality => adj.jpeg_quality = value.round().clamp(1.0, 100.0) as u8,
        }
    }

    /// Where the fill starts: the centre for a signed slider.
    pub fn origin(self) -> f32 {
        match self {
            Slider::Quality => 1.0,
            _ => 0.0,
        }
    }

    /// Values snap to this step so a drag can land exactly on 0.
    fn step(self) -> f32 {
        match self {
            Slider::Exposure => 0.05,
            Slider::Rotation => 0.1,
            _ => 1.0,
        }
    }

    pub fn snap(self, value: f32) -> f32 {
        let step = self.step();
        let (min, max) = self.range();
        ((value / step).round() * step).clamp(min, max)
    }

    fn label(self) -> &'static str {
        match self {
            Slider::Exposure => "Exposure",
            Slider::Blackpoint => "Blackpoint",
            Slider::Shadow => "Shadow",
            Slider::Highlight => "Highlight",
            Slider::Brightness => "Brightness",
            Slider::Contrast => "Contrast",
            Slider::Temperature => "Temperature",
            Slider::Tint => "Tint",
            Slider::Vibrance => "Vibrance",
            Slider::Saturation => "Saturation",
            Slider::Clarity => "Clarity",
            Slider::Rotation => "Rotation",
            Slider::Quality => "JPEG quality",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Divider,
    Guide,
    MirrorH,
    MirrorV,
    Turn,
    Aspect,
    Slider(Slider),
    Compare(ImageControls),
    Save,
    SaveCopy,
    Exit,
    ConfirmYes,
    ConfirmNo,
}

/// The drawn part of a slider row, right of its label.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    pub slider: Slider,
    pub x0: f32,
    pub x1: f32,
    /// Vertical centre of the row.
    pub y: f32,
}

impl Track {
    pub fn x_of(&self, value: f32) -> f32 {
        let (min, max) = self.slider.range();
        let t = ((value - min) / (max - min)).clamp(0.0, 1.0);
        self.x0 + t * (self.x1 - self.x0)
    }

    pub fn value_at(&self, x: f32) -> f32 {
        let (min, max) = self.slider.range();
        let t = ((x - self.x0) / (self.x1 - self.x0).max(1.0)).clamp(0.0, 1.0);
        min + t * (max - min)
    }

    /// Value change for a horizontal pointer move of `dx` pixels.
    pub fn delta(&self, dx: f32) -> f32 {
        let (min, max) = self.slider.range();
        dx / (self.x1 - self.x0).max(1.0) * (max - min)
    }
}

pub struct Panel {
    pub x: f32,
    pub scale: f32,
    /// x0, x1, y0, y1
    pub rows: Vec<(Hit, f32, f32, f32, f32)>,
    pub tracks: Vec<Track>,
    /// Backgrounds for the button rows: x0, y0, x1, y1.
    pub buttons: Vec<[f32; 4]>,
    pub text: String,
    pub text_top: f32,
    pub histogram: [f32; 4],
}

impl Panel {
    pub fn track(&self, slider: Slider) -> Option<Track> {
        self.tracks.iter().copied().find(|track| track.slider == slider)
    }
}

struct Builder {
    rows: Vec<(Hit, f32, f32, f32, f32)>,
    tracks: Vec<Track>,
    buttons: Vec<[f32; 4]>,
    lines: Vec<String>,
    y: f32,
    row_h: f32,
    x0: f32,
    x1: f32,
    track_x0: f32,
    scale: f32,
}

impl Builder {
    fn heading(&mut self, label: &str) {
        self.lines.push(label.to_uppercase());
        self.y += self.row_h;
    }

    fn button(&mut self, hit: Hit, label: String) {
        let inset = 2.0 * self.scale;
        self.buttons
            .push([self.x0, self.y + inset, self.x1, self.y + self.row_h - inset]);
        self.row(hit, label);
    }

    fn slider(&mut self, slider: Slider, adj: &Adjustments) {
        self.tracks.push(Track {
            slider,
            x0: self.track_x0,
            x1: self.x1 - 6.0 * self.scale,
            y: self.y + self.row_h * 0.5,
        });
        self.row(Hit::Slider(slider), slider_line(slider, adj));
    }

    fn row(&mut self, hit: Hit, label: String) {
        self.rows
            .push((hit, self.x0, self.x1, self.y, self.y + self.row_h));
        self.lines.push(label);
        self.y += self.row_h;
    }
}

pub fn layout(
    window_w: f32,
    window_h: f32,
    scale: f32,
    split: f32,
    adj: &Adjustments,
    guide: Guide,
    confirm_exit: bool,
    copy_name: Option<&str>,
    message: Option<&str>,
) -> Panel {
    let scale = scale.max(1.0);
    let x = window_w * (1.0 - split.clamp(0.18, 0.5));
    let top = 8.0 * scale;
    let hist_h = 72.0 * scale;
    let x0 = x + 8.0;
    let x1 = window_w - 8.0;
    let histogram = [x0, top, x1, top + hist_h];
    let width = x1 - x0;
    // Labels need about 120 px at 15 px type. The track keeps at least 60.
    let label_w = (width * 0.45)
        .max(120.0 * scale)
        .min(width - 60.0 * scale)
        .max(0.0);
    let mut b = Builder {
        rows: Vec::new(),
        tracks: Vec::new(),
        buttons: Vec::new(),
        lines: Vec::new(),
        y: histogram[3] + 8.0,
        row_h: 22.0 * scale,
        x0,
        x1,
        track_x0: x0 + label_w,
        scale,
    };
    let text_top = b.y;
    b.button(Hit::Guide, format!("Guide  {}", guide.label()));
    b.button(Hit::MirrorH, format!("Mirror horizontal  {}", mark(adj.mirror_horizontal)));
    b.button(Hit::MirrorV, format!("Mirror vertical  {}", mark(adj.mirror_vertical)));
    b.button(Hit::Turn, "Rotate 90°".into());
    b.heading("Light");
    for slider in [
        Slider::Exposure,
        Slider::Blackpoint,
        Slider::Shadow,
        Slider::Highlight,
        Slider::Brightness,
        Slider::Contrast,
    ] {
        b.slider(slider, adj);
    }
    b.heading("Color");
    for slider in [Slider::Temperature, Slider::Tint, Slider::Vibrance, Slider::Saturation] {
        b.slider(slider, adj);
    }
    b.heading("Detail");
    b.slider(Slider::Clarity, adj);
    b.heading("Frame");
    b.button(Hit::Aspect, format!("Aspect  {}", adj.aspect.label()));
    b.slider(Slider::Rotation, adj);
    b.lines.push(String::new());
    b.y += b.row_h;
    b.slider(Slider::Quality, adj);
    b.lines.push("O    Of    S    Sf".into());
    let compare = [
        Hit::Compare(ImageControls::Original),
        Hit::Compare(ImageControls::OriginalFramed),
        Hit::Compare(ImageControls::Saved),
        Hit::Compare(ImageControls::SavedFramed),
    ];
    let span = (x1 - x0) / 4.0;
    for (index, hit) in compare.into_iter().enumerate() {
        let left = x0 + span * index as f32;
        b.rows.push((hit, left, left + span, b.y, b.y + b.row_h));
    }
    b.y += b.row_h;
    if confirm_exit {
        b.button(Hit::ConfirmYes, "Discard changes".into());
        b.button(Hit::ConfirmNo, "Keep editing".into());
    } else {
        b.button(Hit::Save, "Save".into());
        b.button(Hit::SaveCopy, "Save as copy".into());
        b.button(Hit::Exit, "Exit".into());
    }
    if let Some(name) = copy_name {
        b.lines.push(format!("Copy name  {name}_"));
    }
    if let Some(message) = message {
        b.lines.push(message.to_string());
    }
    let _ = window_h;
    Panel {
        x,
        scale,
        rows: b.rows,
        tracks: b.tracks,
        buttons: b.buttons,
        text: b.lines.join("\n"),
        text_top,
        histogram,
    }
}

fn mark(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn slider_line(slider: Slider, adj: &Adjustments) -> String {
    let value = slider.get(adj);
    let shown = match slider {
        Slider::Quality => format!("{value:.0}"),
        Slider::Exposure => format!("{value:+.2}"),
        Slider::Rotation => format!("{value:+.1}°"),
        _ => format!("{:+.0}", value),
    };
    // `+0` reads as a change. Zero is shown bare.
    let shown = match shown.as_str() {
        "+0" | "-0" => "0".into(),
        "+0.00" | "-0.00" => "0.00".into(),
        "+0.0°" | "-0.0°" => "0.0°".into(),
        _ => shown,
    };
    format!("{}  {shown}", slider.label())
}

pub fn hit_test(panel: &Panel, cursor_x: f32, cursor_y: f32) -> Option<Hit> {
    if (cursor_x - panel.x).abs() <= 6.0 {
        return Some(Hit::Divider);
    }
    if cursor_x < panel.x {
        return None;
    }
    for (hit, x0, x1, y0, y1) in &panel.rows {
        if cursor_x >= *x0 && cursor_x < *x1 && cursor_y >= *y0 && cursor_y < *y1 {
            return Some(*hit);
        }
    }
    None
}

/// Screen rectangle of the frame inside the displayed picture.
pub fn frame_screen(
    pan_x: f32,
    pan_y: f32,
    image_w: f32,
    image_h: f32,
    scale: f32,
    frame_x: f32,
    frame_y: f32,
    frame_w: f32,
    frame_h: f32,
) -> [f32; 4] {
    let left = pan_x + frame_x * image_w * scale;
    let top = pan_y + frame_y * image_h * scale;
    let right = left + frame_w * image_w * scale;
    let bottom = top + frame_h * image_h * scale;
    [left, top, right, bottom]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_sit_right_of_the_labels_and_map_both_ways() {
        let adj = Adjustments::defaults(90);
        let panel = layout(1600.0, 1000.0, 1.0, 0.25, &adj, Guide::None, false, None, None);
        let track = panel.track(Slider::Contrast).unwrap();
        assert!(track.x0 > panel.x + 100.0 && track.x1 < 1600.0);
        let mid = track.x_of(0.0);
        assert!((track.value_at(mid)).abs() < 1.0e-3);
        assert!((track.delta(track.x1 - track.x0) - 200.0).abs() < 1.0e-3);
        assert_eq!(panel.tracks.len(), 13);
    }

    #[test]
    fn snapping_lands_on_zero() {
        assert_eq!(Slider::Contrast.snap(0.4), 0.0);
        assert_eq!(Slider::Exposure.snap(0.02), 0.0);
        assert!((Slider::Exposure.snap(0.33) - 0.35).abs() < 1.0e-5);
    }
}
