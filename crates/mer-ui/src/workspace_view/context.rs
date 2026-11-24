//! The chips under each terminal (git branch, node version) and the picker they open.
//!
//! The shell reports its folder and node version at every prompt (a private title, see
//! `mer_agents::context_from_title`); the branch is a read of `.git/HEAD`. Nothing here polls:
//! chips change when a prompt arrives, and the lists in the picker are read only when it opens.
//! Picking an item types a command into the shell, but only if the shell is idle at its prompt
//! and no other program owns the terminal: mer never sends keys to a running program.

use std::path::{Path, PathBuf};

use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Hsla, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, ParentElement, Pixels, Point, SharedString,
    StatefulInteractiveElement, Styled, anchored, div, prelude::FluentBuilder as _,
};
use mer_agents::ShellReport;
use mer_core::{Accent, NodeFit, NodeNeed, PaneContext, PaneId, filter_items};

use super::{Focus, WorkspaceView};
use crate::settings::AppSettings;
use crate::theme::accent_color;

/// Rows the picker shows at once; the list scrolls to keep the selected row in view.
const PICKER_ROWS: usize = 8;

/// What a picker chooses between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PickerKind {
    Branch,
    Node,
}

impl PickerKind {
    fn placeholder(self) -> &'static str {
        match self {
            Self::Branch => "Search branches…",
            Self::Node => "Search node versions…",
        }
    }

    fn nothing(self) -> &'static str {
        match self {
            Self::Branch => "No branches",
            Self::Node => "No installed versions",
        }
    }
}

/// An open picker. It only exists while open.
pub(super) struct Picker {
    kind: PickerKind,
    pane: PaneId,
    /// Window position of the chip it opened from; the list opens above it.
    position: Point<Pixels>,
    query: String,
    /// `None` while the list is being read.
    items: Option<Vec<String>>,
    selected: usize,
    /// The branch or version in use now.
    current: Option<String>,
    /// For the node picker: the installed version the project asks for, and the file that says so.
    required: Option<(String, String)>,
    /// A line under the list about what the project asks for, and whether it is a problem.
    note: Option<(String, bool)>,
}

impl Picker {
    /// The terminal it was opened from.
    pub(super) fn pane(&self) -> PaneId {
        self.pane
    }

    fn visible(&self) -> Vec<&str> {
        self.items.as_deref().map(|items| filter_items(items, &self.query)).unwrap_or_default()
    }
}

impl WorkspaceView {
    fn chips_enabled(cx: &Context<Self>) -> bool {
        cx.global::<AppSettings>().config.terminal.context_chips
    }

    fn hint_enabled(cx: &Context<Self>) -> bool {
        cx.global::<AppSettings>().config.terminal.node_version_hint
    }

