//! Render-bar segments: where a sequence's video changes structure, what each stretch costs to
//! play, and a content hash that names its render preview.
//!
//! # Segments
//! The timeline is cut at every edge of every clip and transition on the enabled video tracks
//! (snapped up to the sequence frame grid). Between two cuts the *structure* of the picture is
//! constant: the same clips and transitions are visible on the same tracks. Each such stretch with
//! at least one visible layer is a [`Segment`]; gaps produce no segment (no bar, nothing to render).
//!
//! # Content hash
//! [`Segment::hash`] is a 128-bit FNV-1a hash of everything that affects the segment's pixels,
//! expressed *relative to the segment start*: sequence settings, the frame count, and for each
//! visible layer (bottom to top, with its track index) the track item (source in, speed, reverse,
//! frame hold, effects with every keyframe…, its timeline offset from the segment start) and its
//! source (the media reference and stream info, interpretation, nested sequences recursively,
//! adjustment layer settings). Ids, names, labels, markers, link/group membership and source
//! In/Out marks are excluded because they don't change pixels. Because positions are relative, a
//! ripple edit that moves a segment along the timeline keeps its hash (and its preview); only the
//! segments whose content changed get a new hash. Effects that print absolute sequence time (the
//! Timecode effect) also hash the absolute start frame. The hash is computed from the serde JSON
//! form of those values, so it is stable across save/load and across processes.
//!
//! # Cost estimate (yellow vs red)
//! [`Segment::cost_ms`] estimates the milliseconds one frame takes on the playback path, at full
//! sequence resolution. Which path each layer takes is the frame planner's decision, asked through
//! [`crate::slow_paths`] at the segment's first, middle and last tick (this fork: the estimate
//! used to assume that any standard effect and any adjustment layer rendered on the CPU). The sum
//! over visible layers of:
//! * **decode**: per source codec at 1080p — H.264 3 ms, HEVC 6 ms, ProRes 2.5 ms, MJPEG 4 ms,
//!   other files 4 ms, stills/generators 1 ms — scaled by source pixels / 1080p pixels;
//! * **a layer the GPU draws**: 0.5 ms, plus 1 ms per standard effect on the effect stage
//!   (10 ms for a Color Grading beyond its Basic, Creative and Vignette controls), scaled by
//!   sequence pixels / 1080p pixels and counted against one frame duration, not the budget's 1.5; an adjustment layer the GPU runs costs its effects alone;
//! * **a layer the CPU renders**: 10 ms (1080p) for linear conversion, transform and compositing,
//!   plus per enabled standard effect a measured tier cost at 1080p (6 / 20 / 50 ms, see
//!   [`effect_cost_ms`]), scaled the same way;
//! * **transitions**: the ones the compositor mixes 1 ms; any other renders both sides on the
//!   CPU: 12 ms plus the CPU path of both layers.
//!
//! A segment is **red** when something in it sends the whole frame to the CPU (an adjustment
//! layer or transition the GPU can't run, an HDR sequence) or when the estimate exceeds [`REALTIME_BUDGET`] × the frame duration,
//! **yellow** when it plays in real time but is not "native", and has **no bar** when it is a single
//! clip with no standard effects, untouched Motion/Opacity, normal speed, and a source whose frame
//! size and rate match the sequence. Green (a valid preview exists) is decided by the preview
//! store, which knows which hashes have files.

use filmcraft_project::{ClipId, ItemId, ItemKind, Project, Sequence, TrackItem, TrackKind, Transition};
use filmcraft_time::{FrameRate, Tick};
use serde::Serialize;

/// Bump when the hash layout or the preview format changes (invalidates every preview).
pub const HASH_VERSION: &str = "filmcraft-preview-v1";

