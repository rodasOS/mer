//! Shell integration: lets the shell tell mer which command it is about to run.
//!
//! `/proc` only knows the name a process gave itself, and some programs rewrite it (`ionic
//! serve` shows up as `ionic`). The shell knows the exact line, so a small zsh hook sends it to
//! mer as a private title (see [`crate::COMMAND_MARKER`]). It only acts inside mer.

use crate::COMMAND_MARKER;

/// The line added to `~/.zshrc`. Silent when `mer` is not installed.
pub const ZSHRC_LINE: &str = "command -v mer >/dev/null && eval \"$(mer shell-init zsh)\"";

const ZSHRC_COMMENT: &str = "# mer: name tabs after the command being run (only acts inside mer)";

/// The script `mer shell-init zsh` prints.
pub fn zsh_init_script() -> String {
    format!(
        "# mer shell integration: reports the command about to run so tabs can name it.\n\
         # It only acts inside a mer terminal and never launches a process.\n\
         if [[ -n $MER_PANE_ID ]]; then\n  \
           _mer_preexec() {{ print -n -- $'\\e]2;{COMMAND_MARKER}'\"${{1//[[:cntrl:]]/ }}\"$'\\a' }}\n  \
           autoload -Uz add-zsh-hook\n  \
           add-zsh-hook preexec _mer_preexec\n\
         fi\n"
    )
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
