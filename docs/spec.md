# Matejko specification

Status: accepted. The behavior below is what the first version implements.

Matejko is a Windows photo viewer. It stays small: one window, no menus, no thumbnails, no editing. The user starts it with a path to an image or an MP4. The app shows that file and can move to the other JPEG, PNG, and MP4 files in the same folder.

## Goals

- Start quickly and stay responsive while browsing typical camera photos.
- Keep the interface to a window, the picture, and a delete confirmation.
- Drive navigation, fullscreen, and delete from the keyboard. Use the mouse to zoom and pan, and, on a video, to seek.
- Ship as one executable with no installer.

## Non-goals for the first version

- Formats other than JPEG, PNG, and MP4 (no GIF, WebP, HEIC, RAW, or animated PNG).
- Color profiles. Pixels are treated as sRGB.
- Thumbnails, slideshows, editing, rename, copy, print, or a folder tree.
- Remembering window size, zoom, or the last opened file.
- Registering as the default Windows photo app.
- A settings file.

## Confirmed behavior

### Launch

The process takes the file path as its parameter. It opens that file and builds the browsing list from the directory that contains it (the file's parent directory, not the directory above that). The scan is not recursive.

### Browsing list

The list includes JPEG, PNG, and MP4 files in that directory. Left and Right move to the previous and next file in alphabetical order. Home and End jump to the ends. Page Up and Page Down jump by 10 files. Those keys never seek inside a video.

### Folder watching

The app watches the image's parent directory (not recursive) for changes made outside Matejko:

- When a supported file appears or disappears, the browse list is rebuilt. The viewer stays on the same path when that file still exists.
- When the current file's contents change (size or modified time), it is decoded again and shown fitted.
- When the current file is removed or renamed away, navigation matches an in-app delete: show the next image, or the previous if it was last, or exit if none remain.
- Folder events are coalesced for about 100 ms so a single save does not thrash. If the watcher cannot start, the app still runs without live updates.

### Window

Enter toggles between a normal window and fullscreen.

### Delete

Delete starts removal of the current image and asks for confirmation before anything is removed.

### Mouse

The wheel zooms in and out, keeping the image point under the pointer fixed. Dragging with the mouse pans the image.

## Decisions

### Files

- Extensions are `.jpg`, `.jpeg`, `.png`, and `.mp4`, compared case-insensitively.
- The path may be relative to the working directory. The app canonicalizes it when the filesystem allows, then lists that directory.
- Exactly one argument is required, and it must be a JPEG, PNG, or MP4 file. Zero arguments, extra arguments, a missing file, a directory, or a different extension is a fatal error: a message box and exit code 1.
- Files that fail to decode stay in the list. The window shows the file name and the error, and Left and Right still work.
- The list is built once at startup. Deleting a file removes it from the list. External additions are ignored.

### Sort

Alphabetical means the file name only, compared with case-insensitive ordinal order. `img10.jpg` comes before `img2.jpg`. Natural order (where `img2` comes first) is not the default.

### What the window shows

- The app starts windowed, centered, using about 80% of the work area on the monitor that contains the window. The window and the `.exe` use the Matejko painter icon.
- The background is black. PNG transparency is composited onto black.
- The title bar shows `Matejko — name.jpg (3 / 40) — F1 for help`. During normal viewing of a photo there is no other overlay. A video may show a bar along the bottom.
- The window is resizable and has the usual minimize, maximize, and close controls. Close and Alt+F4 quit immediately, including during help or delete confirmation.
- Fullscreen is borderless on the monitor that contains the window. The title bar is not visible there, so the file name is hidden until the user leaves fullscreen.
- In fullscreen the pointer hides as soon as fullscreen starts. Moving it, turning the wheel, or pressing a button shows it for 3 seconds after the last use, then it hides again. A pointer message sent because the window changed size under a still pointer does not count as a move. Leaving fullscreen shows it and leaves it visible.

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

| Input | Viewing | Help / Info | Delete confirmation |
| --- | --- | --- | --- |
| Left / Right | Previous / next image. No wrap. Past either end, the key does nothing. | Ignored | Ignored |
| Home / End | First / last image. Already there, the key does nothing. | Ignored | Ignored |
| Page Up / Page Down | Jump back / forward by 10 images, stopping at the first or last. | Ignored | Ignored |
| Enter | Toggle fullscreen | Ignored | Confirm delete |
| Delete | Open confirmation | Ignored | Ignored |
| I | Open image information | Close info if info is open; otherwise ignored | Ignored |
| O | Open With dialog for the current file | Ignored | Ignored |
| F1 | Open help | Close help if help is open; otherwise ignored | Ignored |
| Space | Pause or play the current video. Ignored on a photo. | Ignored | Ignored |
| Escape | Leave fullscreen if fullscreen, otherwise quit | Close overlay | Cancel |
| Close, Alt+F4 | Quit | Quit | Quit |

Mouse wheel and drag keep working during help, info, and delete confirmation, so the picture stays visible behind the text.

### Help

F1 dims the picture and draws the key list in the center, in the same way as the delete question. Escape or F1 closes it and does not quit.

### Information

I dims the picture and draws a summary for the current file: name, folder, file size, modified and created times, format, and pixel size. A photo also lists available EXIF (camera, lens, taken time, exposure, focal length, flash, orientation, software, artist, copyright, GPS when present). A video lists duration instead of EXIF. Escape or I closes it and does not quit. I is ignored during delete confirmation and while help is open.

### Delete flow

1. Delete dims the picture and draws centered text: `Delete "name.jpg"?` and `Enter to delete, Esc to cancel`.
2. Enter moves the file to the Recycle Bin and removes it from the list. It does not toggle fullscreen.
3. The next image (the one that shifts into the current index) is shown, fitted. If the deleted file was the last in the list, the new last image is shown. If the folder has no images left, the app exits.
4. Escape returns to viewing with no change. A failed Recycle Bin operation shows the error in the same overlay and leaves the file in place; Escape dismisses the error.

### Orientation and decode

- JPEG EXIF orientation is applied when the file is decoded, so phone photos are upright.
- Decode runs off the UI thread. Until the new image is ready, the previous picture stays on screen. If the user moves on before a decode finishes, that result is discarded.
- The decoded image is RGBA8. Zoom and pan only change the draw transform; they do not decode again.
- The next and previous images may be decoded ahead of time. MP4s are not decoded ahead.

### Video

An MP4 starts playing from the beginning as soon as it is the current file, with sound. The presented frame is drawn as the picture, so fit, zoom, and pan are the same as for a photo. The first frame is fitted. Later frames keep the current scale, including while paused and after a seek. Resizing or toggling fullscreen refits only while the view is still fitted.

Left, Right, Home, End, and Page Up/Down leave the video and open the neighboring file. Sound stops immediately. Coming back starts at the beginning.

Space pauses and resumes. At the end, the last frame stays and Space starts from the beginning. Help, info, and delete confirmation pause the video and resume it afterward only if it was playing.

A bar sits along the bottom. It shows a pause/play target, elapsed time and duration, and a track. Click or drag on the track seeks. The wheel still zooms. Dragging the picture still pans, and a click on the picture does not pause. The bar appears when the pointer is used and hides after it has been idle for 3 seconds, including in a normal window. It stays up while playback is paused or the video has ended, and it hides while help, info, or delete confirmation is open. There is no volume control.

A file that will not play stays in the list. The window shows the file name and the error, and the navigation keys still work. Playback uses the decoders installed with Windows, so a typical MP4 is H.264 video with AAC audio. HEVC or AV1 plays only when that PC already has the decoder.

### Known limitations

- Very large images are fully decoded. A 24 megapixel photo is on the order of 100 MB of RGBA. There is no tiled viewer and no memory cap in the first version.
- ICC profiles are ignored. GPS and other EXIF tags appear only in the information overlay; they are not used for viewing.
- Preload does not cross a gap created by a file that is slow to decode; the visible file is always requested first.
- Each video frame is copied into the picture texture. A 1080p frame is cheap. A 4K frame is a large copy. There is no zero-copy path into the GPU.

## Technical approach

Windows only, Rust, one binary. The `windows` subsystem is used so opening the app from Explorer does not flash a console. Fatal errors use a system message box.

Crates:

- `winit` for the window, keyboard, mouse, and fullscreen. The crate is patched past 0.30.13 so dragging onto a monitor with a different DPI (often the screen above or below) does not grow the window in a loop.
- `wgpu` to draw one textured quad. This is what keeps wheel zoom and dragging on the GPU.
- `image` with only the JPEG and PNG features.
- `kamadak-exif` for the information overlay.
- `notify` to watch the folder for external adds, removes, and writes.
- `glyphon` for the delete confirmation, help, info, and decode errors, so file names keep their Unicode characters.
- `trash` to send files to the Recycle Bin.
- `windows` for MP4 playback. Windows MediaPlayer decodes, plays audio, and reports pause, seek, and duration. Matejko copies each frame into the existing texture.

`wgpu` makes the executable several megabytes. That is accepted in exchange for smooth zoom. A software blit would be smaller and slower to pan on large photos.

### Modules

- `main` — arguments, fatal errors, process exit.
- `catalog` — directory listing, sort, index after delete.
- `view` — fit, zoom-at-point, pan, and whether the view is still in fit mode.
- `decode` — background JPEG/PNG decode and EXIF orientation.
- `playback` — Windows MediaPlayer session, audio, and frame copy.
- `transport` — video bar layout and hit testing.
- `gpu` — device, swapchain, texture upload, draw.
- `info` — file and EXIF text for the information overlay.
- `watch` — directory watcher bridged into the event loop.
- `app` — event loop, input, confirmation state.

`catalog` and `view` stay free of `winit` and `wgpu` so the sort, neighbor, delete-index, and zoom math can be unit tested.

### Layout

```
Cargo.toml
src/main.rs
src/catalog.rs
src/view.rs
src/decode.rs
src/playback.rs
src/transport.rs
src/gpu.rs
src/app.rs
```

## Acceptance checks

- `matejko photo.jpg` shows that file, fitted, in a window whose title includes the file name and index.
- Left and Right move in case-insensitive alphabetical order and stop at the ends. Home and End go to the first and last file. Page Up and Page Down jump by 10 and stop at the ends.
- Adding or removing a JPEG, PNG, or MP4 in the folder updates the list. Editing the current file reloads it. Removing the current file outside the app behaves like Delete.
- Files that are not `.jpg`, `.jpeg`, `.png`, or `.mp4` are absent from the sequence. A JPEG in a subfolder is absent.
- An MP4 plays fitted, with sound, and keeps its aspect ratio when the window changes size. The wheel zooms the frame and drag pans it. Left and Right move to the neighboring file instead of seeking.
- Space pauses and resumes a video. The bottom bar seeks. It hides after the pointer is idle and stays while paused.
- Enter toggles borderless fullscreen on the current monitor. Escape leaves fullscreen and does not quit. Escape in the window quits.
- The wheel keeps the image point under the cursor fixed, including when the pointer is near a corner and when the image is letterboxed. Zoom-out stops once the image fits the window.
- Dragging pans without changing scale. Resizing while still fitted refits. Resizing after a zoom keeps the scale.
- Delete then Escape does not change the file. Delete then Enter moves it to the Recycle Bin, shows the next image, and skips the removed file. Deleting the last remaining image exits.
- A phone JPEG with orientation 6 is shown rotated upright.
- A missing path shows a message box and exits with code 1, without a console window.
