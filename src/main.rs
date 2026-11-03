#![cfg_attr(not(test), windows_subsystem = "windows")]

mod app;
mod catalog;
mod decode;
mod edit;
mod gpu;
mod indicator;
mod info;
mod playback;
mod transport;
mod view;
mod watch;

use std::path::PathBuf;

use winit::event_loop::EventLoop;

use crate::app::App;
use crate::catalog::Catalog;
use crate::decode::UserEvent;

fn main() {
    if let Err(err) = run() {
        error_box(&err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let (path, welcome) = match image_argument()? {
        Some(path) => (PathBuf::from(path), false),
        None => (write_welcome_image()?, true),
    };
    let catalog = Catalog::open(&path)?;
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|err| err.to_string())?;
    let mut app = App::new(catalog, welcome, event_loop.create_proxy());
    event_loop
        .run_app(&mut app)
        .map_err(|err| err.to_string())?;
    if let Some(err) = app.fatal {
        return Err(err);
    }
    Ok(())
}

/// The file to open, or `None` when started without one.
fn image_argument() -> Result<Option<std::ffi::OsString>, String> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next();
    if args.next().is_some() {
        return Err("Matejko accepts one file path.".into());
    }
    Ok(path)
}

/// Shown when started without a file: how to open a photo with Open with.
const WELCOME_IMAGE: &[u8] = include_bytes!("../assets/welcome.png");

/// Writes the how-to image into a folder of its own, so the catalog holds only
/// that one file.
fn write_welcome_image() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("matejko-welcome");
    let path = dir.join("welcome.png");
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&path, WELCOME_IMAGE))
        .map_err(|err| format!("Couldn't prepare the welcome image: {err}"))?;
    Ok(path)
}

fn error_box(message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let text = wide(message);
    let title = wide("Matejko");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            0x0000_0010,
        );
    }
}

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(
        window: *mut std::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        kind: u32,
    ) -> i32;
}
