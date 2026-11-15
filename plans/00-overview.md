# Round 2: nothing on the timeline falls back to the CPU

## Problem

`FULL TEST 02.fcproj` (sequence `EDIT_PR_282 News Nov 02`, 1080p60, 22 min 37 s) stutters during
normal playback, the audio crackles and then stops. Measured with `bench_playback` over the whole
timeline in 10-second windows (600 frames due per window, Full resolution):

| Windows | Count | Frames shown (mean) |
|---|---|---|
| Fully under a zoom adjustment layer of Video 2 | 19 | 0 |
| Partly under one | 26 | 275 |
| Without one | 91 | 565 (lowest 488) |

Cause: the 16 zoom layers on Video 2 have Motion Position (960, 550), 10 px below centre. An
adjustment layer that does not cover the whole frame is declined by the GPU plan
(`adjustment_ops` in `crates/render/src/plan.rs`), so every frame under it (320 s, 24 % of the
video) is rendered on the CPU at 100–290 ms per frame. The audio mix is computed on demand in the
device callback; with the CPU saturated it arrives late (crackle), and after three stream restarts
in 10 s playback goes on silent (read from the code, not measured).

Smaller causes found in the same sweep: Lens Distortion (26 transitions of about 0.4 s), Emboss and
VR Digital Glitch (3 moments) on adjustment layers send those frames to the CPU.

Color Grading (`lumetri`) runs on the GPU only with Basic, Creative (sliders and tints) and
Vignette. Curves, Color Wheels, HSL Secondary, the two LUTs, the built-in Looks and Creative
Sharpen each send the whole effect to the CPU.

## Stages

| Plan | What | Why |
|---|---|---|
| 01 | Adjustment layers that do not cover the frame, are not opaque or have opacity masks | the cause above, and every variant of it |
| 02 | Lens Distortion | used in the transitions of this project |
| 03 | Emboss, VR Digital Glitch | used in the transitions of this project |
| 04 | Color Grading: Looks, Color Wheels, Curves | Color Grading on any track without the CPU |
| 05 | Color Grading: Input LUT and Look LUT | same |
| 06 | Color Grading: HSL Secondary and Sharpen | same |

Left out by decision: audio mixing on its own thread, pre-decoding at cuts, masks on single
effects (an effect with a mask of its own still renders on the CPU: apply Color Grading to the whole
clip or layer), HDR sequences, blend modes other than Normal on adjustment layers.

## How every stage is checked

1. The CPU renderer stays the reference. Each new GPU op has a CPU `apply` that reproduces the
   existing CPU effect (test in `crates/render`), and a GPU test in `crates/gpu/src/fx_tests.rs`
   comparing the shader with it on the real device.
2. A plan test: the case that used to plan as one CPU image (`FramePlan::Image`) now plans for
   the GPU, and hostile parameters are declined or bounded.
3. `cargo fmt`, clippy with warnings as errors and the tests of `filmcraft-render`,
   `filmcraft-gpu` and `filmcraft-ui-egui`.
4. The benchmark on the affected windows of `FULL TEST 02.fcproj`; numbers go to `results.md`.
5. One local commit per stage.
