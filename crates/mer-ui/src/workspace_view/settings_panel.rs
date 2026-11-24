//! The settings panel: a full-window overlay that edits the same `Config` as `config.toml`.
//! Every change applies immediately and is written back to the file (keeping its comments).

#![allow(clippy::unused_self)] // the builders are methods so call sites read uniformly

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, BorrowAppContext as _, ClickEvent, Context, Div, Hsla, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder as _, px, relative,
};
use mer_config::{Config, SidebarSide};

mod performance;

use super::WorkspaceView;
use crate::actions::ACTION_NAMES;
use crate::settings::{self, AppSettings, Zoom, next_ui_scale};
use crate::shell::installed_shells;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Section {
    #[default]
    Appearance,
    Terminal,
    Spaces,
    Agents,
    Performance,
    Shortcuts,
}

impl Section {
    const ALL: [Self; 6] = [
        Self::Appearance,
        Self::Terminal,
        Self::Spaces,
        Self::Agents,
        Self::Performance,
        Self::Shortcuts,
    ];

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.label().eq_ignore_ascii_case(name))
    }

    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Terminal => "Terminal",
            Self::Spaces => "Spaces",
            Self::Agents => "Agents",
            Self::Performance => "Performance",
            Self::Shortcuts => "Shortcuts",
        }
    }
}

/// Which agent's settings a hook installation would change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HookTarget {
    Claude,
    Codex,
    Shell,
}

/// A hook installation, computed first so the user can see what will change.
#[derive(Debug, Clone)]
pub(super) struct HookPlan {
    target: HookTarget,
    path: PathBuf,
    new_text: String,
    changed: bool,
    summary: Vec<String>,
}

#[derive(Debug, Default)]
pub(super) struct SettingsState {
    pub open: bool,
    section: Section,
    plan: Option<HookPlan>,
    message: Option<String>,
    /// Whether the KDE window rule that remembers the position is installed. Read when the
    /// panel opens, not on every frame.
    pub remember_position: bool,
}

/// An edit applied to the config by a button.
type ConfigEdit = fn(&mut Config);

const SLEEP_STEPS: [u32; 8] = [0, 5, 10, 15, 30, 60, 120, 240];

/// Volumes the sound stepper walks through, in percent.
const VOLUME_STEPS: [u32; 11] = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];

fn volume_step(config: &mut Config, forward: bool) {
    config.agents.sound_volume = step_through(&VOLUME_STEPS, config.agents.sound_volume, forward);
}

/// Seconds a command must run before its end raises an alert (0 turns it off).
const LONG_COMMAND_STEPS: [u32; 6] = [0, 10, 30, 60, 120, 300];

/// The step after (or before) `current` in an ascending list. A hand-edited value in between
/// snaps to the next step forward, or the one below it backward.
fn step_through(steps: &[u32], current: u32, forward: bool) -> u32 {
    let last = steps.len() - 1;
    // Past the end of the list, the way back is its last step.
    let Some(index) = steps.iter().position(|step| *step >= current) else { return steps[last] };
    let next = if forward {
        (index + usize::from(steps[index] <= current)).min(last)
    } else {
        index.saturating_sub(1)
    };
    steps[next]
}

fn sleep_step(config: &mut Config, forward: bool) {
    config.spaces.auto_sleep_minutes =
        step_through(&SLEEP_STEPS, config.spaces.auto_sleep_minutes, forward);
}

fn long_command_step(config: &mut Config, forward: bool) {
    config.agents.long_command_seconds =
        step_through(&LONG_COMMAND_STEPS, config.agents.long_command_seconds, forward);
}

