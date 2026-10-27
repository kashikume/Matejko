# Vibrance

Status: idea. Not part of the viewer.

Vibrance is the non-uniform chroma control Affinity Photo puts in Develop Persona on the Basic panel, inside Enhance, next to Saturation. Photoshop has the same name on the Vibrance adjustment. Camera Raw and Lightroom have it as the Vibrance slider. Capture One has no slider under that name. The Saturation slider in its Exposure tool, and the Saturation slider in the Basic Color Editor with the multi-color patch selected, are the controls its support article describes as that approach.

Affinity's Basic panel help names the Enhance group and stops: contrast, clarity, saturation, and vibrance. Serif has not published the kernel. Adobe has not published it either. Camera Raw's help says the control changes lower-saturated colors with less effect on higher-saturated colors, so a color is held back as it nears full saturation, and that skin tones are kept from becoming oversaturated. The weight below is the one to implement.

## What the slider does

One strength, default 0, range −100 to +100.

Chroma here is `max(R, G, B) − min(R, G, B)` on encoded RGB in `[0, 1]`. The weight depends on that chroma, on the sign of the slider, and on a skin term.

- Above 0, muted colors take the gain. A grey is eligible for the full gain and stays grey, because there is no chroma to scale. A fully saturated pixel takes none. An orange that is already colorful takes less than another hue of the same chroma, so a face stays off full red while a muted blue beside it moves.
- At 0, the picture is unchanged.
- Below 0, the weight flips. The strongest colors take the loss. A fully saturated pixel is pulled toward its luma. A near-grey is left alone.

That flip is the control. Saturation multiplies every pixel's chroma by the same factor, with no skin term and no dependence on the chroma already there.

Capture One's support article splits the signs of the Exposure tool's Saturation slider on its own terms. Dragging right leaves sufficiently saturated areas alone, which is the behavior they compare to vibrance, including skin. Dragging left desaturates every color equally. darktable's current global vibrance keeps one chroma gate for both signs, so low chroma moves more whichever way the slider goes. The negative half of this slider is the flip. The failure cases check that flip.

## Where it sits

Tone is settled before this weight reads the pixel:

1. Exposure, then Blackpoint, then Shadows/Highlights, then Brightness, then Contrast.
2. Vibrance, then Saturation.
3. Clarity, on luminance.

Contrast is a per-channel curve, so the channels have already spread when vibrance runs. Saturation comes second, and its one factor is the last scale on chroma. A skin tone that vibrance held can still move if Saturation is then raised. Clarity runs on luminance after both, so the chroma decision is already made.

## Algorithm

Work on encoded RGB in `[0, 1]`. Encoded means the document transfer, such as sRGB. Hue in that encoding is the hue a painter means by orange. The same arithmetic on scene-linear samples calls a different set of pixels "skin" and a different set "muted."

### Luma and chroma

```
Y = 0.2126 R + 0.7152 G + 0.0722 B
chroma = max(R, G, B) − min(R, G, B)
```

`Y` is Rec.709 luma on the encoded channels. `chroma` is 0 for a grey and 1 for a pixel with one channel at 0 and another at 1.

Hue is the usual HSV angle in degrees, and it is computed only when `chroma > 0`. On a tie the greatest channel is taken in the order R, then G, then B.

```
if R is greatest:  h = (G − B) / chroma
if G is greatest:  h = 2 + (B − R) / chroma
if B is greatest:  h = 4 + (R − G) / chroma
h = h mod 6
if h < 0: h = h + 6
hue = 60 · h
```

### Skin

`skin` is 0 for most hues. It rises toward 1 for a saturated orange, and it is multiplied by `chroma`, so a grey sitting on an unstable orange hue is not treated as skin. A pale orange is only lightly protected. A saturated orange is protected strongly. That is the whole mask: hue selects the band, chroma selects how saturated the pixel already is.

The band is a raised cosine from −10° to 60°, peak at 25°. Wrap `hue` into (−180°, 180°] first, so a red just below 0° is inside the window:

```
u = hue
if u > 180: u = u − 360

if chroma = 0 or u ≤ −10 or u ≥ 60:
  skin = 0
else:
  t = (u − (−10)) / (60 − (−10))
  skin = chroma · (0.5 − 0.5 · cos(2π · t))
```

`t` is 0 at −10° and 1 at 60°. The cosine term is 0 at both ends and 1 at 25°. Those two bounds are a starting point to judge on portraits. A face that still goes red wants a wider window or a peak moved off 25°. A yellow dress that will not saturate wants the upper bound brought down.

### Gain

Let `a = s / 100`, so the slider is `a` in `[−1, 1]`.

```
weight = (1 − chroma) · (1 − skin)    if a ≥ 0
weight = chroma · (1 − skin)          if a < 0

gain = 1 + a · weight
C′ = (C − Y) · gain + Y
```

Apply that to R, G, and B with the same `gain`, then clamp each channel to `[0, 1]`.

`a = 0` gives `gain = 1` on either branch. The same gain on every channel keeps the direction of `(R − Y, G − Y, B − Y)`, so hue holds until a channel hits the clamp. Positive gain can push a channel that was already near 1 out of range; the clamp is what stops it, and a pixel that clips on one channel only will shift hue. Negative gain is at most 1 and at least 0, so it pulls toward `Y` and stays inside the original channels.

At `chroma = 1` the positive weight is 0 for every hue, skin included. The skin term shows on the positive slider when chroma is high and still below 1, and it shows on the negative slider even at chroma 1: a primary is pulled down, a saturated orange in the window is not.

A blue at chroma 0.8 and `a = 1` has `skin = 0`, `weight = 0.2`, `gain = 1.2`. An orange at hue 25° and the same chroma has `skin = 0.8` and `gain = 1.04`.

### Published weights

Two open implementations were fetched. Both are real, and both differ from the signed weight above. The Enhance slider codes the signed weight.

darktable's deprecated module `src/iop/vibrance.c` runs in Lab. The slider is 0 to 100, default 25, and it has no negative half. With `amount` equal to the slider divided by 100, and `a`, `b` the Lab chroma channels:

```
sw = sqrt(a² + b²) / 256
ls = 1 − (amount · sw) · 0.25
ss = 1 + amount · sw
```

`L` is multiplied by `ls`. `a` and `b` are multiplied by `ss`. The source comment calls `sw` a saturation weight. `sw` rises with chroma, so the most saturated pixels take the larger chroma gain, and they lose lightness. The module text says it saturates and reduces the lightness of the most saturated pixels. The file tells the user to use the vibrance slider in color balance rgb.

That current slider is in `src/iop/colorbalancergb.c`. The parameter is in `[−1, 1]`. On their Ych chroma, the scene-referred linear chroma of that module:

```
vibrance = amount · (1 − chroma ^ |amount|)
chroma_factor = max(1 + chroma_boost + vibrance, 0)
chroma = chroma · chroma_factor
```

`chroma_boost` is the separate global-chroma control. The absolute value on the exponent keeps the same gate for a positive and a negative amount: as chroma falls, the added term grows. The manual describes global vibrance as prioritizing colors with low chroma. The slider tooltip says it increases colorfulness mostly on low-chroma colors. There is no skin term on this line.

RawTherapee's `rtengine/ipvibrance.cc` is a third algorithm, with separate pastel and saturated strengths. Its protect-skins path calls `Color::SkinSat`. The constants beside that path are a hue-remap window, `skbeg = −0.05` and `skend = 1.60` radians (about −3° to 92°), with a transition `dhue = 0.15` radians. That window is not the raised cosine above.

GEGL 0.4.68 added `gegl:vibrance` for PSD adjustment layers. The merge request describes a meta-operation that chains `gegl:saturation` and `gegl:hue-chroma`. No per-pixel weight was published in that description.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `s` | The slider, −100…+100 | 0 |
| `a` | `s / 100` | −1…+1 |
| `Y` weights | Rec.709 luma on encoded RGB | 0.2126, 0.7152, 0.0722 |
| `chroma` | `max − min` of the encoded channels | per pixel |
| hue window | Raised-cosine skin band, degrees | −10 to 60, peak 25 |
| `skin` | That cosine, times `chroma` | 0 outside the window |

