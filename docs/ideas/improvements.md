# Code review: bugs and optimizations

Status: findings from a read-through of all of `src/` on 2026-11-07. Nothing here is fixed yet. The project was not built and the tests were not run for this review. Line numbers refer to the code as it was then.

## Bugs

### 1. Pressing E can be lost: the decoder thread misses its wake-up

[decode.rs:243](../../src/decode.rs#L243)

`pop()` checks `edit` while holding the `queue` lock and then waits on `queue`. `request_edit` only takes the `edit` lock before calling `notify_one`. If E lands between that check and `wait()`, the wake-up is lost and the baseline doesn't load until some other decode arrives. The window is small but real.

Fix: take the `queue` lock in `request_edit`, or keep both in one mutex.

### 2. A file that changes while it is being decoded keeps the old pixels

[app.rs:904](../../src/app.rs#L904), [app.rs:846](../../src/app.rs#L846)

`invalidate_stale_cache` only checks entries already in `cache`, not paths still decoding. When a file is still being written (copying from a camera, or another app saving), this happens:

- the decode starts on the partial file;
- the folder event fires and finds nothing cached to invalidate;
- `sync_images` stores the new stamp;
- the old decode then lands in the cache with a stamp that looks current.

The result is a lasting "Couldn't decode" or a truncated picture.

Fix: have the decoder read the stamp before decoding and return it with the result, or re-queue paths that are mid-decode when their stamp changes.

### 3. Saved JPEGs keep the old embedded thumbnail

[files.rs:497](../../src/edit/files.rs#L497)

`patch_exif` copies the baseline's APP1 block as it is, including the IFD1 thumbnail. Explorer and many viewers show that thumbnail, so they show the unedited, uncropped, unrotated photo.

Fix: drop IFD1 (set IFD0's next-IFD offset to 0) or write a new thumbnail.

### 4. A long EXIF block can corrupt the saved file

[files.rs:701](../../src/edit/files.rs#L701)

`(app1.len() - 2) as u16` wraps silently. Cameras with large maker notes have APP1 blocks close to 64 KB, and appending the Software text pushes them over, which makes the JPEG invalid.

Fix: skip the Software change when it doesn't fit, or drop the thumbnail first to make room. Similarly, `image.width as u16` in [files.rs:394](../../src/edit/files.rs#L394) should use `u16::try_from` and return an error.

### 5. "Of" / "Sf" draw the frame in the wrong place when keystone or lens correction is set

[app.rs:2550](../../src/app.rs#L2550)

`copy_geometry` copies the turn, mirrors, rotation, aspect and frame, but not `vertical`, `horizontal` or `distortion`. The frame is stored as fractions of the usable rectangle, which depends on those three. The spec ("this rotation and frame") was written before keystone existed. Copying those three fields is almost certainly what's intended.

### 6. A brief folder read error closes the app

[catalog.rs:94](../../src/catalog.rs#L94) → [app.rs:879](../../src/app.rs#L879)

If `scan_dir` fails (network share hiccup, folder briefly locked), the list is cleared, `Refresh::Empty` comes back, and the viewer exits.

Fix: a failed scan should keep the current list.

### 7. A failed edit load shows its error on the wrong file

[app.rs:2311](../../src/app.rs#L2311)

The `BaselineReady` error branch doesn't check `path` against the current file, unlike the success branch. If you navigate away first, the error appears on the next file.

### 8. Minor bugs

- [decode.rs:324](../../src/decode.rs#L324): `ImageReader::open` guesses the format from the extension, so a PNG named `.jpg` fails. `.with_guessed_format()` fixes this; edit mode already uses it.
- The vertex buffer holds 8192 vertices and silently cuts off anything past that ([gpu.rs:502](../../src/gpu.rs#L502)). The edit panel is already around 5–6k (the histogram alone is 4,608), so a few more overlay quads would start dropping.

## Optimizations, roughly by payoff

### 1. Remove stale decodes from the queue

[decode.rs:224](../../src/decode.rs#L224)

Nothing is ever removed from the queue. Holding → through 100 photos queues about 100 neighbour decodes (100–300 ms each for 24 MP). The CPU stays busy for seconds afterwards, and the real neighbour prefetch sits behind all of them.

Fix: in `sync_images`, drop queued paths that are no longer current or neighbours.

### 2. Display photos with mipmaps

[gpu.rs:369](../../src/gpu.rs#L369)

The texture has one mip level and the sampler is plain linear. A 24 MP photo fit into a 1080p window is minified 4–6×, which gives visible aliasing and moiré.

Fix: generate a mip chain, either on the decode thread or with GPU blit passes, and set `mipmap_filter: Linear`.

### 3. Use lookup tables instead of `powf` in edit-mode colour conversion

[image.rs:219](../../src/edit/image.rs#L219)

- `from_srgb_rgba` runs `powf` three times per pixel on one thread: about 72M calls for 24 MP, and the main cost of opening edit mode. A 256-entry table, run in parallel, makes it nearly free.
- `histogram_region` ([image.rs:333](../../src/edit/image.rs#L333)) calls `quantize` (another `powf`) per channel over the whole full-size render. Build the histogram from the RGBA bytes already produced, or from the drag-size image.

### 4. Reduce edit pipeline memory

For a 24 MP photo the baseline in f32 is about 288 MB.

- `orient` clones it even when nothing is turned ([pipeline.rs:392](../../src/edit/pipeline.rs#L392)).
- `clarity` clones the image again for its output ([pipeline.rs:1025](../../src/edit/pipeline.rs#L1025)).

Peak use during a full render goes past 1 GB, and several GB for 50–60 MP files.

Fix: borrow the baseline instead of copying it (`Cow`), let `clarity` take `Image` by value and change it in place, and merge the pixel-by-pixel stages (white balance, exposure, blackpoint) into one pass.

### 5. Cache `Warp::usable()`

[pipeline.rs:1364](../../src/edit/pipeline.rs#L1364)

With keystone or distortion set, one call is 40 search steps × 32 bisections × up to 64 border tests, each with a Newton solve. A frame drag calls it up to three times per mouse move (`FrameSpace::new`, `clamp_frame_to_photo`, `fit_aspect`), and a rotation or keystone slider drag calls it again in `refit_frame`. All on the UI thread. Not measured, but it is the likely cause of any lag while dragging.

Fix: cache it on the warp's inputs.

### 6. Make the video frame copy cheaper

[playback.rs:492](../../src/playback.rs#L492)

Each frame does a synchronous `Map` right after `CopyResource` (a GPU stall), allocates a new frame-sized buffer (about 33 MB for 4K), and swaps BGRA to RGBA byte by byte.

Fix: use a `Bgra8UnormSrgb` wgpu texture for video so rows are a plain memcpy, reuse the buffer, and double-buffer the staging texture.

### 7. Build the Info and Help text once

[app.rs:1877](../../src/app.rs#L1877)

`format_image_info` reads and parses EXIF twice per call (`read_exif`, then `gps_coordinates`), and `help_text` parses it once. Both run on every redraw, including every pan step and every frame of the loading wheel.

Fix: build the text when the overlay opens.

### 8. Small ones

- `edit_panel()` lays out the whole panel 3–4 times per redraw and per mouse move.
- `load_session` reads the whole photo even when an adj file or baseline means it isn't used ([files.rs:236](../../src/edit/files.rs#L236)).
- Catalog sorting allocates two `Vec<u16>` per comparison. Precompute the wide names once.
