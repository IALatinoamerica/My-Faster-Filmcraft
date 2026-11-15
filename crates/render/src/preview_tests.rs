//! Render-bar segments: splitting, hashing, invalidation and cost classification.

use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{DemoScene, Generator, MediaSource};
use filmcraft_project::{ClipId, ItemId, ItemKind, Label, MediaClip, MediaRef, ParamValue, Project, SequenceSettings, TrackKind, find_effect};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};

use crate::preview::{Need, Segment, audio_segments, effect_cost_ms, segment_at, video_segments};

fn add_gen(p: &mut Project, g: GeneratorSource) -> ItemId {
    let info = g.info().clone();
    p.add_item(
        &info.name.clone(),
        Label::Iris,
        ItemKind::Media(Box::new(MediaClip {
            media: MediaRef::Generator(g.generator.clone()),
            info,
            interpret: Default::default(),
            mark_in: None,
            mark_out: None,
            markers: vec![],
            offline: false,
            proxy: None,
            identity: None,
        })),
        None,
    )
}

fn setup() -> (Project, ItemId, ItemId, ItemId) {
    let mut p = Project::new("r");
    let matte = add_gen(
        &mut p,
        GeneratorSource::new(Generator::ColorMatte { color: [1.0, 0.0, 0.0, 1.0] }, 1920, 1080, FrameRate::FPS_24, Tick(60 * TICKS_PER_SECOND)),
    );
    let ocean = add_gen(&mut p, GeneratorSource::demo(DemoScene::OceanSunset));
    let seq = p.new_sequence("s", SequenceSettings { width: 1920, height: 1080, frame_rate: FrameRate::FPS_24, ..Default::default() }, 3, 1, None);
    (p, matte, ocean, seq)
}

fn place(p: &mut Project, seq: ItemId, track: usize, item: ItemId, start_f: i64, dur_f: i64) -> ClipId {
    let r = FrameRate::FPS_24;
    let mut ti = p.make_track_item(item, TrackKind::Video, r.tick_of(start_f), TimeRange::new(Tick::ZERO, r.tick_of(dur_f)), r).unwrap();
    for e in ti.effects.iter_mut() {
        filmcraft_project::resolve_auto_points(e, (1920, 1080), (1920, 1080));
    }
    let id = ti.id;
    let t = &mut p.sequence_mut(seq).unwrap().video_tracks[track];
    t.items.push(ti);
    t.sort();
    id
}

fn add_fx(p: &mut Project, seq: ItemId, clip: ClipId, id: &str) {
    let mut inst = find_effect(id).unwrap().instance();
    filmcraft_project::resolve_auto_points(&mut inst, (1920, 1080), (1920, 1080));
    p.sequence_mut(seq).unwrap().find_item_mut(clip).unwrap().1.effects.push(inst);
}

fn bounds(s: &[Segment]) -> Vec<(i64, i64)> {
    s.iter().map(|s| (s.first_frame, s.first_frame + s.frames)).collect()
}

fn by_frame(s: &[Segment], f: i64) -> String {
    segment_at(s, f).unwrap().hash.clone()
}

#[test]
fn segments_split_at_every_edit_point_and_skip_gaps() {
    let (mut p, matte, ocean, seq) = setup();
    place(&mut p, seq, 0, ocean, 0, 48);
    place(&mut p, seq, 0, matte, 48, 24);
    // gap 72..96 on every track
    place(&mut p, seq, 0, ocean, 96, 24);
    place(&mut p, seq, 1, matte, 24, 48); // spans the V1 cut at 48
    let s = video_segments(&p, seq);
    assert_eq!(bounds(&s), vec![(0, 24), (24, 48), (48, 72), (96, 120)]);
    assert_eq!(s[1].clips.len(), 2, "two visible layers in 24..48");
    assert!(segment_at(&s, 80).is_none(), "gap has no segment");
    assert_eq!(segment_at(&s, 30).unwrap().first_frame, 24);
    // segments are frame-aligned ranges
    let r = FrameRate::FPS_24;
    for seg in &s {
        assert_eq!(seg.start, r.tick_of(seg.first_frame));
        assert_eq!(seg.end, r.tick_of(seg.first_frame + seg.frames));
    }
}

