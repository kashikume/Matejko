# Edit mode

Status: draft. Not part of the viewer. Open points are at the end.

Edit mode is a second state of the same window, for a quick correction of a camera photo. The files in mind are JPEGs from phones. The app remains a viewer. E enters edit mode on a JPEG. That key does nothing today, and it does nothing on a PNG.

The picture stays the picture: black background, fit, zoom, and pan. A panel on the right holds a histogram, then the mirror and quarter-turn buttons, then the sliders in four groups: Light, Color, Detail, and Frame. JPEG quality is at the bottom. Save, Save as copy, and Exit are buttons on that panel. Four small buttons sit with them, in one row: O, Of, S, and Sf. The split starts with the picture at about three quarters of the width, and the divider drags. Rotation is the slider in Frame, from −45° to +45°. It turns the whole picture. A button beside it cycles the frame's shape: Original, Free, 3:2, 2:3, and the other ratios in the frame note. Each click fits the largest rectangle of that shape that still lies on the rotated photo. The kept rectangle is dragged on the picture. The drag snaps to the rotated photo's edges, so the empty corners left by the rotation never enter the frame. The margin outside the frame is dimmed. Guides can be drawn over the picture. They are not adjustments and they are not saved. `Edit - screen guides.md`. The layout is drawn in `Edit - wireframe.canvas.tsx`.

## What is edited

JPEG only: `.jpg` and `.jpeg`. The viewer still opens PNG. Edit mode does not.

The session decodes the **baseline** file, not the file the browser is showing, once a baseline exists. Adjustments are numbers. They are applied again from that baseline on every change. Nothing in the pipeline writes back into the baseline.

## Files

A folder the user is browsing, after `myimage.jpg` was opened and saved as a copy named `myimage_ver2.jpg`:

```
holiday/
  myimage.jpg
  myimage_ver2.jpg
  .matejko/
    myimage.jpg
    myimage_ver2.matejko_adj
```

`holiday/myimage.jpg` is the file that was opened. It is still the camera file. Save as copy does not replace it.

`holiday/myimage_ver2.jpg` is the rendered copy.

`.matejko/myimage.jpg` is a byte-for-byte copy of that camera file. It is the baseline. Later renders do not touch it, and a later version does not add a second one.

`.matejko/myimage_ver2.matejko_adj` is the adjustments for the copy, JSON. The adj name is the saved file's name with `.jpg` or `.jpeg` removed, then `.matejko_adj`. The file name of the baseline is stored inside the adj, so another version can point at the same `myimage.jpg`.

The viewer watches the image's parent and not its children, so the copies inside `.matejko` stay out of the browse list. Windows does not hide a folder whose name starts with a dot. The folder is visible in Explorer.

### When the files appear

Slider moves stay in the session. They update the preview and the histogram. They do not write a file.

Save and Save as copy are what write. Entering edit mode and leaving it, with nothing saved, writes nothing.

The session's original is the file name in the adj that was opened, when there is one. With no adj, the original is the name of the file that was opened. The baseline path is `.matejko/` plus that name.

Save writes over the file that is open. It does this in order:

1. If `.matejko` does not exist, create it.
2. If the baseline does not exist, and the open file is the original, copy that file's bytes to `.matejko/<original>`. This happens before the render replaces the open file. A later save leaves the baseline where it is.
3. Write `.matejko/<stem of the open file>.matejko_adj` from the controls as they are now. Its `original` field is the original file name.
4. Render the baseline plus that adj over the open file.

Save as copy asks for a file name in the same folder and writes a new JPEG there. The open file is left as it is. The same first two steps create the baseline when the open file is still the untouched original. The adj is `.matejko/<stem of the new name>.matejko_adj`, and its `original` field is that same original file name. The render goes to the new name. A second version adds another JPEG and another adj, and the baseline stays the one file. If the chosen name is already in the folder, the user confirms before that JPEG and its adj are replaced.

