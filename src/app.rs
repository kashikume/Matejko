//! Window, input, and the delete confirmation.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::MonitorHandleExtWindows;
use winit::window::{Fullscreen, Window};

use crate::catalog::Catalog;
use crate::decode::{DecodedImage, Decoder, UserEvent};
use crate::gpu::{Frame, Gpu};
use crate::view::{Vec2, View};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Left,
    Right,
    Enter,
    Delete,
    Escape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Viewing { fullscreen: bool },
    Confirming,
    DeleteFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Previous,
    Next,
    ToggleFullscreen,
    LeaveFullscreen,
    Quit,
    AskDelete,
    DoDelete,
    CancelOverlay,
}

pub fn action_for(command: Command, mode: Mode, repeat: bool) -> Action {
    let repeat_blocked = repeat && !matches!(command, Command::Left | Command::Right);
    if repeat_blocked {
        return Action::None;
    }
    match (mode, command) {
        (Mode::Viewing { .. }, Command::Left) => Action::Previous,
        (Mode::Viewing { .. }, Command::Right) => Action::Next,
        (Mode::Viewing { .. }, Command::Enter) => Action::ToggleFullscreen,
        (Mode::Viewing { .. }, Command::Delete) => Action::AskDelete,
        (Mode::Viewing { fullscreen: true }, Command::Escape) => Action::LeaveFullscreen,
        (Mode::Viewing { fullscreen: false }, Command::Escape) => Action::Quit,
        (Mode::Confirming, Command::Enter) => Action::DoDelete,
        (Mode::Confirming, Command::Escape) => Action::CancelOverlay,
        (Mode::DeleteFailed, Command::Escape) => Action::CancelOverlay,
        (Mode::Confirming | Mode::DeleteFailed, _) => Action::None,
    }
}

enum CacheEntry {
    Ready(DecodedImage),
    Failed(String),
}

enum Confirm {
    Idle,
    Prompt,
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
    confirm: Confirm,
    decode_error: Option<String>,
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
            confirm: Confirm::Idle,
            decode_error: None,
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
        match self.confirm {
            Confirm::Prompt => Mode::Confirming,
            Confirm::Failed(_) => Mode::DeleteFailed,
            Confirm::Idle => Mode::Viewing { fullscreen },
        }
    }

    fn on_command(&mut self, event_loop: &ActiveEventLoop, command: Command, repeat: bool) {
        match action_for(command, self.mode(), repeat) {
            Action::None => {}
            Action::Previous => self.shift(-1),
            Action::Next => self.shift(1),
            Action::ToggleFullscreen => self.toggle_fullscreen(),
            Action::LeaveFullscreen => self.leave_fullscreen(),
            Action::Quit => event_loop.exit(),
            Action::AskDelete => {
                self.confirm = Confirm::Prompt;
                self.request_redraw();
            }
            Action::DoDelete => self.delete_current(event_loop),
            Action::CancelOverlay => {
                self.confirm = Confirm::Idle;
                self.request_redraw();
            }
        }
    }

    fn toggle_fullscreen(&self) {
        let Some(window) = &self.window else {
            return;
        };
        if window.fullscreen().is_some() {
            window.set_fullscreen(None);
        } else {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
    }

    fn leave_fullscreen(&self) {
        if let Some(window) = &self.window {
            window.set_fullscreen(None);
        }
    }

    fn shift(&mut self, delta: isize) {
        if !self.catalog.move_by(delta) {
            return;
        }
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
                self.confirm = Confirm::Idle;
                self.decode_error = None;
                self.refresh_title();
                self.sync_images();
                self.request_redraw();
            }
            Err(err) => {
                self.confirm = Confirm::Failed(err.to_string());
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
        self.apply_current();
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
        match &self.confirm {
            Confirm::Prompt => Some(format!(
                "Delete \"{name}\"?\nEnter to delete, Esc to cancel"
            )),
            Confirm::Failed(err) => Some(format!(
                "Couldn't move \"{name}\" to the Recycle Bin.\n{err}\nEsc to dismiss"
            )),
            Confirm::Idle => self
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
        let attributes = Window::default_attributes()
            .with_title(self.catalog.title())
            .with_inner_size(size)
            .with_position(position)
            .with_resizable(true);
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
        self.window = Some(window);
        self.sync_images();
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::Decoded { path, result } = event;
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
                if button != MouseButton::Left {
                    return;
                }
                self.dragging = state == ElementState::Pressed;
            }
            WindowEvent::CursorMoved { position, .. } => {
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
                    dim: !matches!(self.confirm, Confirm::Idle),
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
        Key::Named(NamedKey::Enter) => Some(Command::Enter),
        Key::Named(NamedKey::Delete) => Some(Command::Delete),
        Key::Named(NamedKey::Escape) => Some(Command::Escape),
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

#[link(name = "user32")]
extern "system" {
    fn GetMonitorInfoW(monitor: *mut std::ffi::c_void, info: *mut MonitorInfo) -> i32;
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
    }
}
