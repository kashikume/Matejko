# Lens distortion

Status: idea. Not part of the viewer.

Distortion is the lens correction slider. Affinity Photo puts it in Develop Persona on the Lens panel, with the perspective sliders. Lightroom and Camera Raw have it under Manual lens corrections. It straightens lines the lens bent. A wide lens, which is the main camera of most phones, shows barrel distortion: straight lines near the edges bow outward, away from the centre. A long lens can show pincushion: lines bow inward, toward the centre.

Serif has not published the map or the direction. The radial polynomial below is the usual first term of the lens model that profile-based corrections use. The direction follows Lightroom's help, which does publish it.

## What the slider does

One slider, Distortion, −100 to +100, default 0. It sits in the Frame group, after Horizontal.

- Above 0, it straightens barrel distortion. The edges and corners of the picture are pushed out, the corners most. Lines that bowed away from the centre come straight. The photo's straight border becomes curved: its edges bow in between the corners, so the usable rectangle shrinks.
- Below 0, it straightens pincushion distortion. The corners are pulled in. The border bows out between the corners, and empty canvas shows at the corners.
- At 0, the picture is unchanged.

The centre does not move. A line through the centre stays straight at every value, because the map only moves points along the radius.

## Where it sits

Lens distortion is part of the one geometric resample at the end, and it comes first there:

1. Lens distortion: the lens bent the picture before anything else happened to it.
2. Perspective, in `Edit - perspective.md`.
3. The horizon turn, in `Edit - frame and horizon.md`.
4. The frame.

## Algorithm

Offsets are from the photo's centre, in pixels of the oriented photo. `R` is half the photo's diagonal, so the photo's corners are at radius `R`. `ρ` is a radius divided by `R`.

Let `c = 0.36 · |s| / 100` for the slider `s`. The two directions use the same polynomial, written each way round:

```
s > 0, barrel:      a photo point at ρ is shown at   ρ · (1 + c · ρ²)
s < 0, pincushion:  an output point at ρ shows the photo at   ρ · (1 + c · ρ²)
```

The barrel correction pushes the photo out, its corners to 1.36 R at the slider's end. The pincushion correction pulls it in, its corners to about 0.78 R.

In the backward chain from an output point: undo the horizon turn, undo the perspective with `P⁻¹`, then undo the lens correction. Sample the photo there with the Catmull–Rom kernel.

### Why each side is written its own way

`ρ · (1 + c · ρ²)` with `c ≥ 0` only ever grows. Written in the direction each correction moves points outward, it is one to one at any strength.

The same polynomial with a negative coefficient turns back on itself: past `ρ = 1 / √(3 |c|)` it falls again. A barrel correction written backward, `photo = ρ · (1 − c · ρ²)`, would fold before it reached the photo's corners once `c` passed 4/27 ≈ 0.148, and the photo's corners would be lost. Past the fold it would also show the photo a second time, mirrored, in the far corners of the canvas. Writing the barrel side forward avoids both, so the slider can reach 0.36.

### Solving the other way

The backward read of a barrel correction, and the forward map of a pincushion correction, solve

```
ρ · (1 + c · ρ²) = target
```

for `ρ`, with `c ≥ 0`. The left side rises and curves up. Newton's method from `ρ = target`, which is at or past the root, closes in from above without overshooting. A few steps reach a millionth. The barrel correction solves this once per output pixel, which roughly doubles the cost of the resample.

### A curved border

A turn and a keystone keep lines straight. The photo's border stays four straight edges, the photo stays convex, and a rectangle lies on it when its four corners do.

Lens correction bends the border. After a barrel correction the edges bow in, and a rectangle can have all four corners on the photo while its edges run through empty canvas. So, whenever Distortion is not 0:

- A rectangle is on the photo when its corners and 15 points spaced along each edge are. A gap between those points is a fraction of a pixel at the curvature this slider reaches.
- The usable rectangle is found numerically, as for the keystone, with this test in place of the corners.
- The box around the corrected photo walks the whole border, 64 points per edge. After a pincushion correction the border bows out, and its furthest point is between the corners, not at one.

The search that pulls a frame back onto the photo still starts from the centred fit, which lies on the photo. After a barrel correction the photo is no longer convex, so the frames between are not guaranteed to fit. Each one is tested as it is tried.

## Mirrors and quarter turns

The map is radial. A mirror or a quarter turn leaves it as it is. The slider keeps its value.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `s` | Slider, −100…+100. Right straightens barrel | 0 |
| `c` | Radial coefficient, `0.36 · |s| / 100` | 0 |
| `R` | Half the photo's diagonal | per photo |
| edge points | Points tested along each edge of a rectangle | 15 |

No second coefficient and no lens profile. A profile from the lens maker is a set of these coefficients for one lens, and a JPEG does not carry it.

## Failure cases worth testing against

- Distortion 0, with no turn and no keystone. No resample.
- Distortion +100. The photo's corner is shown at 1.36 of its radius, and the centre stays put.
- Forward, then backward. A point returns within a hundredth of a pixel.
- Distortion −100. The photo's corner is shown nearer the centre than it was.
- The full frame at +100, at −100, and at +60 with an 8° turn and a keystone. Every pixel of the crop is a photo pixel. A rectangle one percent larger runs off the photo somewhere along an edge, even where its corners are still on it.
- The box around the corrected photo at −100. It holds the bowed-out middle of each edge, not only the corners.
- Distortion +100, read back from twice the corner's radius. It lands beyond the photo, not back on it. A point that lands on the photo means the map folded.
- An adjustment file written before this control. It has no `distortion`, and it reads as 0.

## Sources

- Affinity Photo help, Lens panel of the Develop Persona. Lens distortion is corrected there. The help publishes neither the map nor the direction.
- Adobe, Lightroom help, "Correct lens distortions". Distortion: drag right to correct barrel distortion and straighten lines that bend away from the centre, drag left to correct pincushion distortion and straighten lines that bend toward the centre.
- D. C. Brown, "Decentering Distortion of Lenses", Photogrammetric Engineering, 1966. The radial polynomial `r · (1 + k₁ r² + k₂ r⁴ + …)`. This control is its first term.
