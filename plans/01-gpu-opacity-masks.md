# 01 — Opacity masks on the GPU (largest expected gain)

## Problem

Every camera clip in the test project (272 of 272 on Video 3) has an opacity mask (a circle).
`gpu_chain` in `crates/render/src/plan.rs` declines any clip with opacity masks:

```rust
if !is_media || item.has_opacity_masks() { return None; }
```

The clip then goes through the CPU path (`item_layer`, end of `push_item`), for every frame:

1. the decoded frame is converted to linear `f32` RGBA;
2. it is resampled onto a full 1920×1080 `f32` canvas (33 MB) with `Image::transformed`;
3. the mask coverage is rasterised and multiplied in (`mask::apply_opacity_masks`);
4. the canvas is converted to half floats (16.6 MB, `filmcraft_gpu::prepare`);
5. the UI thread uploads those 16.6 MB with `write_texture`.

At 60 fps that is about 1 GB/s of texture upload plus the CPU work, for a layer the GPU could draw
from the three YUV planes it already gets. This runs for nearly the whole timeline, so it is what
makes the GPU look idle.

## Change

Carry the mask to the compositor as a coverage plane and multiply it in the layer shader.

1. `crates/render/src/plan.rs`
   - `PlanLayer` gets `mask: Option<Arc<VideoFrame>>`: a one-channel coverage plane in the layer's
     own pixel space (the frame, or the working image when the layer has `fx`). The CPU applies the
     mask before Motion places the layer, so this is the same space.
   - `gpu_chain` stops declining clips with opacity masks.
   - `push_item` builds the plane with the existing `mask::effect_coverage(&op.masks, mt, px_scale, w, h)`,
     so the values are the CPU's own.
   - Cache the plane by (mask values at `mt`, `px_scale`, size) in a small LRU. The masks here have
     no keyframes: one plane is rasterised once and reused, and the compositor's upload cache
     (keyed by the `Arc` pointer) uploads it once.
   - `execute_layers` (the CPU reference) multiplies by the plane, so `execute_cpu` stays the oracle.
2. `crates/gpu/src/lib.rs`, `composite.wgsl`
   - One more texture binding per layer (the dummy texture when there is no mask) and a uniform
     flag; the fragment shader multiplies the premultiplied colour and alpha by the sampled coverage.
   - With `fx`: multiply after the effect stage, as `item_layer_ex` does (effects, then masks).
3. `crates/gpu/src/export_renderer.rs` uses the same plan, so GPU exports gain too. No change there
   beyond what the compositor needs.

`crates/gpu/src/mask.rs` (a compute-shader rasteriser with parity tests) is not needed for this:
a CPU-rasterised plane that is cached costs nothing per frame and is exact by construction.
Animated masks rasterise once per distinct value; that is still far cheaper than today.

## Tests

- `crates/gpu/src/tests.rs`: a masked layer composited on the GPU against `execute_cpu` (the
  existing parity pattern), with Motion scale and offset, inverted, feathered, and with an `fx` chain.
- `crates/render`: `plan_frame` of a masked media clip gives `FramePlan::Layers` with a YUV frame,
  not an `RgbaF32` canvas; `execute_cpu(plan)` equals `render_sequence` within the existing tolerance.
- Hostile input: empty path, NaN vertices, zero-size layer (no panic, layer drawn unmasked or skipped
  as the CPU does).

## Files touched

`crates/render/src/plan.rs`, `crates/gpu/src/lib.rs`, `crates/gpu/src/composite.wgsl`, tests.
About 150–250 lines. No UI, project-format or decoder changes.

## Expected result

Play A (plain section): `renderMs` drops from tens of ms to about 1 ms per frame, the per-frame
upload from ~23 MB to ~6 MB, and 1080p60 should play without dropped frames. Scrubbing gains the
same per frame. To be confirmed against plan 00.

## Risk

Low. The fallback stays in place for anything declined, and the CPU reference is unchanged.
