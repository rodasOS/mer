# 0005. The workspace view outlives its window

Status: accepted. Amends [0003](0003-single-process.md).

## Context
ADR 0003 made the window the owner of the terminals, so closing it killed every running
process. Wanting terminals to keep working after the window is closed (as Herdr does with its
daemon) is a reasonable request, but a daemon plus a client is the complexity 0003 avoided.

## Decision
`WorkspaceView` (the model, the live terminals, the agent state and the IPC server) is created
once, before any window, and kept in a global (`MainView`). Windows are attached to it and
detached from it:

- `attach(window)` registers what is tied to a window (size and activation observers) and asks
  for focus. `window_closed()` drops those and leaves everything else running.
- Nothing in the view takes a `Window` any more. Focus is *requested* (`want_focus`) and applied
  the next time a window draws, because focusing needs a window and there may be none.
- Terminal tasks and the config, auto-sleep and IPC tasks are spawned on the entity, not on a
  window, so they survive it.
- GPUI is told `QuitMode::Explicit`; `on_window_closed` quits unless
  `[spaces] keep_running_on_close` is on (and the instance is not a throwaway `mer -e`).
- Requests that show something (`mer`, `ctl open`, `ctl show`…) open the window first; hooks
  and queries are answered with no window, so agent notifications keep working.
- `mer quit` / `Ctrl+Shift+Q` is the way out. If every shell exits while there is no window,
  the app quits by itself.

The option is off by default: closing the window closes mer, as before.

## Consequences
+ Still one process and no new protocol; terminals keep their live state, not just a snapshot.
+ Idle cost with no window is zero CPU; memory drops only a little (the window's GPU
  resources), because the terminals and their history stay. Measured with the PGO build and
  one empty terminal: 152 MiB with the window open, 146 MiB closed, 0 CPU ticks over 5 s in
  both cases.
- If `mer` crashes, every terminal dies with it. A daemon would not have that weakness. This
  is the trade accepted for staying single-process.
- With no tray icon, a background `mer` is invisible until it is brought back with `mer`.
- Running `mer` while a window is already open does not raise it: Wayland gives an app no way
  to take the focus.

## Rejected
A tmux-style split between a daemon that owns the PTYs and a UI client. More resilient, but a
rewrite of the central path and a second process to supervise.
