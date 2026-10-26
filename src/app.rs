//! Window, input, and the delete confirmation.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use notify::RecommendedWatcher;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::{MonitorHandleExtWindows, WindowAttributesExtWindows};
use winit::window::{Fullscreen, Icon, Window};

use crate::catalog::{Catalog, Refresh};
use crate::decode::{DecodedImage, Decoder, UserEvent};
use crate::gpu::{Frame, Gpu};
use crate::info::format_image_info;
use crate::view::{Vec2, View};
use crate::watch::watch_folder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Delete,
    Escape,
    Help,
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Viewing { fullscreen: bool },
    Help,
    Info,
    Confirming,
    DeleteFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Previous,
    Next,
    First,
    Last,
    PageBack,
    PageForward,
    ToggleFullscreen,
    LeaveFullscreen,
    Quit,
    AskDelete,
    DoDelete,
    ShowHelp,
    ShowInfo,
    CancelOverlay,
}

const PAGE_STEP: isize = 10;
const FOLDER_DEBOUNCE: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: SystemTime,
}

fn read_stamp(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some(FileStamp {
        len: meta.len(),
        modified: meta.modified().ok()?,
    })
}

pub fn action_for(command: Command, mode: Mode, repeat: bool) -> Action {
    let nav_repeat = matches!(
        command,
        Command::Left
            | Command::Right
            | Command::Home
            | Command::End
            | Command::PageUp
            | Command::PageDown
    );
    if repeat && !nav_repeat {
        return Action::None;
    }
    match (mode, command) {
        (Mode::Viewing { .. }, Command::Left) => Action::Previous,
        (Mode::Viewing { .. }, Command::Right) => Action::Next,
        (Mode::Viewing { .. }, Command::Home) => Action::First,
        (Mode::Viewing { .. }, Command::End) => Action::Last,
        (Mode::Viewing { .. }, Command::PageUp) => Action::PageBack,
        (Mode::Viewing { .. }, Command::PageDown) => Action::PageForward,
        (Mode::Viewing { .. }, Command::Enter) => Action::ToggleFullscreen,
        (Mode::Viewing { .. }, Command::Delete) => Action::AskDelete,
        (Mode::Viewing { fullscreen: true }, Command::Escape) => Action::LeaveFullscreen,
        (Mode::Viewing { fullscreen: false }, Command::Escape) => Action::Quit,
        (Mode::Viewing { .. }, Command::Help) => Action::ShowHelp,
        (Mode::Viewing { .. }, Command::Info) => Action::ShowInfo,
        (Mode::Help, Command::Escape | Command::Help) => Action::CancelOverlay,
        (Mode::Info, Command::Escape | Command::Info) => Action::CancelOverlay,
        (Mode::Confirming, Command::Enter) => Action::DoDelete,
        (Mode::Confirming, Command::Escape) => Action::CancelOverlay,
        (Mode::DeleteFailed, Command::Escape) => Action::CancelOverlay,
        (Mode::Help | Mode::Info | Mode::Confirming | Mode::DeleteFailed, _) => Action::None,
    }
}

const HELP_TEXT: &str = "\
Left / Right — previous or next image
Home / End — first or last image
Page Up / Page Down — jump by 10 images
Enter — window or fullscreen
Delete — delete the image
I — image information
Escape — cancel, leave fullscreen, or quit
F1 — help
Mouse wheel — zoom at the pointer
Drag — pan
Fullscreen — the pointer hides at once, then returns for 3 seconds when the mouse is used

Esc or F1 to close";

const CURSOR_VISIBLE_FOR: Duration = Duration::from_secs(3);

/// Fullscreen pointer visibility. Hidden until the mouse is used, then shown
/// until `CURSOR_VISIBLE_FOR` has passed since the last use.
struct CursorVisibility {
    visible_until: Option<Instant>,
}

impl CursorVisibility {
    fn new() -> Self {
        Self {
            visible_until: None,
        }
    }

    /// Returns whether the pointer should be drawn.
    fn on_fullscreen(&mut self, fullscreen: bool) -> bool {
        self.visible_until = None;
        !fullscreen
    }

    fn on_activity(&mut self, now: Instant, fullscreen: bool) -> bool {
        if !fullscreen {
            self.visible_until = None;
            return true;
        }
        self.visible_until = Some(now + CURSOR_VISIBLE_FOR);
        true
    }