On a later visit, adj and baseline both present: decode `.matejko/<original>`, apply the adj. The browsed file is ignored for the edit session, even when it is a previous render of these same numbers. Applying the adj to the render would bake the edit twice.

Adj present, baseline missing: do not apply the adj to the browsed file, and do not create a baseline from that render. Editing needs the baseline.

No adj, baseline present under the open file's name: start from that baseline with every image control at default, and with `jpeg_quality` set from that baseline as below.

No adj and no baseline: the open file is the original. Decode it. The baseline is created on the first Save or Save as copy.

### Adj file

```json
{
  "version": 1,
  "original": "myimage.jpg",
  "adjustments": {
    "quarter_turns": 0,
    "mirror_horizontal": false,
    "mirror_vertical": false,
    "temperature": 0,
    "tint": 0,
    "exposure_ev": 0.0,
    "blackpoint": 0.0,
    "shadow": 0,
    "highlight": 0,
    "brightness": 0,
    "contrast": 0,
    "vibrance": 0,
    "saturation": 0,
    "clarity": 0,
    "rotation": 0.0,
    "aspect": "original",
    "frame": { "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0 },
    "jpeg_quality": 90
  }
}
```

`version` is 1. `original` is the baseline's file name, including its extension, as in the tree above. Unknown versions are refused rather than half-applied. Values are the slider units from the control notes (stops for exposure, degrees for rotation, fractions of the usable rectangle for the frame, −100…+100 for the others, except shadow and highlight, which use their own notes). `quarter_turns` is 0, 1, 2, or 3. The two mirror fields are booleans. `aspect` is `original`, `free`, or one of the ratios in the frame note. `jpeg_quality` is an integer from 1 to 100. The `90` in the sample is the number read from that file, not a fixed default.

One value per control, in the pipeline order below. The stack is that pipeline, run from the baseline. It is not a list the user reorders, and it is not one entry per slider twitch. JPEG quality is stored with them and is not a stage in that stack.

### Save

Save renders into the file that is open and writes that file's adj, in the order above. Save as copy renders into the new name and writes that name's adj. The baseline stays. The folder watcher will see the new JPEG or the write over the open file. During the edit session that event does not reload the session and does not switch which file is open. The session stays in edit mode, and the control values written by that action count as saved for the file it wrote.

JPEG output is 8-bit sRGB, written at `jpeg_quality`.

### Compare

S is the last save of the file that is open. That is the adj loaded at the start of the session, or the controls written by the latest Save, whichever is newer. A file that has never been saved has no last save, and S and Sf do nothing until Save. Save as copy writes another file. It does not change the last save of the file that is still open.

The picture shows the current controls unless one of those buttons is held. O is Original, Of is Original with this rotation and frame, S is the last save, and Sf is the last save with this rotation and frame. There is no Current button. Releasing the button shows the current controls again. Holding one does not change those controls. Save and Save as copy write the current controls. A picture that is only being looked at is not what gets saved.

| View | White balance, tone, and color | Quarter turns, mirrors, rotation, frame |
| --- | --- | --- |
| Current | Current | Current |
| Original | Defaults | Defaults. Rotation is 0 and the frame is the whole picture. |
| Last save | Last save | Last save |
| Original with this rotation and frame | Defaults | Current |
| Last save with this rotation and frame | Last save | Current |

Original is the baseline with every image control at default. The two "this rotation and frame" views use the current quarter turns and mirrors as well as the current rotation and frame, so the photo is the same way up and the same crop is on screen. Tone is the only thing that changes between those two and the current view.

Zoom and pan stay where they are while a button is held, so the same spot can be compared. The histogram follows the picture on screen.

## Pixels

Every calculation uses one layout. A stage that needs a different number converts for the duration of its own formula, then writes this layout back.

### The buffer

