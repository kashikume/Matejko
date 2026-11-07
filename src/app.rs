//! Window, input, and the delete confirmation.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::RecommendedWatcher;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::{MonitorHandleExtWindows, WindowAttributesExtWindows};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{CursorIcon, Fullscreen, Icon, Window};

use crate::catalog::{is_video, Catalog, Refresh};
use crate::decode::{read_stamp, DecodedImage, Decoder, FileStamp, PixelOrder, UserEvent};
use crate::edit::{
    clamp_frame_to_photo, drag_frame, hit_test, layout, Adjustments, Align, EditWorker,
    Frame as EditFrame, Guide, Handle, Hit as EditHit, Image as LinearImage, ImageControls,
    Rendered, SaveTarget, Saved, Slider, Tone, Warp, WorkerEvent,
};
use crate::gpu::{Label, LabelAlign, DIMMED_ROW};
use crate::gpu::{ExtraQuad, Frame, Gpu, TransportDraw, Triangle};
use crate::indicator::{self, EdgeCross, Loading, Side};
use crate::info::{format_image_info, gps_coordinates, maps_url};
use crate::playback::VideoSession;
use crate::transport::{self, Hit};
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
    /// F11: toggles fullscreen like Enter, but never confirms a delete.
    Fullscreen,
    Delete,
    Escape,
    Help,
    Info,
    OpenWith,
    Map,
    Pause,
    Edit,
    /// Ctrl+S.
    Save,
    /// Ctrl+Shift+S.
    SaveCopy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Viewing { fullscreen: bool },
    Help,
    Info,
    Confirming,
    DeleteFailed,
    Editing,
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
    OpenWith,
    OpenMap,
    CancelOverlay,
    TogglePause,
    EnterEdit,
    /// Escape or E in edit mode: what Exit does, or, while the unsaved-changes
    /// question is up, Escape answers Keep editing.
    ExitEdit { escape: bool },
    /// Ctrl+S and Ctrl+Shift+S in edit mode: the Save and Save as copy buttons.
    SaveEdit { copy: bool },
}

const PAGE_STEP: isize = 10;
const FOLDER_DEBOUNCE: Duration = Duration::from_millis(100);


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
        (Mode::Viewing { .. }, Command::Enter | Command::Fullscreen) => Action::ToggleFullscreen,
        (Mode::Viewing { .. }, Command::Delete) => Action::AskDelete,
        (Mode::Viewing { fullscreen: true }, Command::Escape) => Action::LeaveFullscreen,
        (Mode::Viewing { fullscreen: false }, Command::Escape) => Action::Quit,
        (Mode::Viewing { .. }, Command::Help) => Action::ShowHelp,
        (Mode::Viewing { .. }, Command::Info) => Action::ShowInfo,
        (Mode::Viewing { .. }, Command::OpenWith) => Action::OpenWith,
        (Mode::Viewing { .. } | Mode::Info, Command::Map) => Action::OpenMap,
        (Mode::Viewing { .. }, Command::Pause) => Action::TogglePause,
        (Mode::Viewing { .. }, Command::Edit) => Action::EnterEdit,
        (Mode::Editing, Command::Escape) => Action::ExitEdit { escape: true },
        (Mode::Editing, Command::Edit) => Action::ExitEdit { escape: false },
        (Mode::Editing, Command::Save) => Action::SaveEdit { copy: false },
        (Mode::Editing, Command::SaveCopy) => Action::SaveEdit { copy: true },
        (Mode::Viewing { .. }, Command::Save | Command::SaveCopy) => Action::None,
        (Mode::Editing, _) => Action::None,
        (Mode::Help, Command::Escape | Command::Help) => Action::CancelOverlay,
        (Mode::Info, Command::Escape | Command::Info) => Action::CancelOverlay,
        (Mode::Confirming, Command::Enter) => Action::DoDelete,
        (Mode::Confirming, Command::Escape) => Action::CancelOverlay,
        (Mode::DeleteFailed, Command::Escape) => Action::CancelOverlay,
        (Mode::Help | Mode::Info | Mode::Confirming | Mode::DeleteFailed, _) => Action::None,
    }
}

/// The CPU architecture the exe was built for, as Windows names it.
#[cfg(target_arch = "x86_64")]
macro_rules! platform {
    () => {
        "x64"
    };
}
#[cfg(target_arch = "aarch64")]
macro_rules! platform {
    () => {
        "arm64"
    };
}
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
macro_rules! platform {
    () => {
        std::env::consts::ARCH
    };
}

/// One row per line: the key, a tab, then what it does. The overlay lines the
/// rows up on the dash between the two, and centers a line with no tab.
const HELP_TEXT: &str = concat!(
    "Matejko ",
    env!("CARGO_PKG_VERSION"),
    " (",
    platform!(),
    ")

Left / Right\tprevious or next file
Home / End\tfirst or last file
Page Up / Page Down\tjump by 10 files

Enter / F11\twindow or fullscreen
Space\tpause or play a video
Esc\tcancel, leave fullscreen, or quit
Mouse wheel\tzoom at the pointer
Drag\tpan

I\tfile information
O\topen with another app
M\tshow the GPS location on Google Maps
E\tedit a JPEG
Delete\tdelete the file
F1\tshow or hide this help"
);

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum OverlayKind {
    Help,
    Info,
}

/// What a help or info text was built from. Another key builds it again.
#[derive(PartialEq, Eq)]
struct OverlayKey {
    kind: OverlayKind,
    path: PathBuf,
    pixels: Option<(u32, u32)>,
    duration: Option<Duration>,
}

/// Two presses on the same slider within this reset it.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// In edit mode the wheel zooms out to this fraction of the fitted size.
const EDIT_MIN_ZOOM: f32 = 0.1;
/// The full-size render waits this long after the last wheel step on a
/// slider. Each step shows the drag-size picture at once.
const WHEEL_SETTLE: Duration = Duration::from_millis(250);

struct Editing {
    loaded: Arc<crate::edit::LoadedSession>,
    worker: EditWorker,
    histogram: [Vec<u32>; 3],
    /// The newest render asked of the worker, and the newest one on screen.
    requested: u64,
    shown: u64,
    fit_pending: bool,
    /// Where the usable rectangle sits in the picture on screen, as
    /// fractions: left, top, width, height. From the newest render.
    usable: [f32; 4],
    /// `Rendered::photo_long` of the picture on screen, or 0 before the first.
    photo_long: f32,
    /// `Rendered::origin` of the picture on screen.
    origin: [f32; 2],
    current: Adjustments,
    saved: Adjustments,
    guide: Guide,
    split: f32,
    scrubbing: bool,
    holding: Option<ImageControls>,
    confirm_exit: bool,
    notice: Option<String>,
    saved_once: bool,
    saving: bool,
    pointer: Option<EditPointer>,
    last_slider_press: Option<(Slider, Instant)>,
    /// Part of a wheel step not yet applied, for touchpads that scroll in
    /// fractions. Kept per slider.
    wheel_rest: Option<(Slider, f32)>,
    /// When to run the full-size render after wheel steps.
    wheel_settle: Option<Instant>,
    /// How far the panel is scrolled. `layout` clamps it to the content.
    scroll: f32,
}

#[derive(Clone, Copy)]
enum EditPointer {
    Divider,
    /// Drags move the value from `start_value` by the pointer's travel.
    Slider {
        slider: Slider,
        start_x: f32,
        start_value: f32,
    },
    Frame {
        handle: Handle,
        start: EditFrame,
        grab_x: f32,
        grab_y: f32,
    },
    Compare,
    /// The panel's scrollbar thumb, grabbed at `start_y`.
    Scroll {
        start_y: f32,
        start_scroll: f32,
    },
}

pub struct App {
    catalog: Catalog,
    /// Started without a file: the catalog holds only the built-in how-to
    /// image, so only viewing commands apply.
    welcome: bool,
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
    /// The current file as of the last frame that reached the screen, when
    /// that frame showed it in full or showed why it failed.
    presented: Option<PathBuf>,
    shown_size: Option<Vec2>,
    prompt: Option<Prompt>,
    /// The help or info text last drawn, and what it was built from.
    overlay_cache: Option<(OverlayKey, String)>,
    decode_error: Option<String>,
    cursor_visibility: CursorVisibility,
    cursor_drawn: bool,
    /// Screen position when the pointer was last hidden or shown. Fullscreen
    /// entry moves the window under a still pointer, and that must not count
    /// as the user using the mouse.
    pointer_anchor: Option<(i32, i32)>,
    stamps: HashMap<PathBuf, FileStamp>,
    video: Option<VideoSession>,
    video_stamp: Option<FileStamp>,
    controls_until: Option<Instant>,
    transport_drawn: bool,
    seeking: bool,
    folder_dirty_until: Option<Instant>,
    _watcher: Option<RecommendedWatcher>,
    editing: Option<Editing>,
    shift_down: bool,
    ctrl_down: bool,
    cursor_icon: CursorIcon,
    /// The pointer is over the panel divider, or dragging it.
    divider_hot: bool,
    /// The panel row or button under the pointer.
    hover: Option<EditHit>,
    /// The file being waited for after a move, for the loading wheel.
    loading: Option<Loading>,
    /// A wheel still fading out after a move on to another file.
    fading: Option<Loading>,
    /// The red cross after a step past either end of the list.
    edge_cross: Option<EdgeCross>,
    /// The last frame drew the wheel or the cross, so one more is needed
    /// to clear it once neither is up.
    indicator_drawn: bool,
    pub fatal: Option<String>,
}