#[test]
fn transitions_are_their_own_segments() {
    let (mut p, matte, ocean, seq) = setup();
    let a = place(&mut p, seq, 0, ocean, 0, 48);
    let b = place(&mut p, seq, 0, matte, 48, 48);
    let r = FrameRate::FPS_24;
    let tr = filmcraft_project::Transition {
        id: Default::default(),
        effect: find_effect("cross_dissolve").unwrap().instance(),
        start: r.tick_of(36),
        duration: r.tick_of(24),
        from: Some(a),
        to: Some(b),
        align: Default::default(),
        reverse: false,
    };
    p.sequence_mut(seq).unwrap().video_tracks[0].transitions.push(tr);
    let s = video_segments(&p, seq);
    assert_eq!(bounds(&s), vec![(0, 36), (36, 48), (48, 60), (60, 96)]);
    assert_eq!(s[1].clips, vec![a, b]);
    assert_eq!(s[1].need, Need::Realtime);
}

#[test]
fn editing_one_clip_invalidates_only_its_segments() {
    let (mut p, matte, ocean, seq) = setup();
    let a = place(&mut p, seq, 0, ocean, 0, 48);
    let b = place(&mut p, seq, 0, matte, 48, 48);
    add_fx(&mut p, seq, a, "gaussian_blur");
    add_fx(&mut p, seq, b, "tint");
    let before = video_segments(&p, seq);
    // change a parameter of A's effect
    let q = p.sequence_mut(seq).unwrap();
    let (_, it) = q.find_item_mut(a).unwrap();
    it.effect_mut("gaussian_blur").unwrap().params.values_mut().next().unwrap().value = ParamValue::Float(42.0);
    let after = video_segments(&p, seq);
    assert_ne!(by_frame(&before, 10), by_frame(&after, 10), "A changed");
    assert_eq!(by_frame(&before, 60), by_frame(&after, 60), "B untouched");
    // a keyframe on B changes B only
    let q = p.sequence_mut(seq).unwrap();
    let (_, it) = q.find_item_mut(b).unwrap();
    it.effect_mut("opacity").unwrap().params.get_mut("opacity").unwrap().toggle_animation(Tick(1000));
    let kf = video_segments(&p, seq);
    assert_eq!(by_frame(&after, 10), by_frame(&kf, 10));
    assert_ne!(by_frame(&after, 60), by_frame(&kf, 60));
    // sequence settings affect everything
    p.sequence_mut(seq).unwrap().settings.max_render_quality = true;
    let st = video_segments(&p, seq);
    assert_ne!(by_frame(&kf, 10), by_frame(&st, 10));
    assert_ne!(by_frame(&kf, 60), by_frame(&st, 60));
}

#[test]
fn renaming_relabelling_and_moving_keep_hashes() {
    let (mut p, matte, ocean, seq) = setup();
    let a = place(&mut p, seq, 0, ocean, 0, 48);
    let b = place(&mut p, seq, 0, matte, 48, 48);
    add_fx(&mut p, seq, a, "tint");
    let before = video_segments(&p, seq);
    let q = p.sequence_mut(seq).unwrap();
    for it in q.video_tracks[0].items.iter_mut() {
        it.name = "renamed".into();
        it.label = Label::Mango;
        // ripple both clips 10 frames later
        it.start += FrameRate::FPS_24.tick_of(10);
    }
    let after = video_segments(&p, seq);
    assert_eq!(by_frame(&before, 0), by_frame(&after, 10), "moved segment keeps its preview");
    assert_eq!(by_frame(&before, 50), by_frame(&after, 60));
    let _ = b;
    // trimming B changes B's segment only (its frame count changed)
    let q = p.sequence_mut(seq).unwrap();
    q.find_item_mut(b).unwrap().1.duration = FrameRate::FPS_24.tick_of(40);
    let trimmed = video_segments(&p, seq);
    assert_eq!(by_frame(&after, 10), by_frame(&trimmed, 10));
    assert_ne!(by_frame(&after, 60), by_frame(&trimmed, 60));
}

