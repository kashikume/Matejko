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
- Enter — switch between a window and fullscreen
- Delete, then Enter — move the current file to the Recycle Bin
- Space — pause or play a video
- Escape — cancel a delete, leave fullscreen, or quit
- F1 — show these keys
- I — show file information
- O — Open With another app
- Mouse wheel — zoom, centered on the pointer
- Drag — pan
- On a video, the bar along the bottom seeks. It hides while the pointer is idle and stays while paused

Behavior is specified in [docs/spec.md](docs/spec.md).

## License

MIT. See [LICENSE](LICENSE).
