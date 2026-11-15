# 04 — Color Grading: Looks, Color Wheels and Curves on the GPU

Today `FxOp::Lumetri::gpu_capable` is false as soon as a built-in Look, a wheel or a curve is in
use, and the whole effect renders on the CPU (`lumetri` + `lumetri_advanced` in `effects.rs`).

- New op `FxOp::LumetriAdvanced` (in `gpufx_more.rs`), run right after the existing Lumetri pass,
  reproducing `lumetri_advanced` in the same order: Look (procedural, with intensity), wheels,
  RGB / luma curves, then the hue and saturation curves in HSL.
- The curves are the CPU's own 1024-entry tables (`curve_lut`, `hue_lut`, `vs_sat_lut`), handed to
  the shader as one table texture (the per-step table binding of plan 01) and read with the same
  linear interpolation.
- `FxOp::eval` keeps returning one op for `lumetri`: `Lumetri` gains an
  optional boxed `advanced` part that the stage expands into the extra pass.

Tests: each part alone and all together against the CPU effect; GPU against `apply`.
