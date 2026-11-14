# 06 — Safety net: CPU effects, GPU placement

Covers every effect that has no GPU version (about 100 today), now and for effects used later.

## Problem

A clip with one effect the GPU does not run takes the CPU path at the end of `push_item`
(`crates/render/src/plan.rs`):

```rust
if let Some((img, op2, _)) = crate::item_layer(project, seq, item, t, opts, sources, &tc) {
    out.push(PlanLayer::new(cpu_frame(img), Affine::IDENTITY, op2 * extra_opacity, bl));
}
```

`item_layer` does two things: it runs the effects on the clip's working image, then it resamples
that image onto a full output-size canvas (`layer.transformed(w, h, &m)` in `item_layer_ex`,
`crates/render/src/lib.rs`). Only the first needs the CPU. The second costs, per frame at 1080p:
a 33 MB `f32` canvas, the resample, 16.6 MB of half floats and their upload, whatever the size of
the clip on screen. For the camera (51.5 % scale) three quarters of that canvas is empty.

## Change

Hand the compositor the working image after effects and masks, with the placement matrix, and let
the GPU place it. This is what a layer with a GPU effect stage already does (`LayerFx`: "placed as
`item_layer` places it").

1. `crates/render/src/lib.rs`: split the end of `item_layer_ex` so a new
   `item_working_layer(..) -> Option<(Image, Affine, f32, Blend)>` returns the layer and `m` before
   `transformed`. `item_layer_ex` calls it and places as today, so the CPU renderer is unchanged.
2. `crates/render/src/plan.rs`, CPU branch of `push_item`: when `m` does not minify by 2 or more
   (`gpufx::minifies`, the case where the CPU takes its mip path and the shader's filter differs),
   push `PlanLayer::new(cpu_frame(layer), m, op, bl)`. Otherwise keep today's full-canvas path.
3. `execute_layers` already places a layer with its matrix, so the CPU reference needs no change.

No shader change.

### Not included: adjustment layers with a CPU effect

Such a layer still sends the whole frame to the CPU. Doing only the layer on the CPU would mean the
compositor reads the canvas back from the GPU, runs the effect and uploads the result while it is
drawing, which is on the UI thread. That stalls the interface for longer than the current path,
which runs on a frame worker. For adjustment layers the answers are plan 07 (move the effect) and
rendering a preview of the section.

## Tests

- `crates/render`: a media clip with a CPU-only effect (for example Twirl), scaled and moved, plans
  as a layer whose frame is the working-image size, not the output size; `execute_cpu(plan)` equals
  `render_sequence` within the existing tolerance.
- The same with an opacity mask, with a Graphic clip that has an effect, and with a nested sequence
  that has an effect.
- A clip scaled below 50 % of its working image still takes the full-canvas path.
- `crates/gpu/src/tests.rs`: GPU composite of such a plan against `execute_cpu`.

## Files touched

`crates/render/src/lib.rs`, `crates/render/src/plan.rs`, tests. About 60 lines.

## Expected result

For a clip with a CPU effect, the per-frame cost becomes the effect itself plus an upload the size
of the clip's working image. For the camera that is about a quarter of today's upload and no
canvas resample. The test projects have no such clip once plan 01 is in, so this needs a synthetic
case to measure (a copy of the short project with Twirl on the camera).

## Risk

Low. The condition in step 2 keeps every case whose filtering could differ on the old path.
