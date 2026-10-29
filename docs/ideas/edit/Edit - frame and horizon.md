# Frame and horizon

Status: idea. Not part of the viewer.

Horizon is a small rotation that levels the picture. Frame is the rectangle kept from that leveled picture. They run after every tone and color control, and they run in that order: rotate, then cut. Cutting first and rotating afterward leaves empty corners, and the cut is no longer the picture the user framed.

EXIF orientation has already been applied at decode. Mirror and the 90° quarter turns, in the assumptions note, have already been applied as the first pipeline step. They copy pixels and do not resample. Horizon is the small angle after that, not another 90° turn.

## What the controls do

Horizon is one angle, default 0, range −45° to +45°. Positive turns the picture counterclockwise on screen. A horizon that rises to the right needs a negative angle. Zero leaves the pixels where the tone pipeline put them.

Frame is a rectangle inside the leveled photo. Default is the whole usable picture. Tightening it throws away the edges. It does not scale the file back up: the render is the pixels inside the rectangle.

Rotation turns the photo into a quad and leaves empty pixels in the corners of the upright canvas. The frame is an upright rectangle. Every corner of it lies on the photo, inside that quad or on its edge. An empty pixel is never inside the frame.

The usable picture is the largest such rectangle. Its sides touch the rotated photo's edges. That touch is the snap line. Dragging an edge or a corner outward moves the frame until it meets the rotated border, and then the edge sticks there. It does not travel into the empty corner, and it does not stop short of the border while the pointer is still pulling outward. A full frame is that snapped rectangle.

Changing the angle rebuilds the quad and the snap lines. If a corner of the frame would then sit on an empty pixel, the frame is replaced by the largest upright rectangle of its current ratio that still lies on the photo. A tighter frame that still lies entirely on the photo keeps its place. Free has no ratio: an edge that has crossed into empty pixels is pulled back to the rotated border. A stored fraction outside 0…1 is clamped back onto the usable rectangle before the cut.

## Where they sit

Clarity is the last control that reads neighbors. Horizon and frame do not change a pixel's tone. They move and discard pixels. Running them earlier would make clarity's radius and the shadow filter's radius apply to a different set of neighbors than the ones in the rendered file, and the pipeline would resample twice.

The tone pipeline finishes in float. Horizon and frame resample that float image once. The conversion to 8-bit JPEG is after the cut.

## Horizon

θ is the angle in degrees. Rotation is about the center of the baseline.

Sampling reads backward, so the output has no holes. For an output point `(x, y)`, with the center `(cx, cy)` and θ in radians:

```
dx = x − cx
dy = y − cy
sx =  cos(θ) · dx + sin(θ) · dy + cx
sy = −sin(θ) · dx + cos(θ) · dy + cy
```

`(sx, sy)` is where the sample comes from. The kernel is Catmull–Rom, a cubic on the 4×4 neighborhood. A source coordinate outside the baseline is outside the usable picture and is not part of the frame.

θ = 0 skips the resample. The picture is unchanged. The angle comes from the rotation slider.

## Usable rectangle

`W` and `H` are the baseline width and height in pixels. `angle` is `|θ|` in radians. `side_long` is the longer side, `side_short` the shorter, and `width_is_longer` records which.

```
sin_a = sin(angle)
cos_a = cos(angle)
```

When `side_short ≤ 2 · sin_a · cos_a · side_long`, the rectangle is constrained by the short side:

```
x = 0.5 · side_short
```

If the baseline is wider than it is tall, the usable size is `x / sin_a` by `x / cos_a`. If it is taller, the two are swapped.

Otherwise the rectangle touches all four sides:

```
cos_2a = cos_a² − sin_a²
wr = (W · cos_a − H · sin_a) / cos_2a
hr = (H · cos_a − W · sin_a) / cos_2a
```

At θ = 0 this path is not used. The usable rectangle is the baseline, `W` by `H`.

The usable rectangle is centered on the rotated picture.

## Frame

The stored frame is `x`, `y`, `w`, `h`, each in 0…1 of the usable rectangle. Default is `0, 0, 1, 1`.