    /// Applies a change to a pane's context and redraws if it changed anything.
    fn update_context(
        &mut self,
        id: PaneId,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut PaneContext),
    ) {
        let Some(mut context) = self.model.pane(id).map(|pane| pane.context.clone()) else {
            return;
        };
        change(&mut context);
        if self.model.set_pane_context(id, context) {
            cx.notify();
        }
    }

    /// The shell is about to run a command: it is no longer at its prompt.
    pub(super) fn context_busy(&mut self, id: PaneId, cx: &mut Context<Self>) {
        self.update_context(id, cx, |context| context.at_prompt = false);
    }

    /// The shell reached a prompt and reported where it is.
    pub(super) fn on_prompt_report(
        &mut self,
        id: PaneId,
        report: ShellReport,
        cx: &mut Context<Self>,
    ) {
        // A prompt means the shell is free: whatever it announced before has ended, even a command
        // so quick that its process was never seen (`clear`). This is what keeps a finished command
        // out of the title and its duration out of the long-command alert. Not if some program
        // (an inner shell, say) still owns the terminal.
        let free = self
            .live
            .get(&id)
            .is_some_and(|live| mer_agents::foreground(live.view.read(cx).pid()).is_none());
        if free && self.set_program(id, None, cx) {
            cx.notify();
        }
        if !Self::chips_enabled(cx) {
            return;
        }
        let cwd = report.cwd.clone();
        self.update_context(id, cx, |context| {
            // A different folder may be a different repository: forget the branch until read.
            if context.cwd != report.cwd {
                context.branch = None;
            }
            context.at_prompt = true;
            context.cwd = report.cwd;
            context.node = report.node;
            context.nvm_dir = report.nvm_dir;
        });
        let Some(cwd) = cwd else { return };
        // `.git/HEAD` is tiny, but it is still the disk: read it off the interface thread.
        cx.spawn(async move |this, cx| {
            let lookup = cwd.clone();
            let head = cx
                .background_executor()
                .spawn(async move { mer_agents::current_head(&lookup) })
                .await;
            let branch = head.map(|head| head.label().to_owned());
            let _ = this.update(cx, |this, cx| this.apply_branch(id, &cwd, branch, cx));
        })
        .detach();
    }

    /// Stores a branch read for `cwd`, unless the pane has moved on since.
    fn apply_branch(
        &mut self,
        id: PaneId,
        cwd: &PathBuf,
        branch: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.update_context(id, cx, |context| {
            if context.cwd.as_ref() == Some(cwd) {
                context.branch = branch;
            }
        });
        self.lookup_node_need(id, cx);
    }

    /// Reads what the project asks for (`.nvmrc`), lazily: only the first time this folder and
    /// branch are seen with a node version active, and never at startup (it follows the first
    /// prompt). Off the interface thread, and it never runs `nvm` or `node`.
    fn lookup_node_need(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if !Self::hint_enabled(cx) {
            return;
        }
        let Some(context) = self.model.pane(id).map(|pane| pane.context.clone()) else { return };
        if context.node.is_none() || !context.needs_node_lookup() {
            return;
        }
        let Some(cwd) = context.cwd.clone() else { return };
        let key = (cwd.clone(), context.branch.clone());
        let nvm_dir = context.nvm_dir.clone();
        // Marked before reading, so a burst of prompts reads once.
        self.update_context(id, cx, |context| context.need_read_for = Some(key.clone()));
        cx.spawn(async move |this, cx| {
            let need = cx
                .background_executor()
                .spawn(async move {
                    let installed = mer_agents::nvm_dir(nvm_dir.as_deref())
                        .map(|dir| mer_agents::installed_versions(&dir))
                        .unwrap_or_default();
                    read_node_need(&cwd, &installed)
                })
                .await;
            let _ = this.update(cx, |this, cx| this.apply_node_need(id, &key, need, cx));
        })
        .detach();
    }

    /// Stores what the project asks for, unless the pane has moved on since.
    fn apply_node_need(
        &mut self,
        id: PaneId,
        key: &(PathBuf, Option<String>),
        need: Option<NodeNeed>,
        cx: &mut Context<Self>,
    ) {
        self.update_context(id, cx, |context| {
            if context.need_read_for.as_ref() == Some(key) {
                context.required_node = need;
            }
        });
    }

    /// Is the terminal free for mer to type into right now? Reads `/proc` fresh: this is the
    /// guard that keeps mer from sending keys to an agent, an editor or a dev server.
    fn pane_is_idle(&self, id: PaneId, cx: &Context<Self>) -> bool {
        let Some(live) = self.live.get(&id) else { return false };
        let busy = mer_agents::foreground(live.view.read(cx).pid()).is_some();
        self.model.pane(id).is_some_and(|pane| pane.context.accepts_commands(busy))
    }

    pub(super) fn open_picker(
        &mut self,
        kind: PickerKind,
        id: PaneId,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.pane_is_idle(id, cx) {
            return;
        }
        let Some(context) = self.model.pane(id).map(|pane| pane.context.clone()) else { return };
        let current = match kind {
            PickerKind::Branch => context.branch.clone(),
            PickerKind::Node => context.node.clone(),
        };
        self.picker = Some(Picker {
            kind,
            pane: id,
            position,
            query: String::new(),
            items: None,
            selected: 0,
            current,
            required: None,
            note: None,
        });
        self.want_focus = Some(Focus::Picker);
        cx.notify();

        // The lists are read now, once, away from the interface thread.
        let hint = Self::hint_enabled(cx);
        cx.spawn(async move |this, cx| {
            let (items, need) = cx
                .background_executor()
                .spawn(async move {
                    match kind {
                        PickerKind::Branch => (
                            context
                                .cwd
                                .map(|cwd| mer_agents::list_branches(&cwd))
                                .unwrap_or_default(),
                            None,
                        ),
                        PickerKind::Node => {
                            let items = mer_agents::nvm_dir(context.nvm_dir.as_deref())
                                .map(|dir| mer_agents::installed_versions(&dir))
                                .unwrap_or_default();
                            let need = context
                                .cwd
                                .as_deref()
                                .filter(|_| hint)
                                .and_then(|cwd| read_node_need(cwd, &items));
                            (items, need)
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| this.picker_loaded(kind, id, items, need, cx));
        })
        .detach();
    }

    fn picker_loaded(
        &mut self,
        kind: PickerKind,
        id: PaneId,
        items: Vec<String>,
        need: Option<NodeNeed>,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.picker.as_mut().filter(|p| p.kind == kind && p.pane == id) else {
            return;
        };
        picker.required =
            need.as_ref().and_then(|need| Some((need.version.clone()?, need.file.clone())));
        picker.note = need.as_ref().and_then(|need| match (&need.version, need.alias) {
            (_, true) => Some((format!("{} says {}", need.file, need.spec), false)),
            (None, false) => Some((
                format!(
                    "{} asks for {}, which is not installed (nvm install {})",
                    need.file, need.spec, need.spec
                ),
                true,
            )),
            (Some(_), false) => None,
        });
        // Start on the version the project asks for when it is not the active one (Enter then
        // switches to it); otherwise on the item in use, so Enter changes nothing by accident.
        let wanted = picker
            .required
            .as_ref()
            .map(|(version, _)| version)
            .filter(|version| picker.current.as_ref() != Some(*version))
            .or(picker.current.as_ref());
        picker.selected =
            wanted.and_then(|wanted| items.iter().position(|item| item == wanted)).unwrap_or(0);
        picker.items = Some(items);
        if kind == PickerKind::Node && Self::hint_enabled(cx) {
            // The picker just read it: keep the chip in step.
            self.update_context(id, cx, |context| context.required_node = need);
        }
        cx.notify();
    }

    pub(super) fn close_picker(&mut self, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
            self.focus_active(cx);
            cx.notify();
        }
    }

    fn accept_picker(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.take() else { return };
        self.focus_active(cx);
        cx.notify();
        let Some(choice) = picker.visible().get(picker.selected).map(|item| (*item).to_owned())
        else {
            return;
        };
        if picker.current.as_deref() == Some(choice.as_str()) {
            return;
        }
        // Checked again: the shell may have started something while the list was open.
        if !self.pane_is_idle(picker.pane, cx) {
            return;
        }
        let input = match picker.kind {
            PickerKind::Branch => mer_agents::switch_input(&choice),
            PickerKind::Node => mer_agents::use_input(&choice),
        };
        if let (Some(input), Some(live)) = (input, self.live.get(&picker.pane)) {
            live.view.read(cx).write_text(&input);
        }
    }

    fn on_picker_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let Some(picker) = self.picker.as_mut() else { return };
        match keystroke.key.as_str() {
            "escape" => return self.close_picker(cx),
            "enter" => return self.accept_picker(cx),
            "backspace" => {
                picker.query.pop();
                picker.selected = 0;
            }
            "up" => picker.selected = picker.selected.saturating_sub(1),
            "down" => {
                let last = picker.visible().len().saturating_sub(1);
                picker.selected = (picker.selected + 1).min(last);
            }
            _ => {
                let modifiers = keystroke.modifiers;
                if modifiers.control || modifiers.alt || modifiers.platform {
                    return;
                }
                if let Some(text) = keystroke.key_char.as_deref() {
                    picker.query.push_str(text);
                    picker.selected = 0;
                }
            }
        }
        cx.notify();
    }

    /// The node chip: the active version, marked by how it compares with what the project asks for.
    fn node_chip(&self, context: &PaneContext, node: &str, hint: bool) -> Chip {
        let green = self.palette.ansi[2];
        let (amber, red) = (accent_color(Accent::Amber), accent_color(Accent::Red));
        let need = context.required_node.as_ref().filter(|_| hint);
        let (glyph_color, label, label_color, notes) = match (context.node_fit(), need) {
            (NodeFit::Matches, Some(need)) => {
                (green, format!("{node} ✓"), None, vec![format!("Matches {}", need.file)])
            }
            (NodeFit::Differs, Some(need)) => {
                let wanted = need.version.clone().unwrap_or_default();
                let asks = format!("{} asks for {} ({wanted})", need.file, need.spec);
                (amber, format!("{node} → {wanted}"), Some(amber), vec![asks])
            }
            (NodeFit::NotInstalled, Some(need)) => {
                let asks = format!("{} asks for {}, which is not installed", need.file, need.spec);
                let hint = format!("Run: nvm install {}", need.spec);
                (red, format!("{node} → {} ✗", need.spec), Some(red), vec![asks, hint])
            }
            (_, Some(need)) if need.alias => {
                (green, node.to_owned(), None, vec![format!("{} says {}", need.file, need.spec)])
            }
            _ => (green, node.to_owned(), None, Vec::new()),
        };
        Chip { kind: PickerKind::Node, glyph: "⬢", glyph_color, label, label_color, notes }
    }

    /// The strip under a terminal with its chips. `None` when there is nothing to show.
    pub(super) fn render_context_strip(
        &self,
        id: PaneId,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !Self::chips_enabled(cx) {
            return None;
        }
        let pane = self.model.pane(id)?;
        let context = &pane.context;
        if !context.has_chips() {
            return None;
        }
        // Cheap and without `/proc`: a program is known to run, or the shell is mid-command.
        let busy = pane.program.is_some() || !context.at_prompt;
        let hint = Self::hint_enabled(cx);
        let chips: Vec<Chip> = [
            context.node.as_deref().map(|node| self.node_chip(context, node, hint)),
            context.branch.as_ref().map(|branch| Chip {
                kind: PickerKind::Branch,
                glyph: "⎇",
                glyph_color: self.palette.chrome.accent,
                label: branch.clone(),
                label_color: None,
                notes: Vec::new(),
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        let hover = self.palette.hover();
        let (text, dim) = (self.palette.chrome.text, self.palette.chrome.text_dim);

        Some(
            div()
                .flex_none()
                .h(self.ui_px(28.0))
                .px_2()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .border_t_1()
                .border_color(self.palette.hairline())
                .children(chips.into_iter().map(|chip| {
                    let kind = chip.kind;
                    let title = match kind {
                        PickerKind::Branch => "Switch git branch",
                        PickerKind::Node => "Switch node version",
                    };
                    let mut details = chip.notes;
                    if busy {
                        details
                            .push("Available when the shell is waiting at its prompt".to_owned());
                    }
                    div()
                        .id(match kind {
                            PickerKind::Branch => "chip-branch",
                            PickerKind::Node => "chip-node",
                        })
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .h(self.ui_px(20.0))
                        .px_2()
                        .rounded_full()
                        .bg(hover)
                        .text_xs()
                        .text_color(dim)
                        .when(busy, |chip| chip.opacity(0.5))
                        .when(!busy, |chip| {
                            chip.cursor_pointer().hover(move |chip| chip.text_color(text))
                        })
                        .tooltip(crate::tooltip::tip(&self.palette, title, details))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
                            this.open_picker(kind, id, event.position(), cx);
                        }))
                        .child(div().text_color(chip.glyph_color).child(chip.glyph))
                        .child(
                            div()
                                .when_some(chip.label_color, gpui::Styled::text_color)
                                .child(SharedString::from(chip.label)),
                        )
                }))
                .into_any_element(),
        )
    }

    /// The open picker: a search box over a short list, above the chip that opened it.
    #[allow(clippy::too_many_lines)] // one builder chain
    pub(super) fn render_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.picker.as_ref()?;
        let chrome = &self.palette.chrome;
        let (surface, text, dim, accent) =
            (chrome.surface, chrome.text, chrome.text_dim, chrome.accent);
        let border = self.palette.hairline();

        let shown = if picker.query.is_empty() {
            div().text_color(dim).child(picker.kind.placeholder())
        } else {
            div().text_color(text).child(SharedString::from(format!("{}▏", picker.query)))
        };
        let input = div()
            .w_full()
            .h(self.ui_px(32.0))
            .px_3()
            .flex()
            .items_center()
            .rounded_lg()
            .bg(chrome.surface)
            .border_1()
            .border_color(accent)
            .overflow_hidden()
            .child(shown.min_w_0());

        let visible = picker.visible();
        let first = (picker.selected + 1).saturating_sub(PICKER_ROWS);
        let rows: Vec<AnyElement> = if picker.items.is_none() {
            vec![message_row(self, "Loading…")]
        } else if visible.is_empty() {
            vec![message_row(self, picker.kind.nothing())]
        } else {
            visible
                .iter()
                .enumerate()
                .skip(first)
                .take(PICKER_ROWS)
                .map(|(index, item)| {
                    let selected = index == picker.selected;
                    let current = picker.current.as_deref() == Some(*item);
                    let required = picker
                        .required
                        .as_ref()
                        .filter(|(version, _)| version == *item)
                        .map(|(_, file)| file.clone());
                    div()
                        .id(("picker-row", index))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .h(self.ui_px(30.0))
                        .px_3()
                        .rounded_lg()
                        .cursor_pointer()
                        .text_color(if selected { text } else { dim })
                        .when(selected, |row| row.bg(surface))
                        .hover(move |row| row.bg(surface).text_color(text))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                            if let Some(picker) = this.picker.as_mut() {
                                picker.selected = index;
                            }
                            this.accept_picker(cx);
                        }))
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .child(SharedString::from((*item).to_owned())),
                        )
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .children(required.map(|file| {
                                    div()
                                        .text_xs()
                                        .text_color(accent_color(Accent::Amber))
                                        .child(SharedString::from(file))
                                }))
                                .children(current.then(|| div().text_color(accent).child("✓"))),
                        )
                        .into_any_element()
                })
                .collect()
        };

        let panel = div()
            .track_focus(&self.picker_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.on_picker_key(event, cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(self.ui_px(300.0))
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .bg(chrome.bar)
            .border_1()
            .border_color(border)
            .rounded_xl()
            .shadow(crate::theme::overlay_shadow(self.palette.is_dark()))
            .child(input)
            .children(rows)
            .children(picker.note.as_ref().map(|(note, problem)| {
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(if *problem { accent_color(Accent::Amber) } else { dim })
                    .child(SharedString::from(note.clone()))
            }));

        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _window, cx| this.close_picker(cx)),
                )
                .child(
                    anchored()
                        .anchor(Anchor::BottomLeft)
                        .position(picker.position)
                        .snap_to_window()
                        .child(panel),
                )
                .into_any_element(),
        )
    }
}

/// One chip of the strip, ready to draw.
struct Chip {
    kind: PickerKind,
    glyph: &'static str,
    glyph_color: Hsla,
    label: String,
    /// Colors the label when something is worth noticing (the version differs).
    label_color: Option<Hsla>,
    /// Lines for the tooltip, above the generic hint.
    notes: Vec<String>,
}

/// What the project at `cwd` asks for, looked up against the installed versions. Reads a few
/// small files: call it off the interface thread.
fn read_node_need(cwd: &Path, installed: &[String]) -> Option<NodeNeed> {
    let found = mer_agents::required_version(cwd)?;
    Some(NodeNeed {
        alias: mer_agents::is_alias(&found.spec),
        version: mer_agents::resolve(&found.spec, installed),
        file: found.file.to_owned(),
        spec: found.spec,
    })
}

fn message_row(view: &WorkspaceView, message: &'static str) -> AnyElement {
    div()
        .h(view.ui_px(30.0))
        .px_3()
        .flex()
        .items_center()
        .text_color(view.palette.chrome.text_dim)
        .child(message)
        .into_any_element()
}
