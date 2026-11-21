use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use mer_core::{PaneId, WorkspaceData};

use crate::{Scrollback, StateDir, Store, StoreError, UiState};

enum Message {
    Workspace(Box<WorkspaceData>),
    Scrollback { pane: PaneId, ansi: String },
    Forget(PaneId),
    Ui(UiState),
    Close,
}

/// Saves state on a background thread. Only the latest workspace snapshot is written when
/// several arrive in a burst, so a flurry of UI changes costs one transaction.
#[derive(Debug)]
pub struct Writer {
    sender: Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    pub fn spawn(dir: &StateDir) -> Result<Self, StoreError> {
        Self::with_store(Store::open(&dir.database())?, dir)
    }

    /// Like [`Self::spawn`], reusing a database that is already open (and migrated).
    pub fn with_store(store: Store, dir: &StateDir) -> Result<Self, StoreError> {
        let scrollback = Scrollback::new(dir.scrollback_dir());
        let (sender, receiver) = channel();
        let thread = std::thread::Builder::new()
            .name("mer-store".to_owned())
            .spawn(move || run(store, &scrollback, &receiver))?;
        Ok(Self { sender, thread: Some(thread) })
    }

    pub fn save(&self, data: WorkspaceData) {
        let _ = self.sender.send(Message::Workspace(Box::new(data)));
    }

    pub fn save_scrollback(&self, pane: PaneId, ansi: String) {
        let _ = self.sender.send(Message::Scrollback { pane, ansi });
    }

    pub fn save_ui(&self, state: UiState) {
        let _ = self.sender.send(Message::Ui(state));
    }

    /// Deletes the saved history of a pane that no longer exists.
    pub fn forget(&self, pane: PaneId) {
        let _ = self.sender.send(Message::Forget(pane));
    }

    /// Waits until everything queued so far is on disk.
    pub fn close(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        let _ = self.sender.send(Message::Close);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.finish();
    }
}

fn run(mut store: Store, scrollback: &Scrollback, receiver: &Receiver<Message>) {
    let mut closing = false;
    while !closing {
        let Ok(first) = receiver.recv() else { return };
        let mut latest: Option<Box<WorkspaceData>> = None;
        let mut ui: Option<UiState> = None;
        let mut batch = vec![first];
        batch.extend(receiver.try_iter());

        for message in batch {
            match message {
                Message::Workspace(data) => latest = Some(data),
                Message::Scrollback { pane, ansi } => {
                    if let Err(error) = scrollback.write(pane, &ansi) {
                        tracing::warn!(%error, "could not save scrollback");
                    }
                }
                Message::Forget(pane) => scrollback.remove(pane),
                Message::Ui(state) => ui = Some(state),
                Message::Close => closing = true,
            }
        }
        if let Some(data) = latest
            && let Err(error) = store.save(&data)
        {
            tracing::warn!(%error, "could not save workspace");
        }
        if let Some(state) = ui
            && let Err(error) = store.save_ui(&state)
        {
            tracing::warn!(%error, "could not save interface state");
        }
    }
}

#[cfg(test)]
mod tests {
    use mer_core::Workspace;

    use super::*;

    #[test]
    fn queued_state_is_on_disk_after_close() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = StateDir::new(temp.path());
        let workspace = Workspace::new(None);
        let pane = workspace.focused_pane();

        let writer = Writer::spawn(&dir).expect("spawns");
        writer.save(Workspace::new(None).to_data());
        writer.save(workspace.to_data());
        writer.save_scrollback(pane, "hello\r\n".to_owned());
        writer.close();

        let loaded = Store::open(&dir.database()).expect("opens").load().expect("loads");
        assert_eq!(loaded, Some(workspace.to_data()), "only the latest snapshot wins");
        assert_eq!(Scrollback::new(dir.scrollback_dir()).read(pane).as_deref(), Some("hello\r\n"));
    }

    #[test]
    fn forgetting_a_pane_removes_its_history() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = StateDir::new(temp.path());
        let pane = PaneId::new();

        let writer = Writer::spawn(&dir).expect("spawns");
        writer.save_scrollback(pane, "x".to_owned());
        writer.forget(pane);
        writer.close();

        assert_eq!(Scrollback::new(dir.scrollback_dir()).read(pane), None);
    }
}