An image is width × height pixels, row by row, top to bottom. A pixel is three IEEE-754 binary32 values, R then G then B, tightly packed. There is no alpha and no padding between pixels. Twelve bytes per pixel.

A stored number is scene-linear light. Diffuse white is 1. Black is 0. A value above 1 is headroom, and exposure and the highlight shoulder both produce it. A value below 0 is not stored: the stage that would write it writes 0. NaN and infinity are not stored: the stage writes 0.

This is the only image the stack keeps. It is not 8-bit, not 16-bit integer, and not an sRGB code. Float64 does not change a result that started as an 8-bit JPEG. Float16 is coarser than one sRGB step in the shadows after a wide filter, and skies band. A 16-bit integer buffer cannot hold a sample above 1, and it is too narrow for the guided-filter and resample products.

JPEG has no alpha. PNG is not edited.

### Baseline and work

Decode the baseline file once per session. Apply EXIF orientation first, as the viewer does, and ignore the ICC profile. Phone JPEGs rely on the orientation tag. Then convert each 8-bit code with `decode` below and store the linear image. That image is the baseline. Nothing in the pipeline writes to it.

A run uses a second buffer, the work image, in the same layout. Orient, the first stage, writes it by copying pixels out of the baseline. Later stages read and write work. A pointwise stage may overwrite work in place. A stage that reads a neighborhood (the guided filter, clarity, the resample) writes a new buffer of this same layout and that buffer becomes work. Scratch for a luminance plane or a pyramid is the same binary32, one plane or many, and it is discarded when the stage returns.

The baseline's width and height stay fixed. A 90° or 270° turn replaces work with an image whose width and height are swapped. Horizon and frame may replace work with a smaller image still. The next run starts from the baseline again. Adjustments are never applied on top of a previous run's work image.

### Encode and decode

Codes in the file are sRGB. The transfer is IEC 61966-2-1, the piecewise curve, not a single power of 2.2. A power of 2.2 does not round-trip an 8-bit code.

```
decode(c) = c / 12.92                         when c ≤ 0.04045
          = ((c + 0.055) / 1.055) ^ 2.4       otherwise

encode(L) = 12.92 * L                         when L ≤ 0.0031308
          = 1.055 * L ^ (1 / 2.4) − 0.055     otherwise
```

`decode` is used once, when the baseline is built, with `c = code / 255`. The power branch of `encode` and `decode` is also used when `L` or `c` is above 1, so headroom has an encoding. The final 8-bit step is the only place that clamps to 1 before encoding.

Brightness, contrast, vibrance, saturation, and clarity are defined on encoded values. A stage at its default does not touch work, so a default does not pay a round trip. A stage that is not default does this and only this:

1. `e = encode(L)` for each channel, still binary32, still not clamped to 1.
2. Run that stage's formula on `e`. The formula is specified on `[0, 1]`. An encoded value below 0 is treated as 0. An encoded value above 1 is left unchanged, so super-white stays super-white while white stays the fixed point of the curve.
3. `L = decode(e)` and write that back into work.

Work is linear again before the next stage. The image is never left in gamma, and it is never quantized to 8-bit between stages. Resampling in gamma darkens edges, which is why horizon and frame read the linear buffer.

## Pipeline

Each preview and each render is one run. A control at its default is skipped. There is no brush and no heal.

| Order | Stage | Work buffer | Note |
| 1 | Orient | New linear buffer, one source pixel per output pixel. No resample. | Below |
| 2 | White balance | In place. A 3×3 matrix on linear RGB. No clamp at 1. | `Edit - white balance.md` |
| 3 | Exposure | In place. `L = L · 2^ev`. No clamp at 1. | `Edit - exposure.md` |
| 4 | Blackpoint | In place, per channel, one shared threshold. | `Edit - blackpoint.md` |
| 5 | Shadow, Highlight | Neighborhood reads. A new linear buffer becomes work. Luma scratch is binary32. | `Edit - shadows and highlights.md` |
| 6 | Brightness | Encode, formula, decode, as above. | `Edit - brightness.md` |
| 7 | Contrast | Encode, formula, decode. | `Edit - contrast.md` |
| 8 | Vibrance | Encode, formula, decode. | `Edit - vibrance.md` |
| 9 | Saturation | Encode, formula, decode. | `Edit - saturation.md` |
| 10 | Clarity | Encode to a linear-sized luma, filter, restore chroma, decode. A new buffer becomes work. | `Edit - clarity.md` |
| 11–12 | Horizon, then frame | One resample from the linear work image into a new linear image the size of the frame. | `Edit - frame and horizon.md` |

