use std::collections::HashSet;
use std::io::{self, Read};
use std::path::PathBuf;

use mer_core::PaneId;

const COMPRESSION_LEVEL: i32 = 3;
const EXTENSION: &str = "zst";
/// A history that decompresses to more than this is treated as corrupt, so a damaged or
/// hostile file cannot exhaust memory.
const MAX_HISTORY_BYTES: u64 = 1 << 30;

/// One `zstd` file per pane holding its ANSI history.
#[derive(Debug, Clone)]
pub struct Scrollback {
    dir: PathBuf,
}

impl Scrollback {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, pane: PaneId) -> PathBuf {
        self.dir.join(format!("{}.{EXTENSION}", pane.as_uuid()))
    }

    /// Writes through a temporary file so a crash never leaves a half-written history.
    pub fn write(&self, pane: PaneId, ansi: &str) -> io::Result<()> {
        crate::private::create_dir(&self.dir)?;
        let compressed = zstd::encode_all(ansi.as_bytes(), COMPRESSION_LEVEL)?;
        let target = self.path(pane);
        let temporary = target.with_extension("tmp");
        crate::private::write(&temporary, &compressed)?;
        std::fs::rename(temporary, target)
    }

    /// `None` if the pane has no saved history or it cannot be read.
    pub fn read(&self, pane: PaneId) -> Option<String> {
        let bytes = std::fs::read(self.path(pane)).ok()?;
        let decoded = decode_capped(&bytes, MAX_HISTORY_BYTES)?;
        String::from_utf8(decoded).ok()
    }

    pub fn remove(&self, pane: PaneId) {
        let _ = std::fs::remove_file(self.path(pane));
    }

    /// Deletes the history of every pane not in `keep`.
    pub fn prune(&self, keep: &HashSet<PaneId>) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_history = path.extension().is_some_and(|e| e == EXTENSION || e == "tmp");
            let keep_it = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<uuid::Uuid>().ok())
                .is_some_and(|uuid| keep.iter().any(|pane| pane.as_uuid() == uuid))
                && path.extension().is_some_and(|e| e == EXTENSION);
            if is_history && !keep_it {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Decompresses `bytes`, or `None` if they are not valid `zstd` or expand past `cap` bytes.
fn decode_capped(bytes: &[u8], cap: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    zstd::Decoder::new(bytes).ok()?.take(cap + 1).read_to_end(&mut out).ok()?;
    u64::try_from(out.len()).is_ok_and(|len| len <= cap).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_round_trips_and_is_compressed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Scrollback::new(dir.path().join("sb"));
        let pane = PaneId::new();
        let text = "line of output\r\n".repeat(5_000);

        store.write(pane, &text).expect("writes");
        assert_eq!(store.read(pane).as_deref(), Some(text.as_str()));

        let size = std::fs::metadata(store.path(pane)).expect("exists").len();
        assert!(size < text.len() as u64 / 20, "expected strong compression, got {size} bytes");
    }

    #[test]
    fn missing_history_reads_as_none() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(Scrollback::new(dir.path()).read(PaneId::new()), None);
    }

    #[test]
    fn prune_keeps_only_the_requested_panes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Scrollback::new(dir.path());
        let (kept, dropped) = (PaneId::new(), PaneId::new());
        store.write(kept, "keep").expect("writes");
        store.write(dropped, "drop").expect("writes");

        store.prune(&HashSet::from([kept]));
        assert_eq!(store.read(kept).as_deref(), Some("keep"));
        assert_eq!(store.read(dropped), None);
    }

    #[test]
    fn remove_deletes_one_history() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Scrollback::new(dir.path());
        let pane = PaneId::new();
        store.write(pane, "x").expect("writes");
        store.remove(pane);
        assert_eq!(store.read(pane), None);
    }

    #[test]
    fn history_files_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir");
        let store = Scrollback::new(root.path().join("sb"));
        let pane = PaneId::new();
        store.write(pane, "secret output").expect("writes");

        let mode = |p: &std::path::Path| {
            std::fs::metadata(p).expect("exists").permissions().mode() & 0o777
        };
        assert_eq!(mode(&root.path().join("sb")), 0o700);
        assert_eq!(mode(&store.path(pane)), 0o600);
    }

    #[test]
    fn a_history_that_expands_past_the_cap_is_rejected() {
        let bomb = zstd::encode_all("a".repeat(10_000).as_bytes(), 3).expect("compresses");
        assert_eq!(decode_capped(&bomb, 10_000).map(|v| v.len()), Some(10_000));
        assert_eq!(decode_capped(&bomb, 9_999), None);
        assert_eq!(decode_capped(b"not zstd at all", 1_000), None);
    }
}
