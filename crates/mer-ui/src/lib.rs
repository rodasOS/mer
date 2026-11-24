//! GPUI user interface. The only crate (with the binary) that depends on GPUI.

mod actions;
mod commands;
mod kwin;
mod settings;
mod shell;
mod sound;
mod terminal_view;
mod theme;
mod tooltip;
mod workspace_view;

use std::sync::OnceLock;
use std::time::Instant;

use gpui::{
    App, AppContext as _, Bounds, Entity, Global, QuitMode, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};

use crate::workspace_view::WorkspaceView;

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Milliseconds since `run` started, for the `MER_BENCH` first-frame report.
pub(crate) fn ms_since_start() -> u128 {
    STARTED.get().map_or(0, |started| started.elapsed().as_millis())
}

/// A saved window dimension as a float. Saved sizes are clamped to 16000, well inside `u16`.
fn pixels(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

/// What the command line asks the UI to do.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Run this program (and arguments) in the first terminal instead of the shell (`mer -e ...`).
    pub command: Option<Vec<String>>,
}

/// The one workspace view, alive for as long as the app. Windows come and go around it, which
/// is what lets terminals keep running after the window is closed.
struct MainView {
    view: Entity<WorkspaceView>,
    /// `mer -e`: a throwaway instance that never stays in the background.
    throwaway: bool,
}

impl Global for MainView {}

/// Smallest window, in pixels: about 60 columns by 16 rows of terminal.
const MIN_WINDOW: (f32, f32) = (480.0, 320.0);

fn window_options(ui: &mer_store::UiState, cx: &App) -> WindowOptions {
    let window_bounds = match ui.window {
        Some(saved) => {
            let bounds =
                Bounds::centered(None, size(px(pixels(saved.width)), px(pixels(saved.height))), cx);
            match saved.mode {
                mer_store::WindowMode::Windowed => WindowBounds::Windowed(bounds),
                mer_store::WindowMode::Maximized => WindowBounds::Maximized(bounds),
                mer_store::WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
            }
        }
        None => WindowBounds::Windowed(Bounds::centered(None, size(px(1000.0), px(640.0)), cx)),
    };
    WindowOptions {
        window_bounds: Some(window_bounds),
        titlebar: Some(TitlebarOptions { title: Some("mer".into()), ..Default::default() }),
        app_id: Some("mer".to_owned()),
        // Small enough to be a sliver, big enough that every overlay still fits and can be closed.
        window_min_size: Some(size(px(MIN_WINDOW.0), px(MIN_WINDOW.1))),
        ..Default::default()
    }
}

/// Opens the main window on the existing view, unless one is already open.
pub(crate) fn open_main_window(cx: &mut App) {
    let Some(view) = cx.try_global::<MainView>().map(|main| main.view.clone()) else { return };
    if view.read(cx).has_window() {
        return;
    }
    let options = window_options(&view.read(cx).saved_ui(), cx);
    let result = cx.open_window(options, |window, cx| {
        view.update(cx, |view, cx| view.attach(window, cx));
        view
    });
    if let Err(error) = result {
        tracing::error!(%error, "failed to open the main window");
        cx.quit();
    }
}

/// Opens the main window and runs the UI until it is closed.
pub fn run(options: RunOptions) {
    let _ = STARTED.set(Instant::now());
    // GPUI would quit by itself when the last window closes; `on_window_closed` decides instead,
    // because with "keep running" the app outlives its window.
    gpui_platform::application().with_quit_mode(QuitMode::Explicit).run(|cx: &mut App| {
        let (settings, mut warnings) = settings::AppSettings::load();
        warnings.extend(actions::apply_key_bindings(cx, &settings.config.keys));
        for warning in warnings {
            tracing::warn!("{warning}");
        }
        cx.set_global(settings);
        // A throwaway `mer -e` window always opens at the default size.
        let throwaway = options.command.is_some();
        let (ui, store) = if throwaway {
            (mer_store::UiState::default(), None)
        } else {
            workspace_view::open_store()
        };
        let view = cx.new(|cx| WorkspaceView::new(options, ui, store, cx));
        cx.set_global(MainView { view, throwaway });
        open_main_window(cx);

        cx.on_window_closed(|cx, _| {
            if !cx.windows().is_empty() {
                return;
            }
            let keep = cx.global::<settings::AppSettings>().config.spaces.keep_running_on_close;
            match cx.try_global::<MainView>().map(|main| (main.view.clone(), main.throwaway)) {
                // Closing the window leaves mer running; `mer` or the launcher brings it back.
                Some((view, false)) if keep => view.update(cx, WorkspaceView::window_closed),
                _ => cx.quit(),
            }
        })
        .detach();
        cx.activate(true);
    });
}
