# 07 — More effects on the GPU: Drop Shadow, Rounded Crop, Luma Key, Noise, Ultra Key

Requested list: Drop Shadow, Rounded Crop, Ultra Key, Alpha Adjust, Luma Key, Noise.

**Alpha Adjust is already on the GPU** (`FxOp::AlphaAdjust`, in `GPU_EFFECTS`). Nothing to do.

None of the other five is enabled anywhere in the two test projects, so they cannot be measured
with them. Each needs a small synthetic case (the effect on the camera clip of the short project).

## How an effect is added

One implementation serves clips and adjustment layers: `gpu_chain` and `adjustment_ops`
(`crates/render/src/plan.rs`) both go through `FxOp::eval`.

For each effect:

1. `crates/render/src/gpufx.rs`: a new `FxOp` variant; `eval` reads the parameters exactly as the
   CPU function does; `apply` calls the CPU function (the reference); `gpu_ok` checks every number
   is finite; the id goes into `GPU_EFFECTS`.
2. `crates/gpu/src/fx.rs`: `steps` maps the op to one or more passes.
3. `crates/gpu/src/fx.wgsl`: the pass. Helpers already there: `enc` / `dec` (sRGB transfer),
   `luma709`, `rgb_to_hsl` / `hsl_to_rgb`, `smoothstep_fx`, `sample_bilinear`, the box-blur passes,
   and a third working texture that keeps the original (used by Unsharp Mask).
4. `crates/gpu/src/fx_tests.rs`: GPU result against `FxOp::apply`, default and extreme parameters,
   and a hostile-parameter case that `gpu_ok` declines.

Two of the ids have two CPU implementations (`noise` and `ultra_key` appear in both
`crates/render/src/effects.rs` and `crates/render/src/vfx/color.rs`). The first step for those is to
confirm which one the dispatch reaches, and mirror that one.

## The effects, smallest first

### Luma Key (about 30 lines)

CPU: `effects.rs`, `"luma_key"`. Per pixel: encoded BT.709 luma of the straight colour, optional
invert, a linear ramp between Cutoff and Threshold, all four channels multiplied by it. One pass.

### Rounded Crop (about 50 lines)

CPU: `vfx/distort.rs`, `rounded_crop`. Per pixel: signed distance to a rounded rectangle, an
optional border drawn over the picture, then alpha from the distance (feathered or one-pixel
edge). One pass. The existing `Crop` op is the model.

### Drop Shadow (about 80 lines)

CPU: `effects.rs`, `"drop_shadow"`. Steps, all with parts the stage already has:

1. the picture translated by (distance, direction), bilinear, into a second texture, its colour
   replaced by the shadow colour times alpha times opacity;
2. Gaussian blur of that texture when softness > 0.3 (the existing box passes, radii from
   `gaussian_boxes`);
3. the original composited over it, unless Shadow Only.

Uses the third texture to keep the original, as Unsharp Mask does. The shadow is clipped at the
working image's edge, as on the CPU.

### Noise (about 70 lines)

CPU: `vfx/color.rs`, `noise`. Per pixel: three hashes of (grain x, grain y, frame seed), added to
the encoded colour, optional clip. One pass.

The hash (`effects::hash3`) is 64-bit integer arithmetic and the seed is the frame's tick, which
does not fit in 32 bits. WGSL has no 64-bit integers, so the shader needs a 64-bit multiply and
xor-shift built from pairs of `u32`. Integer results are then identical to the CPU's; the seed goes
in as two `u32` in the step's integer parameters.

### Ultra Key (about 200 lines, two stages)

CPU: `vfx/color.rs`, `ultra_key`. The largest of the five.

- **7a. Without matte cleanup** (Choke and Soften at 0): one pass. Matte from the chroma distance to
  the key colour with highlight / shadow / pedestal, matte contrast, spill suppression, the colour
  correction (HSL), and the three output modes. The Setting presets are resolved in `eval`
  (`ultra_key_setting`), so the shader only sees numbers.
- **7b. With Choke or Soften**: the matte goes into its own texture, is blurred with the box passes
  (`blur_plane` is the same Gaussian), combined (`min` for choke), and a final pass applies it to
  the colour. Three or four passes.

Until 7b is done, a key with Choke or Soften above 0 is declined by `eval` and stays on the CPU,
where plan 06 keeps it as cheap as it can be.

## Order inside this plan

Luma Key and Rounded Crop first (they establish the pattern), then Drop Shadow, Noise, Ultra Key 7a,
Ultra Key 7b. Each is its own commit and can stop after any of them.

## Files touched

`crates/render/src/gpufx.rs`, `crates/gpu/src/fx.rs`, `crates/gpu/src/fx.wgsl`,
`crates/gpu/src/fx_tests.rs`. About 430 lines in total plus tests. `fx.wgsl` and `gpufx.rs` are the
files upstream changes when it moves effects itself, so this plan is the most likely to conflict
on a merge; keeping each effect in one contiguous block in each file limits that.

## Expected result

A clip or adjustment layer using only these effects (with the 35 already supported) stays on the
GPU. The largest difference is on adjustment layers, where one CPU effect today costs a whole-frame
CPU render.

## Risk

Luma Key, Rounded Crop, Drop Shadow: low. Noise: low to medium (the 64-bit hash must match bit for
bit; the test compares exact hash values first). Ultra Key: medium (many stages; float differences
between `powf` on CPU and GPU are amplified by matte contrast, so the tolerance may need a
per-effect case, as the existing tests have for some colour ops).
