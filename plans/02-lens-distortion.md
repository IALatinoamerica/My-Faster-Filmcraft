# 02 — Lens Distortion on the GPU

`effects.rs` `"lens_distortion"`: a warp. Each output pixel reads the picture, bilinear with
transparent outside, at `centre + d × (1 − k·|d|²)`, `d` the offset from the (decentred) centre
over half the diagonal.

- `crates/render/src/gpufx_more.rs`: `LensDistortion { k, cx, cy, norm }` with `eval`, `gpu_ok`,
  `apply`; listed in `GPU_EFFECTS`, evaluated in `FxOp::eval`.
- `crates/gpu/src/fx.rs`, `fx.wgsl`: one pass.

The CPU computes the coordinates in `f64`, the shader in `f32`: at 1080p the read position differs
by under 1e-4 px, so the tolerance of the GPU test is 2e-4 instead of 2e-6.

Tests: CPU `apply` equals the effect; GPU equals `apply`; NaN / infinite curvature is declined.