impl App {
    pub fn new(catalog: Catalog, welcome: bool, proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            catalog,
            welcome,
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
            presented: None,
            shown_size: None,
            prompt: None,
            overlay_cache: None,
            decode_error: None,
            cursor_visibility: CursorVisibility::new(),
            cursor_drawn: true,
            pointer_anchor: None,
            stamps: HashMap::new(),
            video: None,
            video_stamp: None,
            controls_until: None,
            transport_drawn: false,
            seeking: false,
            folder_dirty_until: None,
            _watcher: None,
            editing: None,
            shift_down: false,
            ctrl_down: false,
            cursor_icon: CursorIcon::Default,
            divider_hot: false,
            hover: None,
            loading: None,
            fading: None,
            edge_cross: None,
            indicator_drawn: false,
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

    fn title(&self) -> String {
        if self.welcome {
            "Matejko — right-click a photo and choose Open with".into()
        } else if let Some(editing) = &self.editing {
            let name = editing
                .loaded
                .paths
                .open
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("Matejko — Editing: {name}")
        } else {
            self.catalog.title()
        }
    }

    fn refresh_title(&self) {
        if let Some(window) = &self.window {
            if !self.catalog.is_empty() {
                window.set_title(&self.title());
            }
        }
    }

    fn mode(&self) -> Mode {
        let fullscreen = self
            .window
            .as_ref()
            .and_then(|window| window.fullscreen())
            .is_some();
        if self.editing.is_some() {
            return Mode::Editing;
        }
        match &self.prompt {
            Some(Prompt::Help) => Mode::Help,
            Some(Prompt::Info) => Mode::Info,
            Some(Prompt::Delete) => Mode::Confirming,
            Some(Prompt::Failed(_)) => Mode::DeleteFailed,
            None => Mode::Viewing { fullscreen },
        }
    }

    fn on_command(&mut self, event_loop: &ActiveEventLoop, command: Command, repeat: bool) {
        // A held navigation key moves on only once the file it reached is on
        // screen, so every file it passes is shown. The first press always moves.
        if repeat && !self.current_on_screen() {
            return;
        }
        let action = match action_for(command, self.mode(), repeat) {
            Action::AskDelete
            | Action::ShowInfo
            | Action::OpenWith
            | Action::OpenMap
            | Action::EnterEdit
                if self.welcome =>
            {
                Action::None
            }
            action => action,
        };
        match action {
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
                self.pause_video_for_overlay();
                self.prompt = Some(Prompt::Delete);
                self.request_redraw();
            }
            Action::ShowHelp => {
                // Opening reads the file afresh.
                self.overlay_cache = None;
                self.pause_video_for_overlay();
                self.prompt = Some(Prompt::Help);
                self.request_redraw();
            }
            Action::ShowInfo => {
                self.overlay_cache = None;
                self.pause_video_for_overlay();
                self.prompt = Some(Prompt::Info);
                self.request_redraw();
            }
            Action::OpenWith => self.open_with_current(),
            Action::OpenMap => self.open_map_for_current(),
            Action::DoDelete => self.delete_current(event_loop),
            Action::CancelOverlay => {
                self.prompt = None;
                self.resume_video_after_overlay();
                self.request_redraw();
            }
            Action::TogglePause => self.toggle_video_pause(),
            Action::EnterEdit => self.begin_edit(),
            Action::ExitEdit { escape } => self.exit_edit_key(escape),
            Action::SaveEdit { copy: false } => self.save_edit(),
            Action::SaveEdit { copy: true } => self.save_edit_copy(),
        }
    }

    fn open_map_for_current(&self) {
        if self.catalog.is_empty() {
            return;
        }
        let Some(coordinates) = gps_coordinates(&self.catalog.current_path()) else {
            return;
        };
        let url: Vec<u16> = OsStr::new(&maps_url(coordinates))
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let verb: Vec<u16> = OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let hwnd = self
            .window
            .as_ref()
            .and_then(|window| window_hwnd(window))
            .unwrap_or(std::ptr::null_mut());
        unsafe {
            let _ = ShellExecuteW(
                hwnd,
                verb.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    }

    fn open_with_current(&self) {
        if self.catalog.is_empty() {
            return;
        }
        let path = self.catalog.current_path();
        let wide: Vec<u16> = OsStr::new(path.as_os_str())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let hwnd = self
            .window
            .as_ref()
            .and_then(|window| window_hwnd(window))
            .unwrap_or(std::ptr::null_mut());
        let info = OpenAsInfo {
            file: wide.as_ptr(),
            class: std::ptr::null(),
            flags: OAIF_EXEC,
        };
        unsafe {
            let _ = SHOpenWithDialog(hwnd, &info);
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
            self.show_edge_cross(Side::of(delta));
            return;
        }
        self.after_navigate(Side::of(delta));
    }

    fn shift_clamped(&mut self, delta: isize) {
        if !self.catalog.move_clamped(delta) {
            self.show_edge_cross(Side::of(delta));
            return;
        }
        self.after_navigate(Side::of(delta));
    }

    fn jump_first(&mut self) {
        if !self.catalog.go_to_first() {
            return;
        }
        self.after_navigate(Side::Left);
    }

    fn jump_last(&mut self) {
        if !self.catalog.go_to_last() {
            return;
        }
        self.after_navigate(Side::Right);
    }

    fn show_edge_cross(&mut self, side: Side) {
        self.edge_cross = Some(EdgeCross::trigger(self.edge_cross, Instant::now(), side));
        self.request_redraw();
    }

    /// Starts the wait for the loading wheel. It fades out as soon as the
    /// new file is on screen or has failed.
    fn start_loading(&mut self, side: Side) {
        let now = Instant::now();
        if let Some(mut old) = self.loading.take() {
            old.finish(now);
            if !old.gone(now) {
                self.fading = Some(old);
            }
        }
        self.loading = Some(Loading::new(now, side));
    }

    /// Fades the wheel out once its file is shown, and forgets wheels that
    /// have faded.
    fn settle_loading(&mut self, now: Instant) {
        let pending = self.loading_pending();
        if let Some(loading) = &mut self.loading {
            if !pending && !loading.is_finished() {
                loading.finish(now);
            }
            if loading.gone(now) {
                self.loading = None;
            }
        }
        if self.fading.is_some_and(|fading| fading.gone(now)) {
            self.fading = None;
        }
    }

    /// The current file has been drawn: its picture, its first video frame,
    /// or the error that it could not be shown.
    fn current_on_screen(&self) -> bool {
        self.catalog.is_empty() || self.presented.as_ref() == Some(&self.catalog.current_path())
    }

    /// The current file is still being decoded or opened.
    fn loading_pending(&self) -> bool {
        !self.catalog.is_empty()
            && self.editing.is_none()
            && self.decode_error.is_none()
            && self.displayed.as_ref() != Some(&self.catalog.current_path())
    }

    /// The wheel and the cross for this frame.
    fn indicator_shapes(&self, now: Instant) -> Vec<Triangle> {
        let mut shapes = Vec::new();
        let scale = self.scale_factor();
        let (width, height) = (self.window_size.x, self.window_size.y);
        let cross = self
            .edge_cross
            .map(|cross| (cross.side, cross.alpha(now)))
            .filter(|(_, alpha)| *alpha > 0.0);
        for loading in self.fading.iter().chain(&self.loading) {
            let mut alpha = loading.alpha(now);
            // A cross on the same side takes the wheel's place while it is up.
            if let Some((side, cross_alpha)) = cross {
                if side == loading.side {
                    alpha *= 1.0 - cross_alpha;
                }
            }
            let center = indicator::anchor(loading.side, width, height, scale);
            indicator::wheel(center, scale, loading.angle(now), alpha, &mut shapes);
        }
        if let Some((side, alpha)) = cross {
            let center = indicator::anchor(side, width, height, scale);
            indicator::cross(center, scale, alpha, &mut shapes);
        }
        shapes
    }

    fn after_navigate(&mut self, side: Side) {
        self.start_loading(side);
        self.decode_error = None;
        self.refresh_title();
        self.sync_images();
        self.request_redraw();
    }

    fn delete_current(&mut self, event_loop: &ActiveEventLoop) {
        let path = self.catalog.current_path();
        let old_index = self.catalog.index();
        // Close the player first so the file is not locked.
        let was_video = self.video.is_some();
        self.stop_video();
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
                self.start_loading(if self.catalog.index() < old_index {
                    Side::Left
                } else {
                    Side::Right
                });
                self.refresh_title();
                self.sync_images();
                self.request_redraw();
            }
            Err(err) => {
                self.prompt = Some(Prompt::Failed(err.to_string()));
                if was_video {
                    self.sync_playback();
                    if let Some(video) = &mut self.video {
                        video.hold_for_overlay();
                    }
                }
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
            self.stop_video();
            return;
        }
        self.sync_playback();
        let relevant = self.relevant_paths();
        // Files passed on the way here are no longer wanted. Without this,
        // holding an arrow key queues a decode for every file it passes,
        // and the new neighbours wait behind all of them.
        if let Some(decoder) = &self.decoder {
            for path in decoder.retain(&relevant) {
                self.in_flight.remove(&path);
            }
        }
        if !is_video(&relevant[0]) {
            self.request_decode(relevant[0].clone(), true);
        }
        for path in relevant.iter().skip(1) {
            self.request_decode(path.clone(), false);
        }
        self.cache
            .retain(|path, _| relevant.iter().any(|kept| kept == path));
        // A cached file's stamp is the one its decode read, set when the
        // result arrives, so a later change to the file shows as stale.
        self.stamps
            .retain(|path, _| relevant.iter().any(|kept| kept == path));
        if !is_video(&relevant[0]) {
            self.apply_current();
        }
    }

    fn note_folder_dirty(&mut self, event_loop: &ActiveEventLoop) {
        let until = Instant::now() + FOLDER_DEBOUNCE;
        self.folder_dirty_until = Some(until);
        event_loop.set_control_flow(ControlFlow::WaitUntil(until));
    }

    fn apply_folder_changes(&mut self, event_loop: &ActiveEventLoop) {
        // The list stays as it was while editing, so the catalog keeps
        // pointing at the open file even if it vanishes. Leaving edit mode
        // rescans the folder.
        if self.editing.is_some() {
            return;
        }
        // The file under an open overlay may have been rewritten.
        self.overlay_cache = None;
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
                self.start_loading(Side::Right);
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
        if is_video(&path) || self.cache.contains_key(&path) {
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
        self.displayed.is_some() || self.editing.is_some()
    }

    fn begin_edit(&mut self) {
        if self.catalog.is_empty() || self.editing.is_some() {
            return;
        }
        let path = self.catalog.current_path();
        if !is_jpeg(&path) {
            return;
        }
        if let Some(decoder) = &self.decoder {
            decoder.request_edit(path);
        }
    }

    fn accept_baseline(&mut self, path: PathBuf, baseline: crate::decode::EditBaseline) {
        if self.catalog.is_empty() || self.catalog.current_path() != path {
            return;
        }
        let saved = baseline.loaded.adjustments.clone();
        let current = saved.clone();
        let max_edge = self
            .gpu
            .as_ref()
            .map(|gpu| gpu.max_edge())
            .unwrap_or(8192)
            .min(8192);
        let loaded = Arc::new(baseline.loaded);
        let proxy = self.proxy.clone();
        let worker = EditWorker::start(loaded.clone(), baseline.drag, max_edge, move |event| {
            let _ = proxy.send_event(UserEvent::Edit(event));
        });
        self.editing = Some(Editing {
            loaded,
            worker,
            histogram: Default::default(),
            requested: 0,
            shown: 0,
            fit_pending: true,
            usable: [0.0, 0.0, 1.0, 1.0],
            photo_long: 0.0,
            origin: [0.5, 0.5],
            current,
            saved,
            guide: Guide::None,
            split: 0.25,
            scrubbing: false,
            holding: None,
            confirm_exit: false,
            notice: None,
            saved_once: false,
            saving: false,
            pointer: None,
            last_slider_press: None,
            wheel_rest: None,
            wheel_settle: None,
            scroll: 0.0,
        });
        self.refresh_title();
        self.prompt = None;
        self.request_edit_render();
    }

    /// Queues a run of the controls on screen. The worker answers with the
    /// drag-size picture first and, unless a drag is in progress, the
    /// full-size one after it.
    fn request_edit_render(&mut self) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        editing.requested += 1;
        let adjustments = shown_adjustments(editing);
        editing
            .worker
            .render(editing.requested, &adjustments, !editing.scrubbing);
    }

    fn show_edit_render(&mut self, rendered: Rendered) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        // A preview may replace anything older. A full-size picture is shown
        // only while it is still the newest request.
        let fresh = if rendered.full {
            rendered.generation == editing.requested && rendered.generation >= editing.shown
        } else {
            rendered.generation > editing.shown
        };
        if !fresh {
            return;
        }
        editing.shown = rendered.generation;
        editing.histogram = rendered.histogram;
        editing.usable = rendered.usable;
        let old_photo_long = std::mem::replace(&mut editing.photo_long, rendered.photo_long);
        let old_origin = std::mem::replace(&mut editing.origin, rendered.origin);
        let fit = std::mem::take(&mut editing.fit_pending);
        let decoded = DecodedImage {
            width: rendered.width,
            height: rendered.height,
            pixels: rendered.rgba,
            order: PixelOrder::Rgba,
        };
        let new_size = Vec2::new(decoded.width as f32, decoded.height as f32);
        if fit || self.view.fitted {
            self.view.fit(new_size, self.picture_window());
        } else if let Some(old) = self.shown_size {
            // Keep the photo where it was on screen: same size, same centre.
            // The picture's resolution changes between the preview and the
            // full size, and its box changes with rotation and keystone.
            if old_photo_long > 0.0 {
                let centre_x = self.view.pan.x + old.x * old_origin[0] * self.view.scale;
                let centre_y = self.view.pan.y + old.y * old_origin[1] * self.view.scale;
                self.view.scale *= old_photo_long / rendered.photo_long.max(1.0);
                self.view.pan = Vec2::new(
                    centre_x - new_size.x * rendered.origin[0] * self.view.scale,
                    centre_y - new_size.y * rendered.origin[1] * self.view.scale,
                );
            }
        }
        if let Some(gpu) = &mut self.gpu {
            if let Err(err) = gpu.replace_pixels(&decoded) {
                self.decode_error = Some(err);
                return;
            }
        }
        self.shown_size = Some(new_size);
        if !self.catalog.is_empty() {
            self.displayed = Some(self.catalog.current_path());
        }
        self.decode_error = None;
        self.request_redraw();
    }