### Orient

Buttons, not sliders. Mirror horizontal and mirror vertical each toggle. One button rotates 90° clockwise and adds one quarter turn. A half turn is that button twice. `quarter_turns` is stored modulo 4, so four clicks return to the decoded picture. The frame and its shape turn with the picture: `3:2` becomes `2:3`, and the other ratios swap the same way. `Edit - frame and horizon.md`.

This runs before exposure, on the baseline. The tone filters then see the pixels in the orientation the user is editing. A 90° turn here is exact. It is not the horizon angle, which is a small resample at the end.

The copy reads backward, so the output has no holes. `W` and `H` are the baseline width and height. `q` is `quarter_turns`. `q = 1` is 90° clockwise and `q = 3` is 90° counterclockwise. Mirrors happen first, then the quarter turn, so the backward map undoes the turn and then undoes the mirrors.

An output pixel `(x, y)` lands on the mirrored image at `(u, v)`:

```
q = 0:  output size W × H,  (u, v) = (x, y)
q = 1:  output size H × W,  (u, v) = (y, H − 1 − x)
q = 2:  output size W × H,  (u, v) = (W − 1 − x, H − 1 − y)
q = 3:  output size H × W,  (u, v) = (W − 1 − y, x)
```

Then the baseline pixel is `(u, v)` with a mirror reflected:

```
sx = u            when mirror horizontal is off
sx = W − 1 − u    when it is on
sy = v            when mirror vertical is off
sy = H − 1 − v    when it is on
```

The output sample is the baseline sample at `(sx, sy)`. There is no kernel. All three at default skips the copy and work is the baseline.

### Horizon and frame

Horizon and frame share one resample, after clarity. Sampling reads backward with Catmull–Rom, as `Edit - frame and horizon.md` says. A source tap outside the oriented image is 0. θ = 0 and a full frame skip the resample: work is already the output. The horizon angle is not a place to turn the picture by 90°.

### Preview

The session keeps the full-resolution baseline. Once per session that baseline is area-averaged, in linear light, into a drag image whose long side is 1600. A photo already shorter than that on its long side uses the full baseline for both runs.

While a slider or the frame is moving, the pipeline runs on the drag image. The picture and the histogram follow that run. Releasing the control runs the pipeline again on the full baseline, and the picture and the histogram are replaced by that result. Save uses this same full run. A click, such as 90° or the shape button, runs the full baseline directly.

Clarity's radius and the shadow filter's radius are defined in pixels of the oriented image they run on. The drag run multiplies those pixel radii by the drag image's oriented width divided by the full image's oriented width. The full run uses the unscaled radii.

### Eight-bit output

After the frame, and only then, the preview and the rendered file quantize:

```
code = round(clamp(encode(clamp(L, 0, 1)), 0, 1) · 255 + triangular_noise)
code = clamp(code, 0, 255)
```

`triangular_noise` is the sum of two independent uniform draws from `−0.5` to `+0.5`, so it lies in `−1…+1` in units of one code. The noise is what keeps a smooth sky from banding when the float result is finer than 8 bits. The GPU preview is these codes, with alpha opaque. The preview does not pass through a JPEG encoder, so moving the quality slider does not change the picture or the histogram.

