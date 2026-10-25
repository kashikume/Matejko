# Matejko

A small photo viewer for Windows. It opens one image, reads the other JPEG and PNG files in the same folder, and moves through them from the keyboard. The app icon is a baffled painter inspecting a Polaroid.

## Run

```
cargo run --release -- path\to\photo.jpg
```

The window title shows the file name and its place in the folder.

- Left / Right — previous or next image
- Enter — switch between a window and fullscreen
- Delete, then Enter — move the current image to the Recycle Bin
- Escape — cancel a delete, leave fullscreen, or quit
- F1 — show these keys
- I — show file and EXIF information
- Mouse wheel — zoom, centered on the pointer
- Drag — pan

Behavior is specified in [docs/spec.md](docs/spec.md).

## License

MIT. See [LICENSE](LICENSE).
