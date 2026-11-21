use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::Processor;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

#[derive(Debug, thiserror::Error)]
pub enum TermError {
    #[error("failed to open the pty: {0}")]
    Pty(#[from] std::io::Error),
}

/// Terminal size in cells plus the pixel size of one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub lines: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        usize::from(self.lines)
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.lines)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

impl From<GridSize> for WindowSize {
    fn from(size: GridSize) -> Self {
        Self {
            num_lines: size.lines,
            num_cols: size.cols,
            cell_width: size.cell_width,
            cell_height: size.cell_height,
        }
    }
}

/// Forwards engine events to whoever owns the receiving end.
#[derive(Debug, Clone)]
pub struct EventProxy(UnboundedSender<Event>);

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        // The receiver only disappears when the view is gone; nothing to do then.
        let _ = self.0.unbounded_send(event);
    }
}

/// Program to run instead of the user's default shell.
#[derive(Debug, Clone)]
pub struct ShellCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SpawnOptions {
    pub cwd: Option<PathBuf>,
    pub size: GridSize,
    pub scrollback: usize,
    /// `None` runs the user's login shell.
    pub shell: Option<ShellCommand>,
    /// Extra environment variables for the child.
    pub env: Vec<(String, String)>,
    /// ANSI text drawn into the grid before the shell starts (a restored session).
    pub restore: Option<String>,
}

impl SpawnOptions {
    pub fn new(cwd: Option<PathBuf>, size: GridSize, scrollback: usize) -> Self {
        Self { cwd, size, scrollback, shell: None, env: Vec::new(), restore: None }
    }
}

/// A live shell attached to a PTY.
pub struct Session {
    term: Arc<FairMutex<Term<EventProxy>>>,
    sender: EventLoopSender,
    size: GridSize,
    pid: u32,
}

impl Session {
    pub fn spawn(options: SpawnOptions) -> Result<(Self, UnboundedReceiver<Event>), TermError> {
        let (tx, rx) = unbounded();
        let proxy = EventProxy(tx);

        let config = Config { scrolling_history: options.scrollback, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &options.size, proxy.clone())));

        if let Some(ansi) = &options.restore {
            // Drawn before the IO thread exists, so it cannot interleave with shell output.
            Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new()
                .advance(&mut *term.lock(), ansi.as_bytes());
        }

        let mut env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ]);
        env.extend(options.env);
        let shell = options.shell.map(|command| tty::Shell::new(command.program, command.args));
        let pty_options =
            tty::Options { shell, working_directory: options.cwd, env, ..tty::Options::default() };
        let pty = tty::new(&pty_options, options.size.into(), 0)?;
        let pid = pty.child().id();

        let event_loop = EventLoop::new(Arc::clone(&term), proxy, pty, false, false)?;
        let sender = event_loop.channel();
        // The IO thread stops on `Msg::Shutdown` (sent from `Drop`).
        drop(event_loop.spawn());

        Ok((Self { term, sender, size: options.size, pid }, rx))
    }

    pub fn term(&self) -> &Arc<FairMutex<Term<EventProxy>>> {
        &self.term
    }

    /// Process id of the shell.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Current working directory of the shell, read from `/proc`.
    pub fn cwd(&self) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{}/cwd", self.pid)).ok()
    }

    pub fn size(&self) -> GridSize {
        self.size
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    pub fn resize(&mut self, size: GridSize) {
        if size == self.size {
            return;
        }
        self.size = size;
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(size.into()));
    }

    pub fn scroll(&self, delta: i32) {
        self.term.lock().scroll_display(Scroll::Delta(delta));
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// The visible screen and scrollback as ANSI text, ready for [`SpawnOptions::restore`].
    pub fn snapshot_ansi(&self) -> String {
        crate::snapshot::snapshot_ansi(&self.term.lock())
    }

    /// Like [`Self::snapshot_ansi`], but detached from the session so several panes can be
    /// captured at the same time on different threads.
    pub fn snapshotter(&self) -> impl FnOnce() -> String + Send + 'static + use<> {
        let term = Arc::clone(&self.term);
        move || crate::snapshot::snapshot_ansi(&term.lock())
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("size", &self.size).finish_non_exhaustive()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_follows_the_shell_directory() {
        let dir = std::env::temp_dir().canonicalize().expect("temp dir exists");
        let size = GridSize { cols: 80, lines: 24, cell_width: 8, cell_height: 16 };
        let (session, _events) =
            Session::spawn(SpawnOptions::new(Some(dir.clone()), size, 100)).expect("shell starts");
        assert_eq!(session.cwd(), Some(dir));
    }

    #[test]
    fn spawning_a_session_is_fast() {
        let size = GridSize { cols: 80, lines: 24, cell_width: 8, cell_height: 16 };
        let mut worst = std::time::Duration::ZERO;
        for _ in 0..20 {
            let started = std::time::Instant::now();
            let (session, _events) =
                Session::spawn(SpawnOptions::new(None, size, 100)).expect("shell starts");
            worst = worst.max(started.elapsed());
            drop(session);
        }
        eprintln!("worst Session::spawn of 20: {worst:?}");
        // Opening a terminal must never freeze the UI thread noticeably.
        assert!(worst < std::time::Duration::from_millis(100), "spawn took {worst:?}");
    }
}
