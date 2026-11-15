//! What the GPU will not draw in a sequence, found without decoding a frame.
//!
//! Playback is smooth while every layer is drawn by the GPU compositor. A clip or adjustment layer
//! the planner hands to the CPU renderer instead ([`crate::plan`]) can cost many frames, and one
//! such adjustment layer or transition takes the whole frame with it. [`slow_paths`] asks the
//! planner's own questions for every clip in a range and names the ones that render on the CPU,
//! and why. The render bar's estimate asks the same questions ([`item_reason`]).
//!
//! The questions are the planner's decisions alone: they rasterise no mask and build no table, so
//! they can be asked for every clip each time the project changes.
//!
//! Each clip is sampled at its first tick, its middle and its last tick. A keyframed value that
//! leaves the supported range only between those samples is missed, and so is anything that
//! depends on the decoded picture (the colour management of unusual media). The effects of a
//! media clip are evaluated for its stored size, not for the size it is decoded at.

use filmcraft_project::{EffectInstance, ItemId, ItemKind, Project, Sequence, TimeInterpolation, TrackItem};
use filmcraft_time::Tick;

use crate::Blend;
use crate::RenderOptions;
use crate::gpufx::FxOp;
use crate::plan::{AdjustmentCpu, DeviceCaps};

/// Most entries [`slow_paths`] returns; one more says the list was cut.
pub const MAX_ENTRIES: usize = 2000;

/// One clip, adjustment layer or transition that renders on the CPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlowPath {
    /// The video track of the sequence asked about, counted from 1 at the bottom; 0 for the
    /// sequence itself.
    pub track: usize,
    /// The clip's name (a transition's effect id).
    pub name: String,
    /// The affected part of the timeline, inside the asked range.
    pub start: Tick,
    pub end: Tick,
    /// Why, in plain words.
    pub reason: String,
    /// Every frame of this part renders entirely on the CPU, not just this layer.
    pub whole_frame: bool,
    /// The clip, when the entry is one (not a transition or the sequence).
    pub clip: Option<filmcraft_project::ClipId>,
}

/// What renders on the CPU in sequence `seq_id` between `start` and `end` (timeline ticks) on a
/// device that runs `caps`, in timeline order. At most [`MAX_ENTRIES`], plus one saying so.
pub fn slow_paths(project: &Project, seq_id: ItemId, start: Tick, end: Tick, caps: DeviceCaps) -> Vec<SlowPath> {
    let Some(seq) = project.sequence(seq_id) else { return Vec::new() };
    let mut out: Vec<SlowPath> = Vec::new();
    if end <= start {
        return out;
    }
    if let Some(reason) = sequence_reason(seq) {
        out.push(SlowPath { track: 0, name: "Sequence".into(), start, end, reason: reason.into(), whole_frame: true, clip: None });
        return out;
    }
    'tracks: for (index, track) in seq.video_tracks.iter().enumerate().filter(|(_, track)| track.enabled) {
        for trn in &track.transitions {
            let range = trn.range();
            let (a, b) = (range.start.max(start), range.end().min(end));
            if a < b
                && let Some(reason) = crate::plan::transition_cpu(project, track, trn)
            {
                let reason = format!("{reason}: its frames render on the CPU");
                out.push(SlowPath { track: index + 1, name: trn.effect.effect.clone(), start: a, end: b, reason, whole_frame: true, clip: None });
            }
        }
        for item in track.items.iter().filter(|item| item.enabled) {
            let (a, b) = (item.start.max(start), item.end().min(end));
            if a >= b {
                continue;
            }
            for (reason, whole_frame) in item_reasons(project, seq, item, a, b, caps) {
                out.push(SlowPath { track: index + 1, name: item.name.clone(), start: a, end: b, reason, whole_frame, clip: Some(item.id) });
            }
            if out.len() > MAX_ENTRIES {
                break 'tracks;
            }
        }
    }
    out.sort_by_key(|p| (p.start, p.track));
    if out.len() > MAX_ENTRIES {
        out.truncate(MAX_ENTRIES);
        let reason = format!("the list was cut at {MAX_ENTRIES} entries");
        out.push(SlowPath { track: 0, name: "…".into(), start, end, reason, whole_frame: false, clip: None });
    }
    out
}

/// Why every frame of `seq` renders on the CPU, whatever is on its tracks.
pub(crate) fn sequence_reason(seq: &Sequence) -> Option<&'static str> {
    (!seq.settings.color.is_plain()).then_some("the sequence is HDR or wide gamut: every frame renders on the CPU")
}