/// `Off`, `30 s`, `2 min`.
fn long_command_label(seconds: u32) -> String {
    match seconds {
        0 => "Off".to_owned(),
        s if s >= 60 && s % 60 == 0 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

fn round_hundredths(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

/// Colors used by every control.
#[derive(Clone, Copy)]
struct Ui {
    surface: Hsla,
    text: Hsla,
    dim: Hsla,
    accent: Hsla,
    base: Hsla,
    /// Soft line for cards and separators.
    hairline: Hsla,
    /// Interface scale, for the lengths in pixels.
    scale: f32,
    /// The window is narrow: rows stack their control under the label.
    narrow: bool,
}

impl Ui {
    fn px(self, value: f32) -> gpui::Pixels {
        px(value * self.scale)
    }
}

fn small_button(ui: Ui, id: (&'static str, usize), label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(ui.px(28.0))
        .text_color(ui.text)
        .cursor_pointer()
        .hover(move |button| button.bg(ui.surface))
        .child(label)
}

fn pill(ui: Ui, id: (&'static str, usize), label: String, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(ui.px(28.0))
        .px_3()
        .rounded_lg()
        .cursor_pointer()
        .bg(if selected { ui.accent } else { ui.surface })
        .text_color(if selected { ui.base } else { ui.text })
        .child(SharedString::from(label))
}

fn setting_row(ui: Ui, label: &str, hint: &str, control: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_4()
        .py_3()
        .border_b_1()
        .border_color(ui.hairline)
        // In a narrow window the control goes under its label instead of squeezing it.
        .when(ui.narrow, |row| row.flex_col().items_start().gap_2())
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap_1()
                .when(ui.narrow, Styled::w_full)
                .when(!ui.narrow, Styled::flex_1)
                .child(div().text_color(ui.text).child(SharedString::from(label.to_owned())))
                .child(
                    div().text_xs().text_color(ui.dim).child(SharedString::from(hint.to_owned())),
                ),
        )
        .child(control)
        .into_any_element()
}

fn read_only(ui: Ui, value: &str) -> AnyElement {
    div().text_color(ui.dim).child(SharedString::from(value.to_owned())).into_any_element()
}

impl WorkspaceView {
    /// Applies an edit to the config: clamps it, shares it with every view and saves it.
    pub(super) fn update_config(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Config)) {
        let mut config = cx.global::<AppSettings>().config.clone();
        edit(&mut config);
        cx.update_global::<AppSettings, _>(|settings, _| settings.set_config(config));
        settings::save_in_background(cx.global::<AppSettings>().config.clone());
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings.open = false;
        self.settings.plan = None;
        self.settings.message = None;
        self.focus_active(cx);
        cx.notify();
    }

    fn on_settings_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.settings.plan.take().is_some() {
                cx.notify();
            } else {
                self.close_settings(cx);
            }
        }
    }

    fn toggle(
        &self,
        ui: Ui,
        name: &'static str,
        on: bool,
        flip: ConfigEdit,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.switch(ui, name, on, move |this, cx| this.update_config(cx, flip), cx)
    }

    /// A pill switch that runs any action when clicked.
    fn switch(
        &self,
        ui: Ui,
        name: &'static str,
        on: bool,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id((name, 0_usize))
            .w(ui.px(42.0))
            .h(ui.px(24.0))
            .p(ui.px(3.0))
            .flex()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .bg(if on { ui.accent } else { ui.surface })
            .when(on, Styled::justify_end)
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| action(this, cx)))
            .child(div().size(ui.px(18.0)).rounded_full().bg(if on { ui.base } else { ui.dim }))
            .into_any_element()
    }

    /// A segmented control: minus, the value, plus.
    fn stepper(
        &self,
        ui: Ui,
        name: &'static str,
        value: String,
        (minus, plus): (ConfigEdit, ConfigEdit),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(ui.px(28.0))
            .rounded_lg()
            .border_1()
            .border_color(ui.hairline)
            .overflow_hidden()
            .child(small_button(ui, (name, 0), "−").on_click(
                cx.listener(move |this, _: &ClickEvent, _window, cx| this.update_config(cx, minus)),
            ))
            .child(
                div()
                    .min_w(ui.px(64.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_x_1()
                    .border_color(ui.hairline)
                    .text_color(ui.text)
                    .child(SharedString::from(value)),
            )
            .child(small_button(ui, (name, 1), "+").on_click(
                cx.listener(move |this, _: &ClickEvent, _window, cx| this.update_config(cx, plus)),
            ))
            .into_any_element()
    }

    fn render_appearance(
        &self,
        ui: Ui,
        config: &Config,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let themes: Vec<String> =
            cx.global::<AppSettings>().themes.iter().map(|theme| theme.name.clone()).collect();
        let active = cx.global::<AppSettings>().theme_name().to_owned();
        let theme_pills = themes.into_iter().enumerate().map(|(index, name)| {
            let selected = name == active;
            pill(ui, ("theme", index), name.clone(), selected).on_click(cx.listener(
                move |this, _: &ClickEvent, _window, cx| {
                    let name = name.clone();
                    this.update_config(cx, move |config| config.appearance.theme = name);
                },
            ))
        });
        vec![
            setting_row(
                ui,
                "Theme",
                "Add your own in ~/.config/mer/themes/*.toml",
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .max_w(px(380.0))
                    .children(theme_pills)
                    .into_any_element(),
            ),
            setting_row(
                ui,
                "Interface size",
                "Sidebar, tabs, menus and dialogs, not the terminal text. Ctrl+Alt+= and Ctrl+Alt+-",
                self.stepper(
                    ui,
                    "ui-scale",
                    format!("{:.0}%", config.appearance.ui_scale * 100.0),
                    (
                        |c| {
                            c.appearance.ui_scale = next_ui_scale(c.appearance.ui_scale, Zoom::Out);
                        },
                        |c| {
                            c.appearance.ui_scale = next_ui_scale(c.appearance.ui_scale, Zoom::In);
                        },
                    ),
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Interface font",
                "Edit `ui_font` in config.toml",
                read_only(ui, &config.appearance.ui_font),
            ),
            setting_row(
                ui,
                "Sidebar side",
                "Which side of the window the spaces sidebar is on",
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        pill(
                            ui,
                            ("sidebar-side", 0),
                            "Left".to_owned(),
                            config.appearance.sidebar_side == SidebarSide::Left,
                        )
                        .on_click(cx.listener(
                            |this, _: &ClickEvent, _window, cx| {
                                this.update_config(cx, |c| {
                                    c.appearance.sidebar_side = SidebarSide::Left;
                                });
                            },
                        )),
                    )
                    .child(
                        pill(
                            ui,
                            ("sidebar-side", 1),
                            "Right".to_owned(),
                            config.appearance.sidebar_side == SidebarSide::Right,
                        )
                        .on_click(cx.listener(
                            |this, _: &ClickEvent, _window, cx| {
                                this.update_config(cx, |c| {
                                    c.appearance.sidebar_side = SidebarSide::Right;
                                });
                            },
                        )),
                    )
                    .into_any_element(),
            ),
            setting_row(
                ui,
                "Remember window position",
                "KDE reopens the window where you left it (adds a rule to kwinrulesrc)",
                self.remember_position_control(ui, cx),
            ),
        ]
    }

    fn remember_position_control(&self, ui: Ui, cx: &mut Context<Self>) -> AnyElement {
        let on = self.settings.remember_position;
        self.switch(ui, "remember-pos", on, move |this, cx| this.set_remember_position(!on, cx), cx)
    }

    fn set_remember_position(&mut self, on: bool, cx: &mut Context<Self>) {
        let result = if on { crate::kwin::enable() } else { crate::kwin::disable() };
        self.settings.remember_position = crate::kwin::is_enabled();
        self.settings.message = Some(match result {
            Ok(()) if on => {
                "KDE will remember the position from the next time mer closes.".to_owned()
            }
            Ok(()) => "The position rule was removed.".to_owned(),
            Err(error) => error,
        });
        cx.notify();
    }

    fn render_terminal(&self, ui: Ui, config: &Config, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut choices: Vec<Option<String>> = vec![None];
        choices.extend(installed_shells().into_iter().map(Some));
        let pills = choices.into_iter().enumerate().map(|(index, choice)| {
            let selected = choice == config.terminal.shell;
            let label = choice.as_deref().map_or_else(|| "Auto".to_owned(), str::to_owned);
            pill(ui, ("shell", index), label, selected).on_click(cx.listener(
                move |this, _: &ClickEvent, _window, cx| {
                    let choice = choice.clone();
                    this.update_config(cx, move |config| config.terminal.shell = choice);
                },
            ))
        });
        vec![
            setting_row(
                ui,
                "Shell",
                "Applies to terminals opened from now on. Auto prefers zsh when installed",
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .max_w(px(380.0))
                    .children(pills)
                    .into_any_element(),
            ),
            setting_row(
                ui,
                "Terminal font",
                "Edit `font_family` in config.toml",
                read_only(ui, &config.appearance.font_family),
            ),
            setting_row(
                ui,
                "Font size",
                "Terminal text, in points",
                self.stepper(
                    ui,
                    "font-size",
                    format!("{:.0}", config.appearance.font_size),
                    (|c| c.appearance.font_size -= 1.0, |c| c.appearance.font_size += 1.0),
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Line height",
                "Multiplier of the font size",
                self.stepper(
                    ui,
                    "line-height",
                    format!("{:.2}", config.appearance.line_height),
                    (
                        |c| {
                            c.appearance.line_height =
                                round_hundredths(c.appearance.line_height - 0.05);
                        },
                        |c| {
                            c.appearance.line_height =
                                round_hundredths(c.appearance.line_height + 0.05);
                        },
                    ),
                    cx,
                ),
            ),
        ]
    }

    fn render_spaces(&self, ui: Ui, config: &Config, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let minutes = config.spaces.auto_sleep_minutes;
        let value = if minutes == 0 { "Off".to_owned() } else { format!("{minutes} min") };
        vec![
            setting_row(
                ui,
                "Auto-sleep",
                "Put a space to sleep after this long without visiting it (never while something runs)",
                self.stepper(
                    ui,
                    "auto-sleep",
                    value,
                    (|c| sleep_step(c, false), |c| sleep_step(c, true)),
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Restore history",
                "Show each terminal's previous output when a space wakes up",
                self.toggle(
                    ui,
                    "restore-history",
                    config.spaces.restore_scrollback,
                    |c| c.spaces.restore_scrollback = !c.spaces.restore_scrollback,
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Keep running when the window closes",
                "Terminals and agents keep working in the background. Run `mer` to bring the window back, or Ctrl+Shift+Q to quit for good",
                self.toggle(
                    ui,
                    "keep-running",
                    config.spaces.keep_running_on_close,
                    |c| c.spaces.keep_running_on_close = !c.spaces.keep_running_on_close,
                    cx,
                ),
            ),
        ]
    }

    #[allow(clippy::too_many_lines)] // one row per setting
    fn render_agents(&self, ui: Ui, config: &Config, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let hook_button = |name: &'static str, target: HookTarget, cx: &mut Context<Self>| {
            pill(ui, (name, 0), "Preview install…".to_owned(), false).on_click(
                cx.listener(move |this, _: &ClickEvent, _window, cx| this.plan_hooks(target, cx)),
            )
        };
        let mut rows = vec![
            setting_row(
                ui,
                "Notifications",
                "Desktop notification when an agent needs you or finishes while mer is not in front",
                self.toggle(
                    ui,
                    "notify",
                    config.agents.notify,
                    |c| c.agents.notify = !c.agents.notify,
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Sounds",
                "A short sound when an agent needs you or finishes, at the volume shown (silent during Do not disturb). Pick the sounds with sound_waiting and sound_done in config.toml",
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(pill(ui, ("sound-test", 0), "Test".to_owned(), false).on_click(
                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.play_attention_sound(mer_agents::AgentStatus::Waiting, cx);
                        }),
                    ))
                    .child(self.stepper(
                        ui,
                        "sound-volume",
                        format!("{}%", config.agents.sound_volume),
                        (|c| volume_step(c, false), |c| volume_step(c, true)),
                        cx,
                    ))
                    .child(self.toggle(
                        ui,
                        "sound",
                        config.agents.sound,
                        |c| c.agents.sound = !c.agents.sound,
                        cx,
                    ))
                    .into_any_element(),
            ),
            setting_row(
                ui,
                "Long command alert",
                "Notify (and play the finished sound) when a command that ran this long ends while you are looking elsewhere. Editors, pagers, shells and agents never count",
                self.stepper(
                    ui,
                    "long-command",
                    long_command_label(config.agents.long_command_seconds),
                    (|c| long_command_step(c, false), |c| long_command_step(c, true)),
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Resume on wake",
                "Re-open the agent's session when a space wakes up (Claude Code only)",
                self.toggle(
                    ui,
                    "resume",
                    config.agents.resume_on_wake,
                    |c| c.agents.resume_on_wake = !c.agents.resume_on_wake,
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Claude Code hooks",
                "Lets mer know when Claude is working, waiting or done",
                hook_button("hooks-claude", HookTarget::Claude, cx).into_any_element(),
            ),
            setting_row(
                ui,
                "Codex notifications",
                "Lets mer know when Codex finishes a turn",
                hook_button("hooks-codex", HookTarget::Codex, cx).into_any_element(),
            ),
            setting_row(
                ui,
                "Shell integration (zsh)",
                "Tabs show the exact command you ran (`ionic serve`) instead of only the program's own name. Adds one line to ~/.zshrc; it does nothing outside mer",
                hook_button("hooks-shell", HookTarget::Shell, cx).into_any_element(),
            ),
        ];
        if let Some(plan) = &self.settings.plan {
            rows.push(self.render_plan(ui, plan, cx));
        }
        if let Some(message) = &self.settings.message {
            rows.push(
                div()
                    .py_2()
                    .text_color(ui.accent)
                    .child(SharedString::from(message.clone()))
                    .into_any_element(),
            );
        }
        rows
    }

    fn render_plan(&self, ui: Ui, plan: &HookPlan, cx: &mut Context<Self>) -> AnyElement {
        let lines = plan.summary.iter().map(|line| {
            div().text_xs().text_color(ui.text).child(SharedString::from(line.clone()))
        });
        let buttons = div()
            .flex()
            .flex_row()
            .gap_2()
            .when(plan.changed, |row| {
                row.child(pill(ui, ("plan-apply", 0), "Apply".to_owned(), true).on_click(
                    cx.listener(|this, _: &ClickEvent, _window, cx| this.apply_hooks(cx)),
                ))
            })
            .child(pill(ui, ("plan-cancel", 0), "Cancel".to_owned(), false).on_click(cx.listener(
                |this, _: &ClickEvent, _window, cx| {
                    this.settings.plan = None;
                    cx.notify();
                },
            )));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .my_2()
            .rounded_md()
            .border_1()
            .border_color(ui.accent)
            .children(lines)
            .child(buttons)
            .into_any_element()
    }

    fn render_shortcuts(&self, ui: Ui, config: &Config) -> Vec<AnyElement> {
        let mut rows: Vec<AnyElement> = vec![
            div()
                .py_2()
                .text_xs()
                .text_color(ui.dim)
                .child("Override any of these in the [keys] table of config.toml. An empty value unbinds it.")
                .into_any_element(),
        ];
        for (name, default) in ACTION_NAMES {
            let stroke = config.keys.get(*name).map_or(*default, String::as_str);
            let stroke = if stroke.is_empty() { "—" } else { stroke };
            rows.push(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .py_1()
                    .child(
                        div().text_color(ui.text).child(SharedString::from(name.replace('_', " "))),
                    )
                    .child(div().text_color(ui.dim).child(SharedString::from(stroke.to_owned())))
                    .into_any_element(),
            );
        }
        rows
    }

    fn plan_hooks(&mut self, target: HookTarget, cx: &mut Context<Self>) {
        let result = home_dir()
            .ok_or_else(|| "cannot find your home directory".to_owned())
            .and_then(|home| match target {
                HookTarget::Claude => plan_claude(&home, &mer_agents::shell_quote(&mer_exe())),
                HookTarget::Codex => plan_codex(&home, &mer_exe()),
                HookTarget::Shell => Ok(plan_shell(&home)),
            });
        match result {
            Ok(plan) => {
                self.settings.message = (!plan.changed).then(|| "Already installed.".to_owned());
                self.settings.plan = plan.changed.then_some(plan);
            }
            Err(error) => {
                self.settings.plan = None;
                self.settings.message = Some(error);
            }
        }
        cx.notify();
    }

    fn apply_hooks(&mut self, cx: &mut Context<Self>) {
        let Some(plan) = self.settings.plan.take() else { return };
        self.settings.message = Some(match apply_plan(&plan) {
            Ok(note) => note,
            Err(error) => error,
        });
        cx.notify();
    }

    /// Opens the panel on a named section (`mer ctl show settings:agents`). False if unknown.
    pub(super) fn open_settings_section(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let Some(section) = Section::from_name(name) else { return false };
        self.open_settings(cx);
        self.settings.section = section;
        self.settings.plan = None;
        self.settings.message = None;
        true
    }

    /// The overlay, when the panel is open.
    #[allow(clippy::too_many_lines)]
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.settings.open {
            return None;
        }
        let chrome = &self.palette.chrome;
        let ui = Ui {
            surface: chrome.surface,
            text: chrome.text,
            dim: chrome.text_dim,
            accent: chrome.accent,
            base: self.palette.background,
            hairline: self.palette.hairline(),
            scale: self.ui_scale,
            narrow: self.is_narrow(),
        };
        let config = cx.global::<AppSettings>().config.clone();
        let section = self.settings.section;

        let narrow = ui.narrow;
        let nav = Section::ALL
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let item = *item;
                div()
                    .id(("settings-nav", index))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(ui.px(32.0))
                    .px_3()
                    .rounded_lg()
                    .cursor_pointer()
                    .text_color(if item == section { ui.text } else { ui.dim })
                    .when(item == section, |row| row.bg(ui.surface))
                    .hover(move |row| row.bg(ui.surface))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.settings.section = item;
                        this.settings.plan = None;
                        this.settings.message = None;
                        cx.notify();
                    }))
                    // The side nav marks the section with a bar; the strip with its background.
                    .when(!narrow, |row| {
                        row.child(div().w(ui.px(3.0)).h(ui.px(16.0)).flex_none().rounded_full().bg(
                            if item == section {
                                ui.accent
                            } else {
                                gpui::hsla(0.0, 0.0, 0.0, 0.0)
                            },
                        ))
                    })
                    .child(item.label())
            })
            .collect::<Vec<_>>();

        // Wide: a column on the left. Narrow: a strip on top that scrolls sideways.
        let nav_panel = if narrow {
            div()
                .id("settings-nav-strip")
                .w_full()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .overflow_x_scroll()
                .bg(chrome.sidebar)
                .border_b_1()
                .border_color(ui.hairline)
                .children(nav)
        } else {
            div()
                .id("settings-nav-column")
                .w(ui.px(190.0))
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .overflow_y_scroll()
                .bg(chrome.sidebar)
                .border_r_1()
                .border_color(ui.hairline)
                .child(div().px_3().py_1().text_xs().text_color(ui.dim).child("SETTINGS"))
                .children(nav)
        };

        let rows = match section {
            Section::Appearance => self.render_appearance(ui, &config, cx),
            Section::Terminal => self.render_terminal(ui, &config, cx),
            Section::Spaces => self.render_spaces(ui, &config, cx),
            Section::Agents => self.render_agents(ui, &config, cx),
            Section::Performance => self.render_performance(ui, &config, cx),
            Section::Shortcuts => self.render_shortcuts(ui, &config),
        };

        let close = div()
            .id("settings-close")
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .size(ui.px(32.0))
            .rounded_lg()
            .cursor_pointer()
            .text_color(ui.dim)
            .hover(move |button| button.bg(ui.surface).text_color(ui.text))
            .tooltip(crate::tooltip::tip(&self.palette, "Close settings", ["Esc".to_owned()]))
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| this.close_settings(cx)))
            .child("×");

        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                // A margin, so the panel never touches the window edge and the percentages below
                // are measured inside it.
                .p(ui.px(if narrow { 8.0 } else { 16.0 }))
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::hsla(0.0, 0.0, 0.0, 0.45))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                        this.close_settings(cx);
                    }),
                )
                .child(
                    div()
                        .track_focus(&self.prompt_focus)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                            this.on_settings_key(event, cx);
                        }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        // Its natural size, never more than the window leaves it.
                        .w(ui.px(860.0))
                        .h(ui.px(560.0))
                        .max_w(relative(1.0))
                        .max_h(relative(1.0))
                        .flex()
                        .when(narrow, Styled::flex_col)
                        .when(!narrow, Styled::flex_row)
                        .bg(chrome.bar)
                        .border_1()
                        .border_color(ui.hairline)
                        .rounded_xl()
                        .shadow(crate::theme::overlay_shadow(self.palette.is_dark()))
                        .overflow_hidden()
                        .child(nav_panel)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                // The title and the close button stay put; only the rows scroll.
                                .child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .gap_2()
                                        .px_5()
                                        .pt_4()
                                        .pb_2()
                                        .child(
                                            div()
                                                .text_lg()
                                                .text_color(ui.text)
                                                .child(section.label()),
                                        )
                                        .child(close),
                                )
                                .child(
                                    div()
                                        .id("settings-content")
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .px_5()
                                        .pb_5()
                                        // The rows of a section sit in one card; the last separator
                                        // is pulled out of sight under the card's edge.
                                        .child(
                                            div()
                                                .rounded_lg()
                                                .border_1()
                                                .border_color(ui.hairline)
                                                .bg(crate::theme::with_alpha(ui.surface, 0.25))
                                                .px_4()
                                                .overflow_hidden()
                                                .child(div().mb(px(-1.0)).children(rows)),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

// ----- hook installation -----

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Path of this executable, or `mer` (looked up in `PATH`) if it cannot be known or no longer
/// exists on disk (Linux reports a replaced binary as "... (deleted)").
fn mer_exe() -> String {
    std::env::current_exe()
        .ok()
        .filter(|path| path.exists())
        .map_or_else(|| "mer".to_owned(), |path| path.to_string_lossy().into_owned())
}

fn plan_claude(home: &Path, exe: &str) -> Result<HookPlan, String> {
    let path = home.join(".claude/settings.json");
    let current = match std::fs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?,
        _ => serde_json::json!({}),
    };
    let command = format!("{exe} hook --agent claude");
    let (patched, changed) =
        mer_agents::patch_claude_settings(current, &command).map_err(|error| error.to_string())?;
    let new_text =
        serde_json::to_string_pretty(&patched).map_err(|error| error.to_string())? + "\n";
    let summary = vec![
        format!("Will edit {} (a backup is kept as settings.json.bak-mer):", path.display()),
        "hooks → SessionStart, UserPromptSubmit, PostToolUse, Notification and Stop will run:"
            .to_owned(),
        command,
        "Other hooks and settings are left as they are.".to_owned(),
    ];
    Ok(HookPlan { target: HookTarget::Claude, path, new_text, changed, summary })
}

fn plan_codex(home: &Path, exe: &str) -> Result<HookPlan, String> {
    let path = home.join(".codex/config.toml");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let (new_text, changed) =
        mer_agents::patch_codex_config(&current, exe).map_err(|error| error.to_string())?;
    let summary = vec![
        format!("Will edit {} (a backup is kept as config.toml.bak-mer):", path.display()),
        format!("notify = [\"{exe}\", \"hook\", \"--agent\", \"codex\"]"),
        "Comments and other settings are left as they are.".to_owned(),
    ];
    Ok(HookPlan { target: HookTarget::Codex, path, new_text, changed, summary })
}

fn plan_shell(home: &Path) -> HookPlan {
    let path = home.join(".zshrc");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let (new_text, changed) = mer_agents::patch_zshrc(&current);
    let summary = vec![
        format!("Will add one line to {} (a backup is kept as .zshrc.bak-mer):", path.display()),
        mer_agents::ZSHRC_LINE.to_owned(),
        "It reports each command to mer and only acts inside a mer terminal.".to_owned(),
    ];
    HookPlan { target: HookTarget::Shell, path, new_text, changed, summary }
}

/// Backs up the existing file, then writes the new text atomically.
fn apply_plan(plan: &HookPlan) -> Result<String, String> {
    let name = match plan.target {
        HookTarget::Claude => "Claude Code hooks",
        HookTarget::Codex => "Codex notifications",
        HookTarget::Shell => "Shell integration",
    };
    let parent = plan.path.parent().ok_or("invalid path")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    mer_agents::backup_once(&plan.path).map_err(|error| format!("could not back up: {error}"))?;
    // Settings files can hold tokens, so a new one is private; a shell rc file is not.
    let new_file_mode = if plan.target == HookTarget::Shell { 0o644 } else { 0o600 };
    mer_agents::replace_file(&plan.path, &plan.new_text, new_file_mode)
        .map_err(|error| error.to_string())?;
    Ok(match plan.target {
        HookTarget::Shell => format!("{name} installed. Open a new terminal to use it."),
        _ => format!("{name} installed. Restart the agent to pick them up."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_volume_steps_by_tens_and_stops_at_both_ends() {
        let mut config = Config::default();
        assert_eq!(config.agents.sound_volume, 50);
        volume_step(&mut config, true);
        assert_eq!(config.agents.sound_volume, 60);
        for _ in 0..9 {
            volume_step(&mut config, true);
        }
        assert_eq!(config.agents.sound_volume, 100);
        for _ in 0..12 {
            volume_step(&mut config, false);
        }
        assert_eq!(config.agents.sound_volume, 0);
        // A hand-edited 35 snaps to the next step up, or the one below.
        config.agents.sound_volume = 35;
        volume_step(&mut config, true);
        assert_eq!(config.agents.sound_volume, 40);
        config.agents.sound_volume = 35;
        volume_step(&mut config, false);
        assert_eq!(config.agents.sound_volume, 30);
    }

    #[test]
    fn the_long_command_steps_walk_and_label_nicely() {
        let mut config = Config::default();
        assert_eq!(config.agents.long_command_seconds, 30);
        for expected in [60, 120, 300, 300] {
            long_command_step(&mut config, true);
            assert_eq!(config.agents.long_command_seconds, expected);
        }
        for expected in [120, 60, 30, 10, 0, 0] {
            long_command_step(&mut config, false);
            assert_eq!(config.agents.long_command_seconds, expected);
        }
        assert_eq!(long_command_label(0), "Off");
        assert_eq!(long_command_label(30), "30 s");
        assert_eq!(long_command_label(120), "2 min");
        // A value beyond the list comes back to the last step.
        config.agents.long_command_seconds = 900;
        long_command_step(&mut config, false);
        assert_eq!(config.agents.long_command_seconds, 300);
    }

    #[test]
    fn sleep_steps_walk_the_list_and_stop_at_both_ends() {
        let mut config = Config::default();
        assert_eq!(config.spaces.auto_sleep_minutes, 0);
        sleep_step(&mut config, false);
        assert_eq!(config.spaces.auto_sleep_minutes, 0);
        for expected in [5, 10, 15, 30, 60, 120, 240, 240] {
            sleep_step(&mut config, true);
            assert_eq!(config.spaces.auto_sleep_minutes, expected);
        }
        sleep_step(&mut config, false);
        assert_eq!(config.spaces.auto_sleep_minutes, 120);
        // A hand-edited value in between snaps to the next step.
        config.spaces.auto_sleep_minutes = 7;
        sleep_step(&mut config, true);
        assert_eq!(config.spaces.auto_sleep_minutes, 10);
    }

    #[test]
    fn claude_plan_creates_backs_up_and_is_idempotent() {
        let home = tempfile::tempdir().expect("temp home");
        let dir = home.path().join(".claude");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("settings.json"), r#"{"model":"opus"}"#).expect("write");

        let plan = plan_claude(home.path(), "/bin/mer").expect("plans");
        assert!(plan.changed);
        assert!(plan.summary.iter().any(|line| line.contains("/bin/mer hook --agent claude")));
        apply_plan(&plan).expect("applies");

        let written = std::fs::read_to_string(dir.join("settings.json")).expect("read");
        assert!(written.contains("\"model\": \"opus\""));
        assert!(written.contains("UserPromptSubmit"));
        let backup = std::fs::read_to_string(dir.join("settings.json.bak-mer")).expect("backup");
        assert_eq!(backup, r#"{"model":"opus"}"#);

        assert!(!plan_claude(home.path(), "/bin/mer").expect("plans again").changed);
    }

    #[test]
    fn installing_hooks_keeps_a_private_settings_file_private() {
        use std::os::unix::fs::PermissionsExt;

        let home = tempfile::tempdir().expect("temp home");
        let dir = home.path().join(".claude");
        std::fs::create_dir_all(&dir).expect("dir");
        let settings = dir.join("settings.json");
        std::fs::write(&settings, r#"{"model":"opus"}"#).expect("write");
        std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o600)).expect("chmod");

        apply_plan(&plan_claude(home.path(), "/bin/mer").expect("plans")).expect("applies");

        let mode = |path: &Path| std::fs::metadata(path).expect("exists").permissions().mode();
        assert_eq!(mode(&settings) & 0o777, 0o600);
        assert_eq!(mode(&dir.join("settings.json.bak-mer")) & 0o777, 0o600);
    }

    #[test]
    fn a_path_with_shell_characters_cannot_change_the_hook_command() {
        let home = tempfile::tempdir().expect("temp home");
        let exe = mer_agents::shell_quote("/tmp/x; touch pwned/mer");
        let plan = plan_claude(home.path(), &exe).expect("plans");
        let command = "'/tmp/x; touch pwned/mer' hook --agent claude";
        assert!(plan.summary.iter().any(|line| line == command), "{:?}", plan.summary);
    }

    #[test]
    fn claude_plan_rejects_invalid_json_without_touching_the_file() {
        let home = tempfile::tempdir().expect("temp home");
        let dir = home.path().join(".claude");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("settings.json"), "{ not json").expect("write");
        assert!(plan_claude(home.path(), "/bin/mer").is_err());
        assert_eq!(std::fs::read_to_string(dir.join("settings.json")).expect("read"), "{ not json");
    }

    #[test]
    fn shell_plan_appends_one_line_and_is_idempotent() {
        let home = tempfile::tempdir().expect("temp home");
        std::fs::write(home.path().join(".zshrc"), "export A=1\n").expect("write");

        let plan = plan_shell(home.path());
        assert!(plan.changed);
        apply_plan(&plan).expect("applies");
        let written = std::fs::read_to_string(home.path().join(".zshrc")).expect("read");
        assert!(written.starts_with("export A=1\n") && written.contains("mer shell-init zsh"));
        let backup = std::fs::read_to_string(home.path().join(".zshrc.bak-mer")).expect("backup");
        assert_eq!(backup, "export A=1\n");

        assert!(!plan_shell(home.path()).changed);
    }

    #[test]
    fn codex_plan_keeps_comments_and_refuses_a_foreign_notify() {
        let home = tempfile::tempdir().expect("temp home");
        let dir = home.path().join(".codex");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("config.toml"), "# mine\nmodel = \"o3\"\n").expect("write");

        let plan = plan_codex(home.path(), "/bin/mer").expect("plans");
        apply_plan(&plan).expect("applies");
        let written = std::fs::read_to_string(dir.join("config.toml")).expect("read");
        assert!(written.contains("# mine") && written.contains("hook"));

        std::fs::write(dir.join("config.toml"), "notify = [\"other\"]\n").expect("write");
        assert!(plan_codex(home.path(), "/bin/mer").is_err());
    }
}
