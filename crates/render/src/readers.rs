//! Which copy of a media file a clip reads its frames from.
//!
//! A source decodes with one decoder, which runs forward from a keyframe. Two clips that show the
//! same file at the same time, at moments of it far apart (a screen recording shown full frame and
//! again as an inset, a cut of a clip dissolving into a later cut of the same clip), would ask
//! that one decoder for both moments at every frame: it restarts at a keyframe for each request,
//! and both clips stall. Such a clip reads a copy of its own instead
//! ([`SourceProvider::reader_source`]), with a decoder and decoded frames of its own.
//!
//! The choice depends only on the clip and the sequence, not on the frame: a clip keeps the same
//! copy for its whole length, so its decoder runs on from one frame to the next. Clips that show
//! the same moments (a duplicate used as a blurred background) share the file's source, whose
//! cache serves both from one decode.

use filmcraft_media::SharedSource;
use filmcraft_project::{Project, Sequence, Track, TrackItem};
use filmcraft_time::Tick;

use crate::SourceProvider;

/// Media time two clips of one file may lie apart, where they overlap, and still share one
/// decoder: the decoded frames a source keeps cover about this much ahead of and behind the
/// newest one at 1080p, so the second clip's frames are still cached when it asks.
const SHARED_SPAN: Tick = Tick(filmcraft_time::TICKS_PER_SECOND / 2);

/// The timeline range a clip is read over: its own, plus the transitions on its track that show it.
fn read_range(tr: &Track, it: &TrackItem) -> (Tick, Tick) {
    let (mut start, mut end) = (it.start, it.end());
    for x in &tr.transitions {
        if x.to == Some(it.id) {
            start = start.min(x.start);
        }
        if x.from == Some(it.id) {
            end = end.max(x.end());
        }
    }
    (start, end)
}

/// The reader `item` (a clip of `seq`) reads its media with: 0 for the file's shared source, or a
/// number of its own (its clip id plus one) when a clip below it (on a lower track, or earlier on
/// the same track) reads the same media over part of the same time at moments more than
/// [`SHARED_SPAN`] apart. The clip below keeps the shared source, so the cuts of a recording that
/// run one after another on a track keep reading it as before.
pub(crate) fn reader(project: &Project, seq: &Sequence, item: &TrackItem) -> u64 {
    let Some((media, _, _)) = project.resolve_media(item.item) else { return 0 };
    let Some(ti) = seq.video_tracks.iter().position(|tr| tr.items.iter().any(|i| i.id == item.id)) else { return 0 };
    let Some(own_track) = seq.video_tracks.get(ti) else { return 0 };
    let (a0, a1) = read_range(own_track, item);
    for (oi, tr) in seq.video_tracks.iter().enumerate().take(ti + 1) {
        if !tr.enabled {
            continue;
        }
        for o in &tr.items {
            if o.id == item.id || !o.enabled || (oi == ti && o.start >= item.start) {
                continue;
            }
            let (b0, b1) = read_range(tr, o);
            let (s, e) = (a0.max(b0), a1.min(b1));
            if s >= e {
                continue;
            }
            if project.resolve_media(o.item).is_none_or(|(m, _, _)| m != media) {
                continue;
            }
            let last = Tick(e.0.saturating_sub(1).max(s.0));
            let apart = |t: Tick| (item.source_time_at(t).0.saturating_sub(o.source_time_at(t).0)).saturating_abs();
            if apart(s).max(apart(last)) > SHARED_SPAN.0 {
                return item.id.0.saturating_add(1);
            }
        }
    }
    0
}

/// The source `item` (a clip of `seq`) reads its frames from (see [`reader`]).
pub(crate) fn clip_source(project: &Project, seq: &Sequence, item: &TrackItem, sources: &dyn SourceProvider) -> Option<SharedSource> {
    match reader(project, seq, item) {
        0 => sources.source(item.item),
        r => sources.reader_source(item.item, r),
    }
}
