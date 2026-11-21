//! `cargo xtask bench [--quick] [--compare]`.
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::Event;
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use mer_term::{GridSize, Session, ShellCommand, SpawnOptions};

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct Stats {
    median: f64,
    min: f64,
    max: f64,
}

/// Median/min/max of a non-empty sample set.
fn stats(samples: &[f64]) -> Option<Stats> {
    if samples.is_empty() {
        return None;
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let median = if n % 2 == 1 { v[n / 2] } else { f64::midpoint(v[n / 2 - 1], v[n / 2]) };
    Some(Stats { median, min: v[0], max: v[n - 1] })
}

#[allow(clippy::cast_precision_loss)]
fn mib_per_s(bytes: u64, seconds: f64) -> f64 {
    bytes as f64 / MIB as f64 / seconds
}

/// Deterministic mix of plain lines, SGR (256-color and truecolor) lines and Unicode.
pub(crate) fn generate_data(target: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(target + 1024);
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let words = [
        "alpha", "beta", "gamma", "delta", "epsilon", "terminal", "render", "grid", "cell",
        "buffer",
    ];
    let unicode =
        ["héllo wörld", "日本語のテキスト", "naïve café ☕", "→ ← ↑ ↓ ═══╗", "Ελληνικά кириллица"];
    let mut line_no = 0u64;
    while out.len() < target {
        line_no += 1;
        let r = next();
        match r % 4 {
            0 => {
                for _ in 0..(12 + r % 14) {
                    out.extend_from_slice(words[(next() % 10) as usize].as_bytes());
                    out.push(b' ');
                }
            }
            1 => {
                for _ in 0..(6 + r % 8) {
                    let c = next() % 256;
                    let _ = write!(&mut out, "\x1b[38;5;{c}m{}\x1b[0m ", words[(c % 10) as usize]);
                }
            }
            2 => {
                for _ in 0..(5 + r % 6) {
                    let v = next();
                    let _ = write!(
                        &mut out,
                        "\x1b[1;38;2;{};{};{}m{}\x1b[48;2;{};{};{}m  \x1b[0m",
                        v & 255,
                        (v >> 8) & 255,
                        (v >> 16) & 255,
                        words[((v >> 24) % 10) as usize],
                        (v >> 32) & 255,
                        (v >> 40) & 255,
                        (v >> 48) & 255
                    );
                }
            }
            _ => {
                let _ = write!(&mut out, "{line_no:08} ");
                for _ in 0..4 {
                    out.extend_from_slice(unicode[(next() % 5) as usize].as_bytes());
                    out.extend_from_slice(b" | ");
                }
            }
        }
        out.push(b'\n');
    }
    out
}

#[allow(clippy::cast_precision_loss)]
fn cpu_summary(cpuinfo: &str, meminfo: &str) -> String {
    let model = cpuinfo
        .lines()
        .find_map(|l| l.strip_prefix("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map_or("unknown CPU", str::trim);
    let threads = cpuinfo.lines().filter(|l| l.starts_with("processor")).count();
    let mem_kib: u64 = meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    format!("{model}, {threads} threads, {:.1} GiB RAM", mem_kib as f64 / 1024.0 / 1024.0)
}

/// utime + stime in clock ticks from the contents of `/proc/<pid>/stat`.
fn parse_cpu_ticks(stat: &str) -> Option<u64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // `rest` starts at field 3 (state), so utime/stime (fields 14/15) are at 11/12.
    Some(fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?)
}

/// `VmRSS` in KiB from the contents of `/proc/<pid>/status`.
fn parse_rss_kib(status: &str) -> Option<u64> {
    status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?.split_whitespace().next()?.parse().ok()
}

fn parse_first_frame(line: &str) -> Option<f64> {
    line.trim().strip_prefix("first-frame:")?.trim().strip_suffix("ms")?.trim().parse().ok()
}

fn scratch() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("mer-bench");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

// ---------- 1. headless throughput ----------

fn run_pipeline(file: &Path) -> Result<f64> {
    let size = GridSize { cols: 200, lines: 50, cell_width: 8, cell_height: 16 };
    let mut options = SpawnOptions::new(None, size, 10_000);
    options.shell = Some(ShellCommand {
        program: "sh".into(),
        args: vec!["-c".into(), format!("cat '{}'", file.display())],
    });
    let (session, mut events) = Session::spawn(options).context("spawning session")?;
    let start = Instant::now();
    let deadline = start + Duration::from_mins(5);
    let elapsed = futures::executor::block_on(async {
        loop {
            match next_event(&mut events, deadline).await {
                Some(Event::ChildExit(_) | Event::Exit) => return Some(start.elapsed()),
                Some(_) => {}
                None => return None,
            }
        }
    });
    drop(session);
    elapsed.map(|d| d.as_secs_f64()).context("pipeline run timed out or channel closed")
}

/// Next event, giving up (None) when the stream ends or the deadline passed.
async fn next_event(
    events: &mut futures::channel::mpsc::UnboundedReceiver<Event>,
    deadline: Instant,
) -> Option<Event> {
    if Instant::now() > deadline {
        return None;
    }
    events.next().await
}

fn bench_throughput(file: &Path, bytes: u64, runs: usize) -> Result<Stats> {
    let mut speeds = Vec::new();
    for i in 0..runs {
        let secs = run_pipeline(file)?;
        let speed = mib_per_s(bytes, secs);
        eprintln!("  pipeline run {}: {secs:.2} s ({speed:.0} MiB/s)", i + 1);
        speeds.push(speed);
    }
    stats(&speeds).context("no samples")
}

// ---------- 2. shell startup ----------

fn shell_startup(shell: &str) -> Option<Stats> {
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let status = Command::new(shell)
            .args(["-ic", "exit"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    stats(&times)
}

// ---------- 3. app metrics ----------

#[derive(Debug)]
struct AppMetrics {
    first_frame_ms: Option<f64>,
    idle_cpu_percent: Option<f64>,
    rss_mib: Option<f64>,
}

fn terminate(child: &mut Child) {
    let _ = Command::new("kill").args(["-TERM", &child.id().to_string()]).status();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[allow(clippy::cast_precision_loss)]
fn bench_app(binary: &Path) -> Result<AppMetrics> {
    // Isolated state and config, so measuring never touches the user's saved spaces.
    let isolated = scratch()?;
    let mut child = Command::new(binary)
        .env("MER_BENCH", "1")
        .env("XDG_STATE_HOME", isolated.join("state"))
        .env("XDG_CONFIG_HOME", isolated.join("config"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("launching mer")?;
    let stderr = child.stderr.take().context("no stderr")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(ms) = parse_first_frame(&line) {
                let _ = tx.send(ms);
                break;
            }
        }
    });
    let first_frame_ms = rx.recv_timeout(Duration::from_secs(10)).ok();
    let pid = child.id();
    let mut metrics = AppMetrics { first_frame_ms, idle_cpu_percent: None, rss_mib: None };
    if first_frame_ms.is_some() {
        std::thread::sleep(Duration::from_secs(1));
        let read_ticks = || {
            fs::read_to_string(format!("/proc/{pid}/stat")).ok().and_then(|s| parse_cpu_ticks(&s))
        };
        let before = read_ticks();
        let t0 = Instant::now();
        let mut rss = Vec::new();
        while t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(500));
            if let Some(kib) = fs::read_to_string(format!("/proc/{pid}/status"))
                .ok()
                .and_then(|s| parse_rss_kib(&s))
            {
                rss.push(kib as f64 / 1024.0);
            }
        }
        let after = read_ticks();
        if let (Some(a), Some(b)) = (before, after) {
            // CLK_TCK = 100.
            metrics.idle_cpu_percent =
                Some((b - a) as f64 / 100.0 / t0.elapsed().as_secs_f64() * 100.0);
        }
        metrics.rss_mib = rss.iter().copied().max_by(f64::total_cmp);
    }
    terminate(&mut child);
    Ok(metrics)
}

// ---------- 4. compare ----------

/// Waits for the child up to `timeout`, killing it (by its own pid) afterwards.
fn wait_timeout(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    terminate(child);
    false
}

fn parse_timing(text: &str) -> Option<f64> {
    let mut it = text.lines().filter_map(|l| l.trim().parse::<f64>().ok());
    let start = it.next()?;
    let end = it.next()?;
    (end > start).then_some(end - start)
}

fn compare_one(
    name: &str,
    program: &Path,
    lead_args: &[&str],
    data: &Path,
) -> Result<Option<Stats>> {
    let result = scratch()?.join(format!("cmp-{name}.txt"));
    let mut speeds = Vec::new();
    for i in 0..3 {
        let _ = fs::remove_file(&result);
        let script = format!(
            "date +%s.%N > '{r}'; cat '{d}'; date +%s.%N >> '{r}'",
            r = result.display(),
            d = data.display()
        );
        let mut child = Command::new(program)
            .args(lead_args)
            .args(["sh", "-c", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("launching {name}"))?;
        if !wait_timeout(&mut child, Duration::from_mins(2)) {
            eprintln!("  {name} run {} timed out (killed pid {})", i + 1, child.id());
            return Ok(None);
        }
        let Some(secs) = fs::read_to_string(&result).ok().and_then(|t| parse_timing(&t)) else {
            eprintln!("  {name} run {} produced no timing", i + 1);
            return Ok(None);
        };
        eprintln!("  {name} run {}: {secs:.2} s", i + 1);
        speeds.push(secs);
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(stats(&speeds))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

// ---------- report ----------

fn date_string() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".into(), |s| s.trim().to_owned())
}

#[allow(clippy::too_many_lines)]
pub fn run(args: &[String]) -> Result<()> {
    let mut quick = false;
    let mut compare = false;
    for a in args {
        match a.as_str() {
            "--quick" => quick = true,
            "--compare" => compare = true,
            other => bail!("unknown bench flag `{other}`"),
        }
    }
    let root = crate::workspace_root();
    let size_mib: u64 = if quick { 8 } else { 64 };
    let dir = scratch()?;
    let data_path = dir.join(format!("bench-data-{size_mib}.txt"));
    eprintln!("generating {size_mib} MiB of test data");
    let data = generate_data(usize::try_from(size_mib * MIB)?);
    fs::write(&data_path, &data)?;
    let bytes = data.len() as u64;
    drop(data);

    let mut report = String::new();
    let machine = cpu_summary(
        &fs::read_to_string("/proc/cpuinfo").unwrap_or_default(),
        &fs::read_to_string("/proc/meminfo").unwrap_or_default(),
    );
    writeln!(report, "# Performance\n")?;
    writeln!(report, "- Date: {}", date_string())?;
    writeln!(report, "- Machine: {machine}")?;
    writeln!(
        report,
        "- Mode: {} ({size_mib} MiB data set)\n",
        if quick { "quick" } else { "full" }
    )?;

    eprintln!("1) headless pipeline throughput");
    let t = bench_throughput(&data_path, bytes, 3)?;
    writeln!(report, "## Parser/pipeline throughput (headless)\n")?;
    writeln!(
        report,
        "`cat` of the data set through `mer_term::Session` (200x50 grid, 10k scrollback), 3 runs.\n"
    )?;
    writeln!(report, "| Median | Min | Max |\n|---|---|---|")?;
    writeln!(report, "| {:.0} MiB/s | {:.0} MiB/s | {:.0} MiB/s |\n", t.median, t.min, t.max)?;

    eprintln!("2) shell startup");
    writeln!(report, "## Shell startup (`-ic exit`, median of 5)\n")?;
    writeln!(report, "| Shell | Median | Min | Max |\n|---|---|---|---|")?;
    for shell in ["zsh", "bash"] {
        match shell_startup(shell) {
            Some(s) => writeln!(
                report,
                "| {shell} | {:.0} ms | {:.0} ms | {:.0} ms |",
                s.median, s.min, s.max
            )?,
            None => writeln!(report, "| {shell} | unavailable | | |")?,
        }
    }
    writeln!(report)?;

    // `MER_BIN` lets the same benchmark measure another build (e.g. a PGO one).
    let mer =
        std::env::var_os("MER_BIN").map_or_else(|| root.join("target/release/mer"), PathBuf::from);
    let shown = mer.strip_prefix(&root).unwrap_or(&mer).display().to_string();
    writeln!(report, "## App metrics (`{shown}`, `MER_BENCH=1`)\n")?;
    if mer.is_file() {
        eprintln!("3) app metrics (a window opens for ~15 s)");
        let m = bench_app(&mer)?;
        let fmt = |v: Option<f64>, unit: &str, prec: usize| {
            v.map_or_else(|| "unavailable".to_owned(), |v| format!("{v:.prec$} {unit}"))
        };
        writeln!(report, "| Metric | Value |\n|---|---|")?;
        writeln!(report, "| Time to first frame | {} |", fmt(m.first_frame_ms, "ms", 0))?;
        writeln!(report, "| Idle CPU (5 s) | {} |", fmt(m.idle_cpu_percent, "%", 1))?;
        writeln!(report, "| RSS (max while idle) | {} |\n", fmt(m.rss_mib, "MiB", 0))?;
    } else {
        writeln!(
            report,
            "Skipped: `target/release/mer` does not exist (run `cargo build --release -p mer`).\n"
        )?;
    }

    if compare {
        eprintln!("4) comparison against other terminals");
        writeln!(
            report,
            "## Comparison (`cat` of the data set inside a real window, 3 runs, median)\n"
        )?;
        writeln!(
            report,
            "| Terminal | Median time | Throughput | Min..Max time |\n|---|---|---|---|"
        )?;
        let mut notes = Vec::new();
        let mut targets: Vec<(&str, Option<PathBuf>, Vec<&str>)> = vec![
            ("kitty", find_on_path("kitty"), vec![]),
            (
                "ghostty",
                find_on_path("ghostty"),
                vec!["--confirm-close-surface=false", "--gtk-single-instance=false", "-e"],
            ),
        ];
        if !mer.is_file() {
            notes.push(
                "mer skipped: build it first with `cargo build --release -p mer`.".to_owned(),
            );
        }
        targets.push(("mer", mer.is_file().then(|| mer.clone()), vec!["-e"]));
        for (name, path, lead) in targets {
            let Some(path) = path else {
                if name != "mer" {
                    notes.push(format!("{name} not found on PATH, skipped."));
                }
                continue;
            };
            eprintln!(" {name}");
            if let Some(s) = compare_one(name, &path, &lead, &data_path)? {
                writeln!(
                    report,
                    "| {name} | {:.2} s | {:.0} MiB/s | {:.2}..{:.2} s |",
                    s.median,
                    mib_per_s(bytes, s.median),
                    s.min,
                    s.max
                )?;
            } else {
                writeln!(report, "| {name} | failed | | |")?;
                notes.push(format!("{name} timed out or produced no timing."));
            }
        }
        writeln!(report)?;
        for n in notes {
            writeln!(report, "- {n}")?;
        }
    }

    print!("{report}");
    let docs = root.join("docs");
    fs::create_dir_all(&docs)?;
    fs::write(docs.join("performance.md"), &report)?;
    eprintln!("wrote docs/performance.md");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_median_min_max() {
        let s = stats(&[3.0, 1.0, 2.0]).unwrap();
        assert!(
            (s.median - 2.0).abs() < f64::EPSILON
                && (s.min - 1.0).abs() < 1e-9
                && (s.max - 3.0).abs() < 1e-9
        );
        assert!((stats(&[1.0, 2.0, 3.0, 4.0]).unwrap().median - 2.5).abs() < f64::EPSILON);
        assert!(stats(&[]).is_none());
    }

    #[test]
    fn data_is_deterministic_and_sized() {
        let a = generate_data(10_000);
        assert_eq!(a, generate_data(10_000));
        assert!(a.len() >= 10_000);
        let text = String::from_utf8(a).unwrap();
        assert!(text.contains("\x1b[38;5;") && text.contains("\x1b[1;38;2;"));
        assert!(text.contains('é') || text.contains('日') || text.contains('☕'));
    }

    #[test]
    fn parses_proc_stat() {
        let stat = "123 (my (prog)) S 1 2 3 4 5 6 7 8 9 10 250 50 0 0 20";
        assert_eq!(parse_cpu_ticks(stat), Some(300));
        assert_eq!(parse_cpu_ticks("garbage"), None);
    }

    #[test]
    fn parses_status_and_first_frame() {
        assert_eq!(parse_rss_kib("Name:\tmer\nVmRSS:\t  2048 kB\n"), Some(2048));
        assert_eq!(parse_first_frame("first-frame: 123 ms"), Some(123.0));
        assert_eq!(parse_first_frame("other"), None);
    }

    #[test]
    fn parses_timing_and_machine() {
        assert!((parse_timing("10.5\n12.0\n").unwrap() - 1.5).abs() < 1e-9);
        assert!(parse_timing("10.5\n").is_none());
        let s = cpu_summary(
            "processor : 0\nprocessor : 1\nmodel name : Foo CPU\n",
            "MemTotal: 2097152 kB\n",
        );
        assert_eq!(s, "Foo CPU, 2 threads, 2.0 GiB RAM");
    }
}
