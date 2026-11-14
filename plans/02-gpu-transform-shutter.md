# 02 — Transform with its own shutter angle on the GPU

## Problem

The zooms in the test project are adjustment layers with a Transform effect whose "Use Composition's
Shutter Angle" is off (`shutter_override` true, shutter angle 90–360°): 95 of 188 enabled Transform
effects on adjustment layers. `FxOp::eval` in `crates/render/src/gpufx.rs` declines them:

```rust
if crate::vfx::bv(e, "shutter_override") && crate::vfx::fv(e, "shutter_angle", cx) > 0.5 { return None; }
```

An adjustment layer the GPU declines sends the **whole frame** to the CPU (`whole_frame_on_cpu`):
every track is decoded to `f32`, composited on the CPU, the Transform is rendered seven times
(`vfx::distort::transform_fx`) and averaged, and the 16.6 MB result is uploaded.

Sampling the timeline every 0.1 s, 24.8 % of the sequence is under such a layer, including the
time after the zoom's keyframes end, where the seven samples are identical.

## Change, in two steps

### 2a. Seven identical samples are one sample (small)

In `FxOp::eval`, evaluate the Transform's matrix and opacity at the seven sample times the CPU uses.
When all seven are equal, return the ordinary `FxOp::Resample`. This covers the held part of every
zoom (most of the 24.8 %).

The CPU averages seven identical images, which differs from one image only by float rounding. Either
keep within the existing GPU/CPU test tolerance, or make `transform_fx` take the same shortcut so
both paths render one sample (preferred: it also makes CPU exports faster and keeps parity exact).

### 2b. A multi-sample resample op (medium)

New `FxOp::ResampleBlur { samples: Vec<Resample> }` (seven entries) for the moving part of a zoom:

- `crates/render/src/gpufx.rs`: `eval` builds it; `apply` (CPU reference) runs the existing
  seven-sample code; `gpu_ok` checks every matrix as `Resample` does today, and declines when any
  sample minifies (the CPU's mip path).
- `crates/gpu/src/fx.rs`, `fx.wgsl`: one pass with the seven inverse matrices in a uniform array;
  the fragment shader samples the source seven times and averages. Same filter as the existing
  `Resample` pass.

## Tests

- `crates/gpu/src/fx_tests.rs`: `ResampleBlur` against `FxOp::apply` for a zoom, a move and a
  rotation; hostile values (NaN, infinite scale) are declined by `gpu_ok`.
- `crates/render`: a held zoom plans as `PlanStep::Adjust`, not `FramePlan::Image`.
- `crates/render/src/adjustment_tests.rs`: the frame under a held shutter-Transform equals the CPU
  render.

## Files touched

`crates/render/src/gpufx.rs`, `crates/render/src/vfx/distort.rs` (2a shortcut),
`crates/gpu/src/fx.rs`, `crates/gpu/src/fx.wgsl`, tests. 2a is about 40 lines, 2b about 150.

## Expected result

Play B: frames under a zoom go from a whole-frame CPU render to the same cost as any other frame.
Without this, a quarter of the timeline stays slow whatever plan 01 does.

## Not included

- Lens Distortion (0.9 % of the timeline) and Emboss (4 samples) on adjustment layers also send the
  frame to the CPU. Neither is in plan 07's list; a GPU Lens Distortion op would follow the same
  pattern if wanted later.

## Risk

2a low. 2b low to medium: the shader must match the CPU's sample times and filter exactly; the
parity tests decide.