    fn finish_edit_save(&mut self, saved: Saved) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        editing.saving = false;
        editing.notice = Some(match (&saved.target, saved.result) {
            (SaveTarget::Over, Ok(())) => {
                editing.saved = saved.adjustments;
                editing.saved_once = true;
                "Saved.".into()
            }
            (SaveTarget::Copy(path), Ok(())) => format!("Saved {}.", path.display()),
            (_, Err(err)) => err,
        });
        // The open file is reloaded, and the folder rescanned, on leaving.
        self.request_redraw();
    }

    fn picture_window(&self) -> Vec2 {
        let split = self.editing.as_ref().map(|editing| editing.split).unwrap_or(0.0);
        let width = self.window_size.x * (1.0 - split.clamp(0.0, 0.5));
        Vec2::new(width.max(1.0), self.window_size.y.max(1.0))
    }

    fn edit_panel(&self) -> Option<crate::edit::Panel> {
        let editing = self.editing.as_ref()?;
        Some(layout(
            self.window_size.x,
            self.window_size.y,
            self.scale_factor(),
            editing.split,
            &editing.current,
            editing.guide,
            editing.loaded.had_adj || editing.saved_once,
            editing.holding,
            editing.confirm_exit,
            editing.notice.as_deref(),
            editing.scroll,
        ))
    }

    /// The dimmed margin, the frame outline, and the guides. They belong to
    /// the picture, so they stop at the divider and the panel covers them.
    fn edit_picture_quads(&self, editing: &Editing, panel_x: f32) -> Vec<ExtraQuad> {
        let mut quads = Vec::new();
        let mut quad = |x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4]| {
            let x1 = x1.min(panel_x);
            if x1 > x0 {
                quads.push(ExtraQuad { x0, y0, x1, y1, color });
            }
        };
        if let Some((photo, _, screen)) = self.edit_rects() {
            // The margin outside the frame is dimmed, and so are the empty
            // corners the rotation left, which always lie outside it.
            let dim = [0.0, 0.0, 0.0, 0.55];
            quad(photo[0], photo[1], photo[2], screen[1], dim);
            quad(photo[0], screen[3], photo[2], photo[3], dim);
            quad(photo[0], screen[1], screen[0], screen[3], dim);
            quad(screen[2], screen[1], photo[2], screen[3], dim);
            let edge = [1.0, 1.0, 1.0, 0.6];
            quad(screen[0], screen[1], screen[2], screen[1] + 1.0, edge);
            quad(screen[0], screen[3] - 1.0, screen[2], screen[3], edge);
            quad(screen[0], screen[1], screen[0] + 1.0, screen[3], edge);
            quad(screen[2] - 1.0, screen[1], screen[2], screen[3], edge);
            let guide = [1.0, 1.0, 1.0, 0.85];
            match editing.guide {
                Guide::Thirds => {
                    for t in [1.0 / 3.0, 2.0 / 3.0] {
                        let x = screen[0] + (screen[2] - screen[0]) * t;
                        let y = screen[1] + (screen[3] - screen[1]) * t;
                        quad(x, screen[1], x + 1.0, screen[3], guide);
                        quad(screen[0], y, screen[2], y + 1.0, guide);
                    }
                }
                Guide::Center => {
                    let x = (screen[0] + screen[2]) * 0.5;
                    let y = (screen[1] + screen[3]) * 0.5;
                    quad(x, screen[1], x + 1.0, screen[3], guide);
                    quad(screen[0], y, screen[2], y + 1.0, guide);
                }
                Guide::Pointer
                    if self.cursor_valid
                        && self.cursor.x < panel_x
                        && self.cursor.x >= photo[0]
                        && self.cursor.x <= photo[2]
                        && self.cursor.y >= photo[1]
                        && self.cursor.y <= photo[3] =>
                {
                    quad(self.cursor.x, photo[1], self.cursor.x + 1.0, photo[3], guide);
                    quad(photo[0], self.cursor.y, photo[2], self.cursor.y + 1.0, guide);
                }
                Guide::None | Guide::Pointer => {}
            }
        }
        quads
    }

    fn edit_quads(&self) -> Vec<ExtraQuad> {
        let Some(editing) = self.editing.as_ref() else {
            return Vec::new();
        };
        let Some(panel) = self.edit_panel() else {
            return Vec::new();
        };
        // The picture's overlays first: the panel is drawn over them.
        let mut quads = self.edit_picture_quads(editing, panel.x);
        let mut quad = |x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4]| {
            quads.push(ExtraQuad { x0, y0, x1, y1, color });
        };
        quad(panel.x, 0.0, self.window_size.x, self.window_size.y, [0.08, 0.08, 0.08, 0.94]);
        // The divider: a line the full height, and a grip halfway down that
        // lights up under the pointer.
        let s = panel.scale;
        let accent = [0.36, 0.62, 1.0, 0.95];
        let (line_half, line_color) = if self.divider_hot {
            (1.5 * s, accent)
        } else {
            (1.0, [1.0, 1.0, 1.0, 0.35])
        };
        quad(panel.x - line_half, 0.0, panel.x + line_half, self.window_size.y, line_color);
        let mid = self.window_size.y * 0.5;
        let grip = if self.divider_hot { accent } else { [0.78, 0.78, 0.78, 0.95] };
        quad(panel.x - 4.0 * s, mid - 22.0 * s, panel.x + 4.0 * s, mid + 22.0 * s, grip);
        for offset in [-7.0, 0.0, 7.0] {
            let y = mid + offset * s;
            quad(panel.x - 2.0 * s, y - 0.75 * s, panel.x + 2.0 * s, y + 0.75 * s, [0.1, 0.1, 0.1, 0.9]);
        }
        for button in &panel.buttons {
            let [x0, y0, x1, y1] = button.rect;
            let fill = if button.on {
                [0.36, 0.62, 1.0, 0.4]
            } else {
                [1.0, 1.0, 1.0, 0.07]
            };
            quad(x0, y0, x1, y1, fill);
        }
        let active = match editing.pointer {
            Some(EditPointer::Slider { slider, .. }) => Some(slider),
            _ => None,
        };
        // The row under the pointer, or being dragged: title, value, track.
        if let Some(slider) = self.highlighted_slider() {
            if let Some([x0, y0, x1, y1]) = panel.row_rect(EditHit::Slider(slider)) {
                quad(x0 - 6.0 * s, y0, x1 + 4.0 * s, y1, [1.0, 1.0, 1.0, 0.09]);
            }
        }
        if let Some((track, thumb)) = panel.scrollbar {
            let hot = self.hover == Some(EditHit::Scrollbar)
                || matches!(editing.pointer, Some(EditPointer::Scroll { .. }));
            quad(track[0], track[1], track[2], track[3], [1.0, 1.0, 1.0, 0.05]);
            let alpha = if hot { 0.6 } else { 0.3 };
            quad(thumb[0], thumb[1], thumb[2], thumb[3], [1.0, 1.0, 1.0, alpha]);
        }
        for track in &panel.tracks {
            let slider = track.slider;
            let y = track.y;
            let x = track.x_of(slider.get(&editing.current));
            let origin = track.x_of(slider.origin());
            quad(track.x0, y - s, track.x1, y + s, [1.0, 1.0, 1.0, 0.2]);
            if slider.origin() > slider.range().0 {
                quad(origin - 0.5 * s, y - 5.0 * s, origin + 0.5 * s, y + 5.0 * s, [1.0, 1.0, 1.0, 0.45]);
            }
            quad(origin.min(x), y - s, origin.max(x), y + s, accent);
            let knob = if active == Some(slider) { accent } else { [0.92, 0.92, 0.92, 1.0] };
            quad(x - 3.0 * s, y - 7.0 * s, x + 3.0 * s, y + 7.0 * s, knob);
        }
        let hist = &editing.histogram;
        let [hx0, hy0, hx1, hy1] = panel.histogram;
        let height = (hy1 - hy0).max(1.0);
        let slot = (hx1 - hx0) / 256.0;
        let peak = hist
            .iter()
            .filter_map(|channel| channel.iter().max())
            .max()
            .copied()
            .unwrap_or(1)
            .max(1);
        for (channel, color) in hist.iter().zip([
            [1.0, 0.25, 0.25, 0.55],
            [0.25, 1.0, 0.35, 0.55],
            [0.35, 0.45, 1.0, 0.55],
        ]) {
            for (index, count) in channel.iter().enumerate() {
                let bar = (*count as f32 / peak as f32) * height;
                if bar < 0.5 {
                    continue;
                }
                let left = hx0 + index as f32 * slot;
                quad(left, hy1 - bar, left + 0.8 * slot, hy1, color);
            }
        }
        for button in &panel.compare {
            let [x0, y0, x1, y1] = button.rect;
            let fill = if button.held {
                [0.36, 0.62, 1.0, 0.45]
            } else if button.enabled {
                [1.0, 1.0, 1.0, 0.07]
            } else {
                [1.0, 1.0, 1.0, 0.025]
            };
            quad(x0, y0, x1, y1, fill);
        }
        // A button under the pointer brightens. Its row is the button's
        // rectangle without the inset its background has.
        if let Some(hit) = self
            .hover
            .filter(|hit| !matches!(hit, EditHit::Slider(_) | EditHit::Scrollbar))
        {
            if let Some([x0, y0, x1, y1]) = panel.row_rect(hit) {
                let inset = 2.0 * s;
                quad(x0, y0 + inset, x1, y1 - inset, [1.0, 1.0, 1.0, 0.12]);
            }
        }
        quads
    }

    fn edit_press(&mut self, hit: EditHit) {
        let panel = self.edit_panel();
        let cursor = self.cursor;
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        editing.notice = None;
        let mut rerun = false;
        match hit {
            EditHit::Divider => editing.pointer = Some(EditPointer::Divider),
            EditHit::Guide => editing.guide = editing.guide.next(),
            EditHit::MirrorH => {
                editing.current.mirror_horizontally();
                refit_frame(editing);
                rerun = true;
            }
            EditHit::MirrorV => {
                editing.current.mirror_vertically();
                refit_frame(editing);
                rerun = true;
            }
            EditHit::Turn => {
                editing.current.turn_clockwise(0.0, 0.0);
                // The usable rectangle turned with the photo. A locked ratio
                // is refitted in pixels of the new one.
                refit_frame(editing);
                rerun = true;
            }
            EditHit::Aspect => {
                let (width, height) = oriented_extent(&editing.loaded.baseline, &editing.current);
                editing.current.cycle_aspect(width, height);
                rerun = true;
            }
            EditHit::Slider(slider) => {
                let now = Instant::now();
                let double = matches!(
                    editing.last_slider_press,
                    Some((last, at)) if last == slider && now.duration_since(at) < DOUBLE_CLICK
                );
                let before = slider.get(&editing.current);
                if double {
                    // Double-click puts the slider back to its default.
                    let default = if slider == Slider::Quality {
                        editing.loaded.estimated_quality as f32
                    } else {
                        slider.origin()
                    };
                    apply_slider(editing, slider, default);
                    editing.last_slider_press = None;
                } else {
                    // A press on the track jumps there. A press on the label
                    // only grabs, and the drag moves the value from where it is.
                    let track = panel.as_ref().and_then(|panel| panel.track(slider));
                    if let Some(track) = track.filter(|track| cursor.x >= track.x0 - 4.0) {
                        apply_slider(editing, slider, slider.snap(track.value_at(cursor.x)));
                    }
                    editing.pointer = Some(EditPointer::Slider {
                        slider,
                        start_x: cursor.x,
                        start_value: slider.get(&editing.current),
                    });
                    editing.last_slider_press = Some((slider, now));
                }
                let changed = slider.get(&editing.current) != before;
                if changed && slider != Slider::Quality {
                    // A held button is finished by the release. A double-click
                    // has no drag after it, so it asks for the full size now.
                    editing.scrubbing = !double;
                    rerun = true;
                }
            }
            EditHit::Compare(which) => {
                if matches!(which, ImageControls::Saved | ImageControls::SavedFramed)
                    && !editing.loaded.had_adj
                    && !editing.saved_once
                {
                    return;
                }
                editing.holding = Some(which);
                editing.pointer = Some(EditPointer::Compare);
                rerun = true;
            }
            EditHit::Save => {}
            EditHit::SaveCopy => {}
            EditHit::Exit => {
                if editing.saving {
                    editing.notice = Some("Saving. Exit when it is done.".into());
                } else {
                    editing.confirm_exit = editing.current != editing.saved;
                    if !editing.confirm_exit {
                        self.leave_edit();
                        return;
                    }
                }
            }
            EditHit::ConfirmYes => {
                self.leave_edit();
                return;
            }
            EditHit::ConfirmNo => editing.confirm_exit = false,
            EditHit::Scrollbar => {
                if let Some(panel) = &panel {
                    editing.pointer = Some(EditPointer::Scroll {
                        start_y: cursor.y,
                        start_scroll: panel.scroll,
                    });
                }
            }
        }
        if matches!(hit, EditHit::SaveCopy) {
            self.save_edit_copy();
            return;
        }
        if matches!(hit, EditHit::Save) {
            self.save_edit();
            return;
        }
        if rerun {
            self.request_edit_render();
        }
        self.request_redraw();
    }

    fn edit_drag(&mut self, cursor: Vec2) {
        let Some(editing) = self.editing.as_ref() else {
            return;
        };
        let pointer = editing.pointer;
        match pointer {
            Some(EditPointer::Divider) => {
                let split = (1.0 - cursor.x / self.window_size.x.max(1.0)).clamp(0.18, 0.5);
                if let Some(editing) = &mut self.editing {
                    editing.split = split;
                }
                self.request_redraw();
            }
            Some(EditPointer::Slider {
                slider,
                start_x,
                start_value,
            }) => {
                let Some(track) = self.edit_panel().and_then(|panel| panel.track(slider)) else {
                    return;
                };
                let value = slider.snap(start_value + track.delta(cursor.x - start_x));
                let Some(editing) = &mut self.editing else {
                    return;
                };
                if slider.get(&editing.current) == value {
                    return;
                }
                apply_slider(editing, slider, value);
                if slider == Slider::Quality {
                    self.request_redraw();
                } else {
                    editing.scrubbing = true;
                    self.request_edit_render();
                }
            }
            Some(EditPointer::Frame {
                handle,
                start,
                grab_x,
                grab_y,
            }) => {
                let Some((_, usable, _)) = self.edit_rects() else {
                    return;
                };
                let dx = (cursor.x - grab_x) / (usable[2] - usable[0]).max(1.0);
                let dy = (cursor.y - grab_y) / (usable[3] - usable[1]).max(1.0);
                if let Some(editing) = &mut self.editing {
                    let warp = current_warp(editing);
                    editing.current.frame =
                        drag_frame(start, handle, dx, dy, editing.current.aspect, &warp);
                }
                // The frame is drawn over the picture. The histogram catches
                // up on release.
                self.request_redraw();
            }
            Some(EditPointer::Scroll {
                start_y,
                start_scroll,
            }) => {
                let Some(panel) = self.edit_panel() else {
                    return;
                };
                let scroll = panel.scroll_for_drag(start_scroll, cursor.y - start_y);
                if let Some(editing) = &mut self.editing {
                    editing.scroll = scroll;
                }
                self.request_redraw();
            }
            Some(EditPointer::Compare) | None => {}
        }
    }

    fn edit_release(&mut self) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        let rerun = editing.scrubbing
            || editing.holding.is_some()
            || matches!(editing.pointer, Some(EditPointer::Frame { .. }));
        editing.pointer = None;
        editing.holding = None;
        editing.scrubbing = false;
        if rerun {
            self.request_edit_render();
        }
        self.update_edit_cursor();
        self.request_redraw();
    }

    /// Save: render the current controls over the open file.
    fn save_edit(&mut self) {
        self.start_save(SaveTarget::Over);
    }

    /// Save as copy: the Windows Save As dialog picks the name. It opens in
    /// the photo's folder, and any folder will do. It asks before replacing
    /// a file that exists.
    fn save_edit_copy(&mut self) {
        let Some(editing) = self.editing.as_ref() else {
            return;
        };
        if editing.saving {
            return;
        }
        let open = editing.loaded.paths.open.clone();
        let folder = open.parent().unwrap_or(Path::new(".")).to_path_buf();
        let hwnd = self
            .window
            .as_ref()
            .and_then(|window| window_hwnd(window))
            .unwrap_or(std::ptr::null_mut());
        // Modal: the dialog runs its own message loop until it closes.
        let chosen = save_as_dialog(hwnd, &folder, &copy_stem(&open));
        // The press that opened the dialog was released inside it.
        self.dragging = false;
        let Some(chosen) = chosen else {
            return;
        };
        match copy_path(&open, &chosen) {
            Ok(path) => self.start_save(SaveTarget::Copy(path)),
            Err(message) => {
                if let Some(editing) = &mut self.editing {
                    editing.notice = Some(message);
                }
                self.request_redraw();
            }
        }
    }

    fn start_save(&mut self, target: SaveTarget) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        if editing.saving {
            return;
        }
        editing.worker.save(target, &editing.current);
        editing.saving = true;
        editing.confirm_exit = false;
        editing.notice = Some("Saving…".into());
        self.request_redraw();
    }

    /// Escape or E: the Exit button. While the question about unsaved
    /// changes is up, Escape answers Keep editing and E does nothing, so a
    /// second press cannot throw edits away.
    fn exit_edit_key(&mut self, escape: bool) {
        let Some(editing) = self.editing.as_ref() else {
            return;
        };
        if editing.confirm_exit {
            if escape {
                self.edit_press(EditHit::ConfirmNo);
            }
            return;
        }
        self.edit_press(EditHit::Exit);
    }

    fn leave_edit(&mut self) {
        self.editing = None;
        self.update_edit_cursor();
        if self.catalog.is_empty() {
            return;
        }
        let path = self.catalog.current_path();
        self.cache.remove(&path);
        self.displayed = None;
        self.shown_size = None;
        self.stamps.remove(&path);
        // The folder was not rescanned while editing. Rescan now, before the
        // next wait: `apply_folder_changes` reloads the file, moves on if it
        // is gone, fixes the title, and quits if the folder is empty.
        self.folder_dirty_until = Some(Instant::now());
        self.request_redraw();
    }

    /// The photo, the usable rectangle, and the frame on screen, each as
    /// left, top, right, bottom. The frame is the one in view: a held compare
    /// button shows its own.
    fn edit_rects(&self) -> Option<([f32; 4], [f32; 4], [f32; 4])> {
        let editing = self.editing.as_ref()?;
        let size = self.shown_size?;
        let scale = self.view.scale;
        let photo = [
            self.view.pan.x,
            self.view.pan.y,
            self.view.pan.x + size.x * scale,
            self.view.pan.y + size.y * scale,
        ];
        let [ux, uy, uw, uh] = editing.usable;
        let usable = [
            photo[0] + ux * size.x * scale,
            photo[1] + uy * size.y * scale,
            photo[0] + (ux + uw) * size.x * scale,
            photo[1] + (uy + uh) * size.y * scale,
        ];
        let frame = shown_adjustments(editing).frame;
        let usable_w = usable[2] - usable[0];
        let usable_h = usable[3] - usable[1];
        let screen = [
            usable[0] + frame.x * usable_w,
            usable[1] + frame.y * usable_h,
            usable[0] + (frame.x + frame.w) * usable_w,
            usable[1] + (frame.y + frame.h) * usable_h,
        ];
        Some((photo, usable, screen))
    }

    /// The part of the frame under the pointer. Edges and corners resize.
    /// Inside a frame smaller than the picture, a drag moves it. Elsewhere,
    /// and with Shift held, a drag pans.
    fn frame_handle_at(&self, cursor: Vec2) -> Option<Handle> {
        let editing = self.editing.as_ref()?;
        if self.shift_down || editing.holding.is_some() {
            return None;
        }
        // The panel covers the picture. What lies under it is out of reach.
        let panel = self.edit_panel()?;
        if cursor.x >= panel.x - 6.0 * panel.scale {
            return None;
        }
        let (_, _, [left, top, right, bottom]) = self.edit_rects()?;
        let reach = 6.0 * self.scale_factor().max(1.0);
        let inside_x = cursor.x > left - reach && cursor.x < right + reach;
        let inside_y = cursor.y > top - reach && cursor.y < bottom + reach;
        if !inside_x || !inside_y {
            return None;
        }
        let near_left = (cursor.x - left).abs() <= reach;
        let near_right = (cursor.x - right).abs() <= reach && !near_left;
        let near_top = (cursor.y - top).abs() <= reach;
        let near_bottom = (cursor.y - bottom).abs() <= reach && !near_top;
        let handle = match (near_left, near_right, near_top, near_bottom) {
            (true, _, true, _) => Handle::TopLeft,
            (_, true, true, _) => Handle::TopRight,
            (true, _, _, true) => Handle::BottomLeft,
            (_, true, _, true) => Handle::BottomRight,
            (true, ..) => Handle::Left,
            (_, true, ..) => Handle::Right,
            (_, _, true, _) => Handle::Top,
            (.., true) => Handle::Bottom,
            _ => {
                let full = editing.current.frame.w >= 0.999 && editing.current.frame.h >= 0.999;
                let strictly_inside =
                    cursor.x > left && cursor.x < right && cursor.y > top && cursor.y < bottom;
                if full || !strictly_inside {
                    return None;
                }
                Handle::Move
            }
        };
        Some(handle)
    }

    fn press_on_frame(&mut self) -> bool {
        let Some(handle) = self.frame_handle_at(self.cursor) else {
            return false;
        };
        let cursor = self.cursor;
        let Some(editing) = self.editing.as_mut() else {
            return false;
        };
        editing.pointer = Some(EditPointer::Frame {
            handle,
            start: editing.current.frame,
            grab_x: cursor.x,
            grab_y: cursor.y,
        });
        true
    }

    /// The slider row to highlight: the one being dragged, else the one
    /// under the pointer.
    fn highlighted_slider(&self) -> Option<Slider> {
        match self.editing.as_ref()?.pointer {
            Some(EditPointer::Slider { slider, .. }) => Some(slider),
            Some(_) => None,
            None => match self.hover {
                Some(EditHit::Slider(slider)) => Some(slider),
                _ => None,
            },
        }
    }

    /// Moves `slider` by whole steps of the wheel, one slider step each.
    fn wheel_slider(&mut self, slider: Slider, steps: f32) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        let rest = match editing.wheel_rest {
            Some((last, rest)) if last == slider => rest,
            _ => 0.0,
        } + steps;
        let whole = rest.trunc();
        editing.wheel_rest = Some((slider, rest - whole));
        let before = slider.get(&editing.current);
        let value = slider.snap(before + whole * slider.step());
        if value == before {
            return;
        }
        apply_slider(editing, slider, value);
        if slider == Slider::Quality {
            self.request_redraw();
            return;
        }
        editing.wheel_settle = Some(Instant::now() + WHEEL_SETTLE);
        // Drag-size picture now. The full size waits for the wheel to rest.
        editing.scrubbing = true;
        self.request_edit_render();
        if let Some(editing) = self.editing.as_mut() {
            editing.scrubbing = false;
        }
        self.request_redraw();
    }

    /// The pointer guide follows the pointer, so every move redraws it.
    fn pointer_guide_shown(&self) -> bool {
        self.editing
            .as_ref()
            .is_some_and(|editing| editing.guide == Guide::Pointer)
    }

    /// Resize arrows over the frame's edges, a move cursor inside it.
    fn update_edit_cursor(&mut self) {
        let pointer = self.editing.as_ref().and_then(|editing| editing.pointer);
        let divider = match pointer {
            Some(EditPointer::Divider) => true,
            Some(_) => false,
            None => {
                self.cursor_valid
                    && self
                        .edit_panel()
                        .is_some_and(|panel| panel.on_divider(self.cursor.x))
            }
        };
        if divider != self.divider_hot {
            self.divider_hot = divider;
            self.request_redraw();
        }
        let hover = match pointer {
            None if self.cursor_valid && !divider => self
                .edit_panel()
                .and_then(|panel| hit_test(&panel, self.cursor.x, self.cursor.y))
                .filter(|hit| *hit != EditHit::Divider),
            _ => None,
        };
        if hover != self.hover {
            self.hover = hover;
            self.request_redraw();
        }
        let icon = if divider {
            CursorIcon::EwResize
        } else if self.editing.is_some() {
            let handle = match pointer {
                Some(EditPointer::Frame { handle, .. }) => Some(handle),
                Some(_) => None,
                None => self.frame_handle_at(self.cursor),
            };
            match handle {
                Some(Handle::Left | Handle::Right) => CursorIcon::EwResize,
                Some(Handle::Top | Handle::Bottom) => CursorIcon::NsResize,
                Some(Handle::TopLeft | Handle::BottomRight) => CursorIcon::NwseResize,
                Some(Handle::TopRight | Handle::BottomLeft) => CursorIcon::NeswResize,
                Some(Handle::Move) => CursorIcon::Move,
                None => CursorIcon::Default,
            }
        } else {
            CursorIcon::Default
        };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(window) = &self.window {
                window.set_cursor(icon);
            }
        }
    }

    fn cursor_or_center(&self) -> Vec2 {
        if self.cursor_valid {
            self.cursor
        } else {
            Vec2::new(self.window_size.x / 2.0, self.window_size.y / 2.0)
        }
    }

    /// `HELP_TEXT` with the keys that do nothing on the current file greyed out.
    fn help_text(&self) -> String {
        let path = self.catalog.current_path();
        let unavailable = |key: &str| match key {
            "Space" => !is_video(&path),
            "M" => gps_coordinates(&path).is_none(),
            "E" => !is_jpeg(&path),
            _ => false,
        };
        HELP_TEXT
            .lines()
            .map(|line| match line.split_once('\t') {
                Some((key, _)) if unavailable(key) => format!("{DIMMED_ROW}{line}"),
                _ => line.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The help or info text, read from the file only when what it shows
    /// has changed. Both read the EXIF, and a redraw comes with every pan
    /// step and every frame of the loading wheel.
    fn cached_overlay(&mut self, key: OverlayKey) -> String {
        if let Some((cached, text)) = &self.overlay_cache {
            if *cached == key {
                return text.clone();
            }
        }
        let text = match key.kind {
            OverlayKind::Help => self.help_text(),
            OverlayKind::Info => format_image_info(&key.path, key.pixels, key.duration),
        };
        self.overlay_cache = Some((key, text.clone()));
        text
    }

    fn overlay_text(&mut self) -> Option<String> {
        if self.catalog.is_empty() {
            return None;
        }
        let name = self.catalog.file_name();
        let path = self.catalog.current_path();
        match &self.prompt {
            Some(Prompt::Help) => Some(self.cached_overlay(OverlayKey {
                kind: OverlayKind::Help,
                path,
                pixels: None,
                duration: None,
            })),
            Some(Prompt::Info) => {
                // The size and the length can arrive after the overlay opens.
                let pixels = self
                    .image_size()
                    .map(|size| (size.x.round() as u32, size.y.round() as u32));
                let duration = self.video.as_ref().and_then(|video| video.duration());
                Some(self.cached_overlay(OverlayKey {
                    kind: OverlayKind::Info,
                    path,
                    pixels,
                    duration,
                }))
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

    fn sync_playback(&mut self) {
        if self.catalog.is_empty() || self.gpu.is_none() {
            self.stop_video();
            return;
        }
        let path = self.catalog.current_path();
        if !is_video(&path) {
            self.stop_video();
            return;
        }
        let stamp = read_stamp(&path);
        if self
            .video
            .as_ref()
            .is_some_and(|video| video.path() == path)
            && self.video_stamp == stamp
        {
            return;
        }
        let reload = self
            .video
            .as_ref()
            .is_some_and(|video| video.path() == path);
        self.stop_video();
        if reload {
            self.displayed = None;
        }
        let max_edge = self.gpu.as_ref().map(|gpu| gpu.max_edge()).unwrap_or(8192);
        match VideoSession::open(&path, self.proxy.clone(), max_edge) {
            Ok(session) => {
                self.video_stamp = stamp;
                self.video = Some(session);
                self.decode_error = None;
            }
            Err(err) => self.fail_video(err),
        }
    }

    fn stop_video(&mut self) {
        self.video = None;
        self.video_stamp = None;
        self.controls_until = None;
        self.seeking = false;
    }

    fn fail_video(&mut self, message: String) {
        self.stop_video();
        self.decode_error = Some(message);
        self.displayed = None;
        self.shown_size = None;
        if let Some(gpu) = &mut self.gpu {
            gpu.clear_image();
        }
        self.request_redraw();
    }

    fn present_video_frame(&mut self, image: DecodedImage) {
        if self.catalog.is_empty() || !is_video(&self.catalog.current_path()) {
            return;
        }
        let path = self.catalog.current_path();
        let size = Vec2::new(image.width as f32, image.height as f32);
        let upload_error = self
            .gpu
            .as_mut()
            .and_then(|gpu| gpu.replace_pixels(&image).err());
        if let Some(err) = upload_error {
            self.fail_video(err);
            return;
        }
        // The frame is on the GPU. Its buffer takes the next one.
        if let Some(video) = &self.video {
            video.recycle(image.pixels);
        }
        let first = self.displayed.as_ref() != Some(&path) || self.shown_size != Some(size);
        if first {
            self.view.fit(size, self.window_size);
            self.displayed = Some(path);
            self.shown_size = Some(size);
            self.decode_error = None;
        }
        self.request_redraw();
    }

    fn toggle_video_pause(&mut self) {
        if let Some(video) = &mut self.video {
            video.toggle_pause();
        }
        self.request_redraw();
    }

    fn pause_video_for_overlay(&mut self) {
        if let Some(video) = &mut self.video {
            video.pause_for_overlay();
        }
    }

    fn resume_video_after_overlay(&mut self) {
        if let Some(video) = &mut self.video {
            video.resume_after_overlay();
        }
    }

    fn transport_visible(&self, now: Instant) -> bool {
        let Some(video) = &self.video else {
            return false;
        };
        if self.prompt.is_some() {
            return false;
        }
        if video.hold_controls() {
            return true;
        }
        self.controls_until.is_some_and(|until| now < until)
    }

    fn poke_transport(&mut self) {
        if self.video.is_none() || self.prompt.is_some() {
            return;
        }
        self.controls_until = Some(Instant::now() + CURSOR_VISIBLE_FOR);
        if !self.transport_drawn && self.transport_visible(Instant::now()) {
            self.transport_drawn = true;
            self.request_redraw();
        }
    }

    fn scale_factor(&self) -> f32 {
        self.window
            .as_ref()
            .map(|window| window.scale_factor() as f32)
            .unwrap_or(1.0)
    }

    fn transport_layout(&self) -> Option<transport::Layout> {
        let video = self.video.as_ref()?;
        let (position, duration) = video.timeline();
        Some(transport::layout(
            self.window_size.x,
            self.window_size.y,
            self.scale_factor(),
            transport::fraction(position, duration),
        ))
    }

    fn transport_hit(&self) -> Option<Hit> {
        if !self.cursor_valid || !self.transport_visible(Instant::now()) {
            return None;
        }
        let layout = self.transport_layout()?;
        transport::hit(&layout, self.cursor.x, self.cursor.y)
    }

    fn seek_video_at(&mut self, x: f32) {
        let Some(layout) = self.transport_layout() else {
            return;
        };
        let fraction = transport::fraction_at(&layout, x);
        if let Some(video) = &mut self.video {
            video.seek_fraction(fraction);
        }
        self.request_redraw();
    }

    fn transport_draw<'a>(&self, label: Option<&'a str>) -> Option<TransportDraw<'a>> {
        let video = self.video.as_ref()?;
        let label = label?;
        if !self.transport_visible(Instant::now()) {
            return None;
        }
        Some(TransportDraw {
            layout: self.transport_layout()?,
            show_play_icon: video.show_play_icon(),
            label,
        })
    }

    /// Draws the current frame.
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let text = self.overlay_text();
        let scale_factor = self.scale_factor();
        let (image_width, image_height) = self
            .image_size()
            .map(|size| (size.x, size.y))
            .unwrap_or((0.0, 0.0));
        let label = self.video.as_ref().map(|video| {
            let (position, duration) = video.timeline();
            transport::format_clock_range(position, duration)
        });
        let transport = self.transport_draw(label.as_deref());
        let quads = self.edit_quads();
        let now = Instant::now();
        self.settle_loading(now);
        let shapes = self.indicator_shapes(now);
        self.indicator_drawn = !shapes.is_empty();
        let panel = self.edit_panel();
        let bright = self
            .highlighted_slider()
            .map(EditHit::Slider)
            .or(self.hover);
        let labels: Vec<Label> = panel
            .iter()
            .flat_map(|panel| &panel.texts)
            .map(|text| Label {
                text: &text.text,
                x0: text.x0,
                x1: text.x1,
                top: text.top,
                color: match text.tone {
                    Tone::Normal if text.row.is_some() && text.row == bright => {
                        [255, 255, 255]
                    }
                    Tone::Normal => [212, 212, 212],
                    Tone::Heading => [128, 140, 158],
                    Tone::Disabled => [100, 100, 100],
                    Tone::Notice => [240, 208, 140],
                },
                align: match text.align {
                    Align::Left => LabelAlign::Left,
                    Align::Center => LabelAlign::Center,
                    Align::Right => LabelAlign::Right,
                },
                wrap: text.tone == Tone::Notice,
            })
            .collect();
        let frame = Frame {
            image_width,
            image_height,
            scale: self.view.scale,
            pan_x: self.view.pan.x,
            pan_y: self.view.pan.y,
            dim: self.prompt.is_some() && self.editing.is_none(),
            text: if self.editing.is_some() { None } else { text.as_deref() },
            transport: if self.editing.is_some() { None } else { transport },
            scale_factor,
            extra: &quads,
            shapes: &shapes,
            labels: &labels,
        };
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        match gpu.draw(frame) {
            Ok(true) => {
                // The current file is on screen once nothing is left to load
                // for it: its picture, or the error that it cannot be shown.
                self.presented = (!self.catalog.is_empty() && !self.loading_pending())
                    .then(|| self.catalog.current_path());
            }
            Ok(false) => {}
            Err(err) => self.fail(event_loop, err),
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
            .with_title(self.title())
            .with_inner_size(size)
            .with_position(position)
            .with_resizable(true)
            .with_window_icon(icon.clone())
            .with_taskbar_icon(icon)
            // Shown after the first black frame, so the window never flashes
            // white before the GPU has drawn into it.
            .with_visible(false);
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
        self.start_loading(Side::Center);
        self.sync_images();
        self.redraw(event_loop);
        if let Some(window) = &self.window {
            window.set_visible(true);
        }
        self.request_redraw();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.folder_dirty_until.is_some_and(|until| now >= until) {
            self.folder_dirty_until = None;
            self.apply_folder_changes(event_loop);
            if self.fatal.is_some() || self.catalog.is_empty() {
                return;
            }
        }

        let wheel_settled = self.editing.as_mut().is_some_and(|editing| {
            let due = editing.wheel_settle.is_some_and(|at| now >= at);
            if due {
                editing.wheel_settle = None;
            }
            due
        });
        if wheel_settled {
            self.request_edit_render();
        }

        let fullscreen = self.is_fullscreen();
        let visible = self.cursor_visibility.poll(now, fullscreen);
        if visible {
            self.apply_cursor(true);
        } else {
            self.force_cursor(false);
        }

        let bar = self.transport_visible(now);
        if bar != self.transport_drawn {
            self.transport_drawn = bar;
            self.request_redraw();
        }

        self.settle_loading(now);
        if self.edge_cross.is_some_and(|cross| cross.done(now)) {
            self.edge_cross = None;
        }
        let animating = self.edge_cross.is_some()
            || self.fading.is_some()
            || self.loading.is_some_and(|loading| now >= loading.shows_at());
        if animating || self.indicator_drawn {
            // Each frame asks for the next one; Fifo present paces them.
            self.request_redraw();
        }

        let mut wake_at = self.folder_dirty_until;
        if let Some(at) = self.loading.map(|loading| loading.shows_at()) {
            if at > now {
                wake_at = Some(wake_at.map_or(at, |existing| existing.min(at)));
            }
        }
        if let Some(at) = self.editing.as_ref().and_then(|editing| editing.wheel_settle) {
            wake_at = Some(wake_at.map_or(at, |existing| existing.min(at)));
        }
        if fullscreen {
            if let Some(until) = self.cursor_visibility.wake_at() {
                wake_at = Some(wake_at.map_or(until, |existing| existing.min(until)));
            }
        }
        if self
            .video
            .as_ref()
            .is_some_and(|video| !video.hold_controls())
            && self.prompt.is_none()
        {
            if let Some(until) = self.controls_until {
                if now < until {
                    wake_at = Some(wake_at.map_or(until, |existing| existing.min(until)));
                }
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
            UserEvent::Decoded {
                path,
                stamp,
                result,
            } => {
                self.in_flight.remove(&path);
                if !self.is_relevant(&path) {
                    return;
                }
                // The file changed while it was being read, as when it is
                // still being copied in. The pixels are of an older file:
                // read it again. The folder event for that change may
                // already have passed, so this cannot wait for one.
                if read_stamp(&path) != stamp {
                    let front = path == self.catalog.current_path();
                    self.request_decode(path, front);
                    return;
                }
                match stamp {
                    Some(stamp) => self.stamps.insert(path.clone(), stamp),
                    None => self.stamps.remove(&path),
                };
                match result {
                    Ok(image) => {
                        self.cache.insert(path, CacheEntry::Ready(image));
                    }
                    Err(err) => {
                        self.cache.insert(path, CacheEntry::Failed(err));
                    }
                }
                if self.editing.is_none() {
                    self.apply_current();
                    self.request_redraw();
                }
            }
            UserEvent::BaselineReady { path, result } => match result {
                Ok(baseline) => self.accept_baseline(path, baseline),
                Err(err) => {
                    self.decode_error = Some(err);
                    self.request_redraw();
                }
            },
            UserEvent::Edit(WorkerEvent::Rendered(rendered)) => self.show_edit_render(rendered),
            UserEvent::Edit(WorkerEvent::Saved(saved)) => self.finish_edit_save(saved),
            UserEvent::VideoFrame { generation } => {
                let image = {
                    let Some(video) = &self.video else {
                        return;
                    };
                    if video.generation() != generation {
                        return;
                    }
                    video.take_frame()
                };
                if let Some(image) = image {
                    self.present_video_frame(image);
                }
            }
            UserEvent::VideoFailed {
                generation,
                message,
            } => {
                let current = self
                    .video
                    .as_ref()
                    .is_some_and(|video| video.generation() == generation);
                if current {
                    self.fail_video(message);
                }
            }
            UserEvent::VideoEnded { generation } => {
                if let Some(video) = &mut self.video {
                    if video.generation() == generation {
                        video.mark_ended();
                        self.request_redraw();
                    }
                }
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
            WindowEvent::CursorLeft { .. } => {
                // The pointer guide hides while the pointer is off the photo.
                self.cursor_valid = false;
                self.update_edit_cursor();
                if self.pointer_guide_shown() {
                    self.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift_down = modifiers.state().shift_key();
                self.ctrl_down = modifiers.state().control_key();
                self.update_edit_cursor();
            }
            WindowEvent::Occluded(false) => self.request_redraw(),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(command) =
                    command_from(&event.logical_key, self.ctrl_down, self.shift_down)
                {
                    self.on_command(event_loop, command, event.repeat);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.note_mouse();
                self.poke_transport();
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
                if let Some(panel) = self.edit_panel() {
                    // Over the panel the wheel steps the highlighted slider,
                    // or else scrolls the panel. It never zooms the picture.
                    if self.cursor.x >= panel.x - 6.0 * panel.scale {
                        if let Some(EditHit::Slider(slider)) = self.hover {
                            self.wheel_slider(slider, steps);
                        } else if panel.max_scroll > 0.0 {
                            let scroll = (panel.scroll - steps * 48.0 * panel.scale)
                                .clamp(0.0, panel.max_scroll);
                            if let Some(editing) = &mut self.editing {
                                editing.scroll = scroll;
                            }
                            // The rows moved under the pointer.
                            self.update_edit_cursor();
                            self.request_redraw();
                        }
                        return;
                    }
                }
                let cursor = self.cursor_or_center();
                let factor = 1.1_f32.powf(steps);
                if self.editing.is_some() {
                    // The viewer stops zooming out at the fitted size. Edit
                    // mode goes further, to see the frame and its margin.
                    let window = self.picture_window();
                    self.view
                        .zoom_at_down_to(cursor, factor, image, window, EDIT_MIN_ZOOM);
                } else {
                    self.view.zoom_at(cursor, factor, image, self.window_size);
                }
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.note_mouse();
                self.poke_transport();
                if button != MouseButton::Left {
                    return;
                }
                if state != ElementState::Pressed {
                    self.dragging = false;
                    self.seeking = false;
                    if self.editing.is_some() {
                        self.edit_release();
                    }
                    return;
                }
                if self.editing.is_some() {
                    if let Some(panel) = self.edit_panel() {
                        if let Some(hit) = hit_test(&panel, self.cursor.x, self.cursor.y) {
                            self.dragging = false;
                            self.edit_press(hit);
                            return;
                        }
                        if self.cursor.x >= panel.x {
                            // Empty panel space, or a greyed-out button.
                            self.dragging = false;
                            return;
                        }
                    }
                    if self.press_on_frame() {
                        self.dragging = false;
                        return;
                    }
                }
                match self.transport_hit() {
                    Some(Hit::Button) => {
                        self.dragging = false;
                        self.seeking = false;
                        self.toggle_video_pause();
                    }
                    Some(Hit::Track(fraction)) => {
                        self.dragging = false;
                        self.seeking = true;
                        if let Some(video) = &mut self.video {
                            video.seek_fraction(fraction);
                        }
                        self.request_redraw();
                    }
                    Some(Hit::Chrome) => {
                        self.dragging = false;
                        self.seeking = false;
                    }
                    None => {
                        self.seeking = false;
                        self.dragging = true;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.note_pointer_motion();
                self.poke_transport();
                let next = Vec2::new(position.x as f32, position.y as f32);
                if self.editing.as_ref().is_some_and(|editing| editing.pointer.is_some()) {
                    self.cursor = next;
                    self.cursor_valid = true;
                    self.edit_drag(next);
                    return;
                }
                if self.editing.is_some() && !self.dragging {
                    self.cursor = next;
                    self.cursor_valid = true;
                    self.update_edit_cursor();
                    if self.pointer_guide_shown() {
                        self.request_redraw();
                    }
                }
                if self.seeking {
                    self.seek_video_at(next.x);
                } else if self.dragging && self.cursor_valid && self.image_shown() {
                    self.view
                        .pan_by(Vec2::new(next.x - self.cursor.x, next.y - self.cursor.y));
                    self.request_redraw();
                }
                self.cursor = next;
                self.cursor_valid = true;
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }
}

fn shown_adjustments(editing: &Editing) -> Adjustments {
    let Some(hold) = editing.holding else {
        return editing.current.clone();
    };
    match hold {
        ImageControls::Current => editing.current.clone(),
        ImageControls::Original => Adjustments::defaults(editing.current.jpeg_quality),
        ImageControls::OriginalFramed => {
            let mut adjustments = Adjustments::defaults(editing.current.jpeg_quality);
            copy_geometry(&mut adjustments, &editing.current);
            adjustments
        }
        ImageControls::Saved => editing.saved.clone(),
        ImageControls::SavedFramed => {
            let mut adjustments = editing.saved.clone();
            copy_geometry(&mut adjustments, &editing.current);
            adjustments
        }
    }
}

fn copy_geometry(tone: &mut Adjustments, geometry: &Adjustments) {
    tone.quarter_turns = geometry.quarter_turns;
    tone.mirror_horizontal = geometry.mirror_horizontal;
    tone.mirror_vertical = geometry.mirror_vertical;
    tone.rotation = geometry.rotation;
    tone.aspect = geometry.aspect;
    tone.frame = geometry.frame;
}

fn oriented_extent(image: &LinearImage, adjustments: &Adjustments) -> (f32, f32) {
    if adjustments.quarter_turns % 2 == 1 {
        (image.height as f32, image.width as f32)
    } else {
        (image.width as f32, image.height as f32)
    }
}

/// Sets a slider. Rotation also pulls the frame back inside the rotated photo.
fn apply_slider(editing: &mut Editing, slider: Slider, value: f32) {
    slider.set(&mut editing.current, value);
    if matches!(
        slider,
        Slider::Rotation | Slider::Vertical | Slider::Horizontal | Slider::Distortion
    ) {
        refit_frame(editing);
    }
}

/// Rotation and keystone for the current controls, at full resolution.
fn current_warp(editing: &Editing) -> Warp {
    let (width, height) = oriented_extent(&editing.loaded.baseline, &editing.current);
    editing.current.warp(width, height)
}

/// Puts the frame back on the corrected photo after its geometry changed.
fn refit_frame(editing: &mut Editing) {
    let warp = current_warp(editing);
    let current = &editing.current;
    editing.current.frame = clamp_frame_to_photo(current.frame, current.aspect, &warp);
}

fn is_jpeg(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg"))
}

/// The path of a copy chosen in the Save As dialog, if it can be one: a
/// JPEG, and not the open file itself. Any folder will do: the copy takes
/// its baseline along into that folder's `.matejko`.
fn copy_path(open: &Path, chosen: &Path) -> Result<PathBuf, String> {
    if !is_jpeg(chosen) {
        return Err("A copy is a JPEG. End its name with .jpg or .jpeg.".into());
    }
    let same_file = match (fs::canonicalize(open), chosen.parent().map(fs::canonicalize)) {
        (Ok(open), Some(Ok(folder))) => chosen
            .file_name()
            .is_some_and(|name| folder.join(name).to_string_lossy().to_lowercase()
                == open.to_string_lossy().to_lowercase()),
        _ => chosen == open,
    };
    if same_file {
        return Err("That is the open file. Save writes over it.".into());
    }
    Ok(chosen.to_path_buf())
}

/// The Windows Save As dialog, opened in `folder` with `suggested` filled in.
/// It asks before replacing a file that exists. `None` when cancelled.
fn save_as_dialog(hwnd: *mut std::ffi::c_void, folder: &Path, suggested: &str) -> Option<PathBuf> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_EXPLORER, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }
    // The file name buffer: the suggestion in, the chosen path out.
    let mut file = vec![0u16; 1024];
    for (slot, unit) in file.iter_mut().zip(OsStr::new(suggested).encode_wide()) {
        *slot = unit;
    }
    // Pairs of display name and pattern, ended by an empty string.
    let filter: Vec<u16> = "JPEG image\0*.jpg;*.jpeg\0\0".encode_utf16().collect();
    let folder = wide(folder.as_os_str());
    let title = wide(OsStr::new("Save as copy"));
    let extension = wide(OsStr::new("jpg"));
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: HWND(hwnd),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrInitialDir: PCWSTR(folder.as_ptr()),
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(extension.as_ptr()),
        Flags: OFN_EXPLORER
            | OFN_OVERWRITEPROMPT
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | OFN_HIDEREADONLY,
        ..Default::default()
    };
    if !unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
        return None;
    }
    let end = file.iter().position(|unit| *unit == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..end])))
}

fn copy_stem(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "photo".into());
    format!("{stem}_copy.jpg")
}

/// With Ctrl held, only Ctrl+S and Ctrl+Shift+S are commands, so a Ctrl
/// chord never also acts as its plain letter (Ctrl+E does not leave edit mode).
fn command_from(key: &Key, ctrl: bool, shift: bool) -> Option<Command> {
    if ctrl {
        return match key {
            Key::Character(ch) if ch.eq_ignore_ascii_case("s") => {
                Some(if shift { Command::SaveCopy } else { Command::Save })
            }
            _ => None,
        };
    }
    match key {
        Key::Named(NamedKey::ArrowLeft) => Some(Command::Left),
        Key::Named(NamedKey::ArrowRight) => Some(Command::Right),
        Key::Named(NamedKey::Home) => Some(Command::Home),
        Key::Named(NamedKey::End) => Some(Command::End),
        Key::Named(NamedKey::PageUp) => Some(Command::PageUp),
        Key::Named(NamedKey::PageDown) => Some(Command::PageDown),
        Key::Named(NamedKey::Enter) => Some(Command::Enter),
        Key::Named(NamedKey::F11) => Some(Command::Fullscreen),
        Key::Named(NamedKey::Delete) => Some(Command::Delete),
        Key::Named(NamedKey::Escape) => Some(Command::Escape),
        Key::Named(NamedKey::F1) => Some(Command::Help),
        Key::Character(ch) if ch.eq_ignore_ascii_case("i") => Some(Command::Info),
        Key::Character(ch) if ch.eq_ignore_ascii_case("o") => Some(Command::OpenWith),
        Key::Character(ch) if ch.eq_ignore_ascii_case("m") => Some(Command::Map),
        Key::Character(ch) if ch.eq_ignore_ascii_case("e") => Some(Command::Edit),
        Key::Named(NamedKey::Space) => Some(Command::Pause),
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

/// Open the file after the user picks an app in the Open With dialog.
const OAIF_EXEC: u32 = 0x4;

/// Show the browser window that opens the map.
const SW_SHOWNORMAL: i32 = 1;

#[repr(C)]
struct OpenAsInfo {
    file: *const u16,
    class: *const u16,
    flags: u32,
}

#[link(name = "user32")]
extern "system" {
    fn GetMonitorInfoW(monitor: *mut std::ffi::c_void, info: *mut MonitorInfo) -> i32;
    fn GetCursorPos(point: *mut Point) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn SHOpenWithDialog(hwnd_parent: *mut std::ffi::c_void, info: *const OpenAsInfo) -> i32;
    fn ShellExecuteW(
        hwnd: *mut std::ffi::c_void,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> *mut std::ffi::c_void;
}

fn window_hwnd(window: &Window) -> Option<*mut std::ffi::c_void> {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get() as *mut std::ffi::c_void),
        _ => None,
    }
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
    fn a_copy_is_a_jpeg_anywhere_but_not_the_open_photo() {
        let root = std::env::temp_dir().join(format!("matejko-copy-{}", std::process::id()));
        let folder = root.join("holiday");
        let other = root.join("elsewhere");
        fs::create_dir_all(&folder).unwrap();
        fs::create_dir_all(&other).unwrap();
        let open = folder.join("photo.jpg");
        fs::write(&open, b"jpeg").unwrap();
        assert!(copy_path(&open, &folder.join("photo_v2.jpg")).is_ok());
        assert!(copy_path(&open, &folder.join("Photo v2.JPEG")).is_ok());
        // Another folder is fine, even under the photo's own name.
        assert_eq!(copy_path(&open, &other.join("photo.jpg")), Ok(other.join("photo.jpg")));
        assert!(copy_path(&open, &folder.join("photo_v2.png")).is_err());
        assert!(copy_path(&open, &folder.join("PHOTO.jpg")).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn keys_follow_the_viewing_and_confirmation_table() {
        let viewing = Mode::Viewing { fullscreen: false };
        let fullscreen = Mode::Viewing { fullscreen: true };
        assert_eq!(action_for(Command::Left, viewing, false), Action::Previous);
        assert_eq!(action_for(Command::Right, viewing, true), Action::Next);
        assert_eq!(action_for(Command::Home, viewing, false), Action::First);
        assert_eq!(action_for(Command::End, viewing, false), Action::Last);
        assert_eq!(action_for(Command::PageUp, viewing, true), Action::PageBack);
        assert_eq!(
            action_for(Command::PageDown, viewing, false),
            Action::PageForward
        );
        assert_eq!(action_for(Command::Home, Mode::Help, false), Action::None);
        assert_eq!(
            action_for(Command::Enter, viewing, false),
            Action::ToggleFullscreen
        );
        assert_eq!(action_for(Command::Enter, viewing, true), Action::None);
        assert_eq!(
            action_for(Command::Fullscreen, fullscreen, false),
            Action::ToggleFullscreen
        );
        assert_eq!(
            action_for(Command::Fullscreen, Mode::Confirming, false),
            Action::None
        );
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
        assert_eq!(
            action_for(Command::OpenWith, viewing, false),
            Action::OpenWith
        );
        assert_eq!(action_for(Command::OpenWith, viewing, true), Action::None);
        assert_eq!(
            action_for(Command::OpenWith, Mode::Help, false),
            Action::None
        );
        assert_eq!(
            action_for(Command::OpenWith, Mode::Confirming, false),
            Action::None
        );
        assert_eq!(action_for(Command::Map, viewing, false), Action::OpenMap);
        assert_eq!(action_for(Command::Map, viewing, true), Action::None);
        assert_eq!(action_for(Command::Map, Mode::Info, false), Action::OpenMap);
        assert_eq!(action_for(Command::Map, Mode::Help, false), Action::None);
        assert_eq!(
            action_for(Command::Map, Mode::Confirming, false),
            Action::None
        );
        assert_eq!(action_for(Command::Map, Mode::Editing, false), Action::None);
        assert_eq!(
            action_for(Command::Pause, viewing, false),
            Action::TogglePause
        );
        assert_eq!(action_for(Command::Pause, viewing, true), Action::None);
        assert_eq!(action_for(Command::Pause, Mode::Help, false), Action::None);
        assert_eq!(
            action_for(Command::Pause, Mode::Confirming, false),
            Action::None
        );
        assert_eq!(action_for(Command::Left, viewing, false), Action::Previous);
        assert_eq!(
            action_for(Command::Edit, viewing, false),
            Action::EnterEdit
        );
        assert_eq!(action_for(Command::Edit, viewing, true), Action::None);
        assert_eq!(
            action_for(Command::Escape, Mode::Editing, false),
            Action::ExitEdit { escape: true }
        );
        assert_eq!(
            action_for(Command::Edit, Mode::Editing, false),
            Action::ExitEdit { escape: false }
        );
        assert_eq!(action_for(Command::Escape, Mode::Editing, true), Action::None);
        assert_eq!(
            action_for(Command::Save, Mode::Editing, false),
            Action::SaveEdit { copy: false }
        );
        assert_eq!(
            action_for(Command::SaveCopy, Mode::Editing, false),
            Action::SaveEdit { copy: true }
        );
        assert_eq!(action_for(Command::Save, Mode::Editing, true), Action::None);
        assert_eq!(action_for(Command::Save, viewing, false), Action::None);
        assert_eq!(action_for(Command::SaveCopy, Mode::Help, false), Action::None);
        assert_eq!(action_for(Command::Left, Mode::Editing, false), Action::None);
        assert_eq!(action_for(Command::Enter, Mode::Editing, false), Action::None);
        assert_eq!(action_for(Command::Delete, Mode::Editing, false), Action::None);
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
