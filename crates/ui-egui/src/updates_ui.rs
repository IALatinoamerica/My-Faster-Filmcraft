//! Software updates, background check at startup, and startup prompt modal.

use serde_json::Value;

use crate::FilmcraftApp;

#[cfg(not(target_arch = "wasm32"))]
use craft_update::updater::{Action, Status, Updater};
#[cfg(not(target_arch = "wasm32"))]
use craft_update::{CheckResultDialog, CheckResultResponse};

pub(crate) struct Runtime {
    #[cfg(not(target_arch = "wasm32"))]
    updater: Updater,
    /// The desktop app turns the check at start on; tests and the web build leave it off.
    #[cfg(not(target_arch = "wasm32"))]
    startup_check: bool,
    #[cfg(not(target_arch = "wasm32"))]
    check_requested: bool,
    /// The context of the last frame, to wake the UI when a check is queued from a menu callback.
    #[cfg(not(target_arch = "wasm32"))]
    repaint: Option<egui::Context>,
    /// Fork: the release a check found. The updater itself is never told about it (see
    /// [`fork_updater`]), so nothing here can install the original app over this fork.
    #[cfg(not(target_arch = "wasm32"))]
    found: Found,
    pub(crate) manual_window_open: bool,
}

#[cfg(not(target_arch = "wasm32"))]
type Found = std::sync::Arc<std::sync::Mutex<Option<craft_update::Release>>>;

/// Fork: an updater whose checks keep the release they find in `found` and tell the updater that
/// nothing is newer. The library's own prompt installs the original FilmCraft over the installed
/// package ("Update and Restart"), which would replace this fork; it never opens this way, and
/// [`fork_notice`] shows the release with that button greyed out. See `UPDATING.md`.
#[cfg(not(target_arch = "wasm32"))]
fn fork_updater(found: &Found) -> Updater {
    let found = std::sync::Arc::clone(found);
    let version = env!("CARGO_PKG_VERSION");
    Updater::new(&crate::UPDATE_APP, version).with_source(std::sync::Arc::new(move || {
        let http = craft_update::Http::new(&format!("{}/Update", crate::UPDATE_APP.name))?;
        let release = craft_update::check_for_update(&crate::UPDATE_APP, &http, version)?;
        if release.is_some() {
            *found.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = release;
        }
        Ok(None)
    }))
}

/// Fork: the notice of a newer release. Its update button is always disabled; Later closes it
/// until the next check.
#[cfg(not(target_arch = "wasm32"))]
fn fork_notice(ctx: &egui::Context, found: &Found) {
    let mut found = found.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(release) = found.as_ref() else { return };
    let mut close = false;
    egui::Window::new(crate::i18n::t("FilmCraft Updates"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(format!("FilmCraft v{} to {}", env!("CARGO_PKG_VERSION"), release.tag));
            ui.add_space(6.0);
            ui.label(tl!(
                "This copy is a fork with its own optimizations. Installing the original update would replace it, so it is updated from the fork's folder instead (UPDATING.md)."
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_enabled(false, egui::Button::new(tl!("Update and Restart")));
                close = ui.button(tl!("Later")).clicked();
            });
        });
    if close {
        *found = None;
    }
}

impl Default for Runtime {
    fn default() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let found = Found::default();
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            updater: fork_updater(&found),
            #[cfg(not(target_arch = "wasm32"))]
            found,
            #[cfg(not(target_arch = "wasm32"))]
            startup_check: false,
            #[cfg(not(target_arch = "wasm32"))]
            check_requested: false,
            #[cfg(not(target_arch = "wasm32"))]
            repaint: None,
            manual_window_open: false,
        }
    }
}

impl FilmcraftApp {
    /// Whether the app asks for a newer release when it starts (off until the desktop app turns it on).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_startup_update_check(&mut self, on: bool) {
        self.updates_rt.startup_check = on;
    }
}

pub(crate) fn check(app: &mut FilmcraftApp) -> Result<Value, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.updates_rt.check_requested = true;
        app.updates_rt.manual_window_open = true;
        if let Some(ctx) = &app.updates_rt.repaint {
            ctx.request_repaint();
        }
        Ok(Value::Null)
    }
    #[cfg(target_arch = "wasm32")]
    {
        app.updates_rt.manual_window_open = true;
        Ok(Value::Null)
    }
}

pub(crate) fn tick(app: &mut FilmcraftApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let rt = &mut app.updates_rt;
        rt.repaint = Some(ctx.clone());
        if std::mem::take(&mut rt.check_requested) {
            rt.updater.check_now(ctx);
        } else if rt.startup_check {
            rt.updater.check_at_start(ctx);
        }
        rt.updater.tick(ctx);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ctx);
}

