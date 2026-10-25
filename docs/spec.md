# Matejko specification

Status: accepted. The behavior below is what the first version implements.

Matejko is a Windows photo viewer. It stays small: one window, no menus, no thumbnails, no editing. The user starts it with a path to an image. The app shows that image and can move to the other JPEG and PNG files in the same folder.

## Goals

- Start quickly and stay responsive while browsing typical camera photos.
- Keep the interface to a window, the picture, and a delete confirmation.
- Drive navigation, fullscreen, and delete from the keyboard. Use the mouse only to zoom and pan.
- Ship as one executable with no installer.

## Non-goals for the first version

- Formats other than JPEG and PNG (no GIF, WebP, HEIC, RAW, or animated PNG).
- Color profiles. Pixels are treated as sRGB.
- Thumbnails, slideshows, editing, rename, copy, print, or a folder tree.
- Watching the folder for changes made by other programs.
- Remembering window size, zoom, or the last opened file.
- Registering as the default Windows photo app.
- A settings file.

## Confirmed behavior

### Launch

The process takes the image path as its parameter. It opens that image and builds the browsing list from the directory that contains it (the image's parent directory, not the directory above that). The scan is not recursive.

### Browsing list

The list includes JPEG and PNG files in that directory. Left and Right move to the previous and next file in alphabetical order.

### Window

Enter toggles between a normal window and fullscreen.

### Delete

Delete starts removal of the current image and asks for confirmation before anything is removed.

### Mouse

The wheel zooms in and out, keeping the image point under the pointer fixed. Dragging with the mouse pans the image.

## Decisions

### Files

- Extensions are `.jpg`, `.jpeg`, and `.png`, compared case-insensitively.
- The path may be relative to the working directory. The app canonicalizes it when the filesystem allows, then lists that directory.
- Exactly one argument is required, and it must be a JPEG or PNG file. Zero arguments, extra arguments, a missing file, a directory, or a different extension is a fatal error: a message box and exit code 1.
- Files that fail to decode stay in the list. The window shows the file name and the error, and Left and Right still work.
- The list is built once at startup. Deleting a file removes it from the list. External additions are ignored.

### Sort

Alphabetical means the file name only, compared with case-insensitive ordinal order. `img10.jpg` comes before `img2.jpg`. Natural order (where `img2` comes first) is not the default.

### What the window shows

- The app starts windowed, centered, using about 80% of the work area on the monitor that contains the window.
- The background is black. PNG transparency is composited onto black.
- The title bar shows `Matejko — name.jpg (3 / 40)`. There is no other overlay during normal viewing.
- The window is resizable and has the usual minimize, maximize, and close controls. Close and Alt+F4 quit immediately, including during delete confirmation.
- Fullscreen is borderless on the monitor that contains the window. The title bar is not visible there, so the file name is hidden until the user leaves fullscreen.

### Fitting, zoom, and pan

Zoom is a scale against physical pixels. Scale `1.0` means one image pixel covers one physical pixel.

- On open, on resize, on fullscreen toggle, and on every image change, the image is fitted inside the window (the whole picture is visible, aspect ratio preserved, centered).
- The first zoom-in or drag leaves fit mode. Zoom-out while the image already fits the window does nothing. Further resize and fullscreen changes keep the current scale and the image point at the center of the window.
- Each wheel detent multiplies or divides the scale by `1.1`. The point under the cursor stays under the cursor:

  ```
  image_point = (cursor - pan) / scale
  scale = clamp(scale * factor)
  pan = cursor - image_point * scale
  ```

- Zoom-in stops at 32 physical pixels per image pixel. Zoom-out stops at the fit scale. Past that, the picture would be smaller than the window on both axes, so the wheel returns it to the centered fit instead. Fit may still scale a very small image past 32 so the whole picture stays visible; the wheel cannot zoom out from there.
- The left button pans. Movement matches the cursor 1:1 in screen pixels. There is no inertia and no edge clamp; the picture may leave the viewport.
- The right button does nothing. Double-click does nothing.

### Keyboard

| Input | Viewing | Delete confirmation |
| --- | --- | --- |
| Left / Right | Previous / next image. No wrap. Past either end, the key does nothing. | Ignored |
| Enter | Toggle fullscreen | Confirm delete |
| Delete | Open confirmation | Ignored |
| Escape | Leave fullscreen if fullscreen, otherwise quit | Cancel |
| Close, Alt+F4 | Quit | Quit |

Mouse wheel and drag keep working during delete confirmation, so the user can still see the picture they are about to remove.

### Delete flow

1. Delete dims the picture and draws centered text: `Delete "name.jpg"?` and `Enter to delete, Esc to cancel`.
2. Enter moves the file to the Recycle Bin and removes it from the list. It does not toggle fullscreen.
3. The next image (the one that shifts into the current index) is shown, fitted. If the deleted file was the last in the list, the new last image is shown. If the folder has no images left, the app exits.
4. Escape returns to viewing with no change. A failed Recycle Bin operation shows the error in the same overlay and leaves the file in place; Escape dismisses the error.

### Orientation and decode

- JPEG EXIF orientation is applied when the file is decoded, so phone photos are upright.
- Decode runs off the UI thread. Until the new image is ready, the previous picture stays on screen. If the user moves on before a decode finishes, that result is discarded.
- The decoded image is RGBA8. Zoom and pan only change the draw transform; they do not decode again.
- The next and previous images may be decoded ahead of time.

### Known limitations

- Very large images are fully decoded. A 24 megapixel photo is on the order of 100 MB of RGBA. There is no tiled viewer and no memory cap in the first version.
- ICC profiles, GPS, and other metadata are ignored beyond EXIF orientation.
- Preload does not cross a gap created by a file that is slow to decode; the visible file is always requested first.

## Technical approach

Windows only, Rust, one binary. The `windows` subsystem is used so opening the app from Explorer does not flash a console. Fatal errors use a system message box.

Crates:

- `winit` for the window, keyboard, mouse, and fullscreen.
- `wgpu` to draw one textured quad. This is what keeps wheel zoom and dragging on the GPU.
- `image` with only the JPEG and PNG features.
- `glyphon` for the delete confirmation and decode errors, so file names keep their Unicode characters.
- `trash` to send files to the Recycle Bin.

`wgpu` makes the executable several megabytes. That is accepted in exchange for smooth zoom. A software blit would be smaller and slower to pan on large photos.

### Modules

- `main` — arguments, fatal errors, process exit.
- `catalog` — directory listing, sort, index after delete.
- `view` — fit, zoom-at-point, pan, and whether the view is still in fit mode.
- `decode` — background JPEG/PNG decode and EXIF orientation.
- `gpu` — device, swapchain, texture upload, draw.
- `app` — event loop, input, confirmation state.

`catalog` and `view` stay free of `winit` and `wgpu` so the sort, neighbor, delete-index, and zoom math can be unit tested.

### Layout

```
Cargo.toml
src/main.rs
src/catalog.rs
src/view.rs
src/decode.rs
src/gpu.rs
src/app.rs
```

## Acceptance checks

- `matejko photo.jpg` shows that file, fitted, in a window whose title includes the file name and index.
- Left and Right move in case-insensitive alphabetical order and stop at the ends.
- Files that are not `.jpg`, `.jpeg`, or `.png` are absent from the sequence. A JPEG in a subfolder is absent.
- Enter toggles borderless fullscreen on the current monitor. Escape leaves fullscreen and does not quit. Escape in the window quits.
- The wheel keeps the image point under the cursor fixed, including when the pointer is near a corner and when the image is letterboxed. Zoom-out stops once the image fits the window.
- Dragging pans without changing scale. Resizing while still fitted refits. Resizing after a zoom keeps the scale.
- Delete then Escape does not change the file. Delete then Enter moves it to the Recycle Bin, shows the next image, and skips the removed file. Deleting the last remaining image exits.
- A phone JPEG with orientation 6 is shown rotated upright.
- A missing path shows a message box and exits with code 1, without a console window.
