mod cli;

use std::process::ExitCode;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> ExitCode {
    disable_transparent_huge_pages();
    cli::run()
}

/// Keeps the heap in 4 KiB pages.
///
/// mimalloc reserves one big arena and, with transparent huge pages, every 2 MiB chunk it touches
/// becomes resident in full: about 60 MB more RSS in an idle terminal. This is best effort, so a
/// kernel without the prctl is simply ignored. The flag is inherited by child processes.
fn disable_transparent_huge_pages() {
    let _ = rustix::thread::disable_transparent_huge_pages(true);
}