pub(crate) fn show(app: &mut FilmcraftApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // The restart helper is scheduled before the app closes, so no update starts over unsaved work.
        let clean = !app.session.is_dirty();
        match app.updates_rt.updater.show_if(ctx, clean) {
            Action::OpenUrl(url) => crate::links::open(app, ctx, &url),
            Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::None => {}
        }
        let rt = &mut app.updates_rt;
        // Fork: a found release shows in our notice, not as "up to date" in the result dialog.
        if rt.found.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some() && matches!(rt.updater.status(), Status::UpToDate) {
            rt.manual_window_open = false;
            rt.updater.dismiss_status();
        }
        fork_notice(ctx, &rt.found);
        rt.manual_window_open = result_window_open(rt.manual_window_open, rt.updater.status(), rt.check_requested);
        if rt.manual_window_open {
            let dialog = CheckResultDialog::new(&crate::UPDATE_APP, env!("CARGO_PKG_VERSION"));
            match dialog.show(ctx, rt.updater.status()) {
                CheckResultResponse::Close => {
                    rt.manual_window_open = false;
                    rt.updater.dismiss_status();
                }
                CheckResultResponse::CheckAgain => rt.check_requested = true,
                // The next frame's `show_if` asks the window to close once nothing is unsaved.
                CheckResultResponse::RestartToUpdate => {
                    rt.updater.restart_to_update();
                    ctx.request_repaint();
                }
                CheckResultResponse::None => {}
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    if app.updates_rt.manual_window_open {
        let mut open = true;
        egui::Window::new(crate::i18n::t("FilmCraft Updates"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label(crate::i18n::t("Update check is not supported in the web browser."));
                ui.add_space(8.0);
                if ui.button(crate::i18n::t("OK")).clicked() {
                    app.updates_rt.manual_window_open = false;
                }
            });
        if !open {
            app.updates_rt.manual_window_open = false;
        }
    }
}

/// Whether the check result dialog shows this frame. It answers Help ▸ Check for Updates until the
/// requested check ends, and it shows whenever the status is not idle: at the first start after an
/// update, the check at start leaves `Updated` or `InstallFailed`. A check that finds a release
/// leaves the status idle and the startup prompt takes over, so the dialog closes then.
#[cfg(not(target_arch = "wasm32"))]
fn result_window_open(open: bool, status: &Status, check_requested: bool) -> bool {
    !matches!(status, Status::Idle) || (open && check_requested)
}

/// Starts an update that waits for the app to exit. Called from `on_exit`; dropping the updater
/// does the same, and later calls do nothing.
pub(crate) fn exiting(app: &mut FilmcraftApp) {
    #[cfg(not(target_arch = "wasm32"))]
    app.updates_rt.updater.start_pending_update();
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_runtime_is_idle() {
        let rt = Runtime::default();
        assert!(!rt.manual_window_open);
        #[cfg(not(target_arch = "wasm32"))]
        {
            assert!(!rt.startup_check);
            assert!(!rt.updater.prompt_open());
            assert_eq!(rt.updater.status(), &Status::Idle);
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn startup_prompt_shows_and_closes() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        let release = craft_update::Release {
            tag: "v99.0.0".into(),
            version: craft_update::parse_version("99.0.0"),
            name: "FilmCraft 99.0.0".into(),
            html_url: "https://github.com/storytold/filmcraft/releases/tag/v99.0.0".into(),
            ..Default::default()
        };
        app.updates_rt.updater.offer(release);

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(&mut app, ui.ctx());
        });
        output.textures_delta.clear();
        assert!(app.updates_rt.updater.prompt_open());
    }

    /// Fork: a release a check found never reaches the library's prompt (whose button installs
    /// the original app); it shows in our notice, and Later is the only button that works.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_found_release_shows_the_fork_notice_and_not_the_installing_prompt() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        let release = craft_update::Release { tag: "v99.0.0".into(), version: craft_update::parse_version("99.0.0"), ..Default::default() };
        *app.updates_rt.found.lock().unwrap() = Some(release);
        let mut painted = String::new();
        for _ in 0..2 {
            let output = ctx.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui.ctx()));
            painted = format!("{:?}", output.shapes);
        }
        assert!(painted.contains("v99.0.0"), "the notice names the release");
        assert!(!app.updates_rt.updater.prompt_open(), "the library's prompt stays closed");
        assert_eq!(app.updates_rt.updater.status(), &Status::Idle);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn result_window_shows_every_status_but_idle() {
        let shown = [
            Status::Checking,
            Status::UpToDate,
            Status::Failed("offline".into()),
            Status::Installing,
            Status::Ready,
            Status::Managed,
            Status::Updated("1.2.3".into()),
            Status::InstallFailed("access denied".into()),
        ];
        for status in &shown {
            assert!(result_window_open(false, status, false), "{status:?} opens the dialog without a requested check");
        }
        assert!(!result_window_open(false, &Status::Idle, false));
        assert!(!result_window_open(true, &Status::Idle, false), "a check that found a release closes it");
        assert!(result_window_open(true, &Status::Idle, true), "a queued check keeps it open");
        assert!(!result_window_open(false, &Status::Idle, true), "a queued check does not open it");
    }
}