The written file is these codes, 8-bit sRGB, encoded at `jpeg_quality`. The scale is 1…100. Sampling is 4:4:4 at every step of that scale, so the slider changes the quantization and does not also throw away chroma. Save and Save as copy both use the stored value.

A JPEG does not store that 1…100 integer. It stores quantization tables. The integer is the Independent JPEG Group quality factor: the same scale `JpegEncoder::new_with_quality` writes. On a first edit, with no adj file, the slider starts at the factor that best matches the baseline's luminance table. For each candidate `q` from 1 to 100, build the table the encoder would write and sum the absolute difference against the file's 64 luminance entries. The `q` with the smallest sum wins. A tie keeps the higher `q`. A file with no usable luminance table starts at 90. An adj file that already stores `jpeg_quality` keeps that stored integer.

A render where every image control is at default, both mirrors are off, `quarter_turns` is 0, `rotation` is 0, the frame is the whole picture, and `jpeg_quality` is still that starting factor copies the baseline JPEG bytes and does not run this quantize. Re-encoding an untouched phone JPEG would lose more than the pipeline had changed. A different quality, with the image controls still at default, does encode: the user asked for a different file. Encoding uses the IJG tables for the chosen factor. A phone's own tables survive only in that byte copy.

A rendered JPEG carries the baseline's EXIF. The orientation tag is normal, because the pixels are already upright, and the pixel-size tags match the written image. Every other tag stays, including the taken time, the camera, and GPS. The Software tag keeps its existing text. `edited with Matejko` is added to it, once. A file whose Software tag is empty gets that phrase alone. The byte-copy path leaves the baseline EXIF untouched, because that file was not re-encoded.

## Histogram

Edit mode shows a histogram at the top of the right panel. It is the distribution of the framed picture, the work image after horizon and frame, which is the picture on screen. Pixels in the dimmed margin are not in that image and are not counted.

Three counts share one plot, R, G, and B. 256 bins, one per 8-bit code. A sample's bin is the code it would quantize to with the noise left out:

```
bin = round(clamp(encode(clamp(L, 0, 1)), 0, 1) · 255)
bin = clamp(bin, 0, 255)
```

The triangular noise is not added here. Adding it would move counts between neighboring bins on every run, and the plot would flicker while the sliders sat still.

The tallest bin of any channel fills the height of the plot. The other bins scale linearly with it. Bin 0 is the left edge and bin 255 is the right edge, so a pile on the left is the black floor and a pile on the right is clipped white. A gap between the left edge and the first occupied bin is the room a positive blackpoint would close.

The histogram is rebuilt at the end of the same run that updates the picture. While a control is moving it counts the drag image. After the release it counts the full run. Save does not draw a second histogram.

## Leaving edit mode

E enters. Exit is a button on the edit panel, labeled Exit. It is the only way out of the mode.

Esc does not leave edit mode, and it does not quit the app from there. In the viewer, Esc still leaves fullscreen or quits. Left, Right, Delete, and Enter do not leave edit mode either. Zoom and pan keep working.

Exit with nothing unsaved returns to viewing. The browsed file is whatever Save last wrote, or the file that was opened if Save was never used.

A control that differs from the last save is unsaved. If Save has not been used this visit, a control that differs from the loaded adj, or from the defaults when there was no adj, is unsaved. Exit then asks before leaving. Confirming discards the unsaved values and returns to viewing. The picture there is the last save, the same state S shows: the file Save last wrote, or the file that was opened when nothing has been saved. There is no control that copies S back onto the sliders while edit mode stays open. Dismissing the question stays in edit mode with the values still in the session.

## Open

**Highlight recovery on phone JPEGs.** The shadow note rebuilds a clipped channel from its neighbors. A phone JPEG has usually already clipped and thrown the extra samples away, so recovery only helps pixels that are still short of the ceiling. Many phones also write HEIC, which the viewer does not open. Raw is out of scope unless that changes.
