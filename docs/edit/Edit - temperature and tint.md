# Temperature and Tint

Status: idea. Not part of the viewer.

Temperature and Tint are the two white balance sliders. Affinity Photo puts them in Develop Persona on the Basic panel, in the White Balance group. Camera Raw and Lightroom have the same pair at the top of their Basic panel. On a JPEG, both of those show the sliders as relative values, −100 to +100, because the file no longer carries the camera's white balance. That relative pair is what this note specifies.

Temperature moves the picture along the daylight locus: right is warmer (yellow), left is cooler (blue). Tint moves it across that locus: right is magenta, left is green. Together they fix the cast of a light the phone guessed wrong, such as a tungsten room shot as daylight, or a green cast from fluorescent tubes or foliage.

## What the sliders do

Two sliders, each with default 0 and range −100 to +100.

- Temperature above 0 warms the frame. A grey wall turns cream and a blue sky turns less blue. Below 0 cools it. A grey turns blue-grey.
- Tint above 0 adds magenta. A grey turns pink-grey. Below 0 adds green.
- At 0 on both, the picture is unchanged.

A neutral keeps its luminance. Only its color moves, to the color of the new white. Every other color moves with that white, the way the eye adapts to a light, rather than having the same amount added to every pixel.

## Where it sits

White balance is the first tone stage, after Orient and before Exposure.

1. Orient.
2. Temperature and Tint, on scene-linear RGB.
3. Exposure, then Blackpoint, then Shadows and Highlights.
4. Brightness, Contrast, Vibrance, Saturation, and Clarity on encoded values.

It runs on linear light because a white balance is a change of illuminant, and light adds linearly. The same matrix on encoded values tints the shadows and the highlights by different amounts.

It runs before Blackpoint because Blackpoint applies one shared threshold to R, G, and B. A cast still in the picture would make one channel reach the floor before the others, and the new black would be colored. Shadows and Highlights read a luminance and keep the channel ratios, so they need the ratios that are already balanced.

Exposure is a multiply by one number. White balance is a 3×3 matrix. Both are linear, so they commute. The order between those two does not change the result.

## Algorithm

Work on scene-linear sRGB, white = 1. The adjustment is a chromatic adaptation: a neutral under the source white is moved to the destination white, and every other color is moved by the same transform.

### Source white

The source white is the daylight locus point at 153.75 mired (about 6504 K), which is the locus point closest to D65. In CIE xy that is about (0.3127, 0.3291). Using that locus point, rather than the D65 constant (0.3127, 0.3290), makes a slider value of 0 give a destination identical to the source, so the map is the identity at 0 and continuous on either side of it. The two whites differ by about 10⁻⁴ in y.

### Temperature

Let `t = temperature / 100`, in `[−1, 1]`. The slider is linear in mired (micro reciprocal degrees, `10⁶ / K`). Equal mired steps look like roughly equal shifts in warmth. Equal kelvin steps do not.

```
mired = 153.75 + 50 · t
K     = 10⁶ / mired
```

`t = +1` gives 203.75 mired, about 4908 K. `t = −1` gives 103.75 mired, about 9639 K. The destination is a lower color temperature for a warmer picture. The neutral is moved toward that light's yellow.

The point on the daylight locus at `K` (CIE 15, valid from 4000 K to 25000 K, clamp `K` to that range):

```
x = −4.6070e9 / K³ + 2.9678e6 / K² + 0.09911e3 / K + 0.244063    for K ≤ 7000
x = −2.0064e9 / K³ + 1.9018e6 / K² + 0.24748e3 / K + 0.237040    for K > 7000
y = −3.000 · x² + 2.870 · x − 0.275
```

### Tint

Let `n = tint / 100`, in `[−1, 1]`. Tint moves the destination off the locus, along the locus normal at `K`.

The tangent is the finite difference from `K` to `K + 50`, normalized to unit length `(dx, dy)`. The normal is that tangent turned 90°. Of the two normals, take the one whose y component is negative. Lower y in CIE xy is the magenta side of the locus, and higher y is the green side.

