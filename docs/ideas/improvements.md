# Code review: bugs and optimizations

Status: findings from a read-through of all of `src/` on 2026-11-07. Items are removed from this list as they are fixed; everything still here is open. The project was not built and the tests were not run for the review. Line numbers were last checked against the commit that makes the video frame copy cheaper.

## Bugs

### 1. Pressing E can be lost: the decoder thread misses its wake-up

[decode.rs:115](../../src/decode.rs#L115)

`pop()` checks `edit` while holding the `queue` lock and then waits on `queue`. `request_edit` only takes the `edit` lock before calling `notify_one`. If E lands between that check and `wait()`, the wake-up is lost and the baseline doesn't load until some other decode arrives. The window is small but real.

Fix: take the `queue` lock in `request_edit`, or keep both in one mutex.

### 2. A file that changes while it is being decoded keeps the old pixels

[app.rs:953](../../src/app.rs#L953), [app.rs:897](../../src/app.rs#L897)

`invalidate_stale_cache` only checks entries already in `cache`, not paths still decoding. When a file is still being written (copying from a camera, or another app saving), this happens:

- the decode starts on the partial file;
- the folder event fires and finds nothing cached to invalidate;
- `sync_images` stores the new stamp;
- the old decode then lands in the cache with a stamp that looks current.

The result is a lasting "Couldn't decode" or a truncated picture.

Fix: have the decoder read the stamp before decoding and return it with the result, or re-queue paths that are mid-decode when their stamp changes.

### 3. "Of" / "Sf" draw the frame in the wrong place when keystone or lens correction is set

[app.rs:2634](../../src/app.rs#L2634)

`copy_geometry` copies the turn, mirrors, rotation, aspect and frame, but not `vertical`, `horizontal` or `distortion`. The frame is stored as fractions of the usable rectangle, which depends on those three. The spec ("this rotation and frame") was written before keystone existed. Copying those three fields is almost certainly what's intended.

### 4. A brief folder read error closes the app

[catalog.rs:94](../../src/catalog.rs#L94) → [app.rs:928](../../src/app.rs#L928)

If `scan_dir` fails (network share hiccup, folder briefly locked), the list is cleared, `Refresh::Empty` comes back, and the viewer exits. Leaving edit mode always rescans, so a failure at that moment closes the app too.

Fix: a failed scan should keep the current list.

### 5. A failed edit load shows its error on the wrong file

[app.rs:2392](../../src/app.rs#L2392)

The `BaselineReady` error branch doesn't check `path` against the current file, unlike the success branch. If you navigate away first, the error appears on the next file.

### 6. Minor bugs

- [decode.rs:201](../../src/decode.rs#L201): `ImageReader::open` guesses the format from the extension, so a PNG named `.jpg` fails. `.with_guessed_format()` fixes this; edit mode already uses it.
- The vertex buffer holds 8192 vertices and silently cuts off anything past that ([gpu.rs:523](../../src/gpu.rs#L523)). The edit panel is already around 5–6k (the histogram alone is 4,608), so a few more overlay quads would start dropping.

## Optimizations, roughly by payoff

### 1. Display photos with mipmaps

[gpu.rs:383](../../src/gpu.rs#L383)

The texture has one mip level and the sampler is plain linear. A 24 MP photo fit into a 1080p window is minified 4–6×, which gives visible aliasing and moiré.

Fix: generate a mip chain, either on the decode thread or with GPU blit passes, and set `mipmap_filter: Linear`.

### 2. Reduce edit pipeline memory

For a 24 MP photo the baseline in f32 is about 288 MB.

- `orient` clones it even when nothing is turned ([pipeline.rs:392](../../src/edit/pipeline.rs#L392)).
- `clarity` clones the image again for its output ([pipeline.rs:1025](../../src/edit/pipeline.rs#L1025)).

Peak use during a full render goes past 1 GB, and several GB for 50–60 MP files.

Fix: borrow the baseline instead of copying it (`Cow`), let `clarity` take `Image` by value and change it in place, and merge the pixel-by-pixel stages (white balance, exposure, blackpoint) into one pass.

### 3. Cache `Warp::usable()`

[pipeline.rs:1364](../../src/edit/pipeline.rs#L1364)

With keystone or distortion set, one call is 40 search steps × 32 bisections × up to 64 border tests, each with a Newton solve. A frame drag calls it up to three times per mouse move (`FrameSpace::new`, `clamp_frame_to_photo`, `fit_aspect`), and a rotation or keystone slider drag calls it again in `refit_frame`. All on the UI thread. Not measured, but it is the likely cause of any lag while dragging.

Fix: cache it on the warp's inputs.

### 4. Small ones

- `edit_panel()` lays out the whole panel 3–4 times per redraw and per mouse move.
- `load_session` reads the whole photo even when an adj file or baseline means it isn't used ([files.rs:243](../../src/edit/files.rs#L243)).
- Catalog sorting allocates two `Vec<u16>` per comparison. Precompute the wide names once.
