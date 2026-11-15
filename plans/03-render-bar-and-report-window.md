# Stage 3: the render bar asks the planner, and the report is shown in the interface

Decided by Bruno on 2026-11-15: the coloured bar above the timeline is the automatic warning.
It replaces the earlier idea of a separate mark on the Program Monitor.

## Today

The 2-pixel render bar on the timeline ruler (`crates/ui-egui/src/panels/timeline/ruler.rs`)
shows, per segment, red ("render to play back in real time"), yellow ("should play back in real
time"), green (a rendered preview exists) or nothing (a native clip). The colour comes from
`crates/render/src/preview.rs`: `segments` cuts the timeline at every clip and transition edge,
and `estimate_cost` adds up fixed figures per layer; over 1.5 × the frame duration is red.

That estimate never asks the planner. It assumes upstream's original behaviour:

- `layer_cost`: a media clip with any standard effect or a non-Normal blend mode "leaves the
  GPU fast path" and pays 10 ms (at 1080p);
- every adjustment layer pays 10 ms for a whole-frame CPU render, plus its effects;
- `effect_cost_ms`: each effect costs 6, 20 or 50 ms, from CPU timings on another machine.

So after rounds 1 and 2 the bar is still red over stretches that this fork plays in full: a
Color Grading alone (50 ms + 10 ms) exceeds the 25 ms budget of a 60 fps sequence. And every
future optimization would need that table edited by hand.

The same result (`Segment::need`) also decides what `Render Effects In to Out` renders
(`crates/engine/src/previews.rs`), so today that command renders previews nobody needs.

## Change

### A. The estimate asks the planner

For each layer of a segment, `estimate_cost` asks the question stage 2 already answers ("does
this item render on the CPU here, and does it take the whole frame?"), at the segment's first
frame, middle and last frame, and takes the worst answer.

| Planner's answer | Cost counted |
|---|---|
| Whole frame on the CPU (stage 2's `whole_frame` cases) | the segment is red, whatever the sum |
| This layer on the CPU | as today: 10 ms + `effect_cost_ms` per effect (the table stays as the CPU figures) |
| Layer or adjustment layer on the GPU | decode as today, plus a small GPU figure per effect (below) |

GPU figures: 1 ms per effect at 1080p, and 10 ms for a Color Grading that uses more than its
Basic, Creative and Vignette controls (measured in round 2: about 12 ms of GPU time per frame
for one with every section on). Two constants, in `preview.rs`, with the measurement cited.
Scaled by sequence pixels like the rest.

After this, moving an effect to the GPU changes the bar by itself: no table to edit.

**The question must be cheap.** `segments` runs on the interface thread each time the project
changes (the result is kept per project snapshot in `PreviewStore::segments`). It may not
rasterise or build tables. Today `adjustment_ops` builds the layer's weight plane
(`adjustment_plane`), and after stage 1 a masked effect rasterises its coverage: both are a
full-frame image. Split those planner functions into "can the GPU run this?" and "build the
ops", and have stage 2's `slow_paths` and this estimate use only the first. Check also what
`FxOp::eval` builds for Color Grading (LUT and curve tables) and VR Digital Glitch (row table);
if the gate needs their result, cache or skip the table.

**Device.** `crates/render` and `crates/engine` cannot see the GPU. The estimate assumes a
device with the effect stage, as stage 2's command does without a device. A machine without it
is not this fork's target.

**The hash is untouched.** `Segment::hash` names rendered previews; nothing here may change it,
or every existing preview is invalidated.

### B. The bar says why

The bar's tooltip keeps its sentence and, on a red segment, adds the reasons from stage 2 for
that stretch: clip name, track and reason, at most three lines and "…and N more". Computed when
the pointer rests on the bar, not while drawing it.

### C. The list, as a window

`Sequence ▸ CPU Report…` runs the stage 2 command for the active sequence and opens a window:

- a first line: "Nothing in this sequence renders on the CPU." or "N clips render on the CPU; M
  of them take the whole frame.";
- one row per entry, whole-frame entries first: track, clip name, start timecode, duration,
  reason; a click moves the playhead there and selects the clip;
- a Refresh button; the list is computed when the window opens and on Refresh.

The bar is 2 pixels high and its tooltip needs a steady pointer; the window is the readable
form of the same information. Part C is independent of A and B and can be dropped or done later.

### Rules of this codebase that apply (AGENTS.md §3.2)

- every new widget registers an automation id; the window's state is serde;
- interface text goes through `tl!` / `tlf!` with Spanish rows in
  `crates/ui-egui/src/i18n/es.tsv`; the reasons come from `crates/render` in English and are
  shown as they are;
- no panics: an empty or failed report shows a message;
- build the window like the closest existing list window opened from a menu.

## Checks

- `crates/render/src/preview_tests.rs`: a GPU effect on a clip, and an adjustment layer with a
  GPU effect (also moved off centre), are yellow at 60 fps; Twirl on a clip is counted as a CPU
  layer; Twirl on an adjustment layer is red; a full Color Grading is yellow, two of them on one
  frame are red; the hash of every segment of a fixture is the same before and after this
  stage. Upstream tests there that assert the old figures will fail: change them and list them
  in `UPDATING.md` with the other tests changed on purpose.
- Agreement with the benchmark, on `FULL TEST 02.fcproj` (read-only): compare the bar's colours
  with the 10-second windows in `results.md`. The windows that show 528 or more frames of 600
  should be yellow or uncoloured; record how many segments are red before and after, and say
  which stretches remain red and why.
- Cost: time `preview::segments` on `FULL TEST 02.fcproj` before and after (an ignored test or
  a few lines in the benchmark example, release build). It must not grow by more than a few
  milliseconds; record both numbers. If it does, the question is doing work it should not.
- `Render Effects In to Out` on a short range of a copy of a test project renders only the
  segments that are now red.
- `cargo test --release -p filmcraft-ui-egui --lib`: the tooltip lists the reasons; the window
  opens with a synthetic project, lists the entries and a click moves the playhead; the i18n
  tests pass.
- In the running app (control channel + `ui.screenshot`, see `docs/agents.md`), with
  `FX Twirl ADJ.fcproj` and `FULL TEST 02.fcproj`: the bar is red over the Twirl layers and not
  red over the zoom layers. Say in the report whether this was run.
- The benchmark on window 140 s of `FULL TEST 02.fcproj` is unchanged (558 of 600).