```
left   = inset_x + x · wr
top    = inset_y + y · hr
width  = w · wr
height = h · hr
```

`inset_x` and `inset_y` place the usable rectangle on the rotated canvas. `width` and `height` are at least 1 pixel. The output image is that size. Nothing outside it is rendered.

One button cycles the frame's shape. Each click advances, and the label is the step that is showing:

1. Original
2. Free
3. 3:2
4. 2:3
5. 4:3
6. 3:4
7. 1:1
8. 4:5
9. 5:4
10. 16:9
11. 9:16

The click after 9:16 returns to Original. Edit mode opens on Original. Original is the ratio of the picture after the mirrors and quarter turns, before this small rotation.

A 90° turn rotates the picture and the frame together. The stored fractions turn with the pixels. Before the turn they are `(x, y, w, h)` of that picture. After a clockwise turn they are `(1 − y − h, x, h, w)`. A numbered ratio swaps its sides: `3:2` becomes `2:3`, `4:3` becomes `3:4`, `4:5` becomes `5:4`, `16:9` becomes `9:16`, and each of those swaps back on the next turn. `1:1` stays `1:1`. Free stays free. Original stays the word Original, and the picture's own ratio has swapped because the pixels turned. The small rotation is left as it was. If the turned frame would then cover an empty pixel, the fit above runs.

Each click sets the frame to the largest upright rectangle of that ratio that lies inside the rotated photo, centered on the picture. No empty pixel is inside it. Free is the usable rectangle, the largest upright rectangle of any ratio. Dragging then shrinks or moves that rectangle. A numbered ratio keeps its ratio while it is dragged. Free does not. The drag still snaps to the rotated border.

On the picture, the frame is the bright rectangle and the discarded margin is dimmed. The empty corners left by the rotation are part of that margin. They are not a place the frame can sit. The rectangle itself is edited on the picture, and each drag snaps as above. The same angle is a slider on the panel, in the Frame group, −45° to +45°.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `rotation` | Degrees, counterclockwise on screen | 0 |
| range | Clamp on the angle | −45…+45 |
| `x`, `y` | Top-left of the frame, fraction of the usable rectangle | 0, 0 |
| `w`, `h` | Size of the frame, fraction of the usable rectangle | 1, 1 |
| `aspect` | Shape of the frame. `original`, `free`, or a ratio such as `3:2` | original |
| kernel | Resample | Catmull–Rom |

## Failure cases worth testing against

- θ = 0 and a full frame. The render matches the tone pipeline output, pixel for pixel. A resample at zero means the skip was missed.
- Moving the rotation slider to 10°. The whole picture turns counterclockwise and the frame stays upright. A corner that would land on an empty pixel is gone, and the frame is the largest of its ratio that still lies on the photo.
- Clicking the shape button to 1:1. The frame becomes the largest square inside the rotated photo. A square that leaves a band of photo unused on the long side is the right result. A square that contains an empty corner is not.
- Aspect `3:2`, then one click of 90°. The picture is on its side and the aspect is `2:3`. The next click returns it to `3:2`. Original stays labeled Original, and its ratio has swapped with the pixels.
- θ = 10° on a 4:3 phone JPEG. Every output pixel is a photo pixel. An empty corner inside the frame means a drag crossed the rotated border without snapping.
- Dragging the right edge outward at θ = 10°. The edge meets the rotated photo and stays on that border. A gap between the edge and the border, while the drag is still outward, means the snap stopped early. A black triangle inside the edge means it did not stop.
- The same crop fractions at θ = 0 and θ = 10°. Both fill their frame. The 10° frame is the smaller inscribed rectangle, not the baseline with black corners.
- A dark line through the center, rotated 15°. The line is still one pixel wide, without a second soft copy. A double image means the forward rotation was used instead of the inverse sample.
- Clarity after the frame. A radius of 64 px then means 64 px of the cropped picture, and the render no longer matches clarity-then-frame.

## Sources

- The largest upright rectangle inside a rotated rectangle. The half-constrained and fully-constrained cases above are the standard closed form for that crop.
- Catmull and Rom. “A Class of Local Interpolating Splines.” 1974. The cubic used to pull source pixels onto the rotated grid.
