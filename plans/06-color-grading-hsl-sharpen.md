# 06 — Color Grading: HSL Secondary and Sharpen on the GPU

**HSL Secondary** (`hsl_secondary`): a key from hue / saturation / lightness, optionally cleaned
with a median filter (Denoise) and a three-pass box blur (Blur), then the correction (or one of the
three mask views) mixed in by the key. On the GPU: the key into a second texture while the picture
stays untouched (as Ultra Key's matte), a median pass, the box passes the stage already has, and a
final pass.

**Sharpen** (Creative): `unsharp(radius 1.2 × scale, amount)` after everything else: the existing
Unsharp passes.

With this, `gpu_capable` is false only for HDR grading.

Tests: key only, Denoise, Blur, each mask view, and Sharpen against the CPU effect; GPU against
`apply`; a full Color Grading (every section in use) on a clip and on an adjustment layer plans
for the GPU.