/// The times a clip showing from `a` to `b` is asked about: its first tick, its middle and its
/// last tick.
pub(crate) fn samples(a: Tick, b: Tick) -> [Tick; 3] {
    let last = Tick(b.0.saturating_sub(1)).max(a);
    let middle = Tick(a.0.saturating_add(b.0.saturating_sub(a.0) / 2));
    [a, middle, last]
}

/// The distinct reasons `item` renders on the CPU between `a` and `b` ([`samples`]).
fn item_reasons(project: &Project, seq: &Sequence, item: &TrackItem, a: Tick, b: Tick, caps: DeviceCaps) -> Vec<(String, bool)> {
    let mut found: Vec<(String, bool)> = Vec::new();
    for t in samples(a, b) {
        if let Some(hit) = item_reason(project, seq, item, t, caps, 0)
            && !found.contains(&hit)
        {
            found.push(hit);
        }
    }
    found
}

fn effect_name(e: &EffectInstance) -> String {
    e.def().map_or_else(|| e.effect.clone(), |d| d.name.to_string())
}

/// Why `item` of `seq` renders on the CPU at `t` on a device that runs `caps`, and whether that
/// takes the whole frame with it. None: the GPU draws it. `nest` counts the nested sequences
/// followed to reach `item`.
pub(crate) fn item_reason(project: &Project, seq: &Sequence, item: &TrackItem, t: Tick, caps: DeviceCaps, nest: u32) -> Option<(String, bool)> {
    let kind = project.item(item.item).map(|p| &p.kind)?;
    match kind {
        ItemKind::AdjustmentLayer { .. } => adjustment_reason(project, seq, item, t, caps).map(|r| (r, true)),
        ItemKind::Media(_) | ItemKind::Subclip { .. } => media_reason(project, seq, item, t).map(|r| (r, false)),
        ItemKind::Sequence(nested) => nested_reason(project, seq, item, nested, t, caps, nest),
        ItemKind::Graphic { .. } => (item.has_standard_effects() || item.has_opacity_masks())
            .then(|| ("a title or graphic with effects or opacity masks renders on the CPU".into(), false)),
    }
}

fn adjustment_reason(project: &Project, seq: &Sequence, item: &TrackItem, t: Tick, caps: DeviceCaps) -> Option<String> {
    let tail = "so every frame under it renders on the CPU";
    let why = match crate::plan::adjustment_fx(project, seq, item, t, 1.0, caps).err()? {
        AdjustmentCpu::Device => "this device runs no adjustment layers on a frame of this size".to_string(),
        AdjustmentCpu::Blend => "a blend mode other than Normal".to_string(),
        AdjustmentCpu::Opacity => "its opacity is not a number".to_string(),
        AdjustmentCpu::Effect(i) => match item.effects.get(i) {
            Some(e) if !crate::gpufx::GPU_EFFECTS.contains(&e.effect.as_str()) => format!("effect \"{}\" has no GPU version", effect_name(e)),
            Some(e) => format!("effect \"{}\" cannot run on the GPU with these settings", effect_name(e)),
            None => "one of its effects cannot run on the GPU".to_string(),
        },
    };
    Some(format!("adjustment layer: {why}, {tail}"))
}

/// The effect context a media clip's effects are asked about with: its stored size's scale.
fn media_cx<'a>(project: &'a Project, seq: &Sequence, item: &'a TrackItem, t: Tick) -> crate::effects::FxCtx<'a> {
    crate::effects::FxCtx {
        t: item.effect_time_at(t),
        px_scale: 1.0,
        mask_scale: 1.0,
        seconds: (t - item.start).seconds(),
        timecode: "",
        clip_name: &item.name,
        project: Some(project),
        env: None,
        frame_rate: seq.settings.frame_rate.as_f64(),
        working: seq.settings.color.working,
    }
}

/// The size effects of `item` are asked about for: the output's for an adjustment layer, else
/// the stored size of its source.
fn effect_size(project: &Project, seq: &Sequence, item: &TrackItem) -> (usize, usize) {
    let own =
        if matches!(project.item(item.item).map(|p| &p.kind), Some(ItemKind::AdjustmentLayer { .. })) { None } else { crate::source_size(project, item.item) };
    let size = own.unwrap_or((seq.settings.width, seq.settings.height));
    (size.0.max(1) as usize, size.1.max(1) as usize)
}

/// Whether the Color Grading `e` of `item` evaluates to a sequence of passes at `t` (more than
/// its Basic, Creative and Vignette controls in use): the render bar counts it as a heavy effect.
pub(crate) fn is_full_grade(project: &Project, seq: &Sequence, item: &TrackItem, e: &EffectInstance, t: Tick) -> bool {
    let (w, h) = effect_size(project, seq, item);
    matches!(FxOp::eval(e, &media_cx(project, seq, item, t), w, h), Some(FxOp::Seq(_)))
}

