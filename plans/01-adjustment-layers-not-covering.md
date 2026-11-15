# 01 — Adjustment layers that do not cover the frame

## Problem

`adjustment_ops` (`crates/render/src/plan.rs`) returns None, which sends the whole frame to the
CPU, when the layer's opacity is under 100 %, it has opacity masks, or its Motion-placed frame
does not cover the output exactly (`adjustment_covers_output`). A 10 px nudge is enough.

## What the CPU does (`adjustment_layer` in `crates/render/src/lib.rs`)

`adjusted = effects(below)`. A weight per pixel = opacity × opacity masks × coverage of the layer's
frame. For blend Normal the result is `below + (adjusted − below) × weight` on all four channels.

## Change

- `crates/render/src/lib.rs`: the weight computation moves into one function,
  `adjustment_weight`, used by the CPU renderer and by the plan, so both use the same plane.
- `crates/render/src/gpufx.rs`: new op `FxOp::MixBase { weight, cov }`, last op of an adjustment
  step: the picture the step started from, mixed with the adjusted one by `weight × cov`.
  `plan::execute_cpu` keeps the picture from before the ops for it.
- `crates/render/src/plan.rs`: `adjustment_ops` no longer declines those cases; it appends
  `MixBase`. A uniform weight (opacity only) needs no plane. Planes are cached (static layers
  make one plane for all their frames), like the opacity mask planes of plan 01 of round 1.
- `crates/gpu/src/fx.rs`, `fx.wgsl`: the effect stage keeps its first texture while a job with
  `MixBase` runs, and gets a fifth binding for a per-step table (here the coverage plane), since
  the mix reads three pictures: adjusted, base and plane.

Still on the CPU: blend modes other than Normal.

## Tests

- render: an offset layer, a half-opaque one and a masked one plan as `PlanStep::Adjust` and
  `execute_cpu` equals `render_sequence`; hostile Motion values (NaN, huge scale) do not panic.
- gpu: `MixBase` with and without a plane against the CPU reference.

## Expected result

The 45 windows with a Video 2 zoom layer go from 0–400 frames shown to the level of the others.
