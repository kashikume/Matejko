# Matejko

A small photo viewer for Windows. It opens one image or MP4, reads the other JPEG, PNG, and MP4 files in the same folder, and moves through them from the keyboard. A video plays in the same window, fitted to the frame, with the same zoom and pan as a photo. The folder is watched so external adds, deletes, and edits update the list or reload the current file. The app icon is a baffled painter inspecting a Polaroid.

## Run

```
cargo run --release -- path\to\photo.jpg
```

The window title shows the file name and its place in the folder.

- Left / Right — previous or next file
- Home / End — first or last file
- Page Up / Page Down — jump by 10 files
- Enter or F11 — switch between a window and fullscreen
- Delete, then Enter — move the current file to the Recycle Bin
- Space — pause or play a video
- Escape — cancel a delete, leave fullscreen, or quit
- F1 — show these keys
- I — show file information
- O — Open With another app
- M — show the photo's GPS location on Google Maps, when the EXIF has one
- Mouse wheel — zoom, centered on the pointer
- Drag — pan
- On a video, the bar along the bottom seeks. It hides while the pointer is idle and stays while paused

Started without a file, Matejko shows a picture explaining how to open a photo with Open with.

Behavior is specified in [docs/spec.md](docs/spec.md).

## Release build

```
powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1
```

This builds `matejko.exe` for x64 and arm64 and packs both into `target\dist\Matejko-<version>.msi`, which installs only the exe that matches the machine. The installer offers a per-user install in `%LocalAppData%\Programs\Matejko` with no admin rights, or a per-machine install in `C:\Program Files\Matejko`. It registers Matejko for `.jpg`, `.jpeg`, `.png`, and `.mp4`, so it appears under Open with and in Settings → Default apps without becoming the default. The loose exes are copied to `target\dist` too. It needs the MSVC x64 and arm64 build tools and [WiX Toolset 3.14](https://github.com/wixtoolset/wix3/releases).

## License

MIT. See [LICENSE](LICENSE).
