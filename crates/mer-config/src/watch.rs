//! File watching with debounce.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

const DEBOUNCE: Duration = Duration::from_millis(200);

/// Errors from [`watch`].
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("file watcher error: {0}")]
    Notify(#[from] notify::Error),
}

/// Keeps the watch alive; dropping it stops watching.
pub struct WatchHandle {
    _watchers: Vec<RecommendedWatcher>,
    stopped: Arc<AtomicBool>,
}

impl std::fmt::Debug for WatchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchHandle").finish_non_exhaustive()
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

/// Watch files and directories, calling `on_change` (debounced ~200 ms,
/// bursts coalesced) when something changes.
///
/// For a file path the parent directory is watched non-recursively (editors
/// replace files by rename) and only events for that file count. A directory
/// is watched non-recursively and any event inside it counts. Paths whose
/// directory does not exist are skipped silently.
pub fn watch(
    paths: &[PathBuf],
    on_change: impl Fn() + Send + 'static,
) -> Result<WatchHandle, WatchError> {
    let (tx, rx) = mpsc::channel::<()>();
    let stopped = Arc::new(AtomicBool::new(false));
    let mut watchers = Vec::new();

    for path in paths {
        let (dir, file): (PathBuf, Option<PathBuf>) = if path.is_dir() {
            (path.clone(), None)
        } else if let Some(parent) = path.parent().filter(|p| p.is_dir()) {
            (parent.to_path_buf(), Some(path.clone()))
        } else {
            tracing::debug!(path = %path.display(), "watch target missing, skipped");
            continue;
        };
        let tx = tx.clone();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res
                && !ev.kind.is_access()
                && relevant(&ev.paths, file.as_deref())
            {
                let _ = tx.send(());
            }
        })?;
        w.watch(&dir, RecursiveMode::NonRecursive)?;
        watchers.push(w);
    }
    drop(tx);

    let flag = Arc::clone(&stopped);
    std::thread::Builder::new()
        .name("mer-config-watch".into())
        .spawn(move || {
            // Ends when every watcher (and thus every sender) is dropped.
            while rx.recv().is_ok() {
                while rx.recv_timeout(DEBOUNCE).is_ok() {}
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                on_change();
            }
        })
        .map_err(notify::Error::io)?;

    Ok(WatchHandle { _watchers: watchers, stopped })
}

fn relevant(event_paths: &[PathBuf], file: Option<&Path>) -> bool {
    match file {
        None => true,
        Some(f) => event_paths.iter().any(|p| p == f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + 'static) {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = Arc::clone(&n);
        (n, move || {
            n2.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn wait_for(n: &AtomicUsize, at_least: usize) -> bool {
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            if n.load(Ordering::SeqCst) >= at_least {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    #[test]
    fn fires_after_write_and_ignores_siblings() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("config.toml");
        fs::write(&file, "a = 1").unwrap();
        let (n, cb) = counter();
        let _h = watch(std::slice::from_ref(&file), cb).unwrap();
        fs::write(d.path().join("other.txt"), "x").unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(n.load(Ordering::SeqCst), 0);
        fs::write(&file, "a = 2").unwrap();
        assert!(wait_for(&n, 1));
    }

    #[test]
    fn directory_watch_and_rename_replace() {
        let d = tempfile::tempdir().unwrap();
        let (n, cb) = counter();
        let _h = watch(&[d.path().to_path_buf()], cb).unwrap();
        fs::write(d.path().join("t.toml"), "x").unwrap();
        assert!(wait_for(&n, 1));
    }

    #[test]
    fn silent_after_drop_and_missing_dir_ok() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("config.toml");
        fs::write(&file, "a = 1").unwrap();
        let (n, cb) = counter();
        let h = watch(&[file.clone(), d.path().join("no/such/file")], cb).unwrap();
        drop(h);
        std::thread::sleep(Duration::from_millis(100));
        fs::write(&file, "a = 2").unwrap();
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(n.load(Ordering::SeqCst), 0);
    }
}
