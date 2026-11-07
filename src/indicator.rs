//! The loading wheel and the end-of-list cross, drawn at a side of the window.
//!
//! Timing and geometry only. The shapes come out as triangles in physical
//! pixels for `gpu` to draw over the picture.

use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use crate::gpu::Triangle;

/// A file still loading after this long shows the wheel.
pub const LOADING_DELAY: Duration = Duration::from_secs(1);
/// The wheel fades like the cross: in when it appears, out once the file
/// is on screen.
const WHEEL_FADE_IN: Duration = Duration::from_millis(180);
const WHEEL_FADE_OUT: Duration = Duration::from_millis(450);
/// Turns per second.
const WHEEL_SPEED: f32 = 1.1;

/// How long the cross stays, fades included.
pub const CROSS_FOR: Duration = Duration::from_secs(1);
const CROSS_FADE_IN: Duration = Duration::from_millis(180);
const CROSS_FADE_OUT: Duration = Duration::from_millis(450);

/// Linear-light colors: the surface is sRGB.
const BLUE: [f32; 3] = [0.013, 0.246, 1.0];
const RED: [f32; 3] = [0.79, 0.021, 0.021];
const BACKDROP: [f32; 4] = [0.0, 0.0, 0.0, 0.5];

const SEGMENTS: usize = 48;
/// Soft edge width, in physical pixels, so the shapes do not look jagged.
const FEATHER: f32 = 1.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    /// The first file at startup, which has no direction.
    Center,
}

impl Side {
    pub fn of(delta: isize) -> Side {
        if delta < 0 {
            Side::Left
        } else {
            Side::Right
        }
    }
}

/// A file that started loading at `since`, reached by moving toward `side`.
#[derive(Clone, Copy, Debug)]
pub struct Loading {
    since: Instant,
    pub side: Side,
    /// When the file arrived or the wait was given up; the wheel fades out
    /// from there.
    finished: Option<Instant>,
}

impl Loading {
    pub fn new(since: Instant, side: Side) -> Loading {
        Loading {
            since,
            side,
            finished: None,
        }
    }

    pub fn shows_at(&self) -> Instant {
        self.since + LOADING_DELAY
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }

    /// Starts the fade-out. A second call keeps the first time.
    pub fn finish(&mut self, now: Instant) {
        self.finished.get_or_insert(now);
    }

    pub fn alpha(&self, now: Instant) -> f32 {
        // Fade-in stops where the fade-out begins, so a wheel that had not
        // fully appeared fades out from the opacity it reached.
        let until = self.finished.map_or(now, |at| at.min(now));
        let fade_in = match until.checked_duration_since(self.shows_at()) {
            Some(shown) => ramp(shown, WHEEL_FADE_IN),
            None => 0.0,
        };
        let fade_out = match self.finished {
            Some(at) => {
                let gone = now.saturating_duration_since(at);
                ramp(WHEEL_FADE_OUT.saturating_sub(gone), WHEEL_FADE_OUT)
            }
            None => 1.0,
        };
        fade_in * fade_out
    }

    /// Finished and faded out completely, or finished before it showed.
    pub fn gone(&self, now: Instant) -> bool {
        match self.finished {
            Some(at) => at < self.shows_at() || now >= at + WHEEL_FADE_OUT,
            None => false,
        }
    }

    /// Where the head of the wheel points, in radians.
    pub fn angle(&self, now: Instant) -> f32 {
        let turning = now.saturating_duration_since(self.since).as_secs_f32();
        (turning * WHEEL_SPEED).fract() * TAU
    }
}

/// The red cross after a step past the first or last file.
#[derive(Clone, Copy, Debug)]
pub struct EdgeCross {
    since: Instant,
    pub side: Side,
}

impl EdgeCross {
    /// Another press while a cross is up keeps it from blinking: it rejoins
    /// the fade-in at the opacity it has now, then holds for the full time.
    pub fn trigger(previous: Option<EdgeCross>, now: Instant, side: Side) -> EdgeCross {
        let lead = match previous {
            Some(previous) if previous.side == side => {
                CROSS_FADE_IN.mul_f32(unramp(previous.alpha(now)))
            }
            _ => Duration::ZERO,
        };
        EdgeCross {
            since: now.checked_sub(lead).unwrap_or(now),
            side,
        }
    }

    pub fn alpha(&self, now: Instant) -> f32 {
        let age = now.saturating_duration_since(self.since);
        if age >= CROSS_FOR {
            return 0.0;
        }
        let left = CROSS_FOR - age;
        ramp(age, CROSS_FADE_IN).min(ramp(left, CROSS_FADE_OUT))
    }

    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) >= CROSS_FOR
    }
}