    /// `moved` is a real change of the pointer's screen position. A move event
    /// caused by the window resizing under a still pointer does not count.
    fn on_move(&mut self, now: Instant, fullscreen: bool, moved: bool) -> bool {
        if moved {
            self.on_activity(now, fullscreen)
        } else {
            self.poll(now, fullscreen)
        }
    }

    fn poll(&mut self, now: Instant, fullscreen: bool) -> bool {
        if !fullscreen {
            self.visible_until = None;
            return true;
        }
        match self.visible_until {
            Some(until) if now < until => true,
            _ => {
                self.visible_until = None;
                false
            }
        }
    }

    fn wake_at(&self) -> Option<Instant> {
        self.visible_until
    }
}

enum CacheEntry {
    Ready(DecodedImage),
    Failed(String),
}

enum Prompt {
    Help,
    Info,
    Delete,
    Failed(String),
}

pub struct App {
    catalog: Catalog,
    proxy: EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    decoder: Option<Decoder>,
    view: View,
    window_size: Vec2,
    cursor: Vec2,
    cursor_valid: bool,
    dragging: bool,
    cache: HashMap<PathBuf, CacheEntry>,
    in_flight: HashSet<PathBuf>,
    displayed: Option<PathBuf>,
    shown_size: Option<Vec2>,
    prompt: Option<Prompt>,
    decode_error: Option<String>,
    cursor_visibility: CursorVisibility,
    cursor_drawn: bool,
    /// Screen position when the pointer was last hidden or shown. Fullscreen
    /// entry moves the window under a still pointer, and that must not count
    /// as the user using the mouse.
    pointer_anchor: Option<(i32, i32)>,
    stamps: HashMap<PathBuf, FileStamp>,
    folder_dirty_until: Option<Instant>,
    _watcher: Option<RecommendedWatcher>,
    pub fatal: Option<String>,
}

