//! Clips that read one file at the same time, at moments far apart, read copies of their own
//! (`readers`).

use std::sync::{Arc, Mutex};

use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{DemoScene, MediaSource, SharedSource};
use filmcraft_project::{ClipId, ItemKind, Label, MediaClip, MediaRef, Project, SequenceSettings, TrackKind};
use filmcraft_time::{FrameRate, Tick, TimeRange};

use crate::plan::{self, DeviceCaps};
use crate::{RenderOptions, SourceProvider, readers};

const R: FrameRate = FrameRate::FPS_24;

/// A provider that records which reader each source was asked for.
struct Recording {
    src: SharedSource,
    asked: Mutex<Vec<u64>>,
}

impl SourceProvider for Recording {
    fn source(&self, _item: filmcraft_project::ItemId) -> Option<SharedSource> {
        self.asked.lock().unwrap().push(0);
        Some(self.src.clone())
    }
    fn reader_source(&self, _item: filmcraft_project::ItemId, reader: u64) -> Option<SharedSource> {
        self.asked.lock().unwrap().push(reader);
        Some(self.src.clone())
    }
}

/// A project with one 18 s demo clip and a two-track sequence.
fn setup() -> (Project, filmcraft_project::ItemId, filmcraft_project::ItemId, Recording) {
    let mut p = Project::new("r");
    let g = GeneratorSource::demo(DemoScene::OceanSunset);
    let info = g.info().clone();
    let media = p.add_item(
        "ocean",
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
    );
    let seq = p.new_sequence("s", SequenceSettings { width: 320, height: 180, frame_rate: R, ..Default::default() }, 2, 0, None);
    (p, media, seq, Recording { src: Arc::new(g), asked: Mutex::default() })
}

/// Place `item` on `track` at frame `start_f` for `dur_f` frames, from `src_s` seconds into it.
fn place(p: &mut Project, seq: filmcraft_project::ItemId, track: usize, item: filmcraft_project::ItemId, start_f: i64, dur_f: i64, src_s: i64) -> ClipId {
    let src = Tick(src_s * filmcraft_time::TICKS_PER_SECOND);
    let ti = p.make_track_item(item, TrackKind::Video, R.tick_of(start_f), TimeRange::new(src, R.tick_of(dur_f)), R).unwrap();
    let id = ti.id;
    let s = p.sequence_mut(seq).unwrap();
    s.video_tracks[track].items.push(ti);
    s.video_tracks[track].sort();
    id
}

fn reader_of(p: &Project, seq: filmcraft_project::ItemId, id: ClipId) -> u64 {
    let s = p.sequence(seq).unwrap();
    let it = s.video_tracks.iter().flat_map(|t| &t.items).find(|i| i.id == id).unwrap();
    readers::reader(p, s, it)
}

/// The case that stalled playback: a recording shown full frame and again, 10 s further into it,
/// on the track above. The upper clip reads a copy of its own, for the plan and for the fetches
/// before it; the lower one keeps the shared source.
#[test]
fn a_clip_showing_another_moment_of_a_file_below_it_reads_its_own_copy() {
    let (mut p, media, seq, prov) = setup();
    let below = place(&mut p, seq, 0, media, 0, 48, 0);
    let above = place(&mut p, seq, 1, media, 12, 24, 10);
    assert_eq!(reader_of(&p, seq, below), 0);
    assert_eq!(reader_of(&p, seq, above), above.0 + 1);
    let _ = plan::plan_frame(&p, seq, R.tick_of(20), RenderOptions::default(), DeviceCaps::ALL, &prov);
    let asked = std::mem::take(&mut *prov.asked.lock().unwrap());
    assert!(asked.contains(&0) && asked.contains(&(above.0 + 1)), "{asked:?}");
    plan::prefetch_sources(&p, seq, R.tick_of(20), RenderOptions::default(), &prov);
    let asked = std::mem::take(&mut *prov.asked.lock().unwrap());
    assert!(asked.contains(&0) && asked.contains(&(above.0 + 1)), "{asked:?}");
    // the CPU renderer reads the same copies
    let _ = crate::render_sequence(&p, seq, R.tick_of(20), RenderOptions::default(), &prov);
    assert!(prov.asked.lock().unwrap().contains(&(above.0 + 1)));
}

/// A duplicate showing the same moments (a blurred background) shares the source: one decode
/// serves both from the cache. So do clips that never overlap, and clips of other media.
#[test]
fn clips_that_can_share_a_decoder_keep_the_shared_source() {
    let (mut p, media, seq, _) = setup();
    place(&mut p, seq, 0, media, 0, 48, 0);
    let same = place(&mut p, seq, 1, media, 0, 48, 0);
    assert_eq!(reader_of(&p, seq, same), 0, "same moments");
    let (mut p, media, seq, _) = setup();
    place(&mut p, seq, 0, media, 0, 24, 0);
    let later = place(&mut p, seq, 1, media, 24, 24, 10);
    assert_eq!(reader_of(&p, seq, later), 0, "no overlap");
    // the lower clip turned off reads nothing: no copy needed
    let (mut p, media, seq, _) = setup();
    let below = place(&mut p, seq, 0, media, 0, 48, 0);
    let above = place(&mut p, seq, 1, media, 0, 48, 10);
    let s = p.sequence_mut(seq).unwrap();
    s.video_tracks[0].items.iter_mut().filter(|i| i.id == below).for_each(|i| i.enabled = false);
    assert_eq!(reader_of(&p, seq, above), 0, "disabled clip below");
    p.sequence_mut(seq).unwrap().video_tracks[0].enabled = false;
    assert_eq!(reader_of(&p, seq, above), 0, "disabled track below");
}

/// A cut dissolving into a later cut of the same recording on one track: both are read during
/// the transition, so the incoming clip reads a copy of its own.
#[test]
fn a_transition_between_two_moments_of_a_file_gives_the_incoming_clip_its_own_copy() {
    let (mut p, media, seq, _) = setup();
    let a = place(&mut p, seq, 0, media, 0, 24, 0);
    let b = place(&mut p, seq, 0, media, 24, 24, 10);
    assert_eq!(reader_of(&p, seq, b), 0, "a plain cut: one after the other");
    p.sequence_mut(seq).unwrap().video_tracks[0].transitions.push(filmcraft_project::Transition {
        id: filmcraft_project::TransitionId(999),
        effect: filmcraft_project::find_effect("cross_dissolve").unwrap().instance(),
        start: R.tick_of(18),
        duration: R.tick_of(12),
        from: Some(a),
        to: Some(b),
        align: Default::default(),
        reverse: false,
    });
    assert_eq!(reader_of(&p, seq, a), 0);
    assert_eq!(reader_of(&p, seq, b), b.0 + 1);
}