```
(px, py) = unit normal with py < 0
x' = x + 0.02 · n · px
y' = y + 0.02 · n · py
```

`0.02` in xy at `n = ±1` is a strong cast, about what a fluorescent tube or a green-tinted window gives. Tune it on pictures.

### Adaptation

The destination white is `(x', y')`. Convert both whites to XYZ with `Y = 1`:

```
X = x / y
Y = 1
Z = (1 − x − y) / y
```

Bradford cone response:

```
     ⎡  0.8951   0.2664  −0.1614 ⎤
B =  ⎢ −0.7502   1.7135   0.0367 ⎥
     ⎣  0.0389  −0.0685   1.0296 ⎦
```

```
ρ_s = B · white_source
ρ_d = B · white_destination
A   = B⁻¹ · diag(ρ_d / ρ_s) · B
```

The pixel transform composes the sRGB primaries around that adaptation. The sRGB-to-XYZ matrix is the IEC 61966-2-1 one, with a D65 white:

```
M = XYZ_to_sRGB · A · sRGB_to_XYZ
RGB' = M · RGB
```

`M` is computed once per run. Each pixel is one 3×3 multiply.

The source white maps exactly to the destination white, and both have `Y = 1`. A grey `(g, g, g)` therefore keeps `Y = g` and takes the destination's chromaticity.

### Range

A channel that comes out below 0 is written as 0. This happens to saturated colors near the gamut edge, such as a pure blue under a strong warm shift. NaN and infinity are written as 0. Values above 1 stay above 1, as headroom, as in every linear stage.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `temperature` | Slider, −100…+100. Right warms | 0 |
| `tint` | Slider, −100…+100. Right magenta, left green | 0 |
| zero point | Source white, daylight locus at 153.75 mired | ≈ 6504 K |
| mired span | Mired offset at ±100 | ±50 |
| tint span | xy offset along the locus normal at ±100 | 0.02 |
| CAT | Chromatic adaptation | Bradford |
| locus | Daylight locus, clamped | 4000–25000 K |

No eyedropper. A click-to-neutral picker would solve for the two sliders from a picked pixel. It is a separate idea, not part of this pair.

## Failure cases worth testing against

- Both sliders at 0. The linear image is unchanged, and the stage is skipped.
- Linear grey `(0.18, 0.18, 0.18)` at Temperature +50. R is above B, and the Rec. 709 luminance of the result is still 0.18. A luminance that moves means the whites were not normalized to `Y = 1`.
- Linear white `(1, 1, 1)` at Temperature +100. It lands on the 4908 K white, with `Y = 1`.
- Grey at Tint +100. G is below both R and B. G above them means the normal points the wrong way.
- Temperature −100 then +100, as two separate runs from the baseline. The casts are opposite: blue, then yellow.
- The matrix applied to encoded values. Shadows and highlights take different casts on a grey ramp, and a ramp that should be evenly tinted is not.
- White balance after Blackpoint. On a warm photo, the blue channel reaches the floor first and the black turns yellow-brown.
- Exposure +1 then white balance, compared with white balance then Exposure +1, with no clamp between them. The results match.
- Pure linear blue `(0, 0, 1)` at Temperature +100. R and G come out at or above 0. A negative channel in the buffer means the guard was skipped.

## Sources

- Affinity Photo 2 help, "Basic panel" (Develop Persona). White Balance group, with Temperature and Tint.
- Adobe, "Make color and tonal adjustments in Camera Raw." White balance: Temperature and Tint. On JPEG and TIFF files the sliders are relative, −100 to +100.
- CIE 15:2004, Colorimetry. The daylight locus in xy as a function of correlated color temperature, 4000–25000 K.
- K. M. Lam, "Metamerism and Colour Constancy," PhD thesis, University of Bradford, 1985. The Bradford cone-response matrix.
- IEC 61966-2-1, sRGB. The primaries and D65 white behind the sRGB ↔ XYZ matrices.
- Bruce Lindbloom, "Chromatic Adaptation." The `B⁻¹ · diag(ρ_d / ρ_s) · B` construction and the Bradford inverse.