#[test]
fn hashes_are_stable_across_save_and_load() {
    let (mut p, matte, ocean, seq) = setup();
    let a = place(&mut p, seq, 0, ocean, 0, 48);
    let b = place(&mut p, seq, 1, matte, 24, 48);
    add_fx(&mut p, seq, a, "lumetri");
    add_fx(&mut p, seq, b, "gaussian_blur");
    let q = p.sequence_mut(seq).unwrap();
    let (_, it) = q.find_item_mut(b).unwrap();
    let m = it.effect_mut("motion").unwrap();
    m.params.get_mut("scale").unwrap().toggle_animation(Tick(0));
    m.params.get_mut("scale").unwrap().set_at(Tick(TICKS_PER_SECOND), ParamValue::Float(80.0));
    let before = video_segments(&p, seq);
    let loaded = Project::from_json(&p.to_json()).unwrap();
    let after = video_segments(&loaded, seq);
    assert_eq!(before, after);
    // and identical across repeated computation
    assert_eq!(video_segments(&p, seq), before);
}

#[test]
fn need_classification_none_yellow_red() {
    let (mut p, matte, _ocean, seq) = setup();
    let a = place(&mut p, seq, 0, matte, 0, 24);
    let b = place(&mut p, seq, 0, matte, 24, 24);
    let c = place(&mut p, seq, 0, matte, 48, 24);
    add_fx(&mut p, seq, b, "tint");
    // (effects without a GPU version: the CPU renders this layer. The estimate asks the planner,
    // so four heavy effects the effect stage runs would be yellow: see
    // `the_estimate_follows_the_planner`)
    for fx in ["twirl", "wave_warp", "median", "four_color_gradient"] {
        add_fx(&mut p, seq, c, fx);
    }
    let s = video_segments(&p, seq);
    let need = |f| segment_at(&s, f).unwrap().need;
    assert_eq!(need(0), Need::None, "untouched clip matching the sequence plays natively");
    assert_eq!(need(30), Need::Realtime, "one cheap effect: yellow");
    assert_eq!(need(60), Need::Render, "four heavy effects: red ({} ms)", segment_at(&s, 60).unwrap().cost_ms);
    let _ = a;
    // a clip at a different frame size than the sequence needs scaling: yellow
    let (mut p, _m, ocean, seq) = setup();
    place(&mut p, seq, 0, ocean, 0, 24);
    let s = video_segments(&p, seq);
    let ocean_matches = p.item(ocean).unwrap().as_media().unwrap().info.video.as_ref().is_some_and(|v| v.width == 1920 && v.height == 1080);
    let want = if ocean_matches && p.item(ocean).unwrap().frame_rate() == FrameRate::FPS_24 { Need::None } else { Need::Realtime };
    assert_eq!(s[0].need, want);
}

#[test]
fn effect_tiers_are_ordered() {
    assert!(effect_cost_ms("lumetri") > effect_cost_ms("tint"));
    assert!(effect_cost_ms("tint") > effect_cost_ms("crop"));
    assert_eq!(effect_cost_ms("gaussian_blur"), effect_cost_ms("lumetri"));
}

