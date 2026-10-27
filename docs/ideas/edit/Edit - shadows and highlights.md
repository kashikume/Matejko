# Shadows and Highlights

Status: idea. Not part of the viewer.

Shadows and Highlights is the Shadow slider and the Highlight slider of Capture One's High Dynamic Range tool, on the Exposure tool tab. The tool compresses the extreme tones of one frame so the picture reads as a wider range. The Highlight slider lowers the brightness of the highlights and recovers detail in overexposed regions. When one channel has clipped, it can use the data in the other channels to reveal detail. The Shadow slider raises the brightness of the shadows and recovers detail that was recorded in the underexposed end. A heavy shadow lift makes noise visible. From Capture One 20 the same two sliders also amplify those ranges, and that direction increases contrast. Low and medium values stay natural. Pushed further, contrast and saturation fall, and the help sends mid-tone contrast to the Clarity tool.

Phase One has not published the kernel. What follows is the documented behavior of those two sliders, then the algorithm that produces it.

## What the sliders do

Two sliders, Shadow and Highlight. Each runs from −100 to +100, default 0, centered. Before Capture One 20 the recovery travel was 0 to 100 and the sliders only recovered. Version 20 starts them in the middle and doubles that travel, recovery one way and contrast the other. On each slider a move to the right brightens that end and a move to the left darkens it.

- Shadow above 0 opens the dark end. On a step wedge at +100 the floor (−7 and −6 EV) barely moves, the change starts near −5 EV, and the strongest lift sits around −4 to −2 EV. A small amount continues past 0 EV.
- Shadow below 0 deepens that same end.
- Highlight below 0 compresses the bright end and restores detail in overexposed regions. Speculars, reflections, and light sources can stay clipped.
- Highlight above 0 brightens that end.
- At 0 on both, the picture is unchanged.

A flat mid grey stays put. A neutral stays neutral. Hue in a sky that clipped in one channel comes back from the channels that did not.

The auto setting is described as having little effect on the midtones when the frame is not grossly over- or underexposed. The basic tool has no radius slider. Edge behavior is internal.

## White and Black

White and Black are the other two sliders in the High Dynamic Range tool, added in Capture One 20, for the darkest shadows and the brightest highlights. On the same wedge, Black moves the −7 to −4 EV band that Shadow leaves alone, and White moves the brightest stops with less reach down than Highlight. Image Alchemist and Fitzgerald both measure them as a narrow band at each end, separate from the Levels black point and white point. This algorithm is Shadow and Highlight only. Blackpoint has already chosen the dark endpoint. The white point is its own control.

## Affinity's Shadows & Highlights

Affinity Photo's Shadows/Highlights adjustment sits on the Develop Basic panel and the Overlays panel. It sets the lightness of the shadow areas and of the highlight areas, and of the pixels around them. Left darkens that end, right lightens it. The Shadows / Highlights filter is the stronger control: it reduces contrast, boosts shadow detail, and recovers highlight detail. Its Range sliders, moved to the right, take in a greater range of shadow tones or of highlights.

A wider range, applied to the surrounding pixels, is a wide tonal move. Midtones come along, and a hard edge picks up a band from the neighborhood. This note specifies the High Dynamic Range Shadow and Highlight sliders. The weight is finished by middle grey, the black floor has a toe, and the base follows the edge.

## Where it sits

The samples are scene-linear, after Exposure and Blackpoint, before Brightness and Contrast encode them.

1. Exposure. Multiply by 2^ev. Every tone moves the same number of stops.
2. Blackpoint. Chooses which inputs count as pure black.
3. This tool. The broad dark end and the broad bright end.
4. Brightness, a gamma on encoded RGB, then Contrast, the cubic about encoded mid grey.
5. Vibrance, then Saturation.
6. Clarity, a local Laplacian on the mid frequencies, about 4–64 px. Clarity leaves its coarsest pyramid level alone. This tool is that broad scale. Each control has its own radius.

Capture One places the High Dynamic Range tool before Levels and Curves for the same reason. After those curves, the shadow lift is where the noise appears.

## Algorithm

Diffuse white is 1. Middle grey is 0.18. The channel ratios of the input are kept and written back at the end.

### Luminance

Rec. 709:

```
L = 0.2126 R + 0.7152 G + 0.0722 B
```

### Clipped channels

Repair a channel that sits on the clip ceiling while a sibling channel does not. Do this before the base exists. C is the scene-linear value of sensor saturation after Exposure. A blue sky clips blue first; red and green still hold a proportion, and that proportion is what puts the hue back.

For a clipped channel k, let j be an unclipped channel at that pixel (the larger one, when both remain). In a neighborhood, take the median of k/j over pixels where k is below C and j is above a small floor. If the set is empty, leave k at C. Otherwise:

```
k = j · median(k / j)
```

A pixel with R, G, and B all at C has nothing to copy. It stays clipped. That is the specular the help leaves alone. The neighborhood starts at 32 px and is internal.

### Edge-aware base

