# Clarity

Status: idea. Not part of the viewer.

Clarity is the control Affinity Photo puts in Develop Persona on the Basic panel, inside Enhance, next to Contrast, Saturation, and Vibrance. The same effect is the Clarity filter (Filters → Sharpen → Clarity, and the live filter). Affinity's help states it directly: the filter enhances local contrast, with its greatest influence in the midtones, and the result reads as sharpening.

Serif has not published the kernel. What follows is the visible behavior of that slider, then the algorithm that produces it.

## What the slider does

One strength value, default 0. Since Affinity Photo 1.7 the range is −100% to +100%, with 0 in the middle. Develop Persona exposes it as a slider only.

- Above 0, texture and mid-size structure gain contrast. Foliage, masonry, fabric, and cloud detail separate. Large edges stay in place, so the picture looks deeper rather than outlined.
- At 0, the picture is unchanged.
- Below 0, that same texture loses contrast. Skin and fine structure soften. The large shapes stay, which is why a small negative value is used as a portrait smoother.

The midtone bias is the part that separates it from a global contrast curve. A cloud edge and a dark doorway can both be strong edges; Clarity barely moves them. A fold in a coat, a few stops either side of mid grey, moves a lot.

Two limits show up as soon as the slider is pushed. Positive strength lifts local peaks and can clip bright texture (white cloth, cloud tops). Negative strength on skin turns plastic past a moderate setting. Both follow from the algorithm below.

## Where it sits next to the other tone controls

| Control | What changes | Typical scale |
| --- | --- | --- |
| Contrast | One curve for the whole frame. Midtones spread or compress together, edges included. | Whole image |
| Shadows / Highlights | Broad regions, one side of the tonal range. | Large neighborhoods |
| Clarity | Differences that are small next to their own neighborhood, mostly around mid grey. | About 4–64 px |
| Sharpen, Detail Refinement | The finest band, the acutance of an edge. | About 0.5–2 px |

Detail Refinement on the Details panel is a separate control. This note is the Enhance → Clarity slider.

## Algorithm

Clarity is an edge-aware gain on mid-frequency luminance. Two formulations are the same idea. The one to implement is the local Laplacian. The older one is a large-radius unsharp mask, and it is recorded here because it is what this slider used to be.

Work on luminance. Keep chroma and put it back at the end, so hue does not shift. In a raw pipeline, run the filter on an encoded luma (a log or a gamma), on the same signal Contrast sees. A fixed threshold in scene-linear light means a different amount of texture in the shadows than in the highlights.

### Local Laplacian

Paris, Hasinoff, and Kautz (ACM TOG 2011) build the output by its Laplacian pyramid. Each coefficient is taken from a copy of the image that has been remapped around the local base tone. Aubry, Paris, Hasinoff, Kautz, and Durand (ACM TOG 2014) evaluate that without rebuilding a pyramid per pixel. darktable's local-contrast module is a concrete version of this filter; its `clarity` term is the formula below.

Let `L` be the encoded luminance, normalized to a working range such as `[0, 1]`. Let `g` be the Gaussian-pyramid coefficient at the level and pixel being written: a weighted average of the neighborhood at that scale. For a sample intensity `i`,

```
δ = i − g
```

`δ` small means texture inside the neighborhood. `δ` large means an edge.

The clarity remap, with the shadows and highlights terms left at identity, is

```
r(i) = i + a · δ · exp(−δ² / (2 τ²))
```

`a` is the signed strength. `τ` is the amplitude that still counts as detail.

- For `|δ| ≪ τ` the exponential is about 1, so the residual is multiplied by `1 + a`.
- For `|δ| ≫ τ` the added term dies, and `r(i) = i`. Edges are not stretched, which is what keeps the halo off the roofline.
- `a = 0` is the identity.

The original paper writes the same split as a power curve. With the edge slope held at 1:

```
r(i) = g + sign(δ) · τ · (|δ| / τ)^α    if |δ| ≤ τ
r(i) = i                                 if |δ| > τ
```

`α = 1` is the identity. `α < 1` expands small differences (positive clarity). `α > 1` shrinks them (negative clarity). The exponential form is the one to code: `a` is signed, zero is exact, and positive and negative share one path. The power form is the published definition of the same gate.

Applying `r` once to the pixels is a global curve and is not this filter. The local step is:

1. Build the Gaussian pyramid `G` of `L`. Level `ℓ` is the base tone `g` for coefficients at that level.
2. Sample the working range at reference levels `γ_j`, spaced about `τ` apart. Five to twelve samples cover `[0, 1]` for the `τ` below.
3. For each `γ_j`, remap every pixel with `r` using `g = γ_j`, and build the Laplacian pyramid of that remapped image.
4. For each output coefficient at level `ℓ` and pixel `p`, read `g = G_ℓ(p)`, find the two reference levels around it, and linearly interpolate the precomputed Laplacian coefficients.
5. Collapse the pyramid to `L′`.
6. Recombine `L′` with the original chroma.

Step 4 is the whole trick. The gain at a pixel depends on that pixel's own neighborhood, so a leaf and the sky behind it do not share a curve. Sampling every standard deviation of the remap kernel is the recommendation in the 2014 paper. Against the exact per-coefficient remap, that approximation stays above about 30 dB, which is invisible, and the cost is linear in the pixel count times the number of reference levels.

Leave the finest pyramid level alone. That band is sharpening, and it is where noise and JPEG blocks live. Leave the coarsest level alone too. That band is global contrast. Clarity owns the middle levels, wavelengths of roughly 4–64 px at full resolution, on the order of 1–4% of the short side.

### Parameters

| Symbol | Meaning | Starting value |
| --- | --- | --- |
| `s` | The slider, −100…+100 | 0 |
| `a` | `s / 100`, so the residual scale is `1 + a` inside the window | −1…+1 |
| `τ` | Detail amplitude in normalized encoded luminance | 0.12 |
| levels | Pyramid levels that receive the remap | all but the finest and the coarsest |

`a = 1` doubles texture inside the window and is the dramatic end of the slider. `a = −1` flattens it. Portraits usually stop around `−0.4`. Affinity's control is known to be sensitive at small movements; keep the mapping linear in `a` and judge `τ` on real photos rather than inventing a second user-facing radius. The Develop slider has no radius.

### The older unsharp-mask form

Before the 1.7 rewrite, this is the filter described on the Affinity forum, and it is still the usual one-pass approximation. That thread describes the pre-1.7 Clarity as a large-radius unsharp mask blended in luminosity (a radius around 100 px was the suggested recreation), and the replacement as a more complex filter brought in line with the Develop slider.

```
B = gaussian_blur(L, radius)
D = L − B
M = 4 · L · (1 − L)
L′ = clamp(L + a · M · D, 0, 1)
```

`M` is the midtone gate: 0 in black and white, 1 at mid grey. `radius` is tens of pixels, not the 1 px of a sharpen. Luminosity blending is this: the correction is computed on `L` and written back, and chroma is untouched.

At small `a` it reads as Clarity. At large `a` it halos, because a pixel on the bright side of an edge and a pixel on the dark side share one blurred base, so both get a kick. The local Laplacian does not share that base. A guided-filter base in place of the Gaussian is a halfway step: better edges than the blur, short of the pyramid.

```
B = guided_filter(L, L, radius, ε)
D = L − B
W = exp(−D² / (2 τ²))
L′ = clamp(B + D · (1 + a · W), 0, 1)
```

One radius is one scale. Two or three radii (about 4, 16, and 64 px) summed at reduced gain move it closer to the pyramid. The reference remains the local Laplacian.

## Failure cases worth testing against

- Bright texture clips before the rest of the frame looks done. Clouds and white fabric are the test.
- A hard roof against a sky grows a bright line. The edge gate is too wide, or the implementation fell back to a plain unsharp mask.
- Noise and block edges come up with the texture. The finest pyramid level was included.
- A face at `a ≈ −1` loses pores and keeps a waxy cheek. Expected at that strength; the useful negative range is smaller than the positive one.
- The same slider value on a 12-megapixel frame and on a downscaled preview disagree. The pyramid has to be built in full-resolution pixels, or the preview has to use the same pixel radii. A mipmapped preview will lie, which is the same preview issue Affinity documents for sharpening below 100% zoom.

## Sources

- Affinity Photo 2 help, “Clarity”. Local contrast, greatest influence in the midtones. The Develop Persona copy of the filter is Basic → Enhance.
- Affinity forum, Clarity filter range change in 1.7. Slider moved to −100%…+100% centered on 0, matching Develop Persona. The previous filter was described as a luminosity-blended unsharp mask.
- Paris, Hasinoff, Kautz. “Local Laplacian Filters: Edge-aware Image Processing with a Laplacian Pyramid.” ACM Transactions on Graphics, 2011.
- Aubry, Paris, Hasinoff, Kautz, Durand. “Fast Local Laplacian Filters: Theory and Applications.” ACM Transactions on Graphics, 2014.
- darktable, `src/common/locallaplacian.c`. The `clarity` term has this shape. Their code writes the window as `exp(−δ² / (2 σ² / 3))`, which is the Gaussian above with `τ = σ / √3`.
