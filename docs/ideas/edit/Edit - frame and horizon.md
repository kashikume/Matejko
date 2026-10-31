# Frame and horizon

Status: idea. Not part of the viewer.

Horizon is a small rotation that levels the picture. Frame is the rectangle kept from that leveled picture. They run after every tone and color control, and they run in that order: rotate, then cut. Cutting first and rotating afterward leaves empty corners, and the cut is no longer the picture the user framed.

EXIF orientation has already been applied at decode. Mirror and the 90° quarter turns, in the assumptions note, have already been applied as the first pipeline step. They copy pixels and do not resample. Horizon is the small angle after that, not another 90° turn.

## What the controls do

Horizon is one angle, default 0, range −45° to +45°. Positive turns the picture counterclockwise on screen. A horizon that rises to the right needs a negative angle. Zero leaves the pixels where the tone pipeline put them.

Frame is a rectangle inside the leveled photo. Default is the whole usable picture. Tightening it throws away the edges. It does not scale the file back up: the render is the pixels inside the rectangle.

Rotation turns the photo into a quad and leaves empty pixels in the corners of the upright canvas. The frame is an upright rectangle. Every corner of it lies on the photo, inside that quad or on its edge. An empty pixel is never inside the frame.

The usable rectangle is the largest such rectangle. Its sides touch the rotated photo's edges. A full frame is that rectangle. It is not a fence. The frame may reach past it wherever the turned photo does: a tall, narrow frame can rise above it in the middle, where the photo stands higher, and a small frame can move out past its sides halfway down. The only rule is that all four corners lie on the photo.

The rotated border is the snap line. Dragging an edge or a corner outward moves the frame until a corner meets the border, and then the edge sticks there. It does not travel into the empty corner, and it does not stop short of the border while the pointer is still pulling outward. A moved frame that meets the border on one side keeps following the pointer along the other axis.

Changing the angle rebuilds the quad and the snap lines. The frame is stored as a fraction of the usable rectangle, so a full frame stays snapped to the new borders, and a tighter frame stays the same fraction of them. A frame whose corners are no longer on the photo at the new angle is drawn toward the centered fit, the largest frame of its ratio centered in the usable rectangle, until they are. That fit is always on the photo. Every frame between it and the stored one has its corners between two points on the photo, which is convex, so the search finds the closest frame that fits.

## Where they sit

Clarity is the last control that reads neighbors. Horizon and frame do not change a pixel's tone. They move and discard pixels. Running them earlier would make clarity's radius and the shadow filter's radius apply to a different set of neighbors than the ones in the rendered file, and the pipeline would resample twice.

The tone pipeline finishes in float. Horizon and frame resample that float image once. The conversion to 8-bit JPEG is after the cut.

## Horizon

θ is the angle in degrees. Rotation is about the center of the baseline.

Sampling reads backward, so the output has no holes. Coordinates are pixel edges: pixel `i` covers `[i, i + 1)`, and the center is `(W / 2, H / 2)`. Screen y points down. A counterclockwise turn by θ on screen takes a source offset `(dx, dy)` to `(cos θ · dx + sin θ · dy, −sin θ · dx + cos θ · dy)`. The backward read is its inverse. For an output point `(x, y)`, with θ in radians:

```
dx = x − cx
dy = y − cy
sx = cos(θ) · dx − sin(θ) · dy + cx
sy = sin(θ) · dx + cos(θ) · dy + cy
```

`(sx, sy)` is where the sample comes from. A dot right of center at θ = +90° lands above center. The kernel is Catmull–Rom, a cubic on the 4×4 neighborhood, centered on `(sx − 0.5, sy − 0.5)` because pixel centers sit at half-integers. An output point whose `(sx, sy)` falls outside the photo is an empty corner and is 0. A tap that reaches past the photo's edge for a point inside it repeats the edge pixel, so the last row of the photo is not darkened by the empty canvas beside it.

θ = 0 skips the resample. The picture is unchanged, and a crop at θ = 0 copies whole pixels with no kernel. The angle comes from the rotation slider.

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

The stored frame is `x`, `y`, `w`, `h`, in fractions of the usable rectangle. Default is `0, 0, 1, 1`. At θ = 0 the photo is the usable rectangle, so every value is in 0…1. At other angles a value may fall outside 0…1 where the photo reaches past the rectangle.

```
left   = inset_x + x · wr
top    = inset_y + y · hr
width  = w · wr
height = h · hr
```

`inset_x` and `inset_y` place the usable rectangle on the rotated canvas. `width` and `height` are at least 1 pixel. The output image is that size. Nothing outside it is rendered.

An aspect lock, when it is on, fits one of these ratios inside the usable rectangle and keeps that ratio while the frame is dragged: free, the baseline's own ratio, 1:1, 4:5, 3:2, 16:9. Free is the default. The lock changes `w` and `h` together. It does not change θ.

On the picture, the frame is the bright rectangle and the discarded margin is dimmed. The picture on screen is the whole rotated photo, in the upright box that just holds it: `W · |cos θ| + H · |sin θ|` by `W · |sin θ| + H · |cos θ|`, centered where the photo was. No corner of the photo is cut off on screen. The zoom holds the photo's size, so the box grows around it rather than the photo shrinking into it. The empty corners left by the rotation are part of that margin. They are not a place the frame can sit. The rectangle itself is edited on the picture, and each drag snaps as above. The same angle is a slider on the panel, in the Frame group, −45° to +45°.

The pointer on the picture:

- An edge or a corner of the frame, within a few pixels, resizes it. The pointer shows a resize arrow there. A locked ratio holds on screen. A corner follows whichever axis the pointer pulled further. An edge keeps the other axis centered.
- Inside a frame smaller than the picture, a drag moves the frame.
- Anywhere else, including inside a frame that is the whole picture, a drag pans. With Shift held, a drag pans everywhere, so a tight frame can still be panned under while zoomed in.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `rotation` | Degrees, counterclockwise on screen | 0 |
| range | Clamp on the angle | −45…+45 |
| `x`, `y` | Top-left of the frame, fraction of the usable rectangle | 0, 0 |
| `w`, `h` | Size of the frame, fraction of the usable rectangle | 1, 1 |
| aspect | Lock while dragging the frame | free |
| kernel | Resample | Catmull–Rom |

## Failure cases worth testing against

- θ = 0 and a full frame. The render matches the tone pipeline output, pixel for pixel. A resample at zero means the skip was missed.
- Moving the rotation slider to 10°. The whole picture turns counterclockwise and the frame stays upright.
- θ = 10° on a 4:3 phone JPEG. Every output pixel is a photo pixel. An empty corner inside the frame means a drag crossed the rotated border without snapping.
- Dragging the right edge outward at θ = 10°. The edge meets the rotated photo and stays on that border. A gap between the edge and the border, while the drag is still outward, means the snap stopped early. A black triangle inside the edge means it did not stop.
- The same crop fractions at θ = 0 and θ = 10°. Both fill their frame. The 10° frame is the smaller inscribed rectangle, not the baseline with black corners.
- A dark line through the center, rotated 15°. The line is still one pixel wide, without a second soft copy. A double image means the forward rotation was used instead of the inverse sample.
- Clarity after the frame. A radius of 64 px then means 64 px of the cropped picture, and the render no longer matches clarity-then-frame.

## Sources

- The largest upright rectangle inside a rotated rectangle. The half-constrained and fully-constrained cases above are the standard closed form for that crop.
- Catmull and Rom. “A Class of Local Interpolating Splines.” 1974. The cubic used to pull source pixels onto the rotated grid.
