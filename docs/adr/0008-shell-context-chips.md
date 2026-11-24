# 0008. Shell context chips over a private title channel

Status: accepted. Builds on the `mer:cmd:` channel of the shell integration.

## Context
Users switch git branches and node versions all day. Showing both under each terminal, and
letting a click change them, needs `mer` to know three things per pane: the folder, the node
version and whether the shell is idle. `mer` has no input editor of its own (the prompt is
zsh's), and it must stay instant: no polling, no process per prompt, nothing in the render path.

## Decision
- The zsh integration gains a `precmd` hook that sends one private title per prompt:
  `ESC ] 2 ; mer:ctx:node=<v>;nvm=<dir>;cwd=<folder> BEL`. The node version is
  `${NVM_BIN:+${NVM_BIN:h:t}}`, a parameter expansion, so no `node -v` and no process. The
  folder is last because it is the only field that can contain `;`. `mer_agents::context_from_title`
  parses it; `on_title` swallows it, so it is never shown as a title.
- `preexec` (`mer:cmd:`) and `precmd` (`mer:ctx:`) together tell whether the shell is at its
  prompt: `PaneContext::at_prompt` (in `mer-core`, `serde(skip)`, only alive with the pane).
- The branch is a read of `.git/HEAD` (`mer_agents::git`), done on the background executor.
  Listing every branch (`git for-each-ref`) and every installed node version (a directory read of
  `<nvm>/versions/node`) happens only when a picker opens.
- A picker types `git switch <branch>` or `nvm use <version>` into the shell, preceded by Ctrl+U.
  It does so **only if both** hold at the moment of the click and again at the moment of the
  choice: the shell reported a prompt (`at_prompt`), and `/proc` shows no foreground program
  other than the shell (`PaneContext::accepts_commands`). Otherwise the chip is dimmed and does
  nothing. `mer` never sends keys to a running agent, editor or server, and never signals a process.
- Both features have a switch: `[terminal] scrollbar` and `[terminal] context_chips`.

## Amendment: the node version a project asks for
The node chip can also say which version the project wants (`.nvmrc`, else `.node-version`, the
nearest one above the folder). It is read **lazily**: only after the first prompt, only when
the `(folder, branch)` pair changes (a branch switch often changes `.nvmrc`), on the background
executor, and again when the picker opens. It is matched against `<nvm>/versions/node` (exact, or
the newest installed match for a partial spec such as `18`); aliases (`lts/*`) are shown but never
judged, because only nvm can resolve them. `mer` never runs `nvm` or `node` for this.
`[terminal] node_version_hint` turns it off. Idle cost stays zero.

## Consequences
- Idle cost is zero. Per prompt: one title, one small background read. Per picker: one process.
- Only zsh with the integration installed shows chips; other shells show nothing.
- Shells opened before the integration was updated have no `precmd` hook until restarted.
- Only nvm is understood for node. Other managers (fnm, volta, asdf) would need their own field.
- The command is typed into the history of the shell, as if the user had typed it.
