//! Non-destructive JPEG edit. The notes in `docs/ideas/edit` are the spec.

mod files;
mod image;
mod panel;
mod pipeline;
mod worker;

pub use files::{load_session, LoadedSession};
pub use image::Image;
pub use panel::{hit_test, layout, Hit, Panel, Slider, DRAG_LONG_SIDE};
pub use pipeline::{
    clamp_frame_to_photo, drag_frame, Adjustments, Frame, Guide, Handle, ImageControls,
};
pub use worker::{EditWorker, Rendered, SaveTarget, Saved, WorkerEvent};