/// Times every standard video effect on a 1080p frame (run with `--release --ignored --nocapture`)
/// to calibrate [`effect_cost_ms`].
#[test]
#[ignore]
fn effect_costs() {
    use crate::effects::{FxCtx, apply};
    let mut rows = Vec::new();
    for d in filmcraft_project::effect_defs().iter().filter(|d| d.kind == filmcraft_project::EffectKind::Video && !d.intrinsic) {
        let mut img = crate::Image::filled(1920, 1080, [0.4, 0.5, 0.6, 1.0]);
        let e = d.instance();
        let cx = FxCtx {
            t: Tick::ZERO,
            px_scale: 1.0,
            mask_scale: 1.0,
            seconds: 0.0,
            timecode: "00:00:00:00",
            clip_name: "x",
            project: None,
            env: None,
            frame_rate: 24.0,
            working: filmcraft_color::WorkingSpace::Rec709,
        };
        apply(&mut img, &e, &cx);
        let t0 = std::time::Instant::now();
        for _ in 0..3 {
            apply(&mut img, &e, &cx);
        }
        rows.push((t0.elapsed().as_secs_f64() * 1000.0 / 3.0, d.id));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (ms, id) in rows {
        println!("{ms:8.2} ms  {id:24} tier {:.0}", effect_cost_ms(id));
    }
}

/// Put `item` on track 0 of `kind` in `seq` from frame `start_f` for `dur_f` frames.
fn place_kind(p: &mut Project, seq: ItemId, kind: TrackKind, item: ItemId, start_f: i64, dur_f: i64) -> ClipId {
    let r = FrameRate::FPS_24;
    let ti = p.make_track_item(item, kind, r.tick_of(start_f), TimeRange::new(Tick::ZERO, r.tick_of(dur_f)), r).unwrap();
    let id = ti.id;
    let t = &mut p.sequence_mut(seq).unwrap().tracks_mut(kind)[0];
    t.items.push(ti);
    t.sort();
    id
}

fn audio_hashes(p: &Project, seq: ItemId) -> Vec<String> {
    audio_segments(p, seq).into_iter().map(|g| g.hash).collect()
}

#[test]
fn a_nests_audio_previews_follow_the_sound_of_its_sequence() {
    // Inner has a picture clip and a sound clip; Outer nests Inner (picture and sound), and Top
    // nests Outer's sound
    let (mut p, matte, ocean, inner) = setup();
    let pic = place_kind(&mut p, inner, TrackKind::Video, matte, 0, 48);
    let snd = place_kind(&mut p, inner, TrackKind::Audio, ocean, 0, 48);
    let settings = p.sequence(inner).unwrap().settings.clone();
    let outer = p.new_sequence("outer", settings.clone(), 1, 1, None);
    place_kind(&mut p, outer, TrackKind::Video, inner, 24, 48);
    place_kind(&mut p, outer, TrackKind::Audio, inner, 24, 48);
    let top = p.new_sequence("top", settings, 1, 1, None);
    place_kind(&mut p, top, TrackKind::Audio, outer, 12, 72);
    let (audio, picture, deep) = (audio_hashes(&p, outer), video_segments(&p, outer), audio_hashes(&p, top));
    assert_eq!(audio.len(), 1);

    // the sound inside the nest changes: the nest's audio preview is stale, its picture is not
    p.sequence_mut(inner).unwrap().find_item_mut(snd).unwrap().1.gain_db = -12.0;
    assert_ne!(audio_hashes(&p, outer), audio, "a clip's gain inside the nest");
    assert_ne!(audio_hashes(&p, top), deep, "a clip's gain two nests down");
    assert_eq!(bounds(&video_segments(&p, outer)), bounds(&picture));
    assert_eq!(by_frame(&video_segments(&p, outer), 30), by_frame(&picture, 30), "the picture did not change");
    let (audio, deep) = (audio_hashes(&p, outer), audio_hashes(&p, top));
    // the nested sequence's track mix counts too
    p.sequence_mut(inner).unwrap().audio_tracks[0].muted = true;
    assert_ne!(audio_hashes(&p, outer), audio, "a muted track inside the nest");
    p.sequence_mut(inner).unwrap().audio_tracks[0].muted = false;
    assert_eq!(audio_hashes(&p, outer), audio);

    // the picture inside the nest changes: its audio preview stays valid
    add_fx(&mut p, inner, pic, "tint");
    assert_ne!(by_frame(&video_segments(&p, outer), 30), by_frame(&picture, 30));
    assert_eq!(audio_hashes(&p, outer), audio, "a video effect inside the nest");
    assert_eq!(audio_hashes(&p, top), deep);
}

#[test]
fn a_subclip_that_is_its_own_parent_does_not_recurse_forever() {
    let (mut p, _, ocean, seq) = setup();
    let sub = p.add_item(
        "sub",
        Label::Iris,
        ItemKind::Subclip { parent: ocean, range: TimeRange::new(Tick::ZERO, Tick(TICKS_PER_SECOND)), restrict_trims: false },
        None,
    );
    place(&mut p, seq, 0, sub, 0, 24);
    // a damaged project: the subclip names itself as its parent
    if let Some(ItemKind::Subclip { parent, .. }) = p.item_mut(sub).map(|i| &mut i.kind) {
        *parent = sub;
    }
    let s = std::thread::Builder::new().stack_size(256 << 10).spawn(move || video_segments(&p, seq).len()).unwrap().join().unwrap();
    assert_eq!(s, 1);
}

/// The render bar asks the frame planner which path each layer takes: effects the effect stage
/// runs are cheap, on clips and on adjustment layers (also one moved off centre, or with a masked
/// effect), an effect without a GPU version costs its CPU figure, and an adjustment layer the GPU
/// can't run makes the segment red whatever the sum. At 60 fps, as on Bruno's timeline.
#[test]
fn the_estimate_follows_the_planner() {
    let r = FrameRate::FPS_60;
    let build = |clip_fx: &[&str], adj_fx: &[&str], change: &dyn Fn(&mut filmcraft_project::TrackItem, &mut filmcraft_project::TrackItem)| {
        let mut p = Project::new("r");
        let matte = add_gen(&mut p, GeneratorSource::new(Generator::ColorMatte { color: [1.0, 0.0, 0.0, 1.0] }, 1920, 1080, r, Tick(60 * TICKS_PER_SECOND)));
        let seq = p.new_sequence("s", SequenceSettings { width: 1920, height: 1080, frame_rate: r, ..Default::default() }, 2, 0, None);
        let adj =
            p.add_item("Adjustment Layer", Label::Lavender, ItemKind::AdjustmentLayer { width: 1920, height: 1080, rate: r, duration: r.tick_of(600) }, None);
        let mut items = Vec::new();
        for (track, (item, effects)) in [(matte, clip_fx), (adj, adj_fx)].into_iter().enumerate() {
            let mut ti = p.make_track_item(item, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, r.tick_of(60)), r).unwrap();
            for id in effects {
                ti.effects.insert(0, find_effect(id).unwrap().instance());
            }
            for e in ti.effects.iter_mut() {
                filmcraft_project::resolve_auto_points(e, (1920, 1080), (1920, 1080));
            }
            items.push((track, ti));
        }
        let (mut a, mut b) = (items.remove(0).1, items.remove(0).1);
        change(&mut a, &mut b);
        let q = p.sequence_mut(seq).unwrap();
        q.video_tracks[0].items.push(a);
        if !adj_fx.is_empty() {
            q.video_tracks[1].items.push(b);
        }
        let s = video_segments(&p, seq);
        (s[0].need, s[0].cost_ms)
    };
    fn grade(it: &mut filmcraft_project::TrackItem) {
        let e = it.effect_mut("lumetri").unwrap();
        e.params.get_mut("hsl_on").unwrap().value = ParamValue::Bool(true);
        e.params.get_mut("sharpen").unwrap().value = ParamValue::Float(40.0);
    }
    let nothing = |_: &mut filmcraft_project::TrackItem, _: &mut filmcraft_project::TrackItem| {};
    // (what, effects on the clip, effects on the adjustment layer, a change, the colour)
    type Change<'a> = &'a dyn Fn(&mut filmcraft_project::TrackItem, &mut filmcraft_project::TrackItem);
    let cases: Vec<(&str, &[&str], &[&str], Change, Need)> = vec![
        ("four effects the effect stage runs", &["gaussian_blur", "sharpen", "lumetri", "noise"], &[], &nothing, Need::Realtime),
        ("an adjustment layer with effects the effect stage runs", &[], &["invert", "gaussian_blur"], &nothing, Need::Realtime),
        (
            "the same layer moved off centre",
            &[],
            &["invert", "gaussian_blur"],
            &|_, adj| adj.effect_mut("motion").unwrap().params.get_mut("position").unwrap().value = ParamValue::Vec2(filmcraft_geom::Vec2::new(960.0, 550.0)),
            Need::Realtime,
        ),
        (
            "the same layer with a mask on an effect",
            &[],
            &["invert", "gaussian_blur"],
            &|_, adj| {
                adj.effect_mut("invert").unwrap().masks.push(filmcraft_project::Mask::new("m", filmcraft_project::MaskPath::rect(100.0, 100.0, 900.0, 700.0)))
            },
            Need::Realtime,
        ),
        ("a Color Grading with every section, on the adjustment layer", &[], &["lumetri"], &|_, adj| grade(adj), Need::Realtime),
        (
            "two such Color Gradings on one frame",
            &["lumetri"],
            &["lumetri"],
            &|clip, adj| {
                grade(clip);
                grade(adj);
            },
            Need::Render,
        ),
        ("an effect without a GPU version on the clip", &["twirl"], &[], &nothing, Need::Render),
        ("an effect without a GPU version on the adjustment layer", &[], &["twirl"], &nothing, Need::Render),
        (
            "a cheap effect on an adjustment layer the GPU can't run (Multiply)",
            &[],
            &["invert"],
            &|_, adj| {
                let multiply = filmcraft_project::effect::BLEND_MODES.iter().position(|b| *b == "Multiply").unwrap() as u32;
                adj.effect_mut("opacity").unwrap().params.get_mut("blend").unwrap().value = ParamValue::Choice(multiply);
            },
            Need::Render,
        ),
    ];
    for (what, clip_fx, adj_fx, change, want) in cases {
        let (need, cost) = build(clip_fx, adj_fx, change);
        assert_eq!(need, want, "{what}: {cost} ms");
    }
    // the whole-frame case is red although its sum is under the budget (25 ms at 60 fps)
    let (need, cost) = build(&[], &["crop"], cases_multiply());
    assert!(need == Need::Render && cost < 25.0, "{cost} ms");
}