```
detail = L − base
```

base is a guided filter of L, guided by L. In a window ω of box radius r:

```
μ = mean(L, ω)
σ² = variance(L, ω)
a = σ² / (σ² + ε)
b = (1 − a) · μ
```

At each pixel the means of a and of b run over the windows that cover it:

```
base = mean(a) · L + mean(b)
```

r starts at 10% of the short side, inside 5–15%. ε starts at 0.02², in units of diffuse white squared. Both are internal. Where the window contains a hard edge, σ² is large, a stays near 1, and the step remains in the base. The dark side and the bright side then take different base values, which is what keeps a bright line off the roofline. A Gaussian of the same radius shares one base across that step, and the line appears.

A local Laplacian remapped only at the coarsest pyramid level is the same split. The guided filter is the one to code.

### Shadow

S is the Shadow slider in [−100, +100].

```
s      = max(S, 0) / 100
s_down = max(−S, 0) / 100
```

The weight is 1 in the deepest shadows and 0 at t_s = 0.18, middle grey. Published wedges still show a small Shadow leak past 0 EV. Closing the weight at 0.18 makes a mid-grey patch exact. t_s is the starting cutoff, inside 0.15–0.25, and it is tuned on pictures.

```
u = clamp(base / t_s, 0, 1)
w_s = (1 − u)² · (1 + 2u)
```

The toe is 0 at true black, with zero slope there, and it reaches 1 by τ_s = 0.02 (about −3 EV from middle grey):

```
φ = clamp(base / τ_s, 0, 1)
toe = φ²
```

A_s = 0.05 of diffuse white. k_s = 0.5.

```
base_s = base + (s · A_s · toe − s_down · k_s · base) · w_s
```

Zero on the slider leaves the base. The lift term adds light only where the toe has opened, so a black pixel stays 0 at a moderate s. The deepen term scales the same band down and stays non-negative for k_s ≤ 1. At S = +100 and base = 0.02, w_s is about 0.97 and toe is 1, so the base rises to about 0.068.

### Highlight

H is the Highlight slider in [−100, +100]. Recovery is the negative direction, the direction that lowers the highlights.

```
h    = max(−H, 0) / 100
h_up = max(H, 0) / 100
```

The weight is 0 at and below t_h = 0.70 of diffuse white, and 1 at diffuse white and above. The measured Highlight slider reaches lower than this, including a small effect below 0 EV. The cutoff starts at 0.70 so the midtones, and the mid-grey patch, stay still. Tune t_h on pictures.

```
excess = max(base_s − t_h, 0)
v = clamp(excess / (1 − t_h), 0, 1)
w_h = v² · (3 − 2v)
```

c_h = 1. g_h = 0.5. The shoulder blends the excess toward a compressed excess. The positive term stretches the excess.

```
excess_out = excess / (1 + h · excess / c_h)
base' = base_s + w_h · (excess_out − excess + h_up · g_h · excess)
```

h = 0 and h_up = 0 leave base_s. excess_out is monotonic in excess, so the shoulder stays ordered. At H = −100 a base of 4 (two stops over diffuse white) falls to about 1.47, and a base of 1.00 falls to about 0.93.

### Residual

```
L' = max(base' + detail, 0)
```

detail is the original residual, added at full strength. A global curve multiplies that residual by the slope at the base tone. In the shoulder the slope is below 1, and the weave in a white cloth, or the bars in a window, shrinks with it. Writing the residual back after the shoulder is the whole of highlight texture, and the same term is the grain and the fabric in an opened shadow.

### RGB

Where L ≤ 10⁻⁶, the output is neutral at L', and it is 0 when L' is 0. Otherwise the ratios are R/L, G/L, and B/L. The cap pulls those ratios toward 1 while L is below t_n = 0.01, so a lifted noise floor stays uncolored.

```
n = clamp(L / t_n, 0, 1)
ratio' = 1 + (ratio − 1) · n
RGB' = ratio' · L'
```

At and above t_n the hue is the input hue. A neutral has ratios of 1 and stays neutral at every n. Clamp each channel at 0 as a guard. Values above 1 stay above 1. The display transform is what clips them.

## Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `S` | Shadow slider, −100…+100. Right lightens, left darkens | 0 |
| `H` | Highlight slider, −100…+100. Right lightens, left recovers | 0 |
| `s` | `max(S, 0) / 100`, shadow lift | 0…1 |
| `s_down` | `max(−S, 0) / 100`, shadow deepen | 0…1 |
| `h` | `max(−H, 0) / 100`, highlight compression | 0…1 |
| `h_up` | `max(H, 0) / 100`, highlight amplify | 0…1 |
| `t_s` | Shadow weight reaches 0, in diffuse white | 0.18 |
| `τ_s` | Toe width. `toe = 1` at this base | 0.02 |
| `A_s` | Full shadow lift, diffuse white | 0.05 |
| `k_s` | Full shadow deepen, fraction of the base | 0.5 |
| `t_h` | Highlight weight leaves 0, in diffuse white | 0.70 |
| `c_h` | Shoulder scale of the excess | 1 |
| `g_h` | Full highlight amplify, fraction of the excess | 0.5 |
| `r` | Guided-filter box radius | 0.10 × short side |
| `ε` | Guided-filter regularization | 0.02² |
| `t_n` | Chroma cap opens fully at this L | 0.01 |
| neighborhood | Clipped-channel ratio median | 32 px |

