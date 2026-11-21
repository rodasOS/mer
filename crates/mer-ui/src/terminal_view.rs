//! The terminal view: owns a [`Session`], paints its grid and forwards input.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use futures::StreamExt;
use gpui::{
    AnyElement, ClickEvent, ClipboardItem, Context, FocusHandle, Focusable, Font, FontStyle,
    FontWeight, Hsla, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Render,
    ScrollEvent, StatefulInteractiveElement, StrikethroughStyle, Styled, TextAlign, TextRun,
    UnderlineStyle, Window, canvas, div, fill, point, px, size,
};
use gpui::{MouseButton as UiButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
use mer_term::alacritty_terminal::event::Event;
use mer_term::alacritty_terminal::grid::Dimensions as _;
use mer_term::alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use mer_term::alacritty_terminal::selection::{Selection, SelectionType};
use mer_term::alacritty_terminal::term::TermMode;
use mer_term::alacritty_terminal::term::cell::Flags;
use mer_term::alacritty_terminal::vte::ansi::{Color, CursorShape};
use mer_term::keys::{KeyInput, encode};
use mer_term::mouse::{self, MouseAction, MouseButton, MouseEvent};
use mer_term::search::Search;
use mer_term::{GridSize, Session, ShellCommand, SpawnOptions};

use crate::settings::{AppSettings, Zoom};
use crate::theme::{Palette, accent_color, overlay_shadow, resolve_cached, to_rgb, with_alpha};

/// Pixel metrics measured while painting, shared with the input handlers.
#[derive(Debug, Clone, Copy)]
struct Metrics {
    line_height: Pixels,
    cell_width: Pixels,
    /// Top-left corner of the grid, in window coordinates.
    origin: gpui::Point<Pixels>,
}

/// A cell of the visible grid under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellHit {
    col: usize,
    row: usize,
    /// Which half of the cell, so selections can include or skip the edge cell.
    side: Side,
}

/// How to start the shell of a pane.
#[derive(Debug, Clone, Default)]
pub struct PaneLaunch {
    pub cwd: Option<PathBuf>,
    /// ANSI text drawn before the shell starts (history of a pane that was asleep).
    pub restore: Option<String>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Run this program instead of the shell.
    pub command: Option<Vec<String>>,
    /// Lines of scrollback to keep.
    pub scrollback: usize,
    /// Shell to run (`None` = automatic).
    pub shell: Option<String>,
}

/// Font settings read from the config on every frame, so changes apply immediately.
#[derive(Debug, Clone)]
struct Look {
    family: gpui::SharedString,
    font_size: f32,
    line_height: f32,
}

impl Look {
    fn from_settings(settings: &AppSettings) -> Self {
        let appearance = &settings.config.appearance;
        Self {
            family: appearance.font_family.clone().into(),
            font_size: settings.font_size(),
            line_height: appearance.line_height,
        }
    }

    fn cell_line_height(&self) -> Pixels {
        (px(self.font_size) * self.line_height).round()
    }
}

/// What a terminal tells whoever owns it.
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// The program changed the title. Empty means "reset".
    Title(String),
    /// The shell exited.
    Exited,
    /// The terminal produced output. Debounced: at most one event per `ACTIVITY_DEBOUNCE`,
    /// and none while the terminal is idle. The owner uses it to look at the foreground program.
    Activity,
    /// The user right-clicked the terminal (and the program is not tracking the mouse).
    ContextMenu(gpui::Point<gpui::Pixels>),
    /// Ctrl + wheel: the user wants the text bigger or smaller.
    Zoom(crate::settings::Zoom),
    /// The user pressed Esc, Ctrl+C or Enter: it may change what an agent in here is doing.
    Key(mer_agents::AgentKey),
}

const ACTIVITY_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(350);

pub struct TerminalView {
    session: Rc<std::cell::RefCell<Session>>,
    focus: FocusHandle,
    palette: Palette,
    metrics: Rc<Cell<Metrics>>,
    scratch: Rc<std::cell::RefCell<PaintScratch>>,
    scroll_remainder: f32,
    activity_pending: bool,
    /// A selection drag is in progress.
    selecting: bool,
    /// Last cell sent to a program that tracks the mouse, to avoid repeating motion events.
    last_reported: Option<(usize, usize)>,
    /// The search in progress, shared with painting so it can highlight the matches.
    search: Rc<std::cell::RefCell<Option<Search>>>,
    /// What the search bar shows. `Some` exactly while the bar is open.
    search_bar: Option<SearchBar>,
    search_focus: FocusHandle,
    /// Where focus should go on the next frame, once a window is at hand.
    pending_focus: Option<PendingFocus>,
    _events: gpui::Task<()>,
}

