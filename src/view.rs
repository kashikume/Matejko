//! Fit, zoom, and pan. Scale is physical pixels per image pixel.

pub const MAX_SCALE: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub scale: f32,
    pub pan: Vec2,
    pub fitted: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            scale: 1.0,
            pan: Vec2::new(0.0, 0.0),
            fitted: true,
        }
    }
}

impl View {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn image_point(&self, cursor: Vec2) -> Vec2 {
        Vec2::new(
            (cursor.x - self.pan.x) / self.scale,
            (cursor.y - self.pan.y) / self.scale,
        )
    }

    /// Place the whole image in the window and return to fit mode.
    pub fn fit(&mut self, image: Vec2, window: Vec2) {
        let (scale, pan) = fit_transform(image, window);
        self.scale = scale;
        self.pan = pan;
        self.fitted = true;
    }

    /// Wheel zoom. The image point under `cursor` stays under the cursor.
    /// Zoom-out stops at the fitted size, so the picture never becomes
    /// smaller than the window on both axes. Zoom-in stops at `MAX_SCALE`.
    pub fn zoom_at(&mut self, cursor: Vec2, factor: f32, image: Vec2, window: Vec2) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let (fit_scale, _) = fit_transform(image, window);
        if !fit_scale.is_finite() || fit_scale <= 0.0 {
            return;
        }
        let raw = self.scale * factor;
        if factor < 1.0 {
            if self.scale <= fit_scale {
                return;
            }
            if raw <= fit_scale {
                self.fit(image, window);
                return;
            }
        }
        let scale = raw.min(MAX_SCALE.max(fit_scale));
        if (scale - self.scale).abs() <= 1e-4 * self.scale.max(1.0) {
            return;
        }
        let image_point = self.image_point(cursor);
        self.scale = scale;
        self.pan = Vec2::new(
            cursor.x - image_point.x * scale,
            cursor.y - image_point.y * scale,
        );
        self.fitted = false;
    }

    /// Move the image with the pointer. A zero delta does not leave fit mode.
    pub fn pan_by(&mut self, delta: Vec2) {
        if delta.x == 0.0 && delta.y == 0.0 {
            return;
        }
        self.pan.x += delta.x;
        self.pan.y += delta.y;
        self.fitted = false;
    }

    pub fn on_resize(&mut self, old: Vec2, new: Vec2, image: Option<Vec2>) {
        if new.x <= 0.0 || new.y <= 0.0 {
            return;
        }
        if self.fitted {
            if let Some(image) = image {
                self.fit(image, new);
            }
            return;
        }
        if old.x <= 0.0 || old.y <= 0.0 {
            return;
        }
        let image_point = self.image_point(Vec2::new(old.x / 2.0, old.y / 2.0));
        self.pan = Vec2::new(
            new.x / 2.0 - image_point.x * self.scale,
            new.y / 2.0 - image_point.y * self.scale,
        );
    }
}

