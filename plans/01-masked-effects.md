# Stage 1: effects limited by a mask of their own, on the GPU

## Today

An effect can carry masks (`EffectInstance::masks`): the effect then applies only inside them.
On the CPU that is `mask::apply_effect` in `crates/render/src/mask.rs`: keep a copy of the
picture, apply the effect, then `mask::mix` the two by the coverage of the masks
(`mask::effect_coverage`, at `cx.t` and `cx.mask_scale`).

The GPU plan declines such an effect in two places in `crates/render/src/plan.rs`:

- `gpu_chain` returns `None` when any enabled standard effect has a mask whose mode is not
  `MaskMode::None`. The clip's layer is then rendered on the CPU and only placed by the GPU.
- `adjustment_ops` returns `None` for the same reason. The **whole frame** then renders on the
  CPU while that adjustment layer is on screen.

## Change

**A new op** in `crates/render/src/gpufx.rs`:
`FxOp::Masked { op: Box<FxOp>, cov: Arc<Vec<f32>> }`, "run `op`, then keep its result only
where `cov` says".

- CPU `apply`: clone the image, apply `op`, `mask::mix(img, &original, &cov)`. This must equal
  `mask::apply_effect` for the same effect.
- `gpu_ok`: `op` is ok and `cov.len()` is the working image's `w × h`.
- An effect that evaluates to `FxOp::Seq` (a full Color Grading) is wrapped whole: the mask
  mixes the result of the entire sequence with the picture from before it.

**Coverage planes.** Rasterise with `mask::prepare` + `mask::coverage`, through a small cache
like `MASK_PLANES` / `opacity_mask_op` (same file): a mask without keyframes is rasterised once
for all the frames of its clip, and the compositor recognises an already uploaded plane by its
address. Either widen that cache to serve both uses or add a second one of the same shape; keep
it bounded (LRU) and take no lock while rasterising. Masks whose modes are all `None` give no
coverage: the effect is then planned unmasked, as `apply_effect` does.

**Planner** (`plan.rs`):

- `gpu_chain`: drop the `masked ||` condition.
- `push_item`, where the chain is evaluated (`FxOp::eval(e, &cx, lw, lh)`): wrap the op of a
  masked effect in `FxOp::Masked`, with coverage for the `lw × lh` working image at
  `cx.mask_scale` (which is `px_scale` there, not the capped `cx.px_scale`).
- `adjustment_ops`: drop the mask condition and wrap the same way, with coverage for the output
  size at `scale`. The trailing `FxOp::MixBase` stays last.
- `execute_cpu` runs ops through `apply`, so the CPU reference of a plan needs no other change;
  check its `Adjust` arm still treats `MixBase` as today.

**Effect stage** (`crates/gpu/src/fx.rs`, `fx.wgsl`). A masked op needs the picture from before
it at its last pass. The stage already keeps pictures in two ways: the "original" an Unsharp,
Drop Shadow or key reads back (`reads_orig`, `combine`), and the adjustment layer's base
(`mix_base`, texture slot 0). Add a third of the same kind:

- at the first pass of a masked op, the texture holding the current picture is set aside;
- the op's own passes run, including any that keep their own "original";
- a final pass writes `kept + (result − kept) × coverage`, the coverage read from the table
  binding (binding 4), as `OP_MIX_BASE` reads an adjustment plane. Reuse `OP_MIX_BASE` with the
  kept texture as its second input if that is exact; otherwise add op code 119.

Worst case to size the textures for: a masked Unsharp on an adjustment layer that also ends in
`MixBase` (base, set-aside picture, the op's own original, and the two working textures). If the
device cannot allocate them the stage must fail the way it does today, so that the frame falls
back to the CPU instead of drawing something wrong.

## What stays on the CPU

An effect with no GPU version, with or without a mask. A masked effect on a nested sequence or
a title (those render on the CPU with any standard effect today). HDR sequences.

## Checks

- `crates/render` tests: for a colour effect, a blur, Unsharp Mask, Drop Shadow, Transform and a
  full Color Grading, `FxOp::Masked` applied on the CPU equals `mask::apply_effect`; with
  feather, an inverted mask, two masks and a keyframed mask.
- `crates/gpu/src/fx_tests.rs`, GPU against CPU on the real device: masked Gaussian Blur; a
  masked colour effect followed by an unmasked one; masked Unsharp Mask and masked Drop Shadow
  (they keep an original of their own); masked full Color Grading; masked effect plus an opacity
  mask on the same clip; a masked effect on an adjustment layer that also keeps part of the
  picture below (`MixBase`).
- Plan tests (`crates/render/src/adjustment_tests.rs` and the plan tests of media clips): a clip
  with a masked GPU effect gets an effect stage instead of a CPU layer; an adjustment layer with
  a masked effect plans as `PlanStep::Adjust`, not `FramePlan::Image`; hostile mask points
  (non-finite, enormous) do not panic.
- Upstream tests that assert "masked effect → CPU" will fail. Change them to the new behaviour
  and list them in `UPDATING.md` beside the three already listed there.
- Benchmark. Make two copies of `FX Color Grading CAM.fcproj` and `FX Color Grading ADJ.fcproj`
  (in `/media/bruno/SSD 500 2 Linux/Software/Exporter test project/`) in which the Color Grading
  has an elliptical mask with feather, named `FX Color Grading Masked CAM/ADJ.fcproj`. Measure
  the same windows as the unmasked ones before and after this stage. Target: within a few frames
  of the unmasked projects (582 / 559 and 583 / 561 of 600). Never edit Bruno's own projects.