/// What the search bar shows besides the text being typed.
struct SearchBar {
    query: String,
    /// Matches in the whole history, counted up to `SEARCH_COUNT_CAP`.
    total: usize,
}

#[derive(Clone, Copy)]
enum PendingFocus {
    Search,
    Terminal,
}

/// The count of matches stops here; the bar then says `999+`.
const SEARCH_COUNT_CAP: usize = 999;

impl TerminalView {
    pub fn new(launch: PaneLaunch, cx: &mut Context<Self>) -> Self {
        let size = GridSize { cols: 100, lines: 30, cell_width: 8, cell_height: 18 };
        let shell = match launch.command.as_deref() {
            Some([program, args @ ..]) => {
                Some(ShellCommand { program: program.clone(), args: args.to_vec() })
            }
            _ => crate::shell::preferred_shell(launch.shell.as_deref()),
        };
        let options = SpawnOptions {
            shell,
            env: launch.env,
            restore: launch.restore,
            ..SpawnOptions::new(launch.cwd, size, launch.scrollback)
        };
        let (session, mut events) = Session::spawn(options).expect("failed to start the shell");
        let session = Rc::new(std::cell::RefCell::new(session));

        let task = cx.spawn({
            let session = Rc::clone(&session);
            async move |this, cx| {
                while let Some(first) = events.next().await {
                    // Coalesce bursts of wakeups into a single redraw.
                    let mut batch = vec![first];
                    while let Ok(event) = events.try_recv() {
                        batch.push(event);
                    }
                    let session = Rc::clone(&session);
                    let result = this.update(cx, move |this, cx| {
                        this.handle_events(&session, batch, cx);
                    });
                    if result.is_err() {
                        break;
                    }
                }
            }
        });

        Self {
            session,
            focus: cx.focus_handle(),
            palette: cx.global::<AppSettings>().palette.clone(),
            metrics: Rc::new(Cell::new(Metrics {
                line_height: Look::from_settings(cx.global::<AppSettings>()).cell_line_height(),
                cell_width: px(8.0),
                origin: gpui::point(px(0.0), px(0.0)),
            })),
            scratch: Rc::default(),
            scroll_remainder: 0.0,
            activity_pending: false,
            selecting: false,
            last_reported: None,
            search: Rc::default(),
            search_bar: None,
            search_focus: cx.focus_handle(),
            pending_focus: None,
            _events: task,
        }
    }

