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
    pub fn step(self) -> f32 {
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

/// A button's background. Its label is in `Panel::texts`.
#[derive(Clone, Copy, Debug)]
pub struct Button {
    /// x0, y0, x1, y1
    pub rect: [f32; 4],
    /// A toggle that is on, such as a mirror.
    pub on: bool,
}

/// One of the O / Of / S / Sf buttons. Its label is in `Panel::texts`.
#[derive(Clone, Copy, Debug)]
pub struct CompareButton {
    pub rect: [f32; 4],
    pub enabled: bool,
    pub held: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    /// A group title.
    Heading,
    /// A button that does nothing yet.
    Disabled,
    /// A message under the buttons. It may wrap.
    Notice,
}

/// A line of panel text, placed on its own.
#[derive(Clone, Debug)]
pub struct Text {
    pub text: String,
    pub x0: f32,
    pub x1: f32,
    pub top: f32,
    pub align: Align,
    pub tone: Tone,
    /// The row this text names, so a highlighted row can brighten it.
    pub row: Option<Hit>,
}

pub struct Panel {
    pub x: f32,
    pub scale: f32,
    /// x0, x1, y0, y1
    pub rows: Vec<(Hit, f32, f32, f32, f32)>,
    pub tracks: Vec<Track>,
    pub buttons: Vec<Button>,
    pub compare: Vec<CompareButton>,
    pub texts: Vec<Text>,
    pub histogram: [f32; 4],
}

impl Panel {
    /// The pointer is on the divider between the picture and the panel.
    pub fn on_divider(&self, cursor_x: f32) -> bool {
        (cursor_x - self.x).abs() <= 6.0 * self.scale
    }

    pub fn track(&self, slider: Slider) -> Option<Track> {
        self.tracks.iter().copied().find(|track| track.slider == slider)
    }

    /// The row's rectangle: x0, y0, x1, y1.
    pub fn row_rect(&self, hit: Hit) -> Option<[f32; 4]> {
        self.rows
            .iter()
            .find(|row| row.0 == hit)
            .map(|&(_, x0, x1, y0, y1)| [x0, y0, x1, y1])
    }
}

/// Height of one line of panel text, at scale 1. The GPU text layer lays
/// labels out with the same line height.
pub const LINE_HEIGHT: f32 = 22.0;

struct Builder {
    rows: Vec<(Hit, f32, f32, f32, f32)>,
    tracks: Vec<Track>,
    buttons: Vec<Button>,
    texts: Vec<Text>,
    y: f32,
    row_h: f32,
    x0: f32,
    x1: f32,
    track_x0: f32,
    scale: f32,
}

impl Builder {
    fn text(&mut self, text: String, x0: f32, x1: f32, align: Align, tone: Tone, row: Option<Hit>) {
        let top = self.y + (self.row_h - LINE_HEIGHT * self.scale) * 0.5;
        self.texts.push(Text {
            text,
            x0,
            x1,
            top,
            align,
            tone,
            row,
        });
    }

    /// A group title, with room above it to set the group apart.
    fn heading(&mut self, label: &str) {
        self.y += 10.0 * self.scale;
        self.text(label.to_uppercase(), self.x0, self.x1, Align::Left, Tone::Heading, None);
        self.y += self.row_h;
    }

    fn button(&mut self, hit: Hit, label: String) {
        let inset = 2.0 * self.scale;
        self.buttons.push(Button {
            rect: [self.x0, self.y + inset, self.x1, self.y + self.row_h - inset],
            on: false,
        });
        let pad = 8.0 * self.scale;
        self.text(label, self.x0 + pad, self.x1 - pad, Align::Left, Tone::Normal, Some(hit));
        self.row(hit);
    }

    /// Buttons side by side, sharing the row's width. A toggle that is on
    /// shows by its colour, not by its label.
    fn button_row(&mut self, buttons: &[(Hit, &str, bool)]) {
        let gap = 4.0 * self.scale;
        let inset = 2.0 * self.scale;
        let span = (self.x1 - self.x0 + gap) / buttons.len() as f32;
        for (index, (hit, label, on)) in buttons.iter().enumerate() {
            let left = self.x0 + span * index as f32;
            let right = left + span - gap;
            self.buttons.push(Button {
                rect: [left, self.y + inset, right, self.y + self.row_h - inset],
                on: *on,
            });
            self.text((*label).into(), left, right, Align::Center, Tone::Normal, Some(*hit));
            self.rows.push((*hit, left, right, self.y, self.y + self.row_h));
        }
        self.y += self.row_h;
    }

    /// Title on the left, value right-aligned against the track.
    fn slider(&mut self, slider: Slider, adj: &Adjustments) {
        let hit = Hit::Slider(slider);
        let value_x1 = self.track_x0 - 10.0 * self.scale;
        self.text(slider.label().into(), self.x0, value_x1, Align::Left, Tone::Normal, Some(hit));
        self.text(slider_value(slider, adj), self.x0, value_x1, Align::Right, Tone::Normal, Some(hit));
        self.tracks.push(Track {
            slider,
            x0: self.track_x0,
            x1: self.x1 - 6.0 * self.scale,
            y: self.y + self.row_h * 0.5,
        });
        self.row(hit);
    }

    fn row(&mut self, hit: Hit) {
        self.rows
            .push((hit, self.x0, self.x1, self.y, self.y + self.row_h));
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
    has_save: bool,
    holding: Option<ImageControls>,
    confirm_exit: bool,
    copy_name: Option<&str>,
    message: Option<&str>,
) -> Panel {
    let scale = scale.max(1.0);
    let x = window_w * (1.0 - split.clamp(0.18, 0.5));
    let top = 10.0 * scale;
    let hist_h = 72.0 * scale;
    // Controls sit in from the divider, clear of its grip.
    let x0 = x + 18.0 * scale;
    let x1 = window_w - 12.0 * scale;
    let histogram = [x0, top, x1, top + hist_h];
    let width = x1 - x0;
    // A title and its value need about 150 px at 15 px type. The track keeps
    // at least 60.
    let label_w = (width * 0.5)
        .max(150.0 * scale)
        .min(width - 60.0 * scale)
        .max(0.0);
    let mut b = Builder {
        rows: Vec::new(),
        tracks: Vec::new(),
        buttons: Vec::new(),
        texts: Vec::new(),
        y: histogram[3] + 10.0 * scale,
        row_h: LINE_HEIGHT * scale,
        x0,
        x1,
        track_x0: x0 + label_w,
        scale,
    };
    b.button(Hit::Guide, format!("Guide  {}", guide.label()));
    b.button_row(&[
        (Hit::MirrorH, "Mirror H", adj.mirror_horizontal),
        (Hit::MirrorV, "Mirror V", adj.mirror_vertical),
        (Hit::Turn, "Rotate 90°", false),
    ]);
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
    b.heading("Output");
    b.slider(Slider::Quality, adj);
    b.y += 10.0 * scale;
    let gap = 4.0 * scale;
    let inset = 2.0 * scale;
    let span = (x1 - x0 + gap) / 4.0;
    let mut compare = Vec::new();
    for (index, (which, text)) in [
        (ImageControls::Original, "O"),
        (ImageControls::OriginalFramed, "Of"),
        (ImageControls::Saved, "S"),
        (ImageControls::SavedFramed, "Sf"),
    ]
    .into_iter()
    .enumerate()
    {
        let left = x0 + span * index as f32;
        let right = left + span - gap;
        // S and Sf need a save of the open file.
        let enabled =
            has_save || matches!(which, ImageControls::Original | ImageControls::OriginalFramed);
        let hit = Hit::Compare(which);
        if enabled {
            b.rows.push((hit, left, right, b.y, b.y + b.row_h));
        }
        let tone = if enabled { Tone::Normal } else { Tone::Disabled };
        b.text(text.into(), left, right, Align::Center, tone, Some(hit));
        compare.push(CompareButton {
            rect: [left, b.y + inset, right, b.y + b.row_h - inset],
            enabled,
            held: holding == Some(which),
        });
    }
    b.y += b.row_h + 6.0 * scale;
    if confirm_exit {
        b.button(Hit::ConfirmYes, "Discard changes".into());
        b.button(Hit::ConfirmNo, "Keep editing".into());
    } else {
        b.button(Hit::Save, "Save".into());
        b.button(Hit::SaveCopy, "Save as copy".into());
        b.button(Hit::Exit, "Exit".into());
    }
    b.y += 4.0 * scale;
    for line in copy_name
        .map(|name| format!("Copy name  {name}_"))
        .into_iter()
        .chain(message.map(str::to_string))
    {
        b.text(line, x0, x1, Align::Left, Tone::Notice, None);
        b.y += b.row_h;
    }
    let _ = window_h;
    Panel {
        x,
        scale,
        rows: b.rows,
        tracks: b.tracks,
        buttons: b.buttons,
        compare,
        texts: b.texts,
        histogram,
    }
}

fn slider_value(slider: Slider, adj: &Adjustments) -> String {
    let value = slider.get(adj);
    let shown = match slider {
        Slider::Quality => format!("{value:.0}"),
        Slider::Exposure => format!("{value:+.2}"),
        Slider::Rotation => format!("{value:+.1}°"),
        _ => format!("{:+.0}", value),
    };
    // `+0` reads as a change. Zero is shown bare.
    match shown.as_str() {
        "+0" | "-0" => "0".into(),
        "+0.00" | "-0.00" => "0.00".into(),
        "+0.0°" | "-0.0°" => "0.0°".into(),
        _ => shown,
    }
}

pub fn hit_test(panel: &Panel, cursor_x: f32, cursor_y: f32) -> Option<Hit> {
    if panel.on_divider(cursor_x) {
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_sit_right_of_the_labels_and_map_both_ways() {
        let adj = Adjustments::defaults(90);
        let panel = layout(
            1600.0, 1000.0, 1.0, 0.25, &adj, Guide::None, false, None, false, None, None,
        );
        let track = panel.track(Slider::Contrast).unwrap();
        assert!(track.x0 > panel.x + 100.0 && track.x1 < 1600.0);
        let mid = track.x_of(0.0);
        assert!((track.value_at(mid)).abs() < 1.0e-3);
        assert!((track.delta(track.x1 - track.x0) - 200.0).abs() < 1.0e-3);
        assert_eq!(panel.tracks.len(), 13);
    }

    #[test]
    fn rows_have_a_margin_and_groups_have_room_above_their_titles() {
        let adj = Adjustments::defaults(90);
        let panel = layout(
            1600.0, 1000.0, 1.0, 0.25, &adj, Guide::None, false, None, false, None, None,
        );
        // Every control starts clear of the divider.
        for (_, x0, ..) in &panel.rows {
            assert!(*x0 >= panel.x + 16.0, "control at {x0}, divider at {}", panel.x);
        }
        // A slider row has its title on the left and its value right-aligned
        // before the track.
        let exposure = Some(Hit::Slider(Slider::Exposure));
        let texts: Vec<&Text> = panel.texts.iter().filter(|text| text.row == exposure).collect();
        assert_eq!(texts.len(), 2);
        assert_eq!((texts[0].text.as_str(), texts[0].align), ("Exposure", Align::Left));
        assert_eq!((texts[1].text.as_str(), texts[1].align), ("0.00", Align::Right));
        assert!(texts[1].x1 < panel.track(Slider::Exposure).unwrap().x0);
        // The gap above "COLOR" is wider than the gap between two rows.
        let contrast = panel.row_rect(Hit::Slider(Slider::Contrast)).unwrap();
        let temperature = panel.row_rect(Hit::Slider(Slider::Temperature)).unwrap();
        let heading = panel.texts.iter().find(|text| text.text == "COLOR").unwrap();
        assert!(heading.top > contrast[3] + 5.0);
        assert!(temperature[1] > heading.top + LINE_HEIGHT - 1.0);
    }

    #[test]
    fn mirrors_and_the_turn_share_one_row() {
        let mut adj = Adjustments::defaults(90);
        adj.mirror_vertical = true;
        let panel = layout(
            1600.0, 1000.0, 1.0, 0.25, &adj, Guide::None, false, None, false, None, None,
        );
        let h = panel.row_rect(Hit::MirrorH).unwrap();
        let v = panel.row_rect(Hit::MirrorV).unwrap();
        let turn = panel.row_rect(Hit::Turn).unwrap();
        assert!(h[1] == v[1] && v[1] == turn[1], "not one row");
        assert!(h[2] < v[0] && v[2] < turn[0], "buttons overlap");
        // The label names the button. The colour says whether it is on.
        assert!(panel.texts.iter().any(|text| text.text == "Mirror V"));
        assert!(!panel.texts.iter().any(|text| text.text.contains(" on")));
        let on: Vec<bool> = panel
            .buttons
            .iter()
            .filter(|button| button.rect[1] > h[1] && button.rect[3] < h[3])
            .map(|button| button.on)
            .collect();
        assert_eq!(on, [false, true, false]);
    }

    #[test]
    fn s_and_sf_wait_for_a_save() {
        let adj = Adjustments::defaults(90);
        let compare_hits = |has_save| {
            layout(1600.0, 1000.0, 1.0, 0.25, &adj, Guide::None, has_save, None, false, None, None)
                .rows
                .iter()
                .filter(|row| matches!(row.0, Hit::Compare(_)))
                .count()
        };
        assert_eq!(compare_hits(false), 2);
        assert_eq!(compare_hits(true), 4);
    }

    #[test]
    fn snapping_lands_on_zero() {
        assert_eq!(Slider::Contrast.snap(0.4), 0.0);
        assert_eq!(Slider::Exposure.snap(0.02), 0.0);
        assert!((Slider::Exposure.snap(0.33) - 0.35).abs() < 1.0e-5);
    }
}
