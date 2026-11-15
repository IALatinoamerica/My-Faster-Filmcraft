# Stage 2: the report of what renders on the CPU

## Purpose

After stage 1 some things still render on the CPU. Each must be named before Bruno hits play:
which track, which clip, from when to when, why, and whether it costs one layer or the whole
frame. This stage builds the list and a command that returns it; stage 3 uses it for the render
bar and shows it in the app.

## The function

New module `crates/render/src/slow_paths.rs` (same name as the exporter's, to keep the two
projects easy to compare):

```rust
pub struct SlowPath {
    pub track: usize,      // video track, 1 = bottom; 0 for the sequence itself
    pub name: String,      // clip name, or a transition's effect id
    pub start: Tick,       // the affected part of the timeline, inside the asked range
    pub end: Tick,
    pub reason: String,    // plain words, naming the effect
    pub whole_frame: bool, // every frame in the range renders on the CPU, not just this layer
}
pub fn slow_paths(project: &Project, seq: ItemId, start: Tick, end: Tick, caps: DeviceCaps) -> Vec<SlowPath>
```

It decodes nothing. It asks the planner's own questions, so it cannot disagree with playback
about *kind*: `gpu_chain`, `adjustment_ops`, `whole_frame_on_cpu` / `layered_at`,
`nest_is_plain`, `simple_transition`, and `FxOp::eval(..).filter(FxOp::gpu_ok)` per effect to
name the culprit. Make those `pub(crate)` where needed; do not write a second copy of their
rules. It must stay cheap, because stage 3 calls the same question from the render bar on the
interface thread: no rasterising and no table building (see stage 3, "The question must be
cheap"). Each clip is sampled at its first tick, its middle and its last tick (a keyframed value
that leaves the supported range only between samples is missed; say so in the module comment).

Reasons to produce, in this fork's terms (the exporter's list differs, its coverage differs):

| Case | Whole frame? |
|---|---|
| HDR or colour-managed sequence | yes, one entry for the whole range |
| Adjustment layer: an effect with no GPU version, or one the GPU declines with these settings (name it) | yes |
| Adjustment layer: blend mode other than Normal | yes |
| Transition with no GPU version; transition on an adjustment layer | yes |
| Media clip: an effect with no GPU version, or declined with these settings (name it) | no |
| Media clip: Frame Blending / Optical Flow on a speed-changed or rate-conformed clip | no |
| Nested sequence or title with standard effects or opacity masks | no |
| Inside a nested sequence: any of the above, prefixed with the inner clip's name | as the inner case |

"Declined with these settings" covers, for Color Grading: a LUT cube over 65 a side, an HSL
Denoise window over 3 px. Say which when it can be told cheaply; otherwise name the effect.

Bounds (AGENTS.md §0): nested sequences walk with the depth limit the planner uses; the result
is capped (for example 2 000 entries, with a final entry saying the list was cut); ranges are
clamped; no panics on hostile projects.

Reason strings are written in English as plain `&'static str` / `format!` in `crates/render`
(that crate has no translations). Stage 3 decides how they are shown.

## The command

Everything in FilmCraft is a command (AGENTS.md §3.2), and Bruno's agents drive the app through
the control channel and MCP. Add a read-only command in `crates/engine`, following an existing
read-only command of a sequence as the model (id, label, menu path, params, `enabled()`,
`run()`):

- id: `sequence.cpuReport` (check the naming of neighbouring commands and follow it);
- params: optional sequence (default: the active one), optional `start` / `end` (default: the
  whole sequence);
- result: the list as JSON, times as ticks and as timecode, plus `count` and `wholeFrameCount`;
- device: `crates/engine` cannot see the GPU. Use the capabilities of the device the app
  renders with when the UI provides them, else a `DeviceCaps` that assumes the effect stage is
  present, and return which was used (`"device": "actual" | "assumed"`).

Document it where the other commands are documented (`docs/control-protocol.md`).

## Checks

- `crates/render` tests on small synthetic projects: a clean project gives an empty list; each
  row of the table above is produced with the right track, range and `whole_frame`; a masked
  GPU effect (stage 1) is *not* reported; a cyclic nest and absurd ranges do not panic.
- Agreement test: for every reported `whole_frame` entry, `plan::is_cpu_frame` is true at its
  sampled times, and for a clean project it is false at those times.
- Engine test for the command, including hostile parameters (unknown sequence, `end < start`,
  non-finite numbers).
- On the real projects, read-only: run the command on `FULL TEST 02.fcproj` and on
  `FX Twirl ADJ.fcproj` / `FX Twirl CAM.fcproj` and record what it lists in `results.md`. Twirl
  has no GPU version, so the ADJ project must report whole frames and the CAM project layers.
  It must take well under a second.
