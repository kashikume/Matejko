# Blackpoint

Status: idea. Not part of the viewer.

Blackpoint is the control Affinity Photo puts in Develop, in the Exposure group, next to Exposure and Brightness. Affinity's help states it directly: it decides which pixels count as pure black. Drag right and more of the dark tones join that black, so the shadows get heavier. Drag left and those pixels are excluded again, so the shadows get lighter.

Serif has not published the map. It is the black end of a levels control. The formula below is that map.

## What the slider does

One strength, default 0, range −100 to +100.

White stays white. The slider moves only the dark end.

- Above 0, the floor rises to meet the picture. A pixel at the new point becomes pure black, and every pixel darker than that becomes pure black too. A navy coat that was dark grey goes black. Tones above the point spread down to fill the gap, so the change is strongest just above the floor and small near white. Detail that fell through the floor is gone.
- At 0, the picture is unchanged. A pixel that was already 0 stays 0.
- Below 0, pure black is no longer allowed. The darkest pixel rises to a dark grey, and the whole ramp lifts a little, shadows most of all. This is the milky look of a scan with no true black.

On a phone JPEG the useful move is a small positive one. The phone has often left the darkest tone slightly above black, and the picture looks grey until that tone sits on the floor. A large positive value eats hair, dark clothes, and night sky.

## Where it sits next to the other dark-end controls

Exposure multiplies every tone by the same number of stops. Black stays 0. A positive stop lifts the floor along with everything else.

Blackpoint sets that floor after exposure. It does not brighten the frame. It decides the input level that output black corresponds to, then stretches the rest so white is still white.

The shadow slider, next, opens detail in the dark end and keeps a toe so true black stays black. It cannot restore a pixel this control has already mapped to 0. That is why blackpoint comes first: the floor is chosen, then the shadow slider works on what is still above it.

Brightness and contrast hold both encoded ends. They do not choose a new black.

## Algorithm

Run on scene-linear RGB after exposure, one shared threshold on each channel. `X = 1` is diffuse white.

Let `s` be the slider and

```
b = 0.05 · (s / 100)
```

Full scale is five percent of linear white. On a phone JPEG that is already a deep floor. `s = +25` is about one percent.

### Deeper black

When `b > 0`:

```
Y = max(X − b, 0) / (1 − b)
```

`X ≤ b` becomes 0. `X = 1` stays 1. `b = 0` is identity.

A linear ramp at `b = 0.05`: 0.05 maps to 0, 0.18 maps to about 0.137, 1 stays 1. Middle grey drops. The top end barely moves.

### Lifted black

When `b < 0`, let `q = −b`. The output floor becomes `q`, and white stays 1:

```
Y = q + (1 − q) · X
```

`X = 0` maps to `q`. At `s = −100`, `q = 0.05`, so the black of the file becomes a dark grey and a mid grey of 0.18 becomes about 0.22.

The same `b` is applied to R, G, and B. A neutral stays neutral. A dark color with one channel under `b` and another above it loses the channel that hit the floor, so its hue shifts toward the channels that remain. That is the levels black point. A small `b` keeps the shift in the pixels that were nearly black anyway.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `s` | The slider, −100…+100 | 0 |
| `b` | Linear black level, `0.05 · s / 100` | 0 |
| white | Diffuse white | stays 1 |
| channels | What the map is applied to | R, G, and B, one shared `b` |

No radius. A control that should bring detail back out of a dark region, without deleting the floor, is the shadow slider.

## Failure cases worth testing against

- `s = 0`. The linear image is unchanged.
- A ramp from 0 to 1 at `s = +100` (`b = 0.05`). Output is 0 up through 0.05, then a straight line to 1. A pixel above 0.05 that becomes 0 means the threshold was applied twice.
- Linear 0.18 at `b = 0.05` lands near 0.137. Linear 1 stays 1.
- A neutral stays neutral. A pixel `(0.02, 0.08, 0.08)` at `b = 0.05` loses the red channel and stays green-blue.
- `s = −100`. Linear 0 maps to 0.05 and linear 1 stays 1. A result that still has true black skipped the lift.
- Blackpoint after the shadow slider. Pixels the shadow control had opened can fall back through the new floor, and the toe in that control no longer describes the black on screen.

## Sources

- Affinity Photo help, Exposure section of the Develop Basic panel. Blackpoint determines the pixels considered pure black. Right includes more pixels and the shadows increase. Left excludes pixels and the shadows reduce.
- The levels black point: input `b` maps to output 0, input white maps to white, and the segment between them is linear. The crush formula is that segment.
