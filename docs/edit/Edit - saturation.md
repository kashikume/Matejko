# Saturation

Status: idea. Not part of the viewer.

Saturation is the uniform chroma control Affinity Photo puts in Develop Persona on the Basic panel, inside Enhance, next to Contrast, Clarity, and Vibrance. The same slider is Saturation on the Vibrance adjustment. The Basic panel help states the group directly: Enhance adjusts contrast, clarity, saturation, and vibrance. The Vibrance adjustment help states the slider, and it says that adjustment is also on the Basic panel inside Enhance: saturation controls the intensity of colors equally, and clipping can occur. Drag left to reduce intensity. Drag right to boost it.

Serif has not published the kernel. The scale to implement multiplies chroma about the neutral axis by one factor, for every pixel.

## What the slider does

One strength, default 0, range −100 to +100, the same range as Contrast and Clarity in Enhance.

- Above 0, every color moves farther from the neutral axis. Channels that leave [0, 1] clamp.
- At 0, the picture is unchanged.
- Below 0, every color moves toward the neutral axis. At −100 the frame is grey, and the grey of each pixel is its luma.

This factor is the same for a muted color and a fully saturated one. Vibrance, in the companion note, is the control that moves a muted color more than a saturated one.

## Where it sits

Pipeline, in the order the other notes use:

1. Exposure on scene-linear samples
2. Blackpoint
3. Shadows and Highlights (Capture One style)
4. Brightness, then Contrast, on encoded RGB
5. Vibrance, then Saturation
6. Clarity on luminance

Saturation is the last chroma step. It runs after Contrast because Contrast already spreads the channels, and that spread changes chroma. Saturation is the explicit scale after that. Clarity then works on luminance.

## Algorithm

Apply the scale to encoded RGB. Encoded means the document transfer, such as sRGB, the same encoding Contrast uses, where a neutral is R = G = B. On scene-linear samples, encode first.

Rec. 709 specifies the weights below for linear light. This control applies them to the encoded channels, so the axis is the neutral those channels already share.

### Luma scale

Let `R`, `G`, `B` be the encoded channels, `s` the slider, and `a = s / 100`.

```
Y = 0.2126 R + 0.7152 G + 0.0722 B
C′ = (C − Y) · (1 + a) + Y
```

`C` is each channel in turn. Clamp each `C′` to `[0, 1]`.

`a = 0` is identity. `a = −1` pulls every pixel onto `Y`, a grey image of the same luma. `a = 1` doubles the distance from the neutral axis, and channels that leave `[0, 1]` clamp. On a pixel that stayed inside `[0, 1]`, the Rec. 709 luma of the result is still `Y`. A channel that clamped can pull that luma, and it can pull hue.

### HSV scale

The older form scales HSV saturation and leaves hue and value.

Let `M = max(R, G, B)`.

```
C′ = M − (M − C) · (1 + a)
```

Clamp each `C′` to `[0, 1]`. `a = 0` is identity. The largest channel stays at `M`. The channels under it keep their order, so hue holds until a channel clamps.

The luma scale is the one to code. HSV value is `max(R, G, B)`, so scaling `S` darkens saturated colors as a side effect: the peak stays and the lower channels fall. At `a = −1` a pure red becomes white, because its value is 1. The luma form holds the Rec. 709 luma of unclamped pixels, and that red becomes the grey `0.2126`.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `s` | The slider, −100…+100 | 0 |
| `a` | `s / 100` | −1…+1 |
| `Y` | Rec. 709 luma of the encoded channels | `0.2126 R + 0.7152 G + 0.0722 B` |
| scale | Factor on `(C − Y)`, `1 + a` | 1 |
| channels | What the scale is applied to | R, G, and B |

No radius. A control that should move a muted color more than a saturated one is Vibrance.

## Failure cases worth testing against

- A neutral, `R = G = B`, stays that neutral at every strength. A channel that moves was measured from some other axis.
- `a = −1`. Every pixel is grey and equals its own `Y`. Pure red `(1, 0, 0)` lands on `0.2126`. A result of white is the HSV scale. A grey of one third is an equal-weight average of the three channels.
- A primary at `a = 1`. Unclamped red is about `(1.787, −0.213, −0.213)`. After the clamp it is `(1, 0, 0)`. A negative channel left in the buffer means the clamp was skipped.
- Hue of an unclamped color. `(0.6, 0.5, 0.4)` at `a = 1` stays inside `[0, 1]`, the order stays R, then G, then B, and each deviation from `Y` doubles.
- Saturation before Contrast. Contrast spreads the channels again, and the frame disagrees with contrast-then-saturation on the same numbers.

## Sources

- Affinity Photo 2 help, “Basic panel”. Enhance adjusts contrast, clarity, saturation, and vibrance.
- Affinity Photo 2 help, “Enhance adjustment”. Saturation increases or decreases color saturation. Drag left to desaturate. The group appears on the Basic panel.
- Affinity Photo 2 help, “Vibrance adjustment”. Saturation controls the intensity of colors equally and allows clipping. Drag left to reduce intensity, drag right to boost it. The adjustment is also on the Basic panel, inside Enhance.
- ITU-R BT.709, the luma equation `Y = 0.2126 R + 0.7152 G + 0.0722 B`, specified there for linear light.
- W3C, SVG 1.1, `feColorMatrix` type `saturate`. The same construction with weights rounded to `0.213`, `0.715`, `0.072`. Their `s` is 1 at identity and 0 at grey, which is `1 + a` here.
- Alvy Ray Smith, “Color Gamut Transform Pairs”, SIGGRAPH 1978. HSV value is `max(R, G, B)`. Saturation is the relative distance from that maximum down to the minimum.
