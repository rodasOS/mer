//! The Performance panel: the optional features that cost something, each with what it costs.
//!
//! Nothing in `mer` polls, so none of these costs CPU while idle. They are grouped here so the
//! price of each one is in plain sight and a lighter setup is one click away.

use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, div,
};
use mer_config::Config;

use super::{Ui, WorkspaceView, pill, setting_row};

/// Turns every optional feature of this panel on or off together.
fn set_optional(config: &mut Config, on: bool) {
    config.terminal.scrollbar = on;
    config.terminal.context_chips = on;
    config.terminal.node_version_hint = on;
}

/// All of them are `on`.
fn all_optional_are(config: &Config, on: bool) -> bool {
    let terminal = &config.terminal;
    [terminal.scrollbar, terminal.context_chips, terminal.node_version_hint]
        .into_iter()
        .all(|value| value == on)
}

impl WorkspaceView {
    pub(super) fn render_performance(
        &self,
        ui: Ui,
        config: &Config,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let preset = |index: usize, label: &str, on: bool, cx: &mut Context<Self>| {
            pill(ui, ("performance-preset", index), label.to_owned(), all_optional_are(config, on))
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.update_config(cx, move |config| set_optional(config, on));
                }))
        };
        vec![
            div()
                .py_3()
                .text_xs()
                .text_color(ui.dim)
                .child(
                    "Nothing in mer polls, so none of this costs CPU while idle. Each feature \
                     says what it costs and what it needs.",
                )
                .into_any_element(),
            setting_row(
                ui,
                "Preset",
                "Switch the optional features below on or off at once. Scrollback and the \
                 other settings are left as they are",
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(preset(0, "Everything on", true, cx))
                    .child(preset(1, "Lightweight", false, cx))
                    .into_any_element(),
            ),
            setting_row(
                ui,
                "Scrollback",
                "Lines kept per terminal, for new terminals. Memory: grows with the number of \
                 lines each terminal has filled",
                self.stepper(
                    ui,
                    "scrollback",
                    config.terminal.scrollback.to_string(),
                    (
                        |c| c.terminal.scrollback = c.terminal.scrollback.saturating_sub(1000),
                        |c| c.terminal.scrollback = c.terminal.scrollback.saturating_add(1000),
                    ),
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Scrollbar",
                "A scrollbar on each terminal and a button to jump back to the bottom. Cost: a \
                 few extra elements per frame while there is history; nothing otherwise",
                self.toggle(
                    ui,
                    "scrollbar",
                    config.terminal.scrollbar,
                    |c| c.terminal.scrollbar = !c.terminal.scrollbar,
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Git and node chips",
                "The branch and node version under each terminal, with a list to switch them. \
                 Cost: one tiny read when a prompt arrives, in the background. Needs the zsh \
                 integration (Agents)",
                self.toggle(
                    ui,
                    "context-chips",
                    config.terminal.context_chips,
                    |c| c.terminal.context_chips = !c.terminal.context_chips,
                    cx,
                ),
            ),
            setting_row(
                ui,
                "Node version the project asks for",
                "Colors the node chip and marks the version in the list when .nvmrc or \
                 .node-version says otherwise. Cost: a few small file reads, once per folder or \
                 branch, in the background; it never runs nvm. Needs the chips",
                self.toggle(
                    ui,
                    "node-version-hint",
                    config.terminal.node_version_hint,
                    |c| c.terminal.node_version_hint = !c.terminal.node_version_hint,
                    cx,
                ),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_presets_flip_every_optional_feature_together() {
        let mut config = Config::default();
        assert!(all_optional_are(&config, true), "everything is on by default");
        set_optional(&mut config, false);
        assert!(all_optional_are(&config, false));
        assert!(!config.terminal.scrollbar && !config.terminal.context_chips);
        assert!(!config.terminal.node_version_hint);
        config.terminal.scrollbar = true;
        assert!(!all_optional_are(&config, true) && !all_optional_are(&config, false));
        set_optional(&mut config, true);
        assert!(all_optional_are(&config, true));
    }

    #[test]
    fn the_presets_leave_other_settings_alone() {
        let mut config = Config::default();
        config.terminal.scrollback = 777;
        config.spaces.auto_sleep_minutes = 15;
        set_optional(&mut config, false);
        assert_eq!(config.terminal.scrollback, 777);
        assert_eq!(config.spaces.auto_sleep_minutes, 15);
    }
}