/// Multiple of the frame duration a frame may take before a segment is marked red. Playback
/// renders ahead on several worker threads (2–6), so a frame can take somewhat longer than its
/// display duration and still keep up.
///
/// Calibration (M4 Pro, shared with other jobs; 1080p 23.976 sequence, Full playback resolution,
/// 8 s of playback ≈ 192 frames, counted by the Program monitor's dropped-frame stats):
/// * 1080p H.264 High clip, no effects (no bar, est. 3.5 ms): 11 dropped;
/// * same clip + Color Grading, Sharpen, Levels, Tint (red, est. 183 ms): 189–191 dropped, ≤ 17 on time;
/// * after Render Effects In to Out (green; ProRes preview rendered at ~3 fps): 25–46 dropped on
///   the first play right after the render (frame workers still busy with live renders queued
///   before it), then 0–1 dropped on every later play.
pub const REALTIME_BUDGET: f64 = 1.5;

const HD_PIXELS: f64 = 1920.0 * 1080.0;

/// How a segment plays without a preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Need {
    /// Plays natively (no bar).
    None,
    /// Should play in real time (yellow).
    Realtime,
    /// Probably drops frames (red).
    Render,
}

/// A stretch of the timeline with a constant set of visible layers.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Segment {
    /// First frame (sequence frame index) and number of frames.
    pub first_frame: i64,
    pub frames: i64,
    /// Timeline range (frame-aligned).
    pub start: Tick,
    pub end: Tick,
    /// Content hash (32 hex digits) naming the preview file.
    pub hash: String,
    pub need: Need,
    /// Estimated per-frame playback cost at full resolution, in milliseconds.
    pub cost_ms: f64,
    /// Visible clips (including both sides of transitions), for Render Selection.
    pub clips: Vec<ClipId>,
}

impl Segment {
    pub fn contains_frame(&self, f: i64) -> bool {
        f >= self.first_frame && f < self.first_frame + self.frames
    }
}

/// First frame whose start is at or after `t`.
fn frame_ceil(rate: FrameRate, t: Tick) -> i64 {
    let f = rate.frame_at(t);
    if rate.tick_of(f) < t { f + 1 } else { f }
}

enum Layer<'a> {
    Item(usize, &'a TrackItem),
    Transition(usize, &'a Transition, Option<&'a TrackItem>, Option<&'a TrackItem>),
}

fn layers_at<'a>(seq: &'a Sequence, t: Tick) -> Vec<Layer<'a>> {
    let mut out = Vec::new();
    for (ti, tr) in seq.video_tracks.iter().enumerate() {
        if !tr.enabled {
            continue;
        }
        if let Some(x) = tr.transitions.iter().find(|x| x.range().contains(t)) {
            let a = x.from.and_then(|id| tr.item(id)).filter(|i| i.enabled);
            let b = x.to.and_then(|id| tr.item(id)).filter(|i| i.enabled);
            if a.is_some() || b.is_some() {
                out.push(Layer::Transition(ti, x, a, b));
            }
            continue;
        }
        if let Some(it) = tr.item_at(t).filter(|i| i.enabled) {
            out.push(Layer::Item(ti, it));
        }
    }
    out
}