/// Smoothstep from 0 to 1 over `span`.
fn ramp(elapsed: Duration, span: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The fraction of the span at which `ramp` reaches `y`.
fn unramp(y: f32) -> f32 {
    let y = y.clamp(0.0, 1.0);
    0.5 - ((1.0 - 2.0 * y).asin() / 3.0).sin()
}

/// Backdrop radius at scale 1. The wheel and the cross fit inside it.
const BACKDROP_RADIUS: f32 = 34.0;
/// Gap between the backdrop and the window edge at scale 1.
const EDGE_GAP: f32 = 24.0;

/// The center of the indicator for `side`, vertically centered.
pub fn anchor(side: Side, width: f32, height: f32, scale: f32) -> (f32, f32) {
    let inset = ((BACKDROP_RADIUS + EDGE_GAP) * scale).min(width / 2.0);
    let x = match side {
        Side::Left => inset,
        Side::Right => width - inset,
        Side::Center => width / 2.0,
    };
    (x, height / 2.0)
}

/// A blue ring whose opacity runs from the head at `angle` down to a faint
/// tail, on a dark disc.
pub fn wheel(center: (f32, f32), scale: f32, angle: f32, alpha: f32, out: &mut Vec<Triangle>) {
    if alpha <= 0.0 {
        return;
    }
    disc(center, BACKDROP_RADIUS * scale, fade(BACKDROP, alpha), out);
    let outer = 22.0 * scale;
    let inner = outer - 5.0 * scale;
    let (cx, cy) = center;
    let at = |radius: f32, theta: f32| [cx + radius * theta.cos(), cy + radius * theta.sin()];
    for i in 0..SEGMENTS {
        // Segment 0 is the head; later ones trail behind it.
        let shade = |k: usize| {
            let t = k as f32 / SEGMENTS as f32;
            let a = (1.0 - t).powf(1.6) * 0.92 + 0.08;
            [BLUE[0], BLUE[1], BLUE[2], a * alpha]
        };
        let a0 = angle - TAU * i as f32 / SEGMENTS as f32;
        let a1 = angle - TAU * (i + 1) as f32 / SEGMENTS as f32;
        let (c0, c1) = (shade(i), shade(i + 1));
        let (z0, z1) = (clear(c0), clear(c1));
        band(
            [at(inner, a0), at(outer, a0), at(outer, a1), at(inner, a1)],
            [c0, c0, c1, c1],
            out,
        );
        band(
            [
                at(outer, a0),
                at(outer + FEATHER, a0),
                at(outer + FEATHER, a1),
                at(outer, a1),
            ],
            [c0, z0, z1, c1],
            out,
        );
        band(
            [
                at(inner - FEATHER, a0),
                at(inner, a0),
                at(inner, a1),
                at(inner - FEATHER, a1),
            ],
            [z0, c0, c1, z1],
            out,
        );
    }
}

/// A red X on a dark disc.
pub fn cross(center: (f32, f32), scale: f32, alpha: f32, out: &mut Vec<Triangle>) {
    if alpha <= 0.0 {
        return;
    }
    disc(center, BACKDROP_RADIUS * scale, fade(BACKDROP, alpha), out);
    let color = [RED[0], RED[1], RED[2], alpha];
    let half = 3.5 * scale;
    let reach = 18.0 * scale;
    let (cx, cy) = center;
    let (sin, cos) = (TAU / 8.0).sin_cos();
    // Local x and y run along the two strokes; turn them by 45°.
    let place = |x: f32, y: f32| [cx + x * cos - y * sin, cy + x * sin + y * cos];
    // The middle square once, then four arms out from its sides, so a
    // translucent cross is not darker where the strokes cross.
    band(
        [
            place(-half, -half),
            place(half, -half),
            place(half, half),
            place(-half, half),
        ],
        [color; 4],
        out,
    );
    for (ux, uy) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
        // Along the arm (u) and across it (v).
        let (vx, vy) = (-uy, ux);
        let point =
            |along: f32, across: f32| place(ux * along + vx * across, uy * along + vy * across);
        let (near, far) = (half, reach);
        band(
            [
                point(near, -half),
                point(far, -half),
                point(far, half),
                point(near, half),
            ],
            [color; 4],
            out,
        );
        let edge = clear(color);
        let f = FEATHER;
        band(
            [
                point(near, half),
                point(far, half),
                point(far + f, half + f),
                point(near, half + f),
            ],
            [color, color, edge, edge],
            out,
        );
        band(
            [
                point(near, -half - f),
                point(far + f, -half - f),
                point(far, -half),
                point(near, -half),
            ],
            [edge, edge, color, color],
            out,
        );
        band(
            [
                point(far, -half),
                point(far + f, -half - f),
                point(far + f, half + f),
                point(far, half),
            ],
            [color, edge, edge, color],
            out,
        );
    }
}