impl App {
    pub fn new(catalog: Catalog, proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            catalog,
            proxy,
            window: None,
            gpu: None,
            decoder: None,
            view: View::new(),
            window_size: Vec2::new(0.0, 0.0),
            cursor: Vec2::new(0.0, 0.0),
            cursor_valid: false,
            dragging: false,
            cache: HashMap::new(),
            in_flight: HashSet::new(),
            displayed: None,
            shown_size: None,
            prompt: None,
            decode_error: None,
            cursor_visibility: CursorVisibility::new(),
            cursor_drawn: true,
            pointer_anchor: None,
            stamps: HashMap::new(),
            folder_dirty_until: None,
            _watcher: None,
            fatal: None,
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, message: String) {
        self.fatal = Some(message);
        event_loop.exit();
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn refresh_title(&self) {
        if let Some(window) = &self.window {
            if !self.catalog.is_empty() {
                window.set_title(&self.catalog.title());
            }
        }
    }

    fn mode(&self) -> Mode {
        let fullscreen = self
            .window
            .as_ref()
            .and_then(|window| window.fullscreen())
            .is_some();
        match &self.prompt {
            Some(Prompt::Help) => Mode::Help,
            Some(Prompt::Info) => Mode::Info,
            Some(Prompt::Delete) => Mode::Confirming,
            Some(Prompt::Failed(_)) => Mode::DeleteFailed,
            None => Mode::Viewing { fullscreen },
        }
    }

    fn on_command(&mut self, event_loop: &ActiveEventLoop, command: Command, repeat: bool) {
        match action_for(command, self.mode(), repeat) {
            Action::None => {}
            Action::Previous => self.shift(-1),
            Action::Next => self.shift(1),
            Action::First => self.jump_first(),
            Action::Last => self.jump_last(),
            Action::PageBack => self.shift_clamped(-PAGE_STEP),
            Action::PageForward => self.shift_clamped(PAGE_STEP),
            Action::ToggleFullscreen => self.toggle_fullscreen(),
            Action::LeaveFullscreen => self.leave_fullscreen(),
            Action::Quit => event_loop.exit(),
            Action::AskDelete => {
                self.prompt = Some(Prompt::Delete);
                self.request_redraw();
            }
            Action::ShowHelp => {
                self.prompt = Some(Prompt::Help);
                self.request_redraw();
            }
            Action::ShowInfo => {
                self.prompt = Some(Prompt::Info);
                self.request_redraw();
            }
            Action::DoDelete => self.delete_current(event_loop),
            Action::CancelOverlay => {
                self.prompt = None;
                self.request_redraw();
            }
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.window
            .as_ref()
            .and_then(|window| window.fullscreen())
            .is_some()
    }

    fn apply_cursor(&mut self, visible: bool) {
        if self.cursor_drawn == visible {
            return;
        }
        self.force_cursor(visible);
    }

    /// Tell Windows even when the cached state already matches. Entering
    /// fullscreen can show the pointer again after we hid it.
    fn force_cursor(&mut self, visible: bool) {
        self.cursor_drawn = visible;
        if let Some(window) = &self.window {
            window.set_cursor_visible(visible);
        }
    }

    fn note_mouse(&mut self) {
        let visible = self
            .cursor_visibility
            .on_activity(Instant::now(), self.is_fullscreen());
        self.apply_cursor(visible);
    }

    fn note_pointer_motion(&mut self) {
        let fullscreen = self.is_fullscreen();
        let moved = if fullscreen {
            match (self.pointer_anchor, cursor_screen_position()) {
                (Some(anchor), Some(position)) if position == anchor => false,
                (_, Some(position)) => {
                    self.pointer_anchor = Some(position);
                    true
                }
                _ => true,
            }
        } else {
            false
        };
        let visible = self
            .cursor_visibility
            .on_move(Instant::now(), fullscreen, moved);
        if visible {
            self.apply_cursor(true);
        } else {
            self.force_cursor(false);
        }
    }

    fn toggle_fullscreen(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let enter = window.fullscreen().is_none();
        if enter {
            self.pointer_anchor = cursor_screen_position();
            let visible = self.cursor_visibility.on_fullscreen(true);
            self.force_cursor(visible);
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
            self.force_cursor(false);
        } else {
            window.set_fullscreen(None);
            self.pointer_anchor = None;
            let visible = self.cursor_visibility.on_fullscreen(false);
            self.force_cursor(visible);
        }
    }

    fn leave_fullscreen(&mut self) {
        if let Some(window) = &self.window {
            window.set_fullscreen(None);
        }
        self.pointer_anchor = None;
        let visible = self.cursor_visibility.on_fullscreen(false);
        self.force_cursor(visible);
    }

    fn shift(&mut self, delta: isize) {
        if !self.catalog.move_by(delta) {
            return;
        }
        self.after_navigate();
    }

    fn shift_clamped(&mut self, delta: isize) {
        if !self.catalog.move_clamped(delta) {
            return;
        }
        self.after_navigate();
    }

    fn jump_first(&mut self) {
        if !self.catalog.go_to_first() {
            return;
        }
        self.after_navigate();
    }

    fn jump_last(&mut self) {
        if !self.catalog.go_to_last() {
            return;
        }
        self.after_navigate();
    }

    fn after_navigate(&mut self) {
        self.decode_error = None;
        self.refresh_title();
        self.sync_images();
        self.request_redraw();
    }

    fn delete_current(&mut self, event_loop: &ActiveEventLoop) {
        let path = self.catalog.current_path();
        match trash::delete(&path) {
            Ok(()) => {
                self.cache.remove(&path);
                self.in_flight.remove(&path);
                self.catalog.remove_current();
                if self.catalog.is_empty() {
                    event_loop.exit();
                    return;
                }
                self.prompt = None;
                self.decode_error = None;
                self.refresh_title();
                self.sync_images();
                self.request_redraw();
            }
            Err(err) => {
                self.prompt = Some(Prompt::Failed(err.to_string()));
                self.request_redraw();
            }
        }
    }

    fn relevant_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.catalog.current_path()];
        if let Some(path) = self.catalog.neighbor(-1) {
            paths.push(path.to_path_buf());
        }
        if let Some(path) = self.catalog.neighbor(1) {
            paths.push(path.to_path_buf());
        }
        paths
    }

    fn sync_images(&mut self) {
        if self.decoder.is_none() || self.catalog.is_empty() {
            return;
        }
        let relevant = self.relevant_paths();
        self.request_decode(relevant[0].clone(), true);
        for path in relevant.iter().skip(1) {
            self.request_decode(path.clone(), false);
        }
        self.cache
            .retain(|path, _| relevant.iter().any(|kept| kept == path));
        self.stamps
            .retain(|path, _| relevant.iter().any(|kept| kept == path));
        for path in &relevant {
            if let Some(stamp) = read_stamp(path) {
                self.stamps.insert(path.clone(), stamp);
            }
        }
        self.apply_current();
    }