    fn handle_events(
        &mut self,
        session: &Rc<std::cell::RefCell<Session>>,
        events: Vec<Event>,
        cx: &mut Context<Self>,
    ) {
        let saw_output = events.iter().any(|event| matches!(event, Event::Wakeup));
        if saw_output && !self.activity_pending {
            self.activity_pending = true;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ACTIVITY_DEBOUNCE).await;
                let _ = this.update(cx, |this, cx| {
                    this.activity_pending = false;
                    cx.emit(TerminalEvent::Activity);
                });
            })
            .detach();
        }
        for event in events {
            match event {
                Event::Title(title) => cx.emit(TerminalEvent::Title(title)),
                Event::ResetTitle => cx.emit(TerminalEvent::Title(String::new())),
                Event::PtyWrite(text) => session.borrow().write(text.into_bytes()),
                Event::ClipboardStore(_, text) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                Event::ClipboardLoad(_, format) => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        session.borrow().write(format(&text).into_bytes());
                    }
                }
                Event::ColorRequest(index, format) => {
                    let color = to_rgb(self.palette.resolve_index(index));
                    session.borrow().write(format(color).into_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let size = session.borrow().size();
                    session.borrow().write(format(size.into()).into_bytes());
                }
                Event::Exit | Event::ChildExit(_) => cx.emit(TerminalEvent::Exited),
                Event::Wakeup
                | Event::MouseCursorDirty
                | Event::CursorBlinkingChange
                | Event::Bell => {}
            }
        }
        cx.notify();
    }

    /// Where keys should go: the search box while the bar is open, the terminal otherwise.
    pub fn focus_handle(&self) -> FocusHandle {
        if self.search_bar.is_some() { self.search_focus.clone() } else { self.focus.clone() }
    }

    /// Opens the search bar, optionally with a text already typed in. Closes it for `Some("")`.
    pub fn open_search(&mut self, query: Option<&str>, cx: &mut Context<Self>) {
        if query == Some("") {
            return self.close_search(cx);
        }
        if self.search_bar.is_none() {
            *self.search.borrow_mut() = Some(Search::new());
            self.search_bar = Some(SearchBar { query: String::new(), total: 0 });
        }
        self.pending_focus = Some(PendingFocus::Search);
        if let Some(query) = query {
            self.set_search_query(query.to_owned(), cx);
        }
        cx.notify();
    }

    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        if self.search_bar.take().is_some() {
            *self.search.borrow_mut() = None;
            self.pending_focus = Some(PendingFocus::Terminal);
            cx.notify();
        }
    }

    pub fn is_searching(&self) -> bool {
        self.search_bar.is_some()
    }

    /// The text changed: count the matches and jump to the newest one.
    fn set_search_query(&mut self, query: String, cx: &mut Context<Self>) {
        let session = self.session.borrow();
        let mut term = session.term().lock();
        let total = self.search.borrow_mut().as_mut().map_or(0, |search| {
            search.set_query(&query);
            let total = search.count(&term, SEARCH_COUNT_CAP);
            search.jump(&mut term, true);
            total
        });
        drop(term);
        drop(session);
        self.search_bar = Some(SearchBar { query, total });
        cx.notify();
    }

    /// Goes to the next match: `older` is up, otherwise down.
    pub fn search_step(&mut self, older: bool, cx: &mut Context<Self>) {
        let session = self.session.borrow();
        let mut term = session.term().lock();
        if let Some(search) = self.search.borrow_mut().as_mut() {
            search.jump(&mut term, older);
        }
        cx.notify();
    }

    /// Keys typed into the search box. Always handled here, never sent to the shell.
    fn on_search_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let Some(bar) = &self.search_bar else { return };
        let mut query = bar.query.clone();
        match keystroke.key.as_str() {
            "escape" => return self.close_search(cx),
            "enter" => return self.search_step(!modifiers.shift, cx),
            "up" => return self.search_step(true, cx),
            "down" => return self.search_step(false, cx),
            "backspace" => {
                query.pop();
            }
            _ => {
                if modifiers.control || modifiers.alt || modifiers.platform {
                    return;
                }
                match keystroke.key_char.as_deref() {
                    Some(text) => query.push_str(text),
                    None => return,
                }
            }
        }
        self.set_search_query(query, cx);
    }

    /// The search bar, top right of the terminal.
    #[allow(clippy::too_many_lines)] // one builder chain
    fn render_search_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.search_bar.as_ref()?;
        let chrome = &self.palette.chrome;
        let (text, dim, accent) = (chrome.text, chrome.text_dim, chrome.accent);
        let scale = cx.global::<AppSettings>().config.appearance.ui_scale;
        let u = move |value: f32| px(value * scale);
        let hover = self.palette.hover();
        let counted = if bar.total >= SEARCH_COUNT_CAP {
            format!("{SEARCH_COUNT_CAP}+")
        } else {
            bar.total.to_string()
        };
        let summary = if bar.query.is_empty() {
            String::new()
        } else if bar.total == 0 {
            "No matches".to_owned()
        } else {
            format!("{counted} found")
        };
        let shown = if bar.query.is_empty() {
            div().text_color(dim).child("Search…")
        } else {
            div().text_color(text).child(format!("{}▏", bar.query))
        };
        let button = |id: &'static str, glyph: &'static str| {
            div()
                .id(id)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(u(26.0))
                .rounded_lg()
                .cursor_pointer()
                .text_color(dim)
                .hover(move |button| button.bg(hover).text_color(text))
                .child(glyph)
        };
        Some(
            div()
                .absolute()
                .top(u(10.0))
                .right(u(12.0))
                .w(u(360.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(u(6.0))
                .p(u(6.0))
                .rounded_xl()
                .border_1()
                .border_color(self.palette.hairline())
                .bg(chrome.bar)
                .shadow(overlay_shadow(self.palette.is_dark()))
                .track_focus(&self.search_focus)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                    this.on_search_key(event, cx);
                    cx.stop_propagation();
                }))
                // Clicks on the bar are not clicks on the terminal behind it.
                .on_mouse_down(UiButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(u(28.0))
                        .px(u(10.0))
                        .flex()
                        .items_center()
                        .rounded_lg()
                        .border_1()
                        .border_color(accent)
                        .bg(chrome.surface)
                        .overflow_hidden()
                        .child(shown.min_w_0()),
                )
                .child(
                    div()
                        .flex_none()
                        .min_w(u(64.0))
                        .text_xs()
                        .text_color(if bar.total == 0 && !bar.query.is_empty() {
                            accent_warning(&self.palette)
                        } else {
                            dim
                        })
                        .child(summary),
                )
                .child(button("search-older", "↑").on_click(
                    cx.listener(|this, _: &ClickEvent, _window, cx| this.search_step(true, cx)),
                ))
                .child(button("search-newer", "↓").on_click(
                    cx.listener(|this, _: &ClickEvent, _window, cx| this.search_step(false, cx)),
                ))
                .child(button("search-close", "×").on_click(
                    cx.listener(|this, _: &ClickEvent, _window, cx| this.close_search(cx)),
                ))
                .into_any_element(),
        )
    }

    /// Types text into the shell, as if the user had typed it.
    pub fn write_text(&self, text: &str) {
        self.session.borrow().write(text.as_bytes().to_vec());
    }

    /// Process id of the shell.
    pub fn pid(&self) -> u32 {
        self.session.borrow().pid()
    }

    /// A detached capture of the screen and scrollback, for saving several panes in parallel.
    pub fn snapshotter(&self) -> impl FnOnce() -> String + Send + 'static + use<> {
        self.session.borrow().snapshotter()
    }

    /// Current directory of the shell.
    pub fn cwd(&self) -> Option<PathBuf> {
        self.session.borrow().cwd()
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;

        if modifiers.control && modifiers.shift {
            match keystroke.key.as_str() {
                "v" => self.paste(cx),
                "c" => self.copy_selection(cx),
                _ => {}
            }
            return;
        }

        let input = KeyInput {
            key: &keystroke.key,
            key_char: keystroke.key_char.as_deref(),
            ctrl: modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
        };
        let session = self.session.borrow();
        let app_cursor = session.mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = encode(&input, app_cursor) {
            session.term().lock().selection = None;
            session.scroll_to_bottom();
            session.write(bytes);
            drop(session);
            if let Some(key) =
                mer_agents::AgentKey::classify(&keystroke.key, input.ctrl, input.alt, input.shift)
            {
                cx.emit(TerminalEvent::Key(key));
            }
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub fn paste(&self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.paste_text(&text);
        }
    }

    fn paste_text(&self, text: &str) {
        let session = self.session.borrow();
        let bracketed = session.mode().contains(TermMode::BRACKETED_PASTE);
        let bytes = mer_term::paste::paste_bytes(text, bracketed);
        session.scroll_to_bottom();
        session.write(bytes);
    }

    fn on_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let line_height = self.metrics.get().line_height;
        let delta = event.delta.pixel_delta(line_height);
        self.scroll_remainder += delta.y / line_height;
        let lines = self.scroll_remainder.trunc();
        self.scroll_remainder -= lines;
        if lines == 0.0 {
            return;
        }
        #[allow(clippy::cast_possible_truncation)]
        let lines = lines as i32;
        if event.modifiers.control {
            // Ctrl + wheel zooms, even in programs that track the mouse.
            let change = if lines > 0 { Zoom::In } else { Zoom::Out };
            for _ in 0..lines.unsigned_abs() {
                cx.emit(TerminalEvent::Zoom(change));
            }
            return;
        }
        let mode = self.session.borrow().mode();
        let bypass = event.modifiers.shift;

        if mode.intersects(TermMode::MOUSE_MODE) && !bypass {
            let Some(hit) = self.cell_at(event.position) else { return };
            let button = if lines > 0 { MouseButton::WheelUp } else { MouseButton::WheelDown };
            for _ in 0..lines.unsigned_abs() {
                self.report_mouse(button, MouseAction::Press, hit, event.modifiers);
            }
        } else if mode.contains(TermMode::ALT_SCREEN)
            && mode.contains(TermMode::ALTERNATE_SCROLL)
            && !bypass
        {
            // Full-screen programs without mouse tracking (less, man): the wheel moves the cursor.
            let arrow: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            let session = self.session.borrow();
            for _ in 0..lines.unsigned_abs() {
                session.write(arrow.to_vec());
            }
        } else {
            self.session.borrow().scroll(lines);
        }
        cx.notify();
    }

    /// The grid cell under a window position, clamped to the grid.
    fn cell_at(&self, position: gpui::Point<Pixels>) -> Option<CellHit> {
        let metrics = self.metrics.get();
        let size = self.session.borrow().size();
        let (cols, lines) = (usize::from(size.cols), usize::from(size.lines));
        if cols == 0 || lines == 0 || metrics.cell_width <= px(0.0) {
            return None;
        }
        let x = f32::from(position.x - metrics.origin.x).max(0.0);
        let y = f32::from(position.y - metrics.origin.y).max(0.0);
        let (cell_w, cell_h) = (f32::from(metrics.cell_width), f32::from(metrics.line_height));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (col, row) = ((x / cell_w) as usize, (y / cell_h) as usize);
        let half = if (x % cell_w) < cell_w / 2.0 { Side::Left } else { Side::Right };
        Some(CellHit { col: col.min(cols - 1), row: row.min(lines - 1), side: half })
    }

    fn grid_point(&self, hit: CellHit) -> GridPoint {
        let offset = self.session.borrow().term().lock().grid().display_offset();
        let line = i32::try_from(hit.row).unwrap_or(0) - i32::try_from(offset).unwrap_or(0);
        GridPoint::new(Line(line), Column(hit.col))
    }

    fn report_mouse(
        &self,
        button: MouseButton,
        action: MouseAction,
        hit: CellHit,
        modifiers: gpui::Modifiers,
    ) {
        let session = self.session.borrow();
        let sgr = session.mode().contains(TermMode::SGR_MOUSE);
        let event = MouseEvent {
            button,
            action,
            col: hit.col,
            line: hit.row,
            shift: modifiers.shift,
            alt: modifiers.alt,
            ctrl: modifiers.control,
        };
        if let Some(bytes) = mouse::encode(&event, sgr) {
            session.write(bytes);
        }
    }

    /// Does the program running here want mouse events (and is the user not forcing selection)?
    fn mouse_reported(&self, modifiers: gpui::Modifiers) -> bool {
        !modifiers.shift && self.session.borrow().mode().intersects(TermMode::MOUSE_MODE)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(hit) = self.cell_at(event.position) else { return };
        if self.mouse_reported(event.modifiers) {
            let Some(button) = wire_button(event.button) else { return };
            self.last_reported = Some((hit.col, hit.row));
            self.report_mouse(button, MouseAction::Press, hit, event.modifiers);
            return;
        }
        match event.button {
            UiButton::Left => {
                let kind = match event.click_count {
                    1 => SelectionType::Simple,
                    2 => SelectionType::Semantic,
                    _ => SelectionType::Lines,
                };
                let point = self.grid_point(hit);
                self.session.borrow().term().lock().selection =
                    Some(Selection::new(kind, point, hit.side));
                self.selecting = true;
                cx.notify();
            }
            UiButton::Middle => self.paste_primary(cx),
            UiButton::Right => cx.emit(TerminalEvent::ContextMenu(event.position)),
            UiButton::Navigate(_) => {}
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.mouse_reported(event.modifiers) {
            let Some(hit) = self.cell_at(event.position) else { return };
            let Some(button) = wire_button(event.button) else { return };
            self.last_reported = None;
            self.report_mouse(button, MouseAction::Release, hit, event.modifiers);
            return;
        }
        if event.button == UiButton::Left && self.selecting {
            self.selecting = false;
            // Selecting copies to the primary selection, so a middle click pastes it elsewhere.
            let text = self.session.borrow().term().lock().selection_to_string();
            if let Some(text) = text.filter(|text| !text.is_empty()) {
                cx.write_to_primary(ClipboardItem::new_string(text));
            }
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(hit) = self.cell_at(event.position) else { return };
        if self.mouse_reported(event.modifiers) {
            let mode = self.session.borrow().mode();
            let tracks = mode.contains(TermMode::MOUSE_MOTION)
                || (mode.contains(TermMode::MOUSE_DRAG) && event.pressed_button.is_some());
            if !tracks || self.last_reported == Some((hit.col, hit.row)) {
                return;
            }
            self.last_reported = Some((hit.col, hit.row));
            let button = event.pressed_button.and_then(wire_button).unwrap_or(MouseButton::None);
            self.report_mouse(button, MouseAction::Motion, hit, event.modifiers);
            return;
        }
        if self.selecting && event.pressed_button == Some(UiButton::Left) {
            let point = self.grid_point(hit);
            if let Some(selection) = self.session.borrow().term().lock().selection.as_mut() {
                selection.update(point, hit.side);
            }
            cx.notify();
        }
    }

    pub fn copy_selection(&self, cx: &mut Context<Self>) {
        let text = self.session.borrow().term().lock().selection_to_string();
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste_primary(&self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_primary().and_then(|item| item.text()) {
            self.paste_text(&text);
        }
    }
}

impl gpui::EventEmitter<TerminalEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The color that says "nothing found".
fn accent_warning(palette: &Palette) -> Hsla {
    palette.ansi[1]
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.palette = cx.global::<AppSettings>().palette.clone();
        let look = Look::from_settings(cx.global::<AppSettings>());
        let palette = self.palette.clone();
        let session = Rc::clone(&self.session);
        let metrics = Rc::clone(&self.metrics);
        let scratch = Rc::clone(&self.scratch);
        let focused = self.focus.is_focused(window);
        let search = Rc::clone(&self.search);
        // Focus was asked for where no window was at hand; apply it now.
        if let Some(target) = self.pending_focus.take() {
            let handle = match target {
                PendingFocus::Search => self.search_focus.clone(),
                PendingFocus::Terminal => self.focus.clone(),
            };
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        }
        let search_bar = self.render_search_bar(cx);

        div()
            .id("terminal")
            .size_full()
            // The grid keeps a small margin from the card's edge, so nothing is clipped by the
            // rounded corners. The canvas, and the mouse mapping, start inside it.
            .p(px(6.0))
            .rounded_xl()
            .bg(palette.background)
            .track_focus(&self.focus)
            .key_context("Terminal")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.on_key_down(event, cx);
            }))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _window, cx| {
                this.on_scroll(event, cx);
            }))
            .on_mouse_down(
                UiButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.on_mouse_down(event, window, cx);
                }),
            )
            .on_mouse_down(
                UiButton::Middle,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.on_mouse_down(event, window, cx);
                }),
            )
            .on_mouse_down(
                UiButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.on_mouse_down(event, window, cx);
                }),
            )
            .on_mouse_up(
                UiButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| this.on_mouse_up(event, cx)),
            )
            .on_mouse_up(
                UiButton::Middle,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| this.on_mouse_up(event, cx)),
            )
            .on_mouse_up(
                UiButton::Right,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| this.on_mouse_up(event, cx)),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                this.on_mouse_move(event, cx);
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        let style = PaintStyle { palette: &palette, look: &look, focused };
                        paint_grid(
                            bounds,
                            style,
                            &OwnerState {
                                session: &session,
                                metrics: &metrics,
                                scratch: &scratch,
                                search: &search,
                            },
                            window,
                            cx,
                        );
                    },
                )
                .size_full(),
            )
            .children(search_bar)
    }
}

