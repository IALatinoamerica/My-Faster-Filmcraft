# 03 — Seek latency: scrubbing and cuts

## Problem

The footage is HEVC with a keyframe every 120 frames and no B-frames. Showing a frame after a seek
means decoding from the previous keyframe: 60 pictures on average, 119 at worst, per source. Every
picture is a reference, so the existing "skip non-reference pictures while catching up"
(`gop.rs`, `is_disposable`) never applies to these files.

Three things multiply that cost:

1. **Sources are fetched one after another.** `push_tracks` in `crates/render/src/plan.rs` asks each
   track's source in turn, so a frame with screen + camera waits for two seeks in series.
2. **Every picture on the way is read back from the GPU.** `vaapi::hevc::Front::read_out` calls
   `vaGetImage`, deinterleaves NV12 and allocates a 3.1 MB frame for each of the up to 119 pictures
   before the wanted one, although a scrub only keeps the last 2 s (`SCRUB_KEEP_S`).
3. **Cuts are not prepared.** Playback prefetches 14 frames (233 ms at 60 fps,
   `frames::prefetch_depth`). The edit has a cut about every 6 s (452 clips on the two main tracks);
   after a cut to another file, or a jump of more than 48 samples in the same file
   (`CONTINUE_THROUGH`), a decoder is created or reset and decodes up to 119 pictures before the
   first frame of the new clip is ready.

## Change

### 3a. Fetch a frame's sources in parallel (small)

In `plan_frame`, before `push_tracks`, collect the `(source, FrameRequest)` pairs of the media
clips visible at `t` and request them on scoped plain threads (not rayon: see the deadlock note on
`isolated` in `crates/codecs/src/gop.rs`). `push_tracks` then finds them in the GOP caches.
Two-layer scrubs wait for the slower seek instead of the sum.

### 3b. No read-back for pictures nobody will show (medium)

- `filmcraft_codecs::VideoDecoder`: new defaulted method `set_output_from(&mut self, pts: Option<i64>)`
  (no-op for software decoders).
- `GopCache::decode_until` passes `late` (already computed from `late_before`).
- `crates/platform/src/vaapi/h264.rs`, `hevc.rs`: `read_out` skips `accel.read` for outputs with
  `pts < from`; the surface stays in the DPB as a reference, so decoding is unchanged.
- `HybridDecoder` forwards the call.

The pictures within `SCRUB_KEEP_S` of the target are still read back and cached, as today.

### 3c. Prepare the next cut during playback (medium)

- `FrameServer::schedule_playback` (`crates/ui-egui/src/frames.rs`): find the next edit point on the
  sequence within ~1.5 s of the playhead and queue the plan job of its first frame at the lowest
  prefetch priority.
- For a jump inside the same file, that request would reset the decoder playback is using. So the
  GOP cache needs a second decoder slot per source for a position ahead of the running one
  (`crates/codecs/src/gop.rs`), counted against `MAX_LIVE_DECODERS` / `HARD_DECODERS`. When playback
  reaches the cut, the slots swap.

3c is the largest part and touches the cache's decoder accounting. Do it only if plan 00 shows
dropped frames clustered at cuts after plans 01 and 02.

## Tests

- 3a: `plan_frame` of a two-track frame returns the same plan as before; a cancelled job returns
  promptly (no thread left behind).
- 3b: `crates/platform/tests/vaapi.rs` parity: with `set_output_from`, the pictures at and after
  `from` are bit-identical to a run without it; `crates/codecs/src/gop/tests.rs`: a seek with
  `late_before` stores only the pictures from `late` on.
- 3c: `gop/tests.rs`: a request ahead in the same source does not count as a seek for the running
  decoder; decoder counts stay within the caps.

## Files touched

3a: `crates/render/src/plan.rs`. 3b: `crates/codecs/src/video.rs`, `gop.rs`,
`crates/platform/src/vaapi/{h264,hevc}.rs`, `hybrid.rs`. 3c: `crates/ui-egui/src/frames.rs`,
`crates/codecs/src/gop.rs`.

## Expected result

Scrub time-to-frame roughly halved by 3a on two-source frames, and reduced again by 3b (the share
depends on how much of a seek is read-back on this GPU; plan 00 measures it). 3c removes the hitch
at cuts.

## Risk

3a low. 3b low to medium (new trait method; upstream touches these files often, so expect merge
conflicts in `gop.rs`). 3c medium.
