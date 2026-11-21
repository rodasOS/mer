# 0002. alacritty_terminal as the terminal engine

Status: accepted

## Context
Writing a VT parser, grid and PTY layer from scratch is months of work and a source of subtle bugs.

## Decision
Use `alacritty_terminal` (same core as Zed), wrapped behind our own `TerminalBackend` trait in `mer-term`.

## Consequences
+ Mature, fast, damage tracking available.
- Tied to its release cadence. The trait lets us swap the engine later.