/// Compute the render-bar segments of sequence `seq_id`.
pub fn video_segments(project: &Project, seq_id: ItemId) -> Vec<Segment> {
    let Some(seq) = project.sequence(seq_id) else { return Vec::new() };
    let rate = seq.settings.frame_rate;
    let mut cuts: Vec<i64> = Vec::new();
    for tr in seq.video_tracks.iter().filter(|t| t.enabled) {
        for it in &tr.items {
            cuts.push(frame_ceil(rate, it.start));
            cuts.push(frame_ceil(rate, it.end()));
        }
        for x in &tr.transitions {
            cuts.push(frame_ceil(rate, x.start));
            cuts.push(frame_ceil(rate, x.end()));
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = Vec::new();
    for w in cuts.windows(2) {
        let (fa, fb) = (w[0], w[1]);
        if fb <= fa {
            continue;
        }
        let t = rate.tick_of(fa);
        let layers = layers_at(seq, t);
        if layers.is_empty() {
            continue;
        }
        let mut h = Fnv128::new();
        h.json(&HASH_VERSION);
        h.json(&seq.settings);
        h.json(&(fb - fa));
        let mut clips = Vec::new();
        let mut absolute = false;
        for l in &layers {
            match l {
                Layer::Item(ti, it) => {
                    h.json(&("item", ti));
                    hash_item(&mut h, project, it, t, TrackKind::Video, 0);
                    absolute |= prints_time(it);
                    clips.push(it.id);
                }
                Layer::Transition(ti, x, a, b) => {
                    let mut xc = (*x).clone();
                    xc.id = Default::default();
                    xc.start -= t;
                    xc.from = None;
                    xc.to = None;
                    h.json(&("transition", ti, &xc));
                    for side in [a, b] {
                        match side {
                            Some(it) => {
                                hash_item(&mut h, project, it, t, TrackKind::Video, 0);
                                absolute |= prints_time(it);
                                clips.push(it.id);
                            }
                            None => h.json(&"none"),
                        }
                    }
                }
            }
        }
        if absolute {
            h.json(&("absolute", fa));
        }
        let (cost_ms, whole_frame_on_cpu) = estimate_cost(project, seq, &layers, (t, rate.tick_of(fb)));
        let budget = rate.frame_duration().seconds() * 1000.0 * REALTIME_BUDGET;
        let need = if whole_frame_on_cpu || cost_ms > budget {
            Need::Render
        } else if layers.len() == 1 && matches!(layers[0], Layer::Item(_, it) if native(project, seq, it)) {
            Need::None
        } else {
            Need::Realtime
        };
        out.push(Segment { first_frame: fa, frames: fb - fa, start: t, end: rate.tick_of(fb), hash: h.hex(), need, cost_ms, clips });
    }
    out
}

/// The segment containing frame `f` (segments are sorted and disjoint).
pub fn segment_at(segments: &[Segment], f: i64) -> Option<&Segment> {
    let i = segments.partition_point(|s| s.first_frame <= f);
    i.checked_sub(1).map(|i| &segments[i]).filter(|s| s.contains_frame(f))
}

/// A stretch of sequence audio with constant structure (Render Audio), in sequence samples.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AudioSegment {
    pub first_sample: i64,
    pub samples: i64,
    pub hash: String,
}

/// Audio segments of sequence `seq_id`: cut at every audio clip and audio transition edge. The
/// hash covers the mix-wide state (master volume/effects, every track's mute/solo/volume/pan/
/// effects) and, per track, the clip or transition heard, relative to the segment start. Effect
/// tails (delay, reverb) carried across a cut from the previous segment are not part of the hash.
pub fn audio_segments(project: &Project, seq_id: ItemId) -> Vec<AudioSegment> {
    let Some(seq) = project.sequence(seq_id) else { return Vec::new() };
    let sr = seq.settings.sample_rate.max(1) as i64;
    let ceil = |t: Tick| {
        let s = t.to_units_floor(sr);
        if Tick::from_units(s, sr) < t { s + 1 } else { s }
    };
    let mut cuts: Vec<i64> = Vec::new();
    for tr in &seq.audio_tracks {
        for it in &tr.items {
            cuts.push(ceil(it.start));
            cuts.push(ceil(it.end()));
        }
        for x in &tr.transitions {
            cuts.push(ceil(x.start));
            cuts.push(ceil(x.end()));
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    let any_solo = seq.audio_tracks.iter().any(|t| t.solo);
    let mut global = Fnv128::new();
    global.json(&(HASH_VERSION, "audio", sr, seq.master_volume_db, &seq.master_effects, &seq.master_mixer, any_solo));
    for (ti, tr) in seq.audio_tracks.iter().enumerate() {
        global.json(&(ti, tr.muted, tr.solo, tr.volume_db, tr.pan, tr.channels, &tr.effects, &tr.mixer));
    }
    // submixes (routing, inserts, automation) affect every segment
    global.json(&seq.submix_tracks);
    let mut out = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b <= a {
            continue;
        }
        let t = Tick::from_units(a, sr);
        let mut h = Fnv128(global.0);
        h.json(&(b - a));
        let mut heard = false;
        for (ti, tr) in seq.audio_tracks.iter().enumerate() {
            if tr.muted || (any_solo && !tr.solo) {
                continue;
            }
            if let Some(x) = tr.transitions.iter().find(|x| x.range().contains(t)) {
                let mut xc = x.clone();
                xc.id = Default::default();
                xc.start -= t;
                xc.from = None;
                xc.to = None;
                h.json(&("transition", ti, &xc));
                for side in [x.from, x.to] {
                    match side.and_then(|id| tr.item(id)).filter(|i| i.enabled) {
                        Some(it) => {
                            hash_item(&mut h, project, it, t, TrackKind::Audio, 0);
                            heard = true;
                        }
                        None => h.json(&"none"),
                    }
                }
            } else if let Some(it) = tr.item_at(t).filter(|i| i.enabled) {
                h.json(&("item", ti));
                hash_item(&mut h, project, it, t, TrackKind::Audio, 0);
                heard = true;
            }
        }
        if heard {
            out.push(AudioSegment { first_sample: a, samples: b - a, hash: h.hex() });
        }
    }
    out
}

fn prints_time(it: &TrackItem) -> bool {
    it.effects.iter().any(|e| e.enabled && e.effect == "timecode")
}

/// Hash a clip and its source as heard (`kind` Audio) or seen (`kind` Video): a nested sequence
/// contributes only the tracks of that kind, so editing its picture keeps its sound's previews.
fn hash_item(h: &mut Fnv128, project: &Project, it: &TrackItem, origin: Tick, kind: TrackKind, depth: u32) {
    let mut c = it.clone();
    c.id = Default::default();
    c.start -= origin;
    c.label = filmcraft_project::Label::ALL[0];
    c.markers.clear();
    c.link = None;
    c.group = None;
    if !c.effects.iter().any(|e| e.effect == "clip_name") {
        c.name.clear();
    }
    h.json(&c);
    hash_source(h, project, it.item, kind, depth);
}

fn hash_source(h: &mut Fnv128, project: &Project, id: ItemId, kind: TrackKind, depth: u32) {
    let Some(pi) = project.item(id) else {
        h.json(&"missing");
        return;
    };
    if depth > 8 {
        h.json(&"deep");
        return;
    }
    match &pi.kind {
        ItemKind::Media(m) => {
            let mut c = m.clone();
            c.mark_in = None;
            c.mark_out = None;
            c.markers.clear();
            h.json(&("media", &c));
            if matches!(c.media, filmcraft_project::MediaRef::Generator(_)) {
                // generators may draw their name (demo footage)
                h.json(&pi.name);
            }
        }
        ItemKind::Subclip { parent, range, .. } => {
            h.json(&("subclip", range));
            hash_source(h, project, *parent, kind, depth + 1);
        }
        ItemKind::AdjustmentLayer { .. } => h.json(&("adjustment", &pi.kind)),
        ItemKind::Graphic { .. } => h.json(&("graphic", &pi.kind)),
        ItemKind::Sequence(s) => {
            h.json(&("sequence", &s.settings));
            if kind == TrackKind::Audio {
                // the nest's sound is the nested sequence's whole mix (see `audio_segments`)
                h.json(&("mix", s.master_volume_db, &s.master_effects, &s.master_mixer, &s.submix_tracks, &s.multicam));
                for (ti, tr) in s.audio_tracks.iter().enumerate() {
                    h.json(&("track", ti, tr.muted, tr.solo, tr.volume_db, tr.pan, tr.channels, &tr.effects, &tr.mixer, tr.items.len()));
                    for it in &tr.items {
                        hash_item(h, project, it, Tick::ZERO, kind, depth + 1);
                    }
                    for x in &tr.transitions {
                        let mut xc = x.clone();
                        xc.id = Default::default();
                        h.json(&xc);
                    }
                }
                return;
            }
            // a nested sequence's captions are drawn into its picture
            h.json(&("captions", &s.caption_tracks));
            for (ti, tr) in s.video_tracks.iter().enumerate() {
                h.json(&("track", ti, tr.enabled, tr.items.len()));
                for it in &tr.items {
                    hash_item(h, project, it, Tick::ZERO, kind, depth + 1);
                }
                for x in &tr.transitions {
                    let mut xc = x.clone();
                    xc.id = Default::default();
                    h.json(&xc);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- cost

/// Estimated cost of one standard video effect on a 1080p frame (CPU reference), in ms.
///
/// Three tiers from timing every effect at its defaults on a 1920×1080 frame on an M4 Pro
/// (`cargo test -p filmcraft-render --release -- --ignored effect_costs --nocapture` prints the
/// table): heavy ≥ 30 ms (blurs/sharpen at real radii, Color Grading, Levels, keyers…), medium 10–30 ms,
/// light < 10 ms. Unknown ids count as medium.
pub fn effect_cost_ms(id: &str) -> f64 {
    match id {
        "sharpen"
        | "unsharp_mask"
        | "posterize"
        | "leave_color"
        | "levels"
        | "drop_shadow"
        | "emboss"
        | "invert"
        | "ultra_key"
        | "four_color_gradient"
        | "lumetri"
        | "color_balance"
        | "gamma_correction"
        | "wave_warp"
        | "gaussian_blur"
        | "directional_blur"
        | "camera_blur"
        | "median"
        // M5.11 (multi-pass, multi-frame or analysis effects)
        | "bokeh_blur"
        | "compound_blur"
        | "focus_blur"
        | "channel_blur"
        | "turbulent_displace"
        | "warp_stabilizer"
        | "auto_reframe"
        | "echo"
        | "echo_glow"
        | "glint"
        | "wonder_glow"
        | "edge_glow"
        | "volumetric_rays"
        | "brush_strokes"
        | "roughen_edges"
        | "lighting_effects"
        | "vr_blur"
        | "vr_glow"
        | "vr_denoise"
        | "vr_sharpen" => 50.0,
        "vertical_flip" | "horizontal_flip" | "crop" | "timecode" | "black_white" | "extract" | "clip_name" | "strobe" | "mosaic" | "mirror"
        | "simple_text" | "track_matte" => 6.0,
        _ => 20.0,
    }
}

/// Source references [`decode_cost_ms`] follows before giving up (nested sequences stop at 8).
const MAX_COST_DEPTH: u32 = 64;

fn decode_cost_ms(project: &Project, id: ItemId, depth: u32) -> f64 {
    // every source kind, not only sequences: subclip parents can form a cycle in a damaged project
    if depth > MAX_COST_DEPTH {
        return 0.0;
    }
    let Some(pi) = project.item(id) else { return 0.0 };
    match &pi.kind {
        ItemKind::Media(m) => {
            let Some(v) = &m.info.video else { return 0.0 };
            let px = (v.width as f64 * v.height as f64) / HD_PIXELS;
            let c = v.codec.to_ascii_lowercase();
            let base = if matches!(m.info.kind, filmcraft_media::MediaKind::Still | filmcraft_media::MediaKind::Synthetic)
                || matches!(m.media, filmcraft_project::MediaRef::Generator(_))
            {
                1.0
            } else if c.contains("264") || c.contains("avc") {
                3.0
            } else if c.contains("hevc") || c.contains("265") {
                6.0
            } else if c.contains("prores") {
                2.5
            } else {
                // MJPEG and anything else
                4.0
            };
            base * px
        }
        ItemKind::Subclip { parent, .. } => decode_cost_ms(project, *parent, depth + 1),
        ItemKind::AdjustmentLayer { .. } => 0.0,
        // vector layers are drawn on the CPU (cached while static)
        ItemKind::Graphic { .. } => 1.5,
        ItemKind::Sequence(s) => {
            if depth > 8 {
                return 0.0;
            }
            // a nested sequence renders its worst segment on the CPU
            let id_cost = s
                .video_tracks
                .iter()
                .filter(|t| t.enabled)
                .flat_map(|t| t.items.iter())
                .map(|it| decode_cost_ms(project, it.item, depth + 1) + item_fx_cost(it, 1.0))
                .fold(0.0, f64::max);
            10.0 + id_cost
        }
    }
}

fn item_fx_cost(it: &TrackItem, px: f64) -> f64 {
    it.effects.iter().filter(|e| e.enabled && e.def().is_some_and(|d| !d.intrinsic)).map(|e| effect_cost_ms(&e.effect) * px).sum()
}

/// Cost of a standard effect the GPU's effect stage runs, on a 1080p frame, in ms.
const GPU_EFFECT_MS: f64 = 1.0;
/// Cost of a Color Grading that uses more than its Basic, Creative and Vignette controls (a
/// sequence of passes on the effect stage). Measured on `FX Color Grading CAM/ADJ.fcproj` at
/// 1080p (RX 6700 XT): the GPU pass of a frame takes about 12 ms with one, 2 ms without.
const GPU_GRADE_MS: f64 = 10.0;

/// The cost of the enabled standard effects of `it` when the effect stage runs them. The budget
/// allows [`REALTIME_BUDGET`] frame durations because frames are rendered by jobs that run side
/// by side; the passes of the effect stage run one after another on one device, so their time is
/// counted against a single frame duration (scaled up by the same factor here).
fn gpu_fx_cost(project: &Project, seq: &Sequence, it: &TrackItem, t: Tick, px: f64) -> f64 {
    it.effects
        .iter()
        .filter(|e| e.enabled && e.def().is_some_and(|d| !d.intrinsic))
        .map(|e| if e.effect == "lumetri" && crate::slow_paths::is_full_grade(project, seq, it, e, t) { GPU_GRADE_MS * px } else { GPU_EFFECT_MS * px })
        .sum::<f64>()
        * REALTIME_BUDGET
}

/// The cost of a layer the CPU renders: its decode, the CPU layer path and its effects at their
/// CPU cost. An adjustment layer decodes nothing and renders the whole frame.
fn cpu_layer_cost(project: &Project, it: &TrackItem, px: f64) -> f64 {
    if project.item(it.item).is_some_and(|p| matches!(p.kind, ItemKind::AdjustmentLayer { .. })) {
        return 10.0 * px + item_fx_cost(it, px);
    }
    decode_cost_ms(project, it.item, 0) + 10.0 * px + item_fx_cost(it, px)
}

/// The cost of a layer the GPU draws at `t`: its decode, the compositor and its effects on the
/// effect stage. A nested sequence the compositor draws as layers costs what its own layers do.
fn gpu_layer_cost(project: &Project, seq: &Sequence, it: &TrackItem, t: Tick, px: f64, depth: u32) -> f64 {
    match project.item(it.item).map(|p| &p.kind) {
        Some(ItemKind::AdjustmentLayer { .. }) => gpu_fx_cost(project, seq, it, t, px),
        Some(ItemKind::Sequence(nested)) if depth < crate::MAX_NEST_DEPTH => {
            let ft = it.source_time_at(t);
            let angle = it.multicam_angle(nested).and_then(|a| nested.angle_video_track_index(a));
            let inner = nested
                .video_tracks
                .iter()
                .enumerate()
                .filter(|(i, tr)| tr.enabled && angle.is_none_or(|a| a == *i))
                .filter_map(|(_, tr)| tr.item_at(ft).filter(|i| i.enabled))
                .map(|i| gpu_layer_cost(project, nested, i, ft, px, depth + 1))
                .sum::<f64>();
            0.5 + inner
        }
        _ => decode_cost_ms(project, it.item, 0) + 0.5 + gpu_fx_cost(project, seq, it, t, px),
    }
}

/// The cost of the layer of `it` over the segment from `a` to `b`, and whether it sends every
/// frame of the segment to the CPU. The planner decides which path the layer takes
/// ([`crate::slow_paths::item_reason`], asked at the segment's first, middle and last tick): the
/// table of CPU effect costs counts only for a layer the CPU really renders.
fn layer_cost(project: &Project, seq: &Sequence, it: &TrackItem, (a, b): (Tick, Tick), px: f64, caps: crate::plan::DeviceCaps) -> (f64, bool) {
    let mut cpu: Option<bool> = None;
    for t in crate::slow_paths::samples(a, b) {
        if let Some((_, whole)) = crate::slow_paths::item_reason(project, seq, it, t, caps, 0) {
            cpu = Some(cpu.unwrap_or(false) || whole);
        }
    }
    match cpu {
        Some(whole) => (cpu_layer_cost(project, it, px), whole),
        None => (gpu_layer_cost(project, seq, it, a, px, 0), false),
    }
}

/// The estimated cost of one frame of the segment from `a` to `b`, and whether something in it
/// sends the whole frame to the CPU (the segment is then red whatever the sum).
fn estimate_cost(project: &Project, seq: &Sequence, layers: &[Layer], span: (Tick, Tick)) -> (f64, bool) {
    let px = (seq.settings.width as f64 * seq.settings.height as f64) / HD_PIXELS;
    // the device that composites the monitors; before it is known, one that runs every step
    let caps = crate::plan::noted_device().unwrap_or(crate::plan::DeviceCaps::ALL);
    let mut total = 0.0;
    let mut whole = crate::slow_paths::sequence_reason(seq).is_some();
    for l in layers {
        match l {
            Layer::Item(_, it) => {
                let (cost, cpu) = layer_cost(project, seq, it, span, px, caps);
                total += cost;
                whole |= cpu;
            }
            Layer::Transition(ti, x, a, b) => {
                let on_cpu = seq.video_tracks.get(*ti).is_some_and(|tr| crate::plan::transition_cpu(project, tr, x).is_some());
                for it in [a, b].into_iter().flatten() {
                    let (cost, cpu) = layer_cost(project, seq, it, span, px, caps);
                    total += if on_cpu { cpu_layer_cost(project, it, px) } else { cost };
                    whole |= cpu;
                }
                total += if on_cpu { 12.0 * px } else { 1.0 };
                whole |= on_cpu;
            }
        }
    }
    (total, whole)
}

/// A single clip that plays natively: no standard effects, untouched fixed effects, normal speed,
/// source frame size and rate equal to the sequence's.
fn native(project: &Project, seq: &Sequence, it: &TrackItem) -> bool {
    if it.has_standard_effects() || it.has_modified_intrinsics() || (it.speed - 1.0).abs() > 1e-9 || it.reverse || it.frame_hold.is_some() {
        return false;
    }
    match project.item(it.item).map(|p| &p.kind) {
        Some(ItemKind::Media(m)) => {
            m.info.video.as_ref().is_some_and(|v| v.width == seq.settings.width && v.height == seq.settings.height && m.frame_rate() == seq.settings.frame_rate)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------- hashing

/// 128-bit FNV-1a over the serde JSON form of values.
struct Fnv128(u128);

impl Fnv128 {
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013B;
    fn new() -> Self {
        Self(Self::OFFSET)
    }
    fn json<T: Serialize + ?Sized>(&mut self, v: &T) {
        let _ = serde_json::to_writer(&mut *self, v);
        self.0 ^= 0xff; // value separator
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }
    fn hex(&self) -> String {
        format!("{:032x}", self.0)
    }
}

impl std::io::Write for Fnv128 {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &b in buf {
            self.0 ^= b as u128;
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