fn disc(center: (f32, f32), radius: f32, color: [f32; 4], out: &mut Vec<Triangle>) {
    let (cx, cy) = center;
    let at = |r: f32, k: usize| {
        let theta = TAU * k as f32 / SEGMENTS as f32;
        [cx + r * theta.cos(), cy + r * theta.sin()]
    };
    let edge = clear(color);
    for k in 0..SEGMENTS {
        out.push(Triangle {
            points: [[cx, cy], at(radius, k), at(radius, k + 1)],
            colors: [color; 3],
        });
        band(
            [
                at(radius, k),
                at(radius + FEATHER, k),
                at(radius + FEATHER, k + 1),
                at(radius, k + 1),
            ],
            [color, edge, edge, color],
            out,
        );
    }
}

/// A quadrilateral given in order around its edge, as two triangles.
fn band(points: [[f32; 2]; 4], colors: [[f32; 4]; 4], out: &mut Vec<Triangle>) {
    out.push(Triangle {
        points: [points[0], points[1], points[2]],
        colors: [colors[0], colors[1], colors[2]],
    });
    out.push(Triangle {
        points: [points[0], points[2], points[3]],
        colors: [colors[0], colors[2], colors[3]],
    });
}

fn fade(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * alpha]
}

fn clear(color: [f32; 4]) -> [f32; 4] {
    [color[0], color[1], color[2], 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_waits_a_second_then_fades_in() {
        let start = Instant::now();
        let loading = Loading::new(start, Side::Right);
        assert_eq!(loading.alpha(start), 0.0);
        assert_eq!(loading.alpha(start + Duration::from_millis(999)), 0.0);
        let partway = loading.alpha(start + LOADING_DELAY + WHEEL_FADE_IN / 2);
        assert!(partway > 0.0 && partway < 1.0);
        assert_eq!(loading.alpha(start + Duration::from_secs(2)), 1.0);
    }

    #[test]
    fn the_wheel_fades_out_once_the_file_arrives() {
        let start = Instant::now();
        let mut loading = Loading::new(start, Side::Left);
        let arrived = start + Duration::from_secs(3);
        loading.finish(arrived);
        assert_eq!(loading.alpha(arrived), 1.0);
        let fading = loading.alpha(arrived + WHEEL_FADE_OUT / 2);
        assert!(fading > 0.0 && fading < 1.0);
        assert!(!loading.gone(arrived + WHEEL_FADE_OUT / 2));
        assert!(loading.gone(arrived + WHEEL_FADE_OUT));
        assert_eq!(loading.alpha(arrived + WHEEL_FADE_OUT), 0.0);
    }

    #[test]
    fn a_file_that_arrives_in_time_never_shows_the_wheel() {
        let start = Instant::now();
        let mut loading = Loading::new(start, Side::Right);
        loading.finish(start + Duration::from_millis(500));
        assert!(loading.gone(start + Duration::from_millis(500)));
        assert_eq!(loading.alpha(start + Duration::from_secs(2)), 0.0);
    }

    #[test]
    fn the_cross_fades_in_holds_and_fades_out_within_a_second() {
        let start = Instant::now();
        let cross = EdgeCross::trigger(None, start, Side::Left);
        assert_eq!(cross.alpha(start), 0.0);
        // Held between the fade-in and the fade-out.
        assert_eq!(cross.alpha(start + CROSS_FADE_IN + Duration::from_millis(50)), 1.0);
        let fading = cross.alpha(start + CROSS_FOR - CROSS_FADE_OUT / 2);
        assert!(fading > 0.0 && fading < 1.0);
        assert!(!cross.done(start + CROSS_FOR - Duration::from_millis(1)));
        assert!(cross.done(start + CROSS_FOR));
        assert_eq!(cross.alpha(start + CROSS_FOR), 0.0);
    }

    #[test]
    fn another_press_keeps_the_cross_up_without_blinking() {
        let start = Instant::now();
        let first = EdgeCross::trigger(None, start, Side::Right);
        // Partway through the fade-out.
        let later = start + CROSS_FOR - Duration::from_millis(200);
        let before = first.alpha(later);
        let again = EdgeCross::trigger(Some(first), later, Side::Right);
        assert!((again.alpha(later) - before).abs() < 0.02);
        assert!(again.alpha(later + Duration::from_millis(300)) == 1.0);
        let other = EdgeCross::trigger(Some(first), later, Side::Left);
        assert_eq!(other.alpha(later), 0.0);
    }

    #[test]
    fn indicators_sit_inside_the_window_on_their_side() {
        let (left, y) = anchor(Side::Left, 1000.0, 600.0, 1.5);
        let (right, _) = anchor(Side::Right, 1000.0, 600.0, 1.5);
        assert_eq!(y, 300.0);
        assert!(left > BACKDROP_RADIUS * 1.5 && left < 500.0);
        assert!((right - (1000.0 - left)).abs() < 0.001);
        assert_eq!(anchor(Side::Center, 1000.0, 600.0, 1.0).0, 500.0);
    }
}
