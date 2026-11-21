# 0003. Single process

Status: accepted

## Context
Herdr uses a daemon plus a window. That adds IPC and complexity.

## Decision
The window owns the terminals. Closing the window saves every space as sleeping and restores it on the next start.

## Consequences
+ Fastest path from PTY to screen, simplest code.
- Running processes do not survive closing the window. Sleeping spaces only keep layout, cwd and scrollback.