fn media_reason(project: &Project, seq: &Sequence, item: &TrackItem, t: Tick) -> Option<String> {
    let standard = || item.effects.iter().filter(|e| e.enabled && e.def().is_some_and(|d| !d.intrinsic) && !filmcraft_project::graphic::is_layer(e));
    let Some(chain) = crate::plan::gpu_chain(project, item, RenderOptions::default()) else {
        let culprit = standard().find(|e| !crate::gpufx::GPU_EFFECTS.contains(&e.effect.as_str()));
        let what = culprit.map_or_else(|| "one of its effects".to_string(), |e| format!("effect \"{}\"", effect_name(e)));
        return Some(format!("{what} has no GPU version: this clip renders on the CPU"));
    };
    if !chain.is_empty() {
        let (w, h) = effect_size(project, seq, item);
        let cx = media_cx(project, seq, item, t);
        if let Some(e) = chain.iter().find(|e| FxOp::eval(e, &cx, w, h).filter(FxOp::gpu_ok).is_none()) {
            return Some(format!("effect \"{}\" cannot run on the GPU with these settings: this clip renders on the CPU", effect_name(e)));
        }
    }
    // Frame Blending / Optical Flow mix two source frames on the CPU when the clip does not play
    // at the sequence's own rate (`interpolation_blend`)
    let conformed = match project.item(item.item).map(|p| &p.kind) {
        Some(ItemKind::Media(m)) => m.info.video.is_some() && m.frame_rate().frame_duration() != seq.settings.frame_rate.frame_duration(),
        _ => false,
    };
    let retimed = (item.speed.abs() - 1.0).abs() >= 1e-9 || conformed;
    if item.time_interpolation != TimeInterpolation::FrameSampling && item.frame_hold.is_none() && retimed {
        return Some("Frame Blending / Optical Flow on a clip that does not play at the sequence's rate: its in-between frames render on the CPU".into());
    }
    None
}

/// A nested sequence (or multi-camera clip): why it is rendered to an image on the CPU instead of
/// contributing its own layers, or else why something inside it renders on the CPU.
fn nested_reason(project: &Project, seq: &Sequence, item: &TrackItem, nested: &Sequence, t: Tick, caps: DeviceCaps, nest: u32) -> Option<(String, bool)> {
    if nest >= crate::MAX_NEST_DEPTH {
        return None;
    }
    let (ft, mt) = (item.source_time_at(t), item.effect_time_at(t));
    let (op, bl) = crate::opacity_blend(item, mt);
    let same_size = (nested.settings.width, nested.settings.height) == (seq.settings.width, seq.settings.height);
    let placed = !crate::plan::near_identity(&crate::motion_matrix(seq, item, (nested.settings.width, nested.settings.height), Some(nested.settings.par), mt));
    let inside = |inner: &TrackItem| {
        item_reason(project, nested, inner, ft, caps, nest + 1)
            .map(|(reason, whole)| (format!("inside this nested sequence, \"{}\": {reason}", inner.name), whole))
    };
    let cpu = |why: &str| Some((format!("a nested sequence {why} renders on the CPU"), false));
    if item.has_standard_effects() {
        return cpu("with effects");
    }
    if let Some(angle) = item.multicam_angle(nested) {
        // a multi-camera clip shows its angle's clip directly when it is only shown
        if !same_size || placed {
            return cpu("(multi-camera) that is moved, scaled or of another frame size");
        }
        let track = nested.angle_video_track_index(angle).and_then(|i| nested.video_tracks.get(i))?;
        if track.transitions.iter().any(|x| x.range().contains(ft)) {
            return cpu("(multi-camera) at a transition of its angle");
        }
        return track.item_at(ft).filter(|i| i.enabled).and_then(inside);
    }
    if item.has_opacity_masks() {
        return cpu("with opacity masks");
    }
    if bl != Blend::Normal {
        return cpu("with a blend mode other than Normal");
    }
    if op < 1.0 - 1e-6 || !op.is_finite() {
        return cpu("that is not fully opaque");
    }
    if placed {
        return cpu("that is moved, scaled or rotated");
    }
    if !crate::plan::nest_is_plain(project, seq, nested, ft, 1.0, caps) {
        return cpu(
            "whose content cannot be drawn as layers here (another frame size or colour setting, or an adjustment layer, a transition or a blend mode inside it)",
        );
    }
    nested.video_tracks.iter().filter(|track| track.enabled).find_map(|track| track.item_at(ft).filter(|i| i.enabled).and_then(inside))
}
