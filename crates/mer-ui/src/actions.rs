//! Keyboard actions and their key bindings (defaults, overridable from `[keys]` in the config).

use std::collections::BTreeMap;

use gpui::{App, KeyBinding, actions};

actions!(
    mer,
    [
        NewTab,
        CloseTab,
        ClosePane,
        SplitRight,
        SplitDown,
        NewSpace,
        NewTerminal,
        CloseSpace,
        RenameSpace,
        RenameTab,
        NextTab,
        PrevTab,
        NextSpace,
        PrevSpace,
        FocusNext,
        FocusPrev,
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown,
        ResizeLeft,
        ResizeRight,
        ResizeUp,
        ResizeDown,
        ToggleSidebar,
        OpenPalette,
        SleepSpace,
        SleepOthers,
        OpenSettings,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        UiScaleUp,
        UiScaleDown,
        UiScaleReset,
        MoveTabLeft,
        MoveTabRight,
        MoveSpaceUp,
        MoveSpaceDown,
        SearchTerminal,
        ScrollToBottom,
        Quit,
    ]
);

/// An empty default means "unbound unless the user binds it".
macro_rules! binding_table {
    ($($name:literal => $action:ident = $default:literal),* $(,)?) => {
        /// `(config name, default keystroke)` of every action, for the settings panel.
        pub const ACTION_NAMES: &[(&str, &str)] = &[$(($name, $default)),*];

        fn build(
            overrides: &BTreeMap<String, String>,
            warnings: &mut Vec<String>,
            cx: &App,
        ) -> Vec<KeyBinding> {
            let mut bindings = Vec::new();
            $(
                let strokes = overrides.get($name).map_or($default, String::as_str);
                // Several keystrokes can share an action: `ctrl-= | ctrl-+`.
                for stroke in strokes.split('|').map(str::trim).filter(|s| !s.is_empty()) {
                    match KeyBinding::load(stroke, Box::new($action), None, false, None, cx) {
                        Ok(binding) => bindings.push(binding),
                        Err(error) => warnings.push(format!("bad keystroke for `{}`: {error}", $name)),
                    }
                }
            )*
            bindings
        }
    };
}

binding_table! {
    "new_tab" => NewTab = "ctrl-shift-t",
    "close_tab" => CloseTab = "",
    "close_pane" => ClosePane = "ctrl-shift-w",
    "split_right" => SplitRight = "ctrl-shift-d",
    "split_down" => SplitDown = "ctrl-shift-e",
    "new_space" => NewSpace = "ctrl-shift-n",
    "new_terminal" => NewTerminal = "",
    "close_space" => CloseSpace = "",
    "rename_space" => RenameSpace = "",
    "rename_tab" => RenameTab = "",
    "next_tab" => NextTab = "ctrl-tab",
    "prev_tab" => PrevTab = "ctrl-shift-tab",
    "next_space" => NextSpace = "ctrl-shift-pagedown",
    "prev_space" => PrevSpace = "ctrl-shift-pageup",
    "focus_next" => FocusNext = "",
    "focus_prev" => FocusPrev = "",
    "focus_left" => FocusLeft = "ctrl-shift-left",
    "focus_right" => FocusRight = "ctrl-shift-right",
    "focus_up" => FocusUp = "ctrl-shift-up",
    "focus_down" => FocusDown = "ctrl-shift-down",
    "resize_left" => ResizeLeft = "alt-shift-left",
    "resize_right" => ResizeRight = "alt-shift-right",
    "resize_up" => ResizeUp = "alt-shift-up",
    "resize_down" => ResizeDown = "alt-shift-down",
    "toggle_sidebar" => ToggleSidebar = "ctrl-shift-b",
    "open_palette" => OpenPalette = "ctrl-shift-p",
    "sleep_space" => SleepSpace = "ctrl-shift-z",
    "sleep_others" => SleepOthers = "",
    "open_settings" => OpenSettings = "ctrl-,",
    // `ctrl-+` is its own key on many layouts; on a US layout it is `ctrl-shift-=`.
    "zoom_in" => ZoomIn = "ctrl-= | ctrl-+ | ctrl-shift-=",
    "zoom_out" => ZoomOut = "ctrl--",
    "zoom_reset" => ZoomReset = "ctrl-0",
    "ui_scale_up" => UiScaleUp = "ctrl-alt-= | ctrl-alt-+ | ctrl-alt-shift-=",
    "ui_scale_down" => UiScaleDown = "ctrl-alt--",
    "ui_scale_reset" => UiScaleReset = "ctrl-alt-0",
    "move_tab_left" => MoveTabLeft = "",
    "move_tab_right" => MoveTabRight = "",
    "move_space_up" => MoveSpaceUp = "",
    "move_space_down" => MoveSpaceDown = "",
    "search" => SearchTerminal = "ctrl-shift-f",
    "scroll_to_bottom" => ScrollToBottom = "ctrl-shift-end",
    "quit" => Quit = "ctrl-shift-q",
}

/// Replaces every key binding with the defaults plus the user's overrides.
/// Bad keystrokes are skipped and reported in the returned warnings.
pub fn apply_key_bindings(cx: &mut App, overrides: &BTreeMap<String, String>) -> Vec<String> {
    let mut warnings = Vec::new();
    let bindings = build(overrides, &mut warnings, cx);
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    warnings
}

#[cfg(test)]
mod tests {
    use super::ACTION_NAMES;

    #[test]
    fn every_default_keystroke_parses() {
        for (name, strokes) in ACTION_NAMES {
            for stroke in strokes.split('|').map(str::trim).filter(|s| !s.is_empty()) {
                assert!(
                    gpui::Keystroke::parse(stroke).is_ok(),
                    "default keystroke `{stroke}` of `{name}` does not parse"
                );
            }
        }
    }

    #[test]
    fn no_default_keystroke_is_used_twice() {
        let mut seen = std::collections::HashSet::new();
        for (name, strokes) in ACTION_NAMES {
            for stroke in strokes.split('|').map(str::trim).filter(|s| !s.is_empty()) {
                assert!(seen.insert(stroke), "`{stroke}` is bound twice (second time by `{name}`)");
            }
        }
    }
}
