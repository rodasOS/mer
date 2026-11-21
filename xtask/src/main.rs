//! Project tasks: `cargo xtask <task>`.
mod bench;
mod ci;
mod install;
mod pgo;

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let task = args.next();
    let rest: Vec<String> = args.collect();
    match task.as_deref() {
        Some("install") => install::install(rest.iter().any(|arg| arg == "--pgo")),
        Some("pgo") => pgo::run(),
        Some("uninstall") => install::uninstall(),
        Some("bench") => bench::run(&rest),
        Some("ci") => ci::run(),
        None | Some("help" | "--help" | "-h") => {
            println!(
                "tasks:\n  install [--pgo]      install mer for the current user (--pgo uses the build from `pgo`)\n  pgo                  build a profile-guided optimized mer (takes ~25 min)\n  uninstall            remove the installed files\n  ci                   run fmt, clippy and tests, like CI does\n  bench [--quick] [--compare]   run benchmarks, write docs/performance.md"
            );
            Ok(())
        }
        Some(other) => bail!("unknown task `{other}` (try `cargo xtask help`)"),
    }
}

/// Workspace root (the parent of the `xtask` crate directory).
pub fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| std::path::PathBuf::from("."), std::path::Path::to_path_buf)
}