    fn note_folder_dirty(&mut self, event_loop: &ActiveEventLoop) {
        let until = Instant::now() + FOLDER_DEBOUNCE;
        self.folder_dirty_until = Some(until);
        event_loop.set_control_flow(ControlFlow::WaitUntil(until));
    }

    fn apply_folder_changes(&mut self, event_loop: &ActiveEventLoop) {
        let old_current = if self.catalog.is_empty() {
            None
        } else {
            Some(self.catalog.current_path())
        };
        match self.catalog.refresh() {
            Refresh::Empty => {
                event_loop.exit();
                return;
            }
            Refresh::CurrentRemoved => {
                if let Some(old) = &old_current {
                    self.cache.remove(old);
                    self.in_flight.remove(old);
                    self.stamps.remove(old);
                    if self.displayed.as_ref() == Some(old) {
                        self.displayed = None;
                    }
                }
                self.prompt = None;
                self.decode_error = None;
            }
            Refresh::KeptCurrent => {}
        }
        self.invalidate_stale_cache();
        self.refresh_title();
        self.sync_images();
        self.request_redraw();
    }

    fn invalidate_stale_cache(&mut self) {
        let cached: Vec<PathBuf> = self.cache.keys().cloned().collect();
        for path in cached {
            let stale = if !self.catalog.contains_path(&path) {
                true
            } else {
                match (self.stamps.get(&path), read_stamp(&path)) {
                    (Some(old), Some(new)) => old != &new,
                    _ => true,
                }
            };
            if stale {
                self.cache.remove(&path);
                self.in_flight.remove(&path);
                if self.displayed.as_ref() == Some(&path) {
                    self.displayed = None;
                }
            }
        }
    }

    fn request_decode(&mut self, path: PathBuf, front: bool) {
        if self.cache.contains_key(&path) {
            return;
        }
        let Some(decoder) = &self.decoder else {
            return;
        };
        if self.in_flight.contains(&path) {
            if front {
                decoder.promote(&path);
            }
            return;
        }
        self.in_flight.insert(path.clone());
        decoder.enqueue(path, front);
    }

    fn is_relevant(&self, path: &std::path::Path) -> bool {
        self.relevant_paths().iter().any(|kept| kept == path)
    }

    fn apply_current(&mut self) {
        if self.catalog.is_empty() || self.gpu.is_none() {
            return;
        }
        let path = self.catalog.current_path();
        if self.displayed.as_ref() == Some(&path) {
            return;
        }
        let Some(entry) = self.cache.remove(&path) else {
            return;
        };
        match entry {
            CacheEntry::Failed(err) => {
                self.decode_error = Some(err.clone());
                self.cache.insert(path, CacheEntry::Failed(err));
                self.displayed = None;
                self.shown_size = None;
                if let Some(gpu) = &mut self.gpu {
                    gpu.clear_image();
                }
            }
            CacheEntry::Ready(image) => {
                let size = Vec2::new(image.width as f32, image.height as f32);
                let upload_error = self
                    .gpu
                    .as_mut()
                    .and_then(|gpu| gpu.set_image(&image).err());
                if let Some(err) = upload_error {
                    self.decode_error = Some(err.clone());
                    self.cache.insert(path, CacheEntry::Failed(err));
                    self.displayed = None;
                    self.shown_size = None;
                    if let Some(gpu) = &mut self.gpu {
                        gpu.clear_image();
                    }
                    return;
                }
                self.view.fit(size, self.window_size);
                self.cache.insert(path.clone(), CacheEntry::Ready(image));
                self.displayed = Some(path);
                self.shown_size = Some(size);
                self.decode_error = None;
            }
        }
    }

    fn image_size(&self) -> Option<Vec2> {
        self.shown_size
    }

    fn image_shown(&self) -> bool {
        self.displayed.is_some()
    }

    fn cursor_or_center(&self) -> Vec2 {
        if self.cursor_valid {
            self.cursor
        } else {
            Vec2::new(self.window_size.x / 2.0, self.window_size.y / 2.0)
        }
    }

