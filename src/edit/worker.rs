//! Edit renders and saves, off the UI thread. Only the newest request of
//! each kind is kept: a slider drag that outruns the pipeline skips the
//! runs it no longer needs.

use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::edit::files::{save_copy, save_over, LoadedSession};
use crate::edit::image::Image;
use crate::edit::pipeline::{run, Adjustments, Frame, RunOptions};

pub struct Rendered {
    pub generation: u64,
    /// From the full baseline rather than the drag image.
    pub full: bool,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub histogram: [Vec<u32>; 3],
}

pub enum SaveTarget {
    Over,
    Copy(String),
}

pub struct Saved {
    pub target: SaveTarget,
    pub adjustments: Adjustments,
    pub result: Result<(), String>,
}

pub enum WorkerEvent {
    Rendered(Rendered),
    Saved(Saved),
}

struct Render {
    generation: u64,
    adjustments: Adjustments,
}

#[derive(Default)]
struct Pending {
    preview: Option<Render>,
    full: Option<Render>,
    save: Option<(SaveTarget, Adjustments)>,
    stop: bool,
}

pub struct EditWorker {
    shared: Arc<(Mutex<Pending>, Condvar)>,
}

impl EditWorker {
    pub fn start(
        session: Arc<LoadedSession>,
        drag: Image,
        max_edge: u32,
        send: impl Fn(WorkerEvent) + Send + 'static,
    ) -> Self {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let thread_shared = shared.clone();
        thread::Builder::new()
            .name("matejko-edit".into())
            .spawn(move || {
                let (lock, ready) = &*thread_shared;
                loop {
                    let job = {
                        let mut pending = lock.lock().expect("edit worker");
                        loop {
                            if pending.stop {
                                return;
                            }
                            // Preview first so the picture answers at once,
                            // then a save, then the full-size picture.
                            if let Some(render) = pending.preview.take() {
                                break Job::Render(render, false);
                            }
                            if let Some((target, adjustments)) = pending.save.take() {
                                break Job::Save(target, adjustments);
                            }
                            if let Some(render) = pending.full.take() {
                                break Job::Render(render, true);
                            }
                            pending = ready.wait(pending).expect("edit worker");
                        }
                    };
                    let event = match job {
                        Job::Render(render, full) => {
                            let source = if full { &session.baseline } else { &drag };
                            WorkerEvent::Rendered(render_picture(
                                source,
                                session.baseline.width,
                                render,
                                full,
                                max_edge,
                            ))
                        }
                        Job::Save(target, adjustments) => {
                            let result = match &target {
                                SaveTarget::Over => save_over(&session, &adjustments),
                                SaveTarget::Copy(name) => {
                                    save_copy(&session, name, &adjustments).map(|_| ())
                                }
                            };
                            WorkerEvent::Saved(Saved {
                                target,
                                adjustments,
                                result,
                            })
                        }
                    };
                    send(event);
                }
            })
            .expect("edit worker thread");
        Self { shared }
    }

    /// Replaces any waiting render. `full` also queues the full-size run
    /// after the preview; without it a waiting full run is dropped as stale.
    pub fn render(&self, generation: u64, adjustments: &Adjustments, full: bool) {
        let (lock, ready) = &*self.shared;
        let mut pending = lock.lock().expect("edit worker");
        pending.preview = Some(Render {
            generation,
            adjustments: adjustments.clone(),
        });
        pending.full = full.then(|| Render {
            generation,
            adjustments: adjustments.clone(),
        });
        ready.notify_one();
    }

    pub fn save(&self, target: SaveTarget, adjustments: &Adjustments) {
        let (lock, ready) = &*self.shared;
        lock.lock().expect("edit worker").save = Some((target, adjustments.clone()));
        ready.notify_one();
    }
}

impl Drop for EditWorker {
    /// The thread finishes the run it is in, then exits. Nothing waits for it.
    fn drop(&mut self) {
        let (lock, ready) = &*self.shared;
        if let Ok(mut pending) = lock.lock() {
            pending.stop = true;
        }
        ready.notify_one();
    }
}

enum Job {
    Render(Render, bool),
    Save(SaveTarget, Adjustments),
}

fn render_picture(
    source: &Image,
    full_width: u32,
    render: Render,
    full: bool,
    max_edge: u32,
) -> Rendered {
    let radius_scale = source.width as f32 / full_width.max(1) as f32;
    let image = run(
        source,
        &render.adjustments,
        &RunOptions {
            radius_scale,
            dither: false,
            crop: false,
        },
    );
    let histogram = histogram_of(&image, render.adjustments.frame);
    let display = if image.width > max_edge || image.height > max_edge {
        image.downsample_long_side(max_edge)
    } else {
        image
    };
    Rendered {
        generation: render.generation,
        full,
        width: display.width,
        height: display.height,
        rgba: display.to_rgba8(true),
        histogram,
    }
}

/// Counts the part of the picture inside the frame.
fn histogram_of(image: &Image, frame: Frame) -> [Vec<u32>; 3] {
    let x0 = (frame.x * image.width as f32).floor().max(0.0) as u32;
    let y0 = (frame.y * image.height as f32).floor().max(0.0) as u32;
    let x1 = ((frame.x + frame.w) * image.width as f32)
        .ceil()
        .min(image.width as f32) as u32;
    let y1 = ((frame.y + frame.h) * image.height as f32)
        .ceil()
        .min(image.height as f32) as u32;
    image.histogram_region(x0, y0, x1.max(x0 + 1), y1.max(y0 + 1))
}
