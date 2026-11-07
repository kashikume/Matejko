# Code review: bugs and optimizations

Status: findings from read-throughs of all of `src/` on 2026-11-07. Items are removed from this list as they are fixed; everything still here is open. The project was not built and the tests were not run for the review. Line numbers were last checked against the commit that ignores a failed edit load of a file no longer open.

## Bugs

### 1. A second E while the baseline loads replaces the session

[app.rs:1061](../../src/app.rs#L1061)

The mode stays Viewing until the baseline arrives, so a second E on a large JPEG queues a second load. `accept_baseline` doesn't check for an open session. When the second result arrives, it replaces the session and its worker, and any slider moves made in between are lost.

Fix: return early when `self.editing.is_some()`.

### 2. Pressing E can be lost: the decoder thread misses its wake-up

[decode.rs:132](../../src/decode.rs#L132)

`pop()` checks `edit` while holding the `queue` lock and then waits on `queue`. `request_edit` only takes the `edit` lock before calling `notify_one`. If E lands between that check and `wait()`, the wake-up is lost and the baseline doesn't load until some other decode arrives. The window is small but real.

Fix: take the `queue` lock in `request_edit`, or keep both in one mutex.

### 3. Losing focus mid-drag leaves the drag running

[app.rs:2476](../../src/app.rs#L2476)

`Focused(false)` clears `dragging` but not `editing.pointer`, `scrubbing`, or `seeking`. After Alt+Tab during a slider drag, the slider keeps following the pointer with no button held, and the full-size render never runs. A seek on the video bar sticks the same way.

Fix: call `edit_release()` and clear `seeking` there.

### 4. "Of" / "Sf" draw the frame in the wrong place when keystone or lens correction is set

[app.rs:2660](../../src/app.rs#L2660)

`copy_geometry` copies the turn, mirrors, rotation, aspect and frame, but not `vertical`, `horizontal` or `distortion`. The frame is stored as fractions of the usable rectangle, which depends on those three. The spec ("this rotation and frame") was written before keystone existed. Copying those three fields is almost certainly what's intended.

### 5. A brief folder read error closes the app

[catalog.rs:94](../../src/catalog.rs#L94) → [app.rs:927](../../src/app.rs#L927)

If `scan_dir` fails (network share hiccup, folder briefly locked), the list is cleared, `Refresh::Empty` comes back, and the viewer exits. Leaving edit mode always rescans, so a failure at that moment closes the app too.

Fix: a failed scan should keep the current list.

### 6. Minor bugs

- In edit mode, a window resize refits the picture to the whole window instead of `picture_window()`, so a fitted picture slides under the panel ([app.rs:2241](../../src/app.rs#L2241)). Dragging the divider doesn't refit at all ([app.rs:1525](../../src/app.rs#L1525)). The next render puts it right.
- [decode.rs:223](../../src/decode.rs#L223): `ImageReader::open` guesses the format from the extension, so a PNG named `.jpg` fails. `.with_guessed_format()` fixes this; edit mode already uses it.
- The vertex buffer holds 8192 vertices and silently cuts off anything past that ([gpu.rs:523](../../src/gpu.rs#L523)). The edit panel is already around 5–6k (the histogram alone is 4,608), so a few more overlay quads would start dropping.

## Optimizations, roughly by payoff

### 1. Upload textures without the padded copy

[gpu.rs:1069](../../src/gpu.rs#L1069)

`upload_rgba` pads each row to 256 bytes whenever the width isn't a multiple of 64 px, which needs a whole second copy of the image on the UI thread. `Queue::write_texture` doesn't need that alignment; only buffer-to-texture copies do. The copy is about 96 MB for a 6000 px wide photo and about 8 MB per frame for a portrait 1080×1920 video.

Fix: always pass `bytes_per_row: Some(width * 4)`.

### 2. Display photos with mipmaps

[gpu.rs:383](../../src/gpu.rs#L383)

The texture has one mip level and the sampler is plain linear. A 24 MP photo fit into a 1080p window is minified 4–6×, which gives visible aliasing and moiré.

Fix: generate a mip chain, either on the decode thread or with GPU blit passes, and set `mipmap_filter: Linear`.

### 3. Reduce edit pipeline memory and passes

[pipeline.rs:316](../../src/edit/pipeline.rs#L316)

For a 24 MP photo the baseline in f32 is about 288 MB.

- `orient` clones it even when nothing is turned ([pipeline.rs:392](../../src/edit/pipeline.rs#L392)).
- `clarity` clones the image again for its output ([pipeline.rs:1025](../../src/edit/pipeline.rs#L1025)).
- White balance, exposure, blackpoint, encode, brightness, contrast, vibrance, saturation, and decode each run as a separate full pass, so a full render is limited by memory bandwidth.

Peak use during a full render goes past 1 GB, and several GB for 50–60 MP files.

Fix: borrow the baseline instead of copying it (`Cow`), let `clarity` take `Image` by value and change it in place, and merge the pixel-by-pixel stages into one pass before and one after the shadow/highlight step.

### 4. Cache `Warp::usable()`

[pipeline.rs:1364](../../src/edit/pipeline.rs#L1364)

With keystone or distortion set, one call is 40 search steps × 32 bisections × up to 64 border tests, each with a Newton solve. A frame drag calls it up to three times per mouse move (`FrameSpace::new`, `clamp_frame_to_photo`, `fit_aspect`), and a rotation or keystone slider drag calls it again in `refit_frame`. All on the UI thread. Not measured, but it is the likely cause of any lag while dragging.

Fix: cache it on the warp's inputs.

### 5. Encode to 8-bit without `powf`

[image.rs:126](../../src/edit/image.rs#L126)

`to_rgba8` calls `encode` (a `powf`) three times per pixel on every render, about 72 million calls for 24 MP.

Fix: a table over linear values (for example 4096 entries with interpolation), which leaves room for the dither noise. `code_of` already does this for the histogram.

### 6. Don't decode the JPEG a second time on E

[decode.rs:214](../../src/decode.rs#L214), [files.rs:431](../../src/edit/files.rs#L431)

When there is no `.matejko` baseline yet, the baseline is the open file, which the viewer has already decoded to RGBA. Handing that to `Image::from_srgb_rgba` saves a full decode, several hundred milliseconds for a large photo.

### 7. Small ones

- `edit_panel()` lays out the whole panel 3–4 times per redraw and per mouse move, allocating every label string each time.
- `LabelShape` copies each label's text every frame before comparing it ([gpu.rs:765](../../src/gpu.rs#L765)). Compare first, copy only on a change.
- Catalog sorting allocates two `Vec<u16>` per comparison. Precompute the wide names once.
- Each video opens a new D3D11 device ([playback.rs:98](../../src/playback.rs#L98)). One device could be kept and reused.
