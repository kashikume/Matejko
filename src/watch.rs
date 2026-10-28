//! Watch the image folder for adds, removes, and writes.

use std::path::Path;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use winit::event_loop::EventLoopProxy;

use crate::decode::UserEvent;

/// Start a non-recursive watch on `dir`. Returns `None` if the watcher cannot
/// be created; the app then continues without live folder updates.
pub fn watch_folder(dir: &Path, proxy: EventLoopProxy<UserEvent>) -> Option<RecommendedWatcher> {
    let mut watcher = RecommendedWatcher::new(
        move |result: Result<notify::Event, notify::Error>| {
            let Ok(event) = result else {
                return;
            };
            if !is_interesting(&event.kind) {
                return;
            }
            let _ = proxy.send_event(UserEvent::FolderDirty);
        },
        notify::Config::default(),
    )
    .ok()?;
    watcher.watch(dir, RecursiveMode::NonRecursive).ok()?;
    Some(watcher)
}

fn is_interesting(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}
