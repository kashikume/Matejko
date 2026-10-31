# Perspective

Status: idea. Not part of the viewer.

Vertical and Horizontal are the keystone sliders. Affinity Photo puts them in Develop Persona on the Lens panel, next to Rotation and Scale. Lightroom and Camera Raw have the same pair under Transform. They straighten lines that converge because the camera was not square on to the subject: the walls of a building shot from the street lean in toward the top, and a shop front shot from one side shrinks toward its far end.

Serif has not published the map. A camera turned away from a flat subject changes the picture by a projective map: straight lines stay straight, and parallel lines meet. Undoing it is a projective map too. That map, with its strength chosen by the slider, is what to implement.

## What the sliders do

Two sliders, Vertical and Horizontal, each −100 to +100, default 0. They sit in the Frame group, after Rotation.

- Vertical above 0 widens the top of the picture and narrows the bottom. Verticals that lean in toward the top stand up. Below 0 does the opposite, for a picture taken looking down.
- Horizontal above 0 makes the right side taller and the left side shorter. Lines that close in toward the right open out. Below 0 does the opposite.
- At 0 on both, the picture is unchanged.

The photo's centre stays where it is. A line through the centre that was straight stays straight. Every straight line stays straight.

Serif does not publish which way Affinity's sliders turn. The direction above is this note's choice, so the useful direction for the common case, a building shot from below, is to the right.

## Where it sits

Perspective is part of the one geometric resample at the end, with the horizon turn and the frame.

1. Orient: mirrors and quarter turns, exact.
2. The tone stages, on the oriented image.
3. Perspective, then the horizon turn, then the frame: one resample.

The keystone runs before the turn. A building is made upright by its own perspective first, and the horizon is then levelled on the corrected picture. The turn is about the photo's centre, which the keystone keeps still.

## Algorithm

Offsets are from the photo's centre, in pixels of the oriented photo, y down. `W` and `H` are the photo's width and height, `a = W / 2`, `b = H / 2`.

### The keystone

Let `kv = 0.5 · vertical / 100` and `kh = 0.5 · horizontal / 100`. The map takes the photo's corners to a quad:

```
top left      (−a, −b)  →  (−a · (1 + kv), −b · (1 − kh))
top right     ( a, −b)  →  ( a · (1 + kv), −b · (1 + kh))
bottom right  ( a,  b)  →  ( a · (1 − kv),  b · (1 + kh))
bottom left   (−a,  b)  →  (−a · (1 − kv),  b · (1 − kh))
```

At `vertical = +100` the top edge is 1.5 times as long and the bottom edge half as long. A useful correction is usually far smaller.

The four pairs fix one projective map `P`, a 3×3 matrix up to scale, with

```
(u, v) = ((p0 x + p1 y + p2) / (p6 x + p7 y + p8),
          (p3 x + p4 y + p5) / (p6 x + p7 y + p8))
```

Solve the eight linear equations the corners give, with `p8 = 1`. Work in 64-bit floats: the system is small, and the entries differ by the image size squared.

`P` sends the centre to the point where the quad's diagonals cross, which is not the centre. Move it back: with `(tx, ty) = P(0, 0)`, subtract `tx` times the last row from the first, and `ty` times the last row from the second. The result keeps the centre still.

### Reading backward

The output reads backward, as the horizon does. For an output offset `(dx, dy)`:

1. Turn it clockwise by the horizon angle, which undoes the counterclockwise turn.
2. Apply `P⁻¹`.
3. The result is the photo offset to sample, with the Catmull–Rom kernel from `Edit - frame and horizon.md`. A point outside the photo is 0.

One kernel sample per output pixel is enough for the corrections this control is for. A strong keystone shrinks one end of the picture by up to half, and that end may alias a little. No prefilter is applied.

### The usable rectangle

For a turn alone, the usable rectangle has the closed form in `Edit - frame and horizon.md`. A keystone has none. The usable rectangle is then the largest upright rectangle centred on the photo's centre whose corners all lie on the corrected photo.

Find it numerically. For a ratio `r` of half-width to half-height, bisect for the largest half-height `h` with the corners `(±r·h, ±h)` on the photo. Search `r` for the largest area `4·r·h²` by golden section, over `ln r` within 1.5 of the photo's own ratio. A corner is on the photo when reading it backward lands inside `±a, ±b`, with a hundredth of a pixel of slack.

The frame works as it does for the turn. It is stored as fractions of the usable rectangle. It may reach past that rectangle wherever the corrected photo does, and an edge pulled outward stops on the photo's border. A change of either slider draws a frame that no longer fits toward the centred fit until it does.

### On screen

The picture on screen is the whole corrected photo in the upright box that just holds it. The box is not centred on the photo's centre: a keystone pushes one side out further than the other. The view keeps the photo's centre and size steady on screen while a slider moves, so the picture does not drift as the box changes.

## Mirrors and quarter turns

The sliders describe the picture on screen. A button that changes the picture carries them with it, so the corrected result is mirrored or turned as a whole.

- Mirror horizontal: Horizontal and the horizon angle change sign, and the frame flips left to right.
- Mirror vertical: Vertical and the horizon angle change sign, and the frame flips top to bottom.
- A clockwise quarter turn: the old top becomes the right side and the old right side becomes the bottom. New Horizontal is old Vertical. New Vertical is minus old Horizontal.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `vertical` | Slider, −100…+100. Right widens the top | 0 |
| `horizontal` | Slider, −100…+100. Right makes the right side taller | 0 |
| `k` | Edge scale at full slider | 0.5 |
| centre | Fixed point of the map | photo centre |
| kernel | Resample | Catmull–Rom, shared with the horizon |

No focal length. A model of a camera turned in space would need the lens's field of view, which a JPEG may not record. The four-corner keystone needs none, and it is the same family of maps.

## Failure cases worth testing against

- Both sliders at 0, with the horizon at 0. No resample.
- Vertical +40. The top edge is 1.2 / 0.8 = 1.5 times the bottom edge. The centre does not move.
- Three points on a slanted line, before and after. They are still on one line. A bent line means the map is not projective, for example a per-row scale.
- Forward, then backward. Each point returns within a hundredth of a pixel.
- The full frame at Vertical +30, Horizontal −45, and both with a 6° turn. Every pixel of the crop is a photo pixel. A rectangle one percent larger no longer fits.
- Mirror horizontal with Horizontal +20 and a 5° turn. Horizontal is −20 and the turn −5°, and the picture is the mirror image of the one before.
- An adjustment file written before this control. It has no `vertical` or `horizontal`, and both read as 0.

## Sources

- Affinity Photo help, Lens panel of the Develop Persona. Vertical and Horizontal correct perspective. The help publishes neither the map nor the direction.
- Adobe, Lightroom and Camera Raw help, Transform panel. Vertical and Horizontal correct perspective, alongside Rotate and Scale.
- Hartley and Zisserman, "Multiple View Geometry in Computer Vision", chapter 4. The projective map from four point pairs, and why a plane seen from another angle is such a map.
