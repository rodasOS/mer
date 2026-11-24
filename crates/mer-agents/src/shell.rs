//! Shell integration: lets the shell tell mer which command it is about to run.
//!
//! `/proc` only knows the name a process gave itself, and some programs rewrite it (`ionic
//! serve` shows up as `ionic`). The shell knows the exact line, so a small zsh hook sends it to
//! mer as a private title (see [`crate::COMMAND_MARKER`]). It only acts inside mer.

use crate::{COMMAND_MARKER, CONTEXT_MARKER};

/// The line added to `~/.zshrc`. Silent when `mer` is not installed.
pub const ZSHRC_LINE: &str = "command -v mer >/dev/null && eval \"$(mer shell-init zsh)\"";

const ZSHRC_COMMENT: &str = "# mer: name tabs after the command being run (only acts inside mer)";

/// The script `mer shell-init zsh` prints.
///
/// `preexec` reports the command about to run; `precmd` reports, at each prompt, the folder and
/// the nvm node version (`$NVM_BIN` is a plain parameter: no `node -v`, no process). Together
/// they also say whether the shell is idle at its prompt or busy with a command.
pub fn zsh_init_script() -> String {
    format!(
        "# mer shell integration: reports the command about to run and, at each prompt, the folder\n\
         # and node version, so tabs and chips can follow. It only acts inside a mer terminal and\n\
         # never launches a process.\n\
         if [[ -n $MER_PANE_ID ]]; then\n  \
           _mer_preexec() {{ print -rn -- $'\\e]2;{COMMAND_MARKER}'\"${{1//[[:cntrl:]]/ }}\"$'\\a' }}\n  \
           _mer_precmd() {{ print -rn -- $'\\e]2;{CONTEXT_MARKER}node='\"${{NVM_BIN:+${{NVM_BIN:h:t}}}}\"';nvm='\"${{NVM_DIR//[[:cntrl:]]/ }}\"';cwd='\"${{PWD//[[:cntrl:]]/ }}\"$'\\a' }}\n  \
           autoload -Uz add-zsh-hook\n  \
           add-zsh-hook preexec _mer_preexec\n  \
           add-zsh-hook precmd _mer_precmd\n\
         fi\n"
    )
}

/// The keystrokes that run `command` at an idle shell prompt: Ctrl+U clears whatever was
/// half typed, then the command and Enter. `None` if the command has control characters.
///
/// Callers must first make sure nothing but the shell owns the terminal (see
/// `PaneContext::accepts_commands`): these keys would reach a running program otherwise.
pub fn command_input(command: &str) -> Option<String> {
    (!command.chars().any(char::is_control)).then(|| format!("\u{15}{command}\r"))
}

/// Adds the integration to the text of a `.zshrc`. Returns the new text and whether it changed;
/// an already integrated file is returned as it was.
pub fn patch_zshrc(current: &str) -> (String, bool) {
    if current.contains("mer shell-init") {
        return (current.to_owned(), false);
    }
    let mut text = current.to_owned();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(ZSHRC_COMMENT);
    text.push('\n');
    text.push_str(ZSHRC_LINE);
    text.push('\n');
    (text, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_only_acts_inside_mer_and_uses_the_marker() {
        let script = zsh_init_script();
        assert!(script.contains("[[ -n $MER_PANE_ID ]]"));
        assert!(script.contains("mer:cmd:"));
        assert!(script.contains("add-zsh-hook preexec"));
    }

    #[test]
    fn the_script_reports_the_prompt_without_starting_a_process() {
        let script = zsh_init_script();
        assert!(script.contains("add-zsh-hook precmd"));
        assert!(script.contains("mer:ctx:node="));
        assert!(script.contains("${NVM_BIN:+${NVM_BIN:h:t}}"));
        assert!(!script.contains("$(") && !script.contains('`'), "no command substitution");
    }

    #[test]
    fn the_hooks_print_raw_so_a_folder_name_cannot_inject_escapes() {
        // A folder literally named `x\e[2J` must stay text: without `-r`, `print` would turn the
        // two characters `\e` into a real ESC.
        let script = zsh_init_script();
        assert!(script.contains("print -rn -- "));
        assert!(!script.contains("print -n -- "));
    }

    #[test]
    fn commands_are_typed_after_clearing_the_line() {
        assert_eq!(command_input("ls").as_deref(), Some("\u{15}ls\r"));
        assert_eq!(command_input("a\nb"), None);
    }

    #[test]
    fn patching_appends_once_and_keeps_the_rest() {
        let (patched, changed) = patch_zshrc("export A=1");
        assert!(changed);
        assert!(patched.starts_with("export A=1\n\n"));
        assert!(patched.contains(ZSHRC_LINE));
        let (again, changed_again) = patch_zshrc(&patched);
        assert!(!changed_again);
        assert_eq!(again, patched);
    }

    #[test]
    fn an_empty_file_gets_just_the_block() {
        let (patched, changed) = patch_zshrc("");
        assert!(changed);
        assert!(patched.starts_with("# mer:"));
    }
}