pub fn fit_transform(image: Vec2, window: Vec2) -> (f32, Vec2) {
    if image.x <= 0.0 || image.y <= 0.0 || window.x <= 0.0 || window.y <= 0.0 {
        return (1.0, Vec2::new(0.0, 0.0));
    }
    let scale = (window.x / image.x).min(window.y / image.y);
    let pan = Vec2::new(
        (window.x - image.x * scale) / 2.0,
        (window.y - image.y * scale) / 2.0,
    );
    (scale, pan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn fit_letterboxes_a_wide_image() {
        let mut view = View::new();
        view.fit(Vec2::new(400.0, 200.0), Vec2::new(200.0, 200.0));
        assert!(close(view.scale, 0.5));
        assert!(close(view.pan.x, 0.0));
        assert!(close(view.pan.y, 50.0));
        assert!(view.fitted);
    }

    #[test]
    fn fit_may_exceed_the_wheel_clamp() {
        let mut view = View::new();
        view.fit(Vec2::new(10.0, 10.0), Vec2::new(1000.0, 1000.0));
        assert!(view.scale > MAX_SCALE);
        assert!(view.fitted);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut view = View::new();
        view.fit(Vec2::new(100.0, 50.0), Vec2::new(200.0, 200.0));
        let cursor = Vec2::new(10.0, 60.0);
        let image = Vec2::new(100.0, 50.0);
        let window = Vec2::new(200.0, 200.0);
        let before = view.image_point(cursor);
        view.zoom_at(cursor, 1.1, image, window);
        let after = view.image_point(cursor);
        assert!(close(before.x, after.x));
        assert!(close(before.y, after.y));
        assert!(!view.fitted);
    }

    #[test]
    fn zoom_clamps_and_still_anchors_on_the_cursor() {
        let mut view = View::new();
        view.scale = 30.0;
        view.pan = Vec2::new(0.0, 0.0);
        view.fitted = false;
        let cursor = Vec2::new(40.0, 20.0);
        let before = view.image_point(cursor);
        view.zoom_at(cursor, 4.0, Vec2::new(10.0, 10.0), Vec2::new(10.0, 10.0));
        assert!(close(view.scale, MAX_SCALE));
        let after = view.image_point(cursor);
        assert!(close(before.x, after.x));
        assert!(close(before.y, after.y));
    }

    #[test]
    fn zero_drag_stays_fitted() {
        let mut view = View::new();
        view.fitted = true;
        view.pan_by(Vec2::new(0.0, 0.0));
        assert!(view.fitted);
        view.pan_by(Vec2::new(3.0, -2.0));
        assert!(!view.fitted);
        assert!(close(view.pan.x, 3.0));
        assert!(close(view.pan.y, -2.0));
        assert!(close(view.scale, 1.0));
    }

    #[test]
    fn resize_refits_until_the_user_zooms() {
        let mut view = View::new();
        let image = Vec2::new(100.0, 100.0);
        view.fit(image, Vec2::new(200.0, 100.0));
        view.on_resize(
            Vec2::new(200.0, 100.0),
            Vec2::new(300.0, 300.0),
            Some(image),
        );
        assert!(view.fitted);
        assert!(close(view.scale, 3.0));

        view.zoom_at(Vec2::new(150.0, 150.0), 2.0, image, Vec2::new(300.0, 300.0));
        let scale = view.scale;
        let center = view.image_point(Vec2::new(150.0, 150.0));
        view.on_resize(
            Vec2::new(300.0, 300.0),
            Vec2::new(400.0, 200.0),
            Some(image),
        );
        assert!(!view.fitted);
        assert!(close(view.scale, scale));
        let center_after = view.image_point(Vec2::new(200.0, 100.0));
        assert!(close(center.x, center_after.x));
        assert!(close(center.y, center_after.y));
    }

    #[test]
    fn zoom_out_stops_when_the_image_fits_the_window() {
        let image = Vec2::new(400.0, 200.0);
        let window = Vec2::new(200.0, 200.0);
        let mut view = View::new();
        view.fit(image, window);

        view.zoom_at(Vec2::new(20.0, 20.0), 0.5, image, window);
        assert!(view.fitted);
        assert!(close(view.scale, 0.5));
        assert!(close(view.pan.y, 50.0));

        let cursor = Vec2::new(40.0, 80.0);
        view.zoom_at(cursor, 3.0, image, window);
        assert!(!view.fitted);
        let before = view.image_point(cursor);
        view.zoom_at(cursor, 0.8, image, window);
        assert!(!view.fitted);
        assert!(view.scale > 0.5);
        let after = view.image_point(cursor);
        assert!(close(before.x, after.x));
        assert!(close(before.y, after.y));

        view.zoom_at(Vec2::new(10.0, 190.0), 0.1, image, window);
        assert!(view.fitted);
        assert!(close(view.scale, 0.5));
        let displayed_w = image.x * view.scale;
        let displayed_h = image.y * view.scale;
        assert!(displayed_w <= window.x + 1e-3);
        assert!(displayed_h <= window.y + 1e-3);
        assert!(displayed_w + 1e-3 >= window.x || displayed_h + 1e-3 >= window.y);
    }
}
