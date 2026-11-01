# White balance

Status: idea. Not part of the viewer.

White balance is the correction for a phone JPEG whose whites lean warm or cool, or toward green or magenta. The phone already balanced the file. These sliders move that result. They are not a raw illuminant.

## What the sliders do

Two strengths, each default 0, each from −100 to +100.

Temperature warms and cools. Above 0 the picture warms. Below 0 it cools. A neutral moves toward amber or toward blue, and the two ends of the slider are the same size of move.

Tint shifts the same neutral toward magenta or green. Above 0 adds magenta. Below 0 adds green.

At 0 on both, every sample stays where it is. The stage is skipped.

## Where it sits

Orient has already turned the picture. White balance runs next, on the linear buffer, before exposure. Exposure multiplies all three channels by the same factor, so it does not change the balance these sliders set. A grey stays grey under exposure. It does not stay grey under temperature or tint.

The sliders sit at the top of the Color group. Vibrance and saturation come later and work on encoded channels.

## The matrix

The buffer is linear sRGB. The move is a chromatic adaptation of D65 toward a shifted daylight white, so a pixel that was neutral takes on that white. Headroom above 1 is kept. A result below 0 is written as 0, by the buffer rule.

D65 is the sRGB white, `x = 0.3127`, `y = 0.3290`, `Y = 1`.

```
X = x / y
Z = (1 − x − y) / y
```

Temperature `t = temperature / 100` shifts that white along the daylight locus by `50 · t` mireds. A mired is `10^6 / T` with `T` in kelvin. D65 is 6504 K, about 153.75 mired. Positive `t` adds mireds, which lowers the kelvin and warms the white. The ends are about 4900 K and 9600 K.

```
mired = 153.75 + 50 · t
T = 10^6 / mired
```

The daylight chromaticity at `T` is the CIE daylight locus. For `T` from 4000 K to 7000 K:

```
x = −4.6070e9 / T^3 + 2.9678e6 / T^2 + 0.09911e3 / T + 0.244063
```

For `T` from 7000 K to 25000 K:

```
x = −2.0064e9 / T^3 + 1.9018e6 / T^2 + 0.24748e3 / T + 0.237040
```

```
y = −3 x^2 + 2.870 x − 0.275
```

Tint `n = tint / 100` moves that `(x, y)` perpendicular to the locus. The locus direction is `(x, y)` at this `T` minus `(x, y)` at a mired one step warmer. Rotate that direction 90° and pick the side whose `y` component is negative. That side is magenta. The shift is `0.02 · n` along that unit vector. Positive tint lowers `y` and the white goes magenta. Negative tint raises `y` and the white goes green.

The adapted white is that `(x, y)` with `Y = 1`, converted to XYZ as above.

Linear sRGB becomes XYZ, the Bradford matrix adapts D65 to the shifted white, and XYZ comes back to linear sRGB. Bradford, with its inverse:

```
       0.8951  0.2664 −0.1614
B    = −0.7502  1.7135  0.0367
        0.0389 −0.0685  1.0296

        0.9869929 −0.1470543  0.1599627
B⁻¹  =  0.4323053  0.5183603  0.0492912
       −0.0085287  0.0400428  0.9684867
```

`ρ_s` is `B` times the XYZ of D65. `ρ_d` is `B` times the XYZ of the shifted white. The adaptation is `B⁻¹ · diag(ρ_d / ρ_s) · B`, applied to XYZ. The sRGB matrix and its inverse are the IEC 61966-2-1 primaries.

Both sliders at 0 make the shifted white equal D65, the diagonal is 1, and the stage is skipped.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `temperature` | Warm against cool, −100…+100. Positive warms | 0 |
| `tint` | Magenta against green, −100…+100. Positive adds magenta | 0 |
| mired span | How far ±100 sits from D65 | 50 mired |
| tint span | How far ±100 sits off the locus | 0.02 in xy |

## Failure cases worth testing against

- Both sliders at 0. The linear buffer is unchanged.
- A grey ramp, temperature +100. The ramp leans amber and stays smooth. A ramp that leans blue means the mired sign is reversed.
- The same ramp, tint +100. It leans magenta. A green ramp means the perpendicular was taken the other way.
- Temperature +100 and tint 0 on a neutral patch. Red is above blue. Green stays between them.
- A sample already above 1. It is still above 1 after a small temperature move. A clamp at 1 means the stage treated the buffer as display-referred.

## Sources

- CIE 15. The daylight locus used for `x` and `y` from correlated color temperature.
- Lam, K. M. “Metamerism and Colour Constancy.” University of Bradford, 1985. The Bradford matrix used for the adaptation.
- IEC 61966-2-1. The sRGB primaries and the D65 white the buffer is encoded against.
