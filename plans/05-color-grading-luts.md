# 05 — Color Grading: Input LUT and Look LUT on the GPU

`crate::luts::resolve` gives the LUT (a file or a built-in one); the CPU applies it in the
display-encoded signal: the Input LUT before the Basic controls, the Look LUT (with intensity)
in `lumetri_advanced` instead of a procedural Look.

- The LUT's samples go to the shader as a table texture; the shader repeats the CPU's
  interpolation (`Lut::apply`).
- Input LUT: a pass before the Lumetri pass. Look LUT: part of the pass of plan 04.
- Bounded: a LUT larger than the stage accepts is declined (CPU).

Tests: a 1D-shaped and a 3D LUT against the CPU effect; GPU against `apply`.