    fn overlay_text(&self) -> Option<String> {
        if self.catalog.is_empty() {
            return None;
        }
        let name = self.catalog.file_name();
        match &self.prompt {
            Some(Prompt::Help) => Some(HELP_TEXT.to_string()),
            Some(Prompt::Info) => {
                let path = self.catalog.current_path();
                let pixels = self
                    .image_size()
                    .map(|size| (size.x.round() as u32, size.y.round() as u32));
                Some(format_image_info(&path, pixels))
            }
            Some(Prompt::Delete) => Some(format!(
                "Delete \"{name}\"?\nEnter to delete, Esc to cancel"
            )),
            Some(Prompt::Failed(err)) => Some(format!(
                "Couldn't move \"{name}\" to the Recycle Bin.\n{err}\nEsc to dismiss"
            )),
            None => self
                .decode_error
                .as_ref()
                .map(|err| format!("{name}\n{err}")),
        }
    }

    fn resize_to(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        let new = Vec2::new(size.width as f32, size.height as f32);
        let old = self.window_size;
        self.window_size = new;
        if self.image_shown() {
            self.view.on_resize(old, new, self.image_size());
        }
        if let Some(gpu) = &mut self.gpu {
            gpu.resize(size);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (position, size) = initial_placement(event_loop);
        let icon = app_icon();
        let attributes = Window::default_attributes()
            .with_title(self.catalog.title())
            .with_inner_size(size)
            .with_position(position)
            .with_resizable(true)
            .with_window_icon(icon.clone())
            .with_taskbar_icon(icon);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.fail(event_loop, format!("Couldn't open a window. {err}"));
                return;
            }
        };
        let inner = window.inner_size();
        self.window_size = Vec2::new(inner.width.max(1) as f32, inner.height.max(1) as f32);
        let display_handle = event_loop.owned_display_handle();
        let gpu = match pollster::block_on(Gpu::new(window.clone(), display_handle)) {
            Ok(gpu) => gpu,
            Err(err) => {
                self.fail(event_loop, err);
                return;
            }
        };
        let max_edge = gpu.max_edge();
        self.gpu = Some(gpu);
        self.decoder = Some(Decoder::spawn(self.proxy.clone(), max_edge));
        self._watcher = watch_folder(self.catalog.dir(), self.proxy.clone());
        self.window = Some(window);
        self.sync_images();
        self.request_redraw();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self
            .folder_dirty_until
            .is_some_and(|until| now >= until)
        {
            self.folder_dirty_until = None;
            self.apply_folder_changes(event_loop);
            if self.fatal.is_some() || self.catalog.is_empty() {
                return;
            }
        }

        let fullscreen = self.is_fullscreen();
        let visible = self.cursor_visibility.poll(now, fullscreen);
        if visible {
            self.apply_cursor(true);
        } else {
            self.force_cursor(false);
        }

        let mut wake_at = self.folder_dirty_until;
        if fullscreen {
            if let Some(until) = self.cursor_visibility.wake_at() {
                wake_at = Some(wake_at.map_or(until, |existing| existing.min(until)));
            }
        }
        match wake_at {
            Some(until) => event_loop.set_control_flow(ControlFlow::WaitUntil(until)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::FolderDirty => self.note_folder_dirty(event_loop),
            UserEvent::Decoded { path, result } => {
                self.in_flight.remove(&path);
                if !self.is_relevant(&path) {
                    return;
                }
                match result {
                    Ok(image) => {
                        self.cache.insert(path, CacheEntry::Ready(image));
                    }
                    Err(err) => {
                        self.cache.insert(path, CacheEntry::Failed(err));
                    }
                }
                self.apply_current();
                self.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.resize_to(size);
                self.request_redraw();
            }
            WindowEvent::Focused(false) => self.dragging = false,
            WindowEvent::Occluded(false) => self.request_redraw(),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(command) = command_from(&event.logical_key) {
                    self.on_command(event_loop, command, event.repeat);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.note_mouse();
                if !self.image_shown() {
                    return;
                }
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => lines,
                    MouseScrollDelta::PixelDelta(offset) => (offset.y / 120.0) as f32,
                };
                if steps == 0.0 {
                    return;
                }
                let Some(image) = self.image_size() else {
                    return;
                };
                self.view.zoom_at(
                    self.cursor_or_center(),
                    1.1_f32.powf(steps),
                    image,
                    self.window_size,
                );
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.note_mouse();
                if button != MouseButton::Left {
                    return;
                }
                self.dragging = state == ElementState::Pressed;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.note_pointer_motion();
                let next = Vec2::new(position.x as f32, position.y as f32);
                if self.dragging && self.cursor_valid && self.image_shown() {
                    self.view
                        .pan_by(Vec2::new(next.x - self.cursor.x, next.y - self.cursor.y));
                    self.request_redraw();
                }
                self.cursor = next;
                self.cursor_valid = true;
            }
            WindowEvent::RedrawRequested => {
                let text = self.overlay_text();
                let scale_factor = self
                    .window
                    .as_ref()
                    .map(|window| window.scale_factor() as f32)
                    .unwrap_or(1.0);
                let (image_width, image_height) = self
                    .image_size()
                    .map(|size| (size.x, size.y))
                    .unwrap_or((0.0, 0.0));
                let frame = Frame {
                    image_width,
                    image_height,
                    scale: self.view.scale,
                    pan_x: self.view.pan.x,
                    pan_y: self.view.pan.y,
                    dim: self.prompt.is_some(),
                    text: text.as_deref(),
                    scale_factor,
                };
                let Some(gpu) = &mut self.gpu else {
                    return;
                };
                if let Err(err) = gpu.draw(frame) {
                    self.fail(event_loop, err);
                }
            }
            _ => {}
        }
    }
}