No radius. A second user-facing parameter would be Saturation, which is the uniform scale and runs after this one.

## Failure cases worth testing against

- A grey, `R = G = B`, at +100 and at −100. The pixel is unchanged. A hue read off a zero chroma and then treated as skin will move it.
- A fully saturated primary, chroma 1, hue well outside the window, at +100. The pixel is unchanged. Any move means the positive weight was not `(1 − chroma)`.
- A muted blue, small chroma, hue far from the window, at +100. Chroma scales by `1 + (1 − chroma)`. The blue moves a lot next to the primary, which does not move.
- An orange at hue 25° and a blue of the same chroma. The orange's `|gain − 1|` is the blue's multiplied by `(1 − skin)`. At chroma 1 and a positive slider both gains are 1, so that pair does not show the mask. At chroma 1 and −100 the blue falls to its luma `(0.0722, 0.0722, 0.0722)` for pure blue, and the orange at `(1, 0.417, 0)` stays put.
- −100 on a fully saturated primary with `skin = 0`. Gain is 0 and the pixel becomes its luma. A grey beside it is unchanged. If the grey moves, the negative weight was `(1 − chroma)` and the sign never flipped.
- The same +100 on scene-linear samples with this encoded-RGB window. Skin and "muted" land on the wrong pixels.
- Saturation before Vibrance. The uniform scale has already changed the chroma the skin gate reads, and a later vibrance cannot be the last word.
- Clarity before Vibrance. Local luminance contrast and the chroma weight are interleaved. The order is tone, then vibrance, then saturation, then clarity on luminance.

## Sources

- Affinity Photo 2 help, “Basic panel” (Develop Persona). Enhance adjusts contrast, clarity, saturation, and vibrance. https://affinity.help/photo2/English.lproj/pages/Raw/raw_panelBasic.html
- Adobe, “Make color and tonal adjustments in Camera Raw.” Vibrance changes lower-saturated colors with less effect on higher-saturated colors, limits clipping as colors approach full saturation, and prevents skin tones from becoming oversaturated. https://helpx.adobe.com/camera-raw/using/make-color-tonal-adjustments-camera.html
- Adobe, “Adjust vibrance” (Photoshop). The same statement for the Vibrance adjustment: less-saturated colors increase more than colors that are already saturated, and skin tones are kept from becoming oversaturated. https://helpx.adobe.com/photoshop/using/adjust-vibrance.html
- Capture One support, “Is there a Vibrance slider?” There is no Vibrance slider. Exposure-tool Saturation and Basic Color Editor Saturation (multi-color patch) are the intelligent controls: positive leaves sufficiently saturated areas, negative desaturates all colors equally. https://support.captureone.com/hc/en-us/articles/360016451177-Is-there-a-Vibrance-slider
- darktable, `src/iop/vibrance.c`. Deprecated Lab module. `sw = sqrt(a² + b²) / 256`, `ls = 1 − (amount · sw) · 0.25`, `ss = 1 + amount · sw`. https://github.com/darktable-org/darktable/blob/master/src/iop/vibrance.c
- darktable, `src/iop/colorbalancergb.c`. Current global vibrance: `amount · (1 − chroma ^ |amount|)`, then added into the chroma factor. Tooltip: increase colorfulness mostly on low-chroma colors. Manual, “color balance rgb,” global vibrance prioritizes low chroma: https://docs.darktable.org/usermanual/4.6/en/module-reference/processing-modules/color-balance-rgb/
- RawTherapee, `rtengine/ipvibrance.cc`. Skin-hue window constants `skbeg = −0.05`, `skend = 1.60`, `dhue = 0.15` radians, and `Color::SkinSat` when protect-skins is on. RawPedia, “Vibrance”: https://rawpedia.pixls.us/vibrance/
- GEGL 0.4.68, `gegl:vibrance`. Merge request 244 describes a meta-operation chaining `gegl:saturation` and `gegl:hue-chroma` for PSD adjustment layers. https://gitlab.gnome.org/GNOME/gegl/-/merge_requests/244
