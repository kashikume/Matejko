//! Layout of the video bar. Hit testing uses the same rectangles the GPU draws.

use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    pub fn width(self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }

    pub fn height(self) -> f32 {
        (self.y1 - self.y0).max(0.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub bar: Rect,
    pub button: Rect,
    pub track: Rect,
    pub fill: Rect,
    pub label_x: f32,
    pub label_top: f32,
    pub label_size: f32,
    pub label_line: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Button,
    /// 0 at the left of the track, 1 at the right.
    Track(f32),
    /// The rest of the bar. A press here does not pan the picture.
    Chrome,
}

pub fn fraction(position: Duration, duration: Duration) -> f32 {
    let total = duration.as_secs_f64();
    if total <= 0.0 {
        0.0
    } else {
        (position.as_secs_f64() / total).clamp(0.0, 1.0) as f32
    }
}

pub fn format_clock(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub fn format_clock_range(position: Duration, duration: Duration) -> String {
    format!("{} / {}", format_clock(position), format_clock(duration))
}

/// `fraction` is how full the track is, from 0 to 1. Coordinates are physical pixels.
pub fn layout(window_w: f32, window_h: f32, scale: f32, fraction: f32) -> Layout {
    let scale = scale.max(1.0);
    let height = (48.0 * scale).round().max(32.0);
    let margin = (12.0 * scale).round().max(8.0);
    let button = (28.0 * scale).round().max(20.0);
    let label_size = 16.0 * scale;
    let label_line = 20.0 * scale;
    let label_w = (168.0 * scale).round();
    let y0 = (window_h - height).max(0.0);
    let bar = Rect {
        x0: 0.0,
        y0,
        x1: window_w.max(0.0),
        y1: window_h.max(y0),
    };
    let button_y = y0 + (height - button) / 2.0;
    let button_rect = Rect {
        x0: margin,
        y0: button_y,
        x1: margin + button,
        y1: button_y + button,
    };
    let track_x = button_rect.x1 + margin;
    let track_right = (window_w - margin - label_w).max(track_x + 8.0);
    let track_h = (4.0 * scale).round().max(4.0);
    let track_y = y0 + (height - track_h) / 2.0;
    let track = Rect {
        x0: track_x,
        y0: track_y,
        x1: track_right.min(window_w.max(track_x)),
        y1: track_y + track_h,
    };
    let fill_w = track.width() * fraction.clamp(0.0, 1.0);
    let fill = Rect {
        x0: track.x0,
        y0: track.y0,
        x1: track.x0 + fill_w,
        y1: track.y1,
    };
    Layout {
        bar,
        button: button_rect,
        track,
        fill,
        label_x: track.x1 + margin * 0.5,
        label_top: y0 + (height - label_line) / 2.0,
        label_size,
        label_line,
    }
}

pub fn hit(layout: &Layout, x: f32, y: f32) -> Option<Hit> {
    if layout.button.contains(x, y) {
        return Some(Hit::Button);
    }
    let grab = Rect {
        x0: layout.track.x0,
        y0: layout.bar.y0,
        x1: layout.track.x1,
        y1: layout.bar.y1,
    };
    if grab.contains(x, y) {
        return Some(Hit::Track(fraction_at(layout, x)));
    }
    if layout.bar.contains(x, y) {
        return Some(Hit::Chrome);
    }
    None
}

pub fn fraction_at(layout: &Layout, x: f32) -> f32 {
    let width = layout.track.width().max(1.0);
    ((x - layout.track.x0) / width).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_drop_the_hour_until_it_is_needed() {
        assert_eq!(format_clock(Duration::from_secs(0)), "0:00");
        assert_eq!(format_clock(Duration::from_secs(65)), "1:05");
        assert_eq!(format_clock(Duration::from_secs(3661)), "1:01:01");
        assert_eq!(
            format_clock_range(Duration::from_secs(5), Duration::from_secs(90)),
            "0:05 / 1:30"
        );
    }

    #[test]
    fn the_track_seeks_and_the_picture_area_does_not() {
        let layout = layout(800.0, 600.0, 1.0, 0.25);
        assert!(layout.bar.y0 < 600.0);
        assert_eq!(layout.bar.y1, 600.0);
        assert!(layout.track.x0 > layout.button.x1);
        let filled = layout.fill.width() / layout.track.width();
        assert!((filled - 0.25).abs() < 0.02, "{filled}");

        assert_eq!(
            hit(&layout, layout.button.x0 + 1.0, layout.button.y0 + 1.0),
            Some(Hit::Button)
        );
        assert!(matches!(
            hit(
                &layout,
                layout.track.x0 + 1.0,
                (layout.bar.y0 + layout.bar.y1) / 2.0
            ),
            Some(Hit::Track(fraction)) if fraction < 0.05
        ));
        let mid = (layout.track.x0 + layout.track.x1) / 2.0;
        assert!(matches!(
            hit(&layout, mid, layout.bar.y0 + 1.0),
            Some(Hit::Track(fraction)) if (fraction - 0.5).abs() < 0.05
        ));
        assert_eq!(hit(&layout, 10.0, 10.0), None);
        assert_eq!(
            hit(&layout, layout.label_x + 4.0, layout.bar.y0 + 4.0),
            Some(Hit::Chrome)
        );
    }
}
