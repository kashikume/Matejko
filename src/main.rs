#![cfg_attr(not(test), windows_subsystem = "windows")]

mod app;
mod catalog;
mod decode;
mod gpu;
mod info;
mod playback;
mod transport;
mod view;
mod watch;

use std::path::Path;

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
    let path = image_argument()?;
    let catalog = Catalog::open(Path::new(&path))?;
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|err| err.to_string())?;
    let mut app = App::new(catalog, event_loop.create_proxy());
    event_loop
        .run_app(&mut app)
        .map_err(|err| err.to_string())?;
    if let Some(err) = app.fatal {
        return Err(err);
    }
    Ok(())
}

fn image_argument() -> Result<std::ffi::OsString, String> {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next() else {
        return Err(
            "Open Matejko with one JPEG, PNG, or MP4 file.\n\nmatejko.exe photo.jpg".into(),
        );
    };
    if args.next().is_some() {
        return Err("Matejko accepts one file path.".into());
    }
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