/// The button as terminal programs know it; `None` for buttons they cannot be told about.
fn wire_button(button: UiButton) -> Option<MouseButton> {
    match button {
        UiButton::Left => Some(MouseButton::Left),
        UiButton::Middle => Some(MouseButton::Middle),
        UiButton::Right => Some(MouseButton::Right),
        UiButton::Navigate(_) => None,
    }
}

fn base_font(look: &Look) -> Font {
    Font {
        family: look.family.clone(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

/// Everything that decides how the grid looks, gathered to keep signatures short.
#[derive(Clone, Copy)]
struct PaintStyle<'a> {
    palette: &'a Palette,
    look: &'a Look,
    focused: bool,
}

/// What decides whether two neighbouring cells can share one shaped run of text.
#[derive(Clone, Copy, PartialEq)]
struct StyleKey {
    color: Hsla,
    bold: bool,
    italic: bool,
    /// An underline is present; `true` when it is wavy.
    underline: Option<bool>,
    strikethrough: bool,
}

impl StyleKey {
    fn run(self, len: usize, font: &Font) -> TextRun {
        let mut font = font.clone();
        if self.bold {
            font.weight = FontWeight::BOLD;
        }
        if self.italic {
            font.style = FontStyle::Italic;
        }
        TextRun {
            len,
            font,
            color: self.color,
            background_color: None,
            underline: self.underline.map(|wavy| UnderlineStyle {
                thickness: px(1.0),
                color: Some(self.color),
                wavy,
            }),
            strikethrough: self
                .strikethrough
                .then(|| StrikethroughStyle { thickness: px(1.0), color: Some(self.color) }),
        }
    }
}

/// One horizontal run of cells sharing the same text style.
struct Batch {
    column: usize,
    /// Width in cells (wide characters take two).
    cells: usize,
    text: String,
    style: StyleKey,
}

/// A cell copied out of the terminal while it is locked: only what painting needs, so the
/// lock (which the shell's output also needs) is held for little more than a copy.
#[derive(Clone, Copy)]
struct CellCopy {
    row: u16,
    column: u16,
    ch: char,
    fg: Color,
    bg: Color,
    flags: Flags,
    selected: bool,
}

/// Buffers reused from frame to frame, so painting does not allocate them again.
#[derive(Default)]
pub struct PaintScratch {
    cells: Vec<CellCopy>,
    rows: Vec<Vec<Batch>>,
    selections: Vec<(usize, usize, usize)>,
    backgrounds: Vec<(usize, usize, usize, Hsla)>,
    /// Search matches on screen as (row, first column, end column, is the current one).
    matches: Vec<(usize, usize, usize, bool)>,
}

/// What painting shares with the view that owns it.
struct OwnerState<'a> {
    session: &'a Rc<std::cell::RefCell<Session>>,
    metrics: &'a Rc<Cell<Metrics>>,
    scratch: &'a Rc<std::cell::RefCell<PaintScratch>>,
    search: &'a Rc<std::cell::RefCell<Option<Search>>>,
}

/// The screen rows a search match covers (it may wrap over several), as spans of columns.
fn push_match_spans(
    found: &mer_term::alacritty_terminal::term::search::Match,
    offset: i32,
    (lines, columns): (usize, usize),
    current: bool,
    out: &mut Vec<(usize, usize, usize, bool)>,
) {
    let (start, end) = (found.start(), found.end());
    for line in start.line.0..=end.line.0 {
        let Ok(row) = usize::try_from(line + offset) else { continue };
        if row >= lines {
            continue;
        }
        let first = if line == start.line.0 { start.column.0 } else { 0 };
        let last = if line == end.line.0 { end.column.0 + 1 } else { columns };
        out.push((row, first, last, current));
    }
}

/// What the cursor looks like, read while the terminal is locked.
struct CursorCopy {
    shape: CursorShape,
    character: char,
    row: i32,
    column: usize,
}

/// Copies the visible cells and the cursor out of the terminal.
fn copy_cells(
    session: &Session,
    scratch: &mut PaintScratch,
    lines: usize,
    search: Option<&mut Search>,
) -> CursorCopy {
    let term = session.term().lock();
    let content = term.renderable_content();
    let display_offset = i32::try_from(content.display_offset).unwrap_or(0);
    let selection = content.selection;
    let cursor = content.cursor;
    scratch.cells.clear();
    for indexed in content.display_iter {
        let cell = indexed.cell;
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let Ok(row) = usize::try_from(indexed.point.line.0 + display_offset) else { continue };
        if row >= lines {
            continue;
        }
        let (Ok(row), Ok(column)) = (u16::try_from(row), u16::try_from(indexed.point.column.0))
        else {
            continue;
        };
        scratch.cells.push(CellCopy {
            row,
            column,
            ch: cell.c,
            fg: cell.fg,
            bg: cell.bg,
            flags: cell.flags,
            selected: selection.is_some_and(|range| range.contains(indexed.point)),
        });
    }
    // Highlights for an open search: only the lines on screen are looked at.
    scratch.matches.clear();
    if let Some(search) = search {
        let current = search.current().cloned();
        let columns = term.columns();
        for found in search.visible(&term) {
            let is_current = current.as_ref() == Some(&found);
            push_match_spans(
                &found,
                display_offset,
                (lines, columns),
                is_current,
                &mut scratch.matches,
            );
        }
    }
    CursorCopy {
        shape: cursor.shape,
        character: term.grid()[cursor.point].c,
        row: cursor.point.line.0 + display_offset,
        column: cursor.point.column.0,
    }
}

/// Turns the copied cells into rows of same-styled runs, plus background and selection spans.
fn build_runs(scratch: &mut PaintScratch, palette: &Palette, lines: usize) {
    let PaintScratch { cells, rows, selections, backgrounds, matches: _ } = scratch;
    rows.truncate(lines);
    rows.resize_with(lines, Vec::new);
    rows.iter_mut().for_each(Vec::clear);
    selections.clear();
    backgrounds.clear();
    let mut last_spec = None;

    for cell in cells.iter() {
        let (row, column) = (usize::from(cell.row), usize::from(cell.column));
        let width = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };

        if cell.selected {
            match selections.last_mut() {
                Some((last_row, _, end)) if *last_row == row && *end == column => *end += width,
                _ => selections.push((row, column, column + width)),
            }
        }

        let mut fg = resolve_cached(palette, cell.fg, &mut last_spec);
        let mut bg = resolve_cached(palette, cell.bg, &mut last_spec);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.contains(Flags::DIM) {
            fg.a *= 0.66;
        }
        let hidden = cell.flags.contains(Flags::HIDDEN);
        if hidden {
            fg = bg;
        }

        if bg != palette.background {
            match backgrounds.last_mut() {
                Some((last_row, _, end, color))
                    if *last_row == row && *end == column && *color == bg =>
                {
                    *end += 1;
                }
                _ => backgrounds.push((row, column, column + 1, bg)),
            }
        }

        let ch = if hidden || cell.ch == '\0' { ' ' } else { cell.ch };
        let style = StyleKey {
            color: fg,
            bold: cell.flags.contains(Flags::BOLD),
            italic: cell.flags.contains(Flags::ITALIC),
            underline: cell
                .flags
                .intersects(Flags::ALL_UNDERLINES)
                .then(|| cell.flags.contains(Flags::UNDERCURL)),
            strikethrough: cell.flags.contains(Flags::STRIKEOUT),
        };

        let batches = &mut rows[row];
        match batches.last_mut() {
            Some(batch) if batch.style == style && batch.column + batch.cells == column => {
                batch.text.push(ch);
                batch.cells += width;
            }
            _ => batches.push(Batch { column, cells: width, text: ch.to_string(), style }),
        }
    }
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn paint_grid(
    bounds: gpui::Bounds<Pixels>,
    style: PaintStyle,
    owner: &OwnerState,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let OwnerState { session, metrics, scratch, search } = owner;
    let PaintStyle { palette, look, focused } = style;
    let font_size = px(look.font_size);
    let font = base_font(look);
    let font_id = window.text_system().resolve_font(&font);
    let cell_width = window
        .text_system()
        .advance(font_id, font_size, 'm')
        .map_or(px(8.0), |advance| advance.width);
    let line_height = look.cell_line_height();
    metrics.set(Metrics { line_height, cell_width, origin: bounds.origin });

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let new_size = GridSize {
        cols: ((bounds.size.width / cell_width).floor() as u16).max(2),
        lines: ((bounds.size.height / line_height).floor() as u16).max(1),
        cell_width: f32::from(cell_width).round() as u16,
        cell_height: f32::from(line_height) as u16,
    };
    session.borrow_mut().resize(new_size);

    let lines = usize::from(new_size.lines);
    let mut scratch = scratch.borrow_mut();
    let cursor = copy_cells(&session.borrow(), &mut scratch, lines, search.borrow_mut().as_mut());
    build_runs(&mut scratch, palette, lines);

    let origin = bounds.origin;
    let cell_origin = |row: usize, column: usize| {
        point(origin.x + cell_width * column as f32, origin.y + line_height * row as f32)
    };

    for (row, start, end, color) in &scratch.backgrounds {
        let top_left = cell_origin(*row, *start);
        let rect =
            gpui::Bounds::new(top_left, size(cell_width * (end - start) as f32, line_height));
        window.paint_quad(fill(rect, *color));
    }

    // Search matches: yellow, and the one the user jumped to in the accent color.
    for (row, start, end, current) in &scratch.matches {
        let color = if *current {
            with_alpha(palette.chrome.accent, 0.60)
        } else {
            with_alpha(accent_color(mer_core::Accent::Amber), 0.45)
        };
        let rect = gpui::Bounds::new(
            cell_origin(*row, *start),
            size(cell_width * (end - start) as f32, line_height),
        );
        window.paint_quad(fill(rect, color));
    }

    // Selection highlight: translucent, so text colors stay readable underneath.
    let mut highlight = palette.chrome.accent;
    highlight.a = 0.35;
    for (row, start, end) in &scratch.selections {
        let rect = gpui::Bounds::new(
            cell_origin(*row, *start),
            size(cell_width * (end - start) as f32, line_height),
        );
        window.paint_quad(fill(rect, highlight));
    }

    for (row, batches) in scratch.rows.iter_mut().enumerate() {
        for batch in batches {
            if batch.text.chars().all(|c| c == ' ')
                && batch.style.underline.is_none()
                && !batch.style.strikethrough
            {
                continue;
            }
            let run = batch.style.run(batch.text.len(), &font);
            // The text is moved into the shaped line, not copied.
            let text = std::mem::take(&mut batch.text);
            let shaped = window.text_system().shape_line(
                text.into(),
                font_size,
                std::slice::from_ref(&run),
                Some(cell_width),
            );
            let _ = shaped.paint(
                cell_origin(row, batch.column),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }

    if cursor.shape != CursorShape::Hidden
        && let Ok(row) = usize::try_from(cursor.row)
        && row < lines
    {
        paint_cursor(
            cursor.shape,
            cell_origin(row, cursor.column),
            (cell_width, line_height),
            cursor.character,
            (palette, look, focused),
            window,
            cx,
        );
    }
}

fn paint_cursor(
    shape: CursorShape,
    origin: gpui::Point<Pixels>,
    (cell_width, line_height): (Pixels, Pixels),
    character: char,
    (palette, look, focused): (&Palette, &Look, bool),
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let color = palette.cursor;
    let font_size = px(look.font_size);
    if !focused {
        let rect = gpui::Bounds::new(origin, size(cell_width, line_height));
        window.paint_quad(gpui::outline(rect, color, gpui::BorderStyle::Solid));
        return;
    }
    match shape {
        CursorShape::Underline => {
            let rect = gpui::Bounds::new(
                point(origin.x, origin.y + line_height - px(2.0)),
                size(cell_width, px(2.0)),
            );
            window.paint_quad(fill(rect, color));
        }
        CursorShape::Beam => {
            let rect = gpui::Bounds::new(origin, size(px(2.0), line_height));
            window.paint_quad(fill(rect, color));
        }
        CursorShape::HollowBlock => {
            let rect = gpui::Bounds::new(origin, size(cell_width, line_height));
            window.paint_quad(gpui::outline(rect, color, gpui::BorderStyle::Solid));
        }
        CursorShape::Block | CursorShape::Hidden => {
            let rect = gpui::Bounds::new(origin, size(cell_width, line_height));
            window.paint_quad(fill(rect, color));
            if character != ' ' && character != '\0' {
                let run = TextRun {
                    len: character.len_utf8(),
                    font: base_font(look),
                    color: palette.background,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = window.text_system().shape_line(
                    character.to_string().into(),
                    font_size,
                    &[run],
                    Some(cell_width),
                );
                let _ = shaped.paint(origin, line_height, TextAlign::Left, None, window, cx);
            }
        }
    }
}
