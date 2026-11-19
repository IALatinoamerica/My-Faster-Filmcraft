//! The copies of a file the media pool opens for clips that read it beside another clip of the
//! same media (`filmcraft_render::readers`, [`crate::media_pool::MediaPool::reader_source`]).

use std::sync::Arc;

use filmcraft_media::FrameRequest;
use filmcraft_project::ItemId;
use filmcraft_render::SourceProvider;

/// A reader other than 0 gets a copy of the file with a decoder of its own, the same copy each
/// time, a different one per reader; the copy shows the same pictures. Reader 0 is the shared
/// source. A job that reads its own copies gets its own copy per reader too.
#[test]
fn a_reader_reads_its_own_copy_of_the_file() {
    let movie = crate::media_test_util::tmp_dir("reader-copies").join("a.mp4");
    crate::media_test_util::make_movie(&movie, filmcraft_media::DemoScene::Aurora, 64, 36, 12);
    let (s, items, _) = crate::media_test_util::session_with(&[&movie]);
    let item = items[0];
    let shared = s.media.source_for(&s.project, item, &*s.services).unwrap();
    let monitors = s.media.provider(s.project.clone(), s.services.clone());
    assert!(Arc::ptr_eq(&monitors.reader_source(item, 0).unwrap(), &shared), "reader 0 is the shared source");
    assert_eq!(s.media.open_readers(), 0);
    let copy = monitors.reader_source(item, 7).unwrap();
    assert_eq!(s.media.open_readers(), 1);
    assert!(!Arc::ptr_eq(&copy, &shared), "a reader of its own reads the shared source");
    assert!(Arc::ptr_eq(&copy, &monitors.reader_source(item, 7).unwrap()), "one copy per reader");
    assert!(!Arc::ptr_eq(&copy, &monitors.reader_source(item, 8).unwrap()), "each reader its own copy");
    // kept by the pool: another provider of the monitors finds the same copy
    let again = s.media.provider(s.project.clone(), s.services.clone()).reader_source(item, 7).unwrap();
    assert!(Arc::ptr_eq(&again, &copy));
    let req = FrameRequest { time: filmcraft_time::Tick::ZERO, scale: 1.0 };
    assert_eq!(copy.video_frame(req).unwrap().to_rgba8(), shared.video_frame(req).unwrap().to_rgba8());

    let job = s.media.full_res_provider(s.project.clone(), s.services.clone()).own_sources();
    let job_copy = job.reader_source(item, 7).unwrap();
    assert!(!Arc::ptr_eq(&job_copy, &copy) && !Arc::ptr_eq(&job_copy, &job.source(item).unwrap()), "the job's copy is its own");
    assert!(Arc::ptr_eq(&job_copy, &job.reader_source(item, 7).unwrap()));
}

/// The pool keeps a bounded number of copies, dropping the one used longest ago; a forgotten item
/// and `clear` drop theirs.
#[test]
fn the_pool_keeps_a_bounded_number_of_copies() {
    let movie = crate::media_test_util::tmp_dir("reader-cap").join("a.mp4");
    crate::media_test_util::make_movie(&movie, filmcraft_media::DemoScene::Aurora, 64, 36, 12);
    let (s, items, _) = crate::media_test_util::session_with(&[&movie]);
    let item = items[0];
    let open = |r: u64| s.media.reader_source(&s.project, item, r, &*s.services, false).unwrap();
    let first = open(1);
    let mut kept = Vec::new();
    for r in 2..=40 {
        kept.push(open(r));
    }
    assert!(!Arc::ptr_eq(&first, &open(1)), "the copy used longest ago was closed");
    let newest = open(40);
    assert!(Arc::ptr_eq(&newest, kept.last().unwrap()), "a recent copy is kept");
    s.media.remove(item);
    assert!(!Arc::ptr_eq(&newest, &open(40)), "a removed item's copies go");
}

/// What cannot be copied (media that is not a file) reads the shared source.
#[test]
fn media_that_is_not_a_file_reads_the_shared_source() {
    let mut s = crate::Session::default();
    let r = s.execute("file.importDemoFootage", serde_json::json!({"scene": "Aurora"})).unwrap();
    let generator = ItemId(r["items"][0].as_u64().unwrap());
    let shared = s.media.source_for(&s.project, generator, &*s.services).unwrap();
    let monitors = s.media.provider(s.project.clone(), s.services.clone());
    assert!(Arc::ptr_eq(&monitors.reader_source(generator, 3).unwrap(), &shared));
    let job = s.media.full_res_provider(s.project.clone(), s.services.clone()).own_sources();
    assert!(job.reader_source(generator, 3).is_some());
    // an item that does not exist has no source at all
    assert!(monitors.reader_source(ItemId(u64::MAX), 3).is_none());
}