`r`, `ε`, and the neighborhood are internal. The sliders are the only user controls. `t_s`, `τ_s`, `A_s`, `k_s`, `t_h`, `c_h`, and `g_h` are starting values, tuned on pictures.

## Failure cases worth testing against

- A flat patch at linear 0.18, detail 0. Shadow +100 and Highlight −100 leave it at 0.18. A move means `t_s` is above middle grey or `t_h` has fallen into the midtones.
- A black pixel, base 0, at Shadow +50. It stays 0. A grey floor means the toe was skipped and a constant was added under `w_s`.
- A −6 EV patch (about 0.003) at Shadow +100 stays near the floor. A −3 EV patch (0.02) rises by most of `A_s`. Equal lifts at those two levels mean the toe is too wide.
- A hard roof against a bright sky, Shadow +100 and Highlight −100 together. No bright line on the sky side of the edge. A line means the base was a Gaussian, or `ε` was large enough that `a` fell across the step.
- Texture inside a recovered highlight. A fine grating whose base is well above `t_h` keeps its linear amplitude at Highlight −100. A smaller amplitude means `detail` went through the shoulder.
- A neutral ramp stays neutral at every slider position, including deep shadows where `n` is below 1. A colored cast in the lifted blacks means the ratios were scaled by `L' / L` with the cap left open.
- A sky with blue at C and red and green below C, Highlight −100. Blue returns in the neighbor ratio and the sky stays blue. White, or a grey of the recovered luminance, means the ratio step was skipped. A disk with all three channels at C stays clipped.
- The same Highlight recovery after the Contrast cubic. Headroom that was still in the linear highlight has already been pinned by the encoded curve, and the shoulder is then compressing code values.

## Sources

- Capture One help, “The High Dynamic Range tool overview”. Separate Highlight and Shadow sliders compress the extreme tones of a single image. Highlight lowers highlight brightness and can rebuild a clipped channel from the others. Shadow raises shadow brightness; too much of it shows noise. The sliders also amplify the ranges and increase contrast. Black and White tune the darkest shadows and the brightest highlights. Levels and Curves remain the further control.
- Capture One help, “Recovering details in the highlights and shadows”. Highlight compresses color and luminance at the right of the histogram. Shadow lightens the deepest shadows. Natural at low and medium values; excess lowers contrast and saturation, and Clarity is the mid-tone contrast that follows. The tool is placed before Levels and Curves. Speculars, reflections, and light sources can stay clipped. White and Black are adjusted after Shadow and Highlight.
- Capture One help, “Auto adjust HDR”. The auto adjustment has little effect on midtones when the frame is not grossly mis-exposed.
- Capture One 20 release notes, and “What’s new in Capture One 20”. The sliders start in the middle. Recovery and the contrast direction both exist. Black and White are added in that version.
- Paul Reiffer, “Launching Capture One 20”. Before version 20 the recovery sliders ran 0 to 100. From version 20, zero is unchanged, positive highlight brightens, negative highlight recovers, positive shadow recovers, negative shadow deepens. Right is brighter and left is darker on each slider.
- Image Alchemist, “Capture One High Dynamic Range”, and “High Dynamic Range in Capture One 20”. Midtones are much less affected than in Capture One 7 and earlier. Wedge at Shadow +100: little change at −7 and −6 EV, effect from about −5 EV, strongest around −4 to −2 EV, a small effect past 0 EV. Highlight −100 matches White on the brightest stops and reaches further toward the midtones. Black owns the darkest band Shadow leaves. White and Black are not a black point and a white point.
- Thomas Fitzgerald, “Capture One 20 HDR Tool”. Shadow and Highlight are weighted toward the bottom half and the top half. Black and White are a narrower band at the two ends, and they are not a Levels endpoint. Examples are given at Shadow +100 and Highlight −100. The sliders do not lift the floor into grey.
- Affinity Photo 2 help, “Shadows/Highlights adjustment”. Lightness of shadows and of highlights, and of the surrounding pixels. Develop Persona, Basic and Overlays. Left darkens, right lightens.
- Affinity Photo 2 help, “Shadows / Highlights” filter. Stronger manipulation of those regions: reduce contrast, boost shadow detail, recover highlight detail. Range, moved right, includes a greater range of tones.
- Affinity Photo 2 help, “Basic panel”. Shadows & Highlights applies a tonal adjustment to the darkest and lightest areas.
- He, Sun, Tang. “Guided Image Filtering.” IEEE Transactions on Pattern Analysis and Machine Intelligence, 2013. The self-guided filter above is this filter.
