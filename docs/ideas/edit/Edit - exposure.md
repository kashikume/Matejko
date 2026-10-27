# Exposure

Status: idea. Not part of the viewer.

Exposure is the control Affinity Photo puts in Develop Persona on the Basic panel, in the Exposure group, next to Brightness and Blackpoint. Current help calls that workspace Develop Studio. The same slider is the Exposure adjustment. Its help text says it controls highlight and shadow levels together, across the whole frame: drag left to increase shadows, drag right to increase highlights. Left corrects overexposure. Right corrects underexposure.

Affinity has not published the kernel. A Develop tutorial calls this control a linear exposure adjustment and names the unit: stops. ISO 2720 defines one step as a factor of two in exposure. Scene-linear samples scale with exposure, so the step is a multiply. That multiply is what to implement.

## What the slider does

One strength, default 0, in stops. `ev = 0` leaves every sample where it is. Help does not publish the numeric ends. The Develop slider has been reported from −5 to +5, and a Photo 2 report gives steps of 0.1 up to +5. Use that travel on the slider. The multiply is the same function at every value, including past those ends.

A stop moves every tone by the same factor.

- Above 0, the frame gets brighter by `ev` stops. At one stop, 18% grey goes from linear 0.18 to 0.36, and a highlight at linear 0.7 goes to 1.4.
- At 0, the picture is unchanged.
- Below 0, the frame gets darker by the same number of stops. At one stop down, 0.18 goes to 0.09, and 0.7 goes to 0.35.

Black stays 0, because the factor multiplies it. A neutral stays neutral: the three channels take the same factor, so a grey's ratios stay put. A channel that clips loses its ratio, and the hue shifts toward the channels that are still inside the range. Linear (0.8, 0.4, 0.2) at `ev = 1` is (1.6, 0.8, 0.4) while those values are kept. Capped at 1 it becomes (1, 0.8, 0.4). Red was twice the green. After the cap it is 1.25 times the green.

## Headroom

On scene-linear raw data, a positive `ev` uses range the samples still have above the current exposure. 0.7 becoming 1.4 is still a real sample when the buffer can hold it. The highlight meets the display white later, in the display transform, if it is still above 1 there. A photosite already saturated in the file has nothing above the clip. A further positive stop leaves it saturated.

On a display-referred buffer already limited to 0..1, the same stop has nowhere above 1 to land. At `ev = 1`, every linear value from 0.5 upward sits on 1. Mid grey 0.18 becomes 0.36 and still fits. The histogram slides until it piles up on white.

## Where it sits next to Brightness

Exposure and Brightness sit in the same group because a stop and a gamma are different pictures.

Exposure multiplies scene-linear light by one factor. Shadow and highlight move by the same number of stops. A positive stop can carry a highlight through 1.

Brightness is a gamma on encoded RGB. It bends code values and holds encoded 0 and encoded 1. A stop and a brightness setting that agree on a mid grey come apart on a white shirt: the shirt clips under the stop, and the gamma leaves the shirt at white while the middle of the ramp moves.

Blackpoint is the other control in the group. It chooses which input values count as pure black. Exposure leaves an input of 0 at 0.

Shadows and Highlights, the next stage, move the dark end and the light end by different amounts. Exposure has already slid both ends by the same number of stops.

Order, shared with the other notes: Exposure on scene-linear samples, then Blackpoint, then Shadows and Highlights, then Brightness on encoded channels, then Contrast, then Vibrance, then Saturation, then Clarity.

## Linear light

A stop is a factor of two in light. A gamma-encoded code value is a different quantity, and a factor of two on that code value is a different change from one stop of light.

Encoded mid grey in sRGB is the code value 0.5. The light under it is about 0.214. One stop of that light encodes back to about 0.69. Doubling the code value sends 0.5 to 1, which is white, and the highlight is already gone.

When the buffer is still encoded, decode, multiply, and encode. The multiply in the middle is the stop. The encode and decode are the document transfer, such as sRGB.

```
y = encode(decode(x) · 2^ev)
```

## Algorithm

Apply the factor to each channel of scene-linear RGB, before encoding. Chroma is not separated out. Equal channels stay equal, which is why a grey stays grey until a channel clips.

Let `X` be one scene-linear channel and `ev` the slider in stops.

```
Y = X · 2^ev
```

`ev = 0` gives `Y = X`. `X = 0` gives `Y = 0` for every `ev`. For a fixed `ev` the map is monotonic.

### A buffer already in 0..1

The display-referred path caps the result at 1:

```
Y = min(X · 2^ev, 1)
```

The cap binds when `X · 2^ev` exceeds 1. Scene-linear samples keep a result above 1. That retained range is the headroom a later positive stop can still use.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `ev` | The slider, in stops. Reported travel −5…+5. Help does not publish the ends. | 0 |
| `X` | One scene-linear channel | `Y = X` at `ev = 0` |
| black | Linear 0 | stays 0 |
| channels | What the factor is applied to | R, G, and B independently |
| cap | Display-referred buffer already in 0..1 | clip at 1 |

No radius. A curve that holds encoded 0 and 1 and bends the middle is Brightness.

## Failure cases worth testing against

- A grey ramp, scene-linear 0 to 1. `ev = 1` keeps 0 at 0, doubles the rest, and stays monotonic. On a 0..1 buffer, everything from 0.5 upward sits on 1. On a buffer with headroom, the top end is 2.
- A known 18% grey patch, linear 0.18 on all three channels. `ev = 1` lands on 0.36. `ev = −1` lands on 0.09. The channels still match.
- A clipped highlight. Linear (0.8, 0.4, 0.2) at `ev = 1` is (1.6, 0.8, 0.4) while headroom holds. Capped at 1 it is (1, 0.8, 0.4), and the red-to-green ratio has moved from 2 to 1.25.
- The multiply applied after gamma, to code values. Encoded 0.5 at `ev = 1` becomes 1. Decoded first, the same stop encodes to about 0.69. A result of white means the factor ran on code values.

## Sources

- Affinity Help Center, “Exposure adjustment”. The slider controls highlight and shadow levels of the composition globally. Left increases shadows and corrects overexposure. Right increases highlights and corrects underexposure. The page publishes neither a numeric range nor a formula. The adjustment is also available from the Basic panel in Develop Studio.
- Affinity Help Center, “Basic panel (Develop Studio)”. The Exposure section covers overall exposure, the black point, and overall brightness.
- Affinity Help Center, “Editing tonality in Develop Studio (RAW)”. Exposure sets overall brightness. Blackpoint sets the darkest tone. Brightness works the midtones.
- Affinity Photo, “RAW Development” (2022). The Basic panel’s exposure control is a linear exposure adjustment, in stops.
- ISO 2720:1974, clause 3.1.5. A change of one exposure value unit requires a change in exposure by a factor of 2. That unit is a step.
- Reported slider travel, not a published kernel: the Affinity forum topic “Where are Levels?” describes the Exposure slider as −5 to +5. An Affinity Photo 2 report describes steps of 0.1 up to 5.0.