fn command_from(key: &Key) -> Option<Command> {
    match key {
        Key::Named(NamedKey::ArrowLeft) => Some(Command::Left),
        Key::Named(NamedKey::ArrowRight) => Some(Command::Right),
        Key::Named(NamedKey::Home) => Some(Command::Home),
        Key::Named(NamedKey::End) => Some(Command::End),
        Key::Named(NamedKey::PageUp) => Some(Command::PageUp),
        Key::Named(NamedKey::PageDown) => Some(Command::PageDown),
        Key::Named(NamedKey::Enter) => Some(Command::Enter),
        Key::Named(NamedKey::Delete) => Some(Command::Delete),
        Key::Named(NamedKey::Escape) => Some(Command::Escape),
        Key::Named(NamedKey::F1) => Some(Command::Help),
        Key::Character(ch) if ch.eq_ignore_ascii_case("i") => Some(Command::Info),
        _ => None,
    }
}

struct WorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn initial_placement(event_loop: &ActiveEventLoop) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next());
    if let Some(monitor) = monitor {
        let work = work_area(&monitor).unwrap_or(WorkArea {
            x: monitor.position().x,
            y: monitor.position().y,
            width: monitor.size().width.max(1),
            height: monitor.size().height.max(1),
        });
        let width = ((work.width as f32) * 0.8).round().max(1.0) as u32;
        let height = ((work.height as f32) * 0.8).round().max(1.0) as u32;
        let x = work.x + (work.width as i32 - width as i32) / 2;
        let y = work.y + (work.height as i32 - height as i32) / 2;
        return (
            PhysicalPosition::new(x, y),
            PhysicalSize::new(width, height),
        );
    }
    (PhysicalPosition::new(160, 80), PhysicalSize::new(1280, 720))
}

