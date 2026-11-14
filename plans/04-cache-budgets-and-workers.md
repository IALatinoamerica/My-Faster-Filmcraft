# 04 — Cache budgets and worker count for a large machine

## Problem

Several limits are constants sized for small machines. This one has 62 GB of RAM and 24 threads.

| Limit | Where | Value | Effect on this workload |
|---|---|---|---|
| Decoded frames per source | `gop.rs` `FRAME_CACHE_CEILING` | 384 MB | 123 frames of 1080p: barely one 120-frame GOP. Scrubbing back across a keyframe evicts the GOP just decoded. |
| Decoded frames, all sources | `gop.rs` `FRAME_BUDGET` | 1 GB | About 2.7 GOPs in total across screen + camera sources. |
| Finished GPU plans | `frames.rs` `PlanCache::MAX` / `BUDGET` | 96 plans / 1 GB | 1.6 s of 60 fps frames. A scrub back past that re-plans every frame. |
| Frame workers | `frames.rs` `default_workers` | clamp(2, 6) | 6 of 24 threads. Only matters while frames still render on the CPU. |

`perf.stats` reports `cacheBudgetMB` from the constant; Settings ▸ Memory only sets the RGBA frame
cache (`set_cache_budget`), which the GPU path does not use.

## Change

1. `crates/codecs/src/gop.rs`: make the shared budget and the per-source ceiling settable at run
   time (`set_frame_budget(bytes)`, atomics read where the constants are read now; the constants
   stay as defaults and as the hard floor for the VP9 `show_existing_frame` guard).
2. `crates/engine/src/settings/schema.rs` + `crates/ui-egui/src/app/prefs.rs`: derive both from the
   existing Settings ▸ Memory value when preferences are applied (for example shared budget =
   memory setting, per-source ceiling = a third of it). No new setting.
3. `crates/ui-egui/src/frames.rs`: `PlanCache` limits follow the same value (count cap scaled with
   the byte budget).
4. `default_workers`: leave as it is. After plans 01 and 02 the jobs are bound by one decoder per
   source, not by worker count. Revisit only if plan 00 shows the queue backing up.

## Tests

- `gop/tests.rs`: with a larger budget two GOPs of one source stay cached; with the default the
  behaviour is unchanged; a zero or huge value is clamped.
- `frames.rs`: `PlanCache` evicts by the configured limits.

## Files touched

`crates/codecs/src/gop.rs`, `crates/codecs/src/lib.rs` (export), `crates/engine/src/settings/schema.rs`,
`crates/engine/src/perf.rs`, `crates/ui-egui/src/app/prefs.rs`, `crates/ui-egui/src/frames.rs`.
About 80 lines.

## Expected result

Scrubbing back and forth over the last several seconds becomes cache hits instead of re-decodes.
No change to first-time seeks or to playback rate.

## Risk

Low. Memory use rises by what the setting allows; the caps on live hardware decoders are untouched.
