# 0004. SQLite and zstd for persistence

Status: accepted

## Context
Sleeping spaces must record how many terminals they have and where, plus their visible history.

## Decision
SQLite (`rusqlite`, bundled, versioned migrations) for spaces, panes and agent state. Scrollback goes to one zstd file per pane under `$XDG_STATE_HOME/mer/scrollback/`.

## Consequences
+ Transactional, queryable, small on disk.
- Writes must happen off the UI thread.
