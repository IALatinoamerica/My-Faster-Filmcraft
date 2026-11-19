//! The monitors' provider hands a clip the copy of its media the pool opened for its reader
//! (`filmcraft_render::readers`): without it, a file shown twice at once stalls playback again.

use std::sync::Arc;

use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{DemoScene, Generator, MediaSource};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, Project, SequenceSettings, TrackKind};
use filmcraft_render::SourceProvider;
use filmcraft_time::{FrameRate, Tick, TimeRange};

use super::JobProvider;

/// Write a short movie of a demo scene to `path` (ProRes in a QuickTime file).
fn make_movie(path: &std::path::Path) {
    let rate = FrameRate::FPS_24;
    let dur = rate.tick_of(12);
    let src = GeneratorSource::new(Generator::Demo(DemoScene::Aurora), 64, 36, rate, dur);
    let mut p = Project::new("fixture");
    let clip = MediaClip {
        media: MediaRef::Generator(Generator::Demo(DemoScene::Aurora)),
        info: src.info().clone(),
        interpret: Default::default(),
        mark_in: None,
        mark_out: None,
        markers: vec![],
        offline: false,
        proxy: None,
        identity: None,
    };
    let item = p.add_item("scene", Label::Iris, ItemKind::Media(Box::new(clip)), None);
    let seq = p.new_sequence("s", SequenceSettings { width: 64, height: 36, frame_rate: rate, ..Default::default() }, 1, 0, None);
    let v = p.make_track_item(item, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, dur), rate).unwrap();
    p.sequence_mut(seq).unwrap().video_tracks[0].items.push(v);
    let settings =
        filmcraft_export::ExportSettings { format: filmcraft_export::Format::ProRes, path: path.to_string_lossy().into_owned(), ..Default::default() };
    let shared: filmcraft_media::SharedSource = Arc::new(src);
    let provider = move |id: ItemId| (id == item).then(|| shared.clone());
    filmcraft_export::export(&Arc::new(p), seq, &settings, &provider, &Default::default()).unwrap();
}

#[test]
fn the_monitors_provider_passes_a_readers_copy_on() {
    let dir = std::env::temp_dir().join(format!("filmcraft-job-readers-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let movie = dir.join("a.mov");
    make_movie(&movie);
    let mut s = filmcraft_engine::Session::default();
    let r = s.execute("file.import", serde_json::json!({"paths": [movie.to_string_lossy()]})).unwrap();
    let item = ItemId(r["items"][0].as_u64().unwrap());
    let job = JobProvider { inner: s.media.provider(s.project.clone(), s.services.clone()) };
    let shared = s.media.source_for(&s.project, item, &*s.services).unwrap();
    assert_eq!(s.media.open_readers(), 0);
    let req = filmcraft_media::FrameRequest { time: Tick::ZERO, scale: 1.0 };
    let via_job = job.reader_source(item, 5).unwrap();
    assert_eq!(s.media.open_readers(), 1, "the monitors' provider opened the reader's copy");
    assert_eq!(via_job.video_frame(req).unwrap().to_rgba8(), shared.video_frame(req).unwrap().to_rgba8());
    // reader 0 is the shared source and opens nothing
    let _ = job.reader_source(item, 0).unwrap();
    assert_eq!(s.media.open_readers(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
