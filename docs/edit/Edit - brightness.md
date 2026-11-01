# Brightness

Status: idea. Not part of the viewer.

Brightness is the lightness control Affinity Photo puts in Develop Persona on the Basic panel, in the Exposure group, next to Exposure and Blackpoint. The same idea is the Brightness slider of the Brightness / Contrast adjustment. Both help texts say the same thing about the direction: drag left to darken, drag right to lighten.

Serif has not published the curve. The adjustment does publish the split that matters, via the Linear checkbox, and it is the same split as Contrast. The default changes a pixel relative to its own lightness and keeps the ends from clipping. Linear is an absolute offset, and clipping can occur. The curve to implement is the default.

## What the slider does

One strength, default 0. On the Brightness / Contrast adjustment the range is −100 to +100, the same range as Contrast.

Black stays black. White stays white. Everything between them moves, and the middle moves the most.

- Above 0, the frame gets lighter. Shadows open up. Highlights stay under the white point instead of piling onto it.
- At 0, the picture is unchanged.
- Below 0, the frame gets darker. Highlights come down. Shadows stay above a crushed black.

A neutral stays neutral, because the same curve runs on each channel. A pure primary stays a pure primary: a channel sitting on 0 or 1 does not leave it. Colors in between shift a little in saturation as the channels take different steps along the curve. Hue of a grey does not move.

## Where it sits next to Exposure

Exposure and Brightness both make a picture lighter or darker. They are different curves, which is why the Basic panel has both.

Exposure is a gain in scene-linear light, measured in stops:

```
Y = X · 2^ev
```

Every value, shadow and highlight, moves by the same number of stops. Black stays 0. Anything pushed past the white point clips. The histogram slides sideways until it hits an end. Affinity's help describes that slider as the one that controls highlight and shadow levels together: left increases shadows, right increases highlights.

Brightness does not slide the histogram. It bends the encoded values, leaves the two ends where they are, and spends the change in the middle. A stop of exposure and a notch of brightness that look similar on a mid grey do not look similar on a white shirt.

Blackpoint is the third control in that group. It decides which input values count as pure black, and it does move the dark end. Brightness leaves that end alone, so the two sliders do not fight.

Order, matching the panel: Exposure on linear samples, then Blackpoint, then Brightness on encoded channels, then Contrast, then Clarity. Brightness runs before Contrast so the contrast pivot at encoded 0.5 is applied to the lightened or darkened code values. GIMP's brightness-contrast map does those two in the same order.

## Algorithm

Apply a point curve to each channel of encoded RGB. Encoded means the document transfer, such as sRGB, where the code value 0.5 is mid grey. On scene-linear samples, encode first and decode after. The same exponent on linear light is a different picture: the shadows move far too much, and it starts to behave like a second exposure control.

### Default curve

The default holds 0 and 1 and is identity at strength 0. A gamma is the one-parameter curve with that behavior.

Let `x` be one encoded channel in `[0, 1]`, and `a = s / 100` the slider in `[-1, 1]`.

```
γ = 2^(−a)
y = x^γ
```

`a = 0` gives `γ = 1` and `y = x`. `x = 0` and `x = 1` stay put for every `a`. The curve is monotonic.

- `a = 1` gives `γ = 0.5`. Encoded mid grey, 0.5, rises to about 0.71.
- `a = −1` gives `γ = 2`. Encoded mid grey falls to 0.25.

RawTherapee's Lightness slider is this same contract, built as a NURBS instead of a power: the black point and the white point are fixed, a toe and a shoulder move with the slider, and the curve is applied per channel. The power above is the closed form to code. It is not their spline.

### The blend that moves one end

GIMP's brightness, in the same function as its contrast slope, is a different non-clipping map. With `b = a / 2` (the slider is stored as −1…+1 and the operation uses half of it):

```
y = x + (1 − x) · b    if b ≥ 0
y = x · (1 + b)        if b < 0
```

Brightening pins white and lifts black. At the top of the slider `b = 0.5`, so black becomes mid grey and white stays white. Darkening pins black and pulls white down. Nothing leaves `[0, 1]`, so it does not clip, but one end moves. Next to a Blackpoint slider that end is no longer the black point. That is why the gamma is the default here and this blend is not.

### Linear offset

Linear on, in the help's words, modifies pixels by absolute values, and clipping may occur.

```
y = clamp(x + a, 0, 1)
```

`a = 0.5` (slider +50) sends every encoded value above 0.5 to white. `a = −0.5` sends every value below 0.5 to black. The shift is the same number of code values at every tone, which is the absolute change the checkbox selects. It is the wrong default for the Develop slider: a modest positive value already spends the highlights.

When Linear is on for the whole Brightness / Contrast adjustment, apply this offset first and the linear contrast slope second, then clamp once.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `s` | The slider, −100…+100 | 0 |
| `a` | `s / 100` | −1…+1 |
| `γ` | Exponent, `2^(−a)` | 1 |
| ends | Encoded black and white | fixed at 0 and 1 |
| channels | What the curve is applied to | R, G, and B independently |

No radius. A curve that should move the black end is Blackpoint. A curve that should move every tone by the same number of stops is Exposure.

## Failure cases worth testing against

- A grey ramp. Both ends stay, the midpoint moves, and the ramp stays monotonic. A reversed or clipped end means the linear offset was used.
- Encoded mid grey at `a = 1` lands near 0.71, and at `a = −1` lands at 0.25.
- A neutral grey stays neutral. Pure red, with channels on 0 and 1 only, stays pure red.
- The same mid-grey lift done with Exposure instead. The white end of the ramp clips, and a deep shadow moves by the same number of stops as the mid grey. Brightness does neither.
- Gamma applied to scene-linear samples without encoding. The shadows open or crush much harder than the same slider on encoded values.
- Brightness after Contrast. The contrast pivot has already spread the tones, and the lightness change no longer matches brightness-then-contrast on the same numbers.

## Sources

- Affinity Photo 2 help, “Brightness / Contrast adjustment”. Brightness controls lightness. With Linear off, pixels change relative to their lightness and the ends are kept from clipping. With Linear on, the change is absolute and clipping can occur.
- Affinity Photo help, Exposure section of the Develop Basic panel. Brightness is lightness. Exposure is the combined highlight and shadow level. Blackpoint chooses what counts as pure black.
- GIMP, `gimpoperationbrightnesscontrast.c`. Brightness before contrast. The brightness half is the blend above, with the slider halved.
- RawTherapee, `rtengine/curves.cc`, and RawPedia, “Lightness”. A per-channel tone curve whose black point and white point stay put.
