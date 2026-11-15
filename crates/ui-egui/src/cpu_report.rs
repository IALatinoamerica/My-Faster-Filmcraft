//! Fork: what renders on the CPU, named in the render bar's tooltip. The list is
//! `filmcraft_render::slow_paths`: the clips, adjustment layers and transitions the GPU
//! compositor does not draw, with the reason for each. (Agents get the whole list from the
//! engine's `sequence.cpuReport`.)

use filmcraft_time::TimeRange;

/// Lines of the render bar's tooltip at most.
const TOOLTIP_LINES: usize = 3;

/// What renders on the CPU between `range` of sequence `seq`, as tooltip lines: at most
/// [`TOOLTIP_LINES`], then how many more there are. Empty: nothing there renders on the CPU (a
/// red stretch is then red for the sum of its effects alone).
pub fn reasons(app: &crate::FilmcraftApp, seq: filmcraft_project::ItemId, range: TimeRange) -> Vec<String> {
    let caps = filmcraft_render::plan::noted_device().unwrap_or(filmcraft_render::plan::DeviceCaps::ALL);
    let list = filmcraft_render::slow_paths::slow_paths(&app.session.project, seq, range.start, range.end(), caps);
    let mut lines: Vec<String> = Vec::new();
    for e in &list {
        let line = if e.track == 0 { e.reason.clone() } else { format!("V{} · {}: {}", e.track, e.name, e.reason) };
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    let more = lines.len().saturating_sub(TOOLTIP_LINES);
    lines.truncate(TOOLTIP_LINES);
    if more > 0 {
        lines.push(tlf!("…and {n} more", n = more));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FilmcraftApp;
    use filmcraft_time::Tick;

    /// The tooltip names the clip with an effect that has no GPU version, and nothing for the
    /// stretch the GPU draws.
    #[test]
    fn the_tooltip_names_what_renders_on_the_cpu() {
        use filmcraft_media::Generator;
        use filmcraft_media::generators::GeneratorSource;
        use filmcraft_project::{Label, SequenceSettings, TrackKind, find_effect};
        let mut s = filmcraft_engine::Session::default();
        let r = filmcraft_time::FrameRate::FPS_24;
        let mut p = (*s.project).clone();
        let source = GeneratorSource::new(Generator::ColorMatte { color: [0.2, 0.5, 0.8, 1.0] }, 64, 36, r, r.tick_of(240));
        let matte = filmcraft_engine::demo::add_generator(&mut p, &s.media, source, "Matte", Label::Iris, None);
        let seq = p.new_sequence("Seq", SequenceSettings { width: 64, height: 36, frame_rate: r, ..Default::default() }, 2, 0, None);
        let plain = p.make_track_item(matte, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, r.tick_of(24)), r).unwrap();
        let mut twirled = p.make_track_item(matte, TrackKind::Video, r.tick_of(24), TimeRange::new(Tick::ZERO, r.tick_of(24)), r).unwrap();
        twirled.name = "Twirled".into();
        twirled.effects.insert(0, find_effect("twirl").unwrap().instance());
        p.sequence_mut(seq).unwrap().video_tracks[0].items = vec![plain, twirled];
        s.project = std::sync::Arc::new(p);
        let app = FilmcraftApp::new(s);
        let lines = reasons(&app, seq, TimeRange::new(r.tick_of(24), r.tick_of(24)));
        assert!(matches!(lines.as_slice(), [l] if l.starts_with("V1 · Twirled: ") && l.contains("Twirl")), "{lines:?}");
        assert!(reasons(&app, seq, TimeRange::new(Tick::ZERO, r.tick_of(24))).is_empty());
    }
}