fn cases_multiply() -> &'static dyn Fn(&mut filmcraft_project::TrackItem, &mut filmcraft_project::TrackItem) {
    &|_, adj| {
        let multiply = filmcraft_project::effect::BLEND_MODES.iter().position(|b| *b == "Multiply").unwrap() as u32;
        adj.effect_mut("opacity").unwrap().params.get_mut("blend").unwrap().value = ParamValue::Choice(multiply);
    }
}

/// How long the render bar's segments take to compute for a real project (the planner is asked
/// about every layer of every segment, on the interface thread, each time the project changes).
/// `FILMCRAFT_BAR_PROJECT=<file.fcproj> cargo test -p filmcraft-render --release -- --ignored
/// render_bar_timing --nocapture` prints the time per sequence.
#[test]
#[ignore = "needs a project file: FILMCRAFT_BAR_PROJECT"]
fn render_bar_timing() {
    let Ok(path) = std::env::var("FILMCRAFT_BAR_PROJECT") else { return };
    let text = std::fs::read_to_string(&path).unwrap();
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    let p = Project::from_json(&file["project"].to_string()).unwrap();
    for item in p.items.values() {
        if !matches!(item.kind, ItemKind::Sequence(_)) {
            continue;
        }
        let t0 = std::time::Instant::now();
        let mut n = 0;
        for _ in 0..20 {
            n = video_segments(&p, item.id).len();
        }
        println!("{:?}: {n} segments, {:.2} ms per computation", item.name, t0.elapsed().as_secs_f64() * 1000.0 / 20.0);
    }
}