fn work_area(monitor: &winit::monitor::MonitorHandle) -> Option<WorkArea> {
    let mut info = MonitorInfo {
        cb_size: std::mem::size_of::<MonitorInfo>() as u32,
        rc_monitor: Rect::default(),
        rc_work: Rect::default(),
        dw_flags: 0,
    };
    let ok = unsafe { GetMonitorInfoW(monitor.hmonitor() as *mut std::ffi::c_void, &mut info) };
    if ok == 0 {
        return None;
    }
    let width = info.rc_work.right.saturating_sub(info.rc_work.left);
    let height = info.rc_work.bottom.saturating_sub(info.rc_work.top);
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(WorkArea {
        x: info.rc_work.left,
        y: info.rc_work.top,
        width: width as u32,
        height: height as u32,
    })
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: Rect,
    rc_work: Rect,
    dw_flags: u32,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[link(name = "user32")]
extern "system" {
    fn GetMonitorInfoW(monitor: *mut std::ffi::c_void, info: *mut MonitorInfo) -> i32;
    fn GetCursorPos(point: *mut Point) -> i32;
}

fn cursor_screen_position() -> Option<(i32, i32)> {
    let mut point = Point { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        None
    } else {
        Some((point.x, point.y))
    }
}

fn app_icon() -> Option<Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()?
        .into_rgba8();
    let width = image.width();
    let height = image.height();
    Icon::from_rgba(image.into_raw(), width, height).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_the_viewing_and_confirmation_table() {
        let viewing = Mode::Viewing { fullscreen: false };
        let fullscreen = Mode::Viewing { fullscreen: true };
        assert_eq!(action_for(Command::Left, viewing, false), Action::Previous);
        assert_eq!(action_for(Command::Right, viewing, true), Action::Next);
        assert_eq!(action_for(Command::Home, viewing, false), Action::First);
        assert_eq!(action_for(Command::End, viewing, false), Action::Last);
        assert_eq!(
            action_for(Command::PageUp, viewing, true),
            Action::PageBack
        );
        assert_eq!(
            action_for(Command::PageDown, viewing, false),
            Action::PageForward
        );
        assert_eq!(
            action_for(Command::Home, Mode::Help, false),
            Action::None
        );
        assert_eq!(
            action_for(Command::Enter, viewing, false),
            Action::ToggleFullscreen
        );
        assert_eq!(action_for(Command::Enter, viewing, true), Action::None);
        assert_eq!(
            action_for(Command::Delete, viewing, false),
            Action::AskDelete
        );
        assert_eq!(action_for(Command::Escape, viewing, false), Action::Quit);
        assert_eq!(
            action_for(Command::Escape, fullscreen, false),
            Action::LeaveFullscreen
        );
        assert_eq!(
            action_for(Command::Enter, Mode::Confirming, false),
            Action::DoDelete
        );
        assert_eq!(
            action_for(Command::Escape, Mode::Confirming, false),
            Action::CancelOverlay
        );
        assert_eq!(
            action_for(Command::Left, Mode::Confirming, false),
            Action::None
        );
        assert_eq!(
            action_for(Command::Enter, Mode::Confirming, true),
            Action::None
        );
        assert_eq!(
            action_for(Command::Escape, Mode::DeleteFailed, false),
            Action::CancelOverlay
        );
        assert_eq!(
            action_for(Command::Enter, Mode::DeleteFailed, false),
            Action::None
        );
        assert_eq!(action_for(Command::Help, viewing, false), Action::ShowHelp);
        assert_eq!(action_for(Command::Help, viewing, true), Action::None);
        assert_eq!(
            action_for(Command::Help, Mode::Help, false),
            Action::CancelOverlay
        );
        assert_eq!(
            action_for(Command::Escape, Mode::Help, false),
            Action::CancelOverlay
        );
        assert_eq!(action_for(Command::Enter, Mode::Help, false), Action::None);
        assert_eq!(action_for(Command::Delete, Mode::Help, false), Action::None);
        assert_eq!(
            action_for(Command::Help, Mode::Confirming, false),
            Action::None
        );
        assert_eq!(action_for(Command::Info, viewing, false), Action::ShowInfo);
        assert_eq!(action_for(Command::Info, viewing, true), Action::None);
        assert_eq!(
            action_for(Command::Info, Mode::Info, false),
            Action::CancelOverlay
        );
        assert_eq!(
            action_for(Command::Escape, Mode::Info, false),
            Action::CancelOverlay
        );
        assert_eq!(action_for(Command::Enter, Mode::Info, false), Action::None);
        assert_eq!(
            action_for(Command::Info, Mode::Confirming, false),
            Action::None
        );
        assert_eq!(action_for(Command::Info, Mode::Help, false), Action::None);
        assert_eq!(action_for(Command::Help, Mode::Info, false), Action::None);
    }

    #[test]
    fn fullscreen_pointer_hides_until_the_mouse_is_used() {
        let start = Instant::now();
        let mut cursor = CursorVisibility::new();
        assert!(!cursor.on_fullscreen(true));
        assert!(!cursor.poll(start, true));
        assert!(!cursor.on_move(start, true, false));

        assert!(cursor.on_activity(start, true));
        assert!(cursor.poll(
            start + Duration::from_secs(3) - Duration::from_millis(1),
            true
        ));
        assert!(!cursor.poll(start + Duration::from_secs(3), true));

        let again = start + Duration::from_secs(10);
        assert!(cursor.on_activity(again, true));
        assert!(cursor.poll(again + Duration::from_secs(2), true));
        assert!(cursor.on_activity(again + Duration::from_secs(2), true));
        assert!(cursor.poll(again + Duration::from_secs(4), true));
        assert!(!cursor.poll(again + Duration::from_secs(5), true));

        assert!(cursor.on_fullscreen(false));
        assert!(cursor.poll(again, false));
        assert!(cursor.on_activity(again, false));
        assert!(cursor.wake_at().is_none());
    }
}
