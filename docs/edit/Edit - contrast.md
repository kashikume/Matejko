# Contrast

Status: idea. Not part of the viewer.

Contrast is the global control Affinity Photo puts in Develop Persona on the Basic panel, inside Enhance, next to Clarity, Saturation, and Vibrance. The same idea is the Contrast slider of the Brightness / Contrast adjustment. Affinity's help states it directly: Contrast controls the tonal range, and dragging right increases the difference between dark and light areas.

Serif has not published the polynomial. The Brightness / Contrast adjustment does publish the split that matters, via the Linear checkbox. The default holds the ends of the range. Linear is a straight slope, and clipping can occur. Develop's Enhance slider is one strength, with no checkbox. The curve to implement for that slider is the default.

## What the slider does

One strength, default 0, range −100 to +100, the same range as the Brightness / Contrast adjustment.

The curve pivots at mid grey. A pixel there stays put at every strength.

- Above 0, tones below mid grey go darker and tones above it go lighter. The subject separates from the background. Color moves with it: the channels spread apart, so saturation rises.
- At 0, the picture is unchanged.
- Below 0, tones slide back toward mid grey. The frame looks flatter, and saturation falls with it.

This is one curve for every pixel. A shadow and a highlight on opposite sides of the frame share it. Neighborhoods do not. That is the difference from Clarity, which only stretches differences that are small next to their own neighborhood.

Levels is the other neighbor. Levels moves the black point and the white point. Default contrast does not: black stays black, white stays white, and the extra separation is spent in the middle.

## Algorithm

Apply a point curve to each channel of encoded RGB. Encoded means the document transfer, such as sRGB, where the code value 0.5 is mid grey. Chroma is not separated out first. Running the curve per channel is why a positive slider makes the picture richer, and why Saturation and Vibrance sit in the same Enhance group: they exist to put that side effect back where the user wants it.

Do this before Clarity. Contrast decides where mid grey is; Clarity then builds its neighborhood detail on that result. Enhance lists them in that order.

### Default curve

The default matches the help text for Linear left off: the adjustment avoids shadow and highlight clipping, and the change depends on the pixel's own lightness. On `[0, 1]` there is one cubic with that behavior. It pins 0, 1, and 0.5, and its slope at 0.5 is the strength.

Let `x` be one encoded channel, `a = s / 100` the slider in `[-1, 1]`, and `t = x − 0.5`.

```
m = 1 + 0.5 · a
y = 0.5 + t · (m + 4 · (1 − m) · t²)
```

`m` is the slope at mid grey. `a = 0` gives `m = 1` and `y = x`.

- `a = 1` gives `m = 1.5`. A code value of 0.25 falls to about 0.16. The ends do not move, and the slope at 0 and 1 falls to 0, so the last bits of black and white are left alone.
- `a = −1` gives `m = 0.5`. A code value of 0.25 rises to about 0.34. Black and white still do not move.

The cubic stays monotonic for `m` in `[0.5, 1.5]`, which is the whole slider. The slope at the ends is `3 − 2m`, so `m = 1.5` is the steepest this polynomial can go without turning backward and posterizing the shoulders. Past that, the control to reach for is Curves, not a larger `a`.

Clamp `y` to `[0, 1]` only as a guard. For `x` already in `[0, 1]` the cubic lands on 0 and 1 by itself.

### Linear slope

Linear on, in the help's words, modifies pixels by absolute values, and clipping may occur. The published form of that slope is the GIMP brightness-contrast map, which ImageMagick uses as well. With the same `a` in `(-1, 1)` and `x` in `[0, 1]`:

```
slant = tan((a + 1) · π / 4)
y = clamp((x − 0.5) · slant + 0.5, 0, 1)
```

`a = 0` gives `slant = 1`. Positive `a` rotates the line about mid grey and cuts off whatever falls outside `[0, 1]`. At `a = 0.5`, `slant ≈ 2.41`, and every encoded value outside about `[0.29, 0.71]` has already clamped. Negative `a` flattens toward 0.5, and at the limit black and white both become mid grey.

`a = 1` is `tan(π / 2)`, which is undefined. Stop the slider at about `0.99`, or use a finite taper with the same shape. One that is defined at both ends, with `n = 255 · a` in `[-255, 255]`:

```
factor = (259 · (n + 255)) / (255 · (259 − n))
y = clamp(factor · (x − 0.5) + 0.5, 0, 1)
```

`n = 0` gives `factor = 1`. `n = 255` gives `factor ≈ 129`. Same line about mid grey, no singularity, still clips.

This is the mode the Develop persona is often described as using for its built-in tone sliders. It is the wrong default for an Enhance contrast slider that has no checkbox: a moderate positive value already crushes blacks and blows the shoulders, and Clarity cannot put that texture back.

### Scene-linear samples

Both formulas above assume encoded code values, where 0.5 is mid grey. On scene-linear samples, 0.5 is a light grey (about one and a half stops above middle grey), so a pivot there drags the brightness of the picture along with the contrast.

The linear-slope idea in scene-linear light, pivoted on middle grey `G = 0.18`, is a power:

```
Y = G · (X / G)^m
```

`m` is the same midpoint slope as above. `X = 0` stays 0, `X = G` stays `G`, and highlights run past 1 and clip later in the display transform. That is the scene-referred version of the Linear option. The default Enhance curve should still be the cubic, run after encoding, so the pivot the user calls mid grey is the pivot the math uses.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `s` | The slider, −100…+100 | 0 |
| `a` | `s / 100` | −1…+1 |
| `m` | Slope at mid grey, `1 + 0.5 · a` | 1 |
| pivot | Encoded mid grey | 0.5 |
| channels | What the curve is applied to | R, G, and B independently |

No radius. A second user-facing parameter would be Curves.

## Failure cases worth testing against

- A grey ramp. Default contrast must keep the two ends and the midpoint, and stay monotonic. Linear contrast must clip the ends and keep the midpoint.
- Mid grey (encoded 0.5, or 18% linear if the power form is under test) must not brighten or darken as the slider moves.
- A saturated red next to a neutral of the same luma. Positive contrast makes the red stronger. If it does not, the curve was applied to luma only.
- The same slider on a linear buffer with the pivot left at 0.5. The frame shifts lighter as contrast rises. That is the pivot bug, not a property of contrast.
- `a = 1` on the default cubic. The shoulders flatten and do not reverse. A reversed shoulder means `m` was allowed past 1.5.
- Contrast after Clarity. Texture that Clarity added gets stretched again, and the result is harsher than Enhance order (contrast, then clarity) on the same numbers.

## Sources

- Affinity Photo 2 help, “Basic panel”. Enhance adjusts contrast, clarity, saturation, and vibrance.
- Affinity Photo 2 help, “Brightness / Contrast adjustment”. Contrast controls the tonal range. With Linear off, pixels change relative to their lightness and the ends are kept from clipping. With Linear on, the change is absolute and clipping can occur.
- GIMP, `gimpoperationbrightnesscontrast.c`. The linear mode: `slant = tan((contrast + 1) · π / 4)`, then `(x − 0.5) · slant + 0.5`, per channel. ImageMagick's `-brightness-contrast` is this map.
