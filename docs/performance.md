# Performance

- Date: 2026-11-19 01:06
- Machine: AMD Ryzen 7 5700U with Radeon Graphics, 16 threads, 30.7 GiB RAM
- Mode: full (64 MiB data set)

## Parser/pipeline throughput (headless)

`cat` of the data set through `mer_term::Session` (200x50 grid, 10k scrollback), 3 runs.

| Median | Min | Max |
|---|---|---|
| 35 MiB/s | 34 MiB/s | 35 MiB/s |

## Shell startup (`-ic exit`, median of 5)

| Shell | Median | Min | Max |
|---|---|---|---|
| zsh | 201 ms | 200 ms | 229 ms |
| bash | 1430 ms | 1252 ms | 1854 ms |

## App metrics (`target/pgo/use/release/mer`, `MER_BENCH=1`)

| Metric | Value |
|---|---|
| Time to first frame | 300 ms |
| Idle CPU (5 s) | 0.0 % |
| RSS (max while idle) | 145 MiB |

## Comparison (`cat` of the data set inside a real window, 3 runs, median)

| Terminal | Median time | Throughput | Min..Max time |
|---|---|---|---|
| kitty | 1.80 s | 36 MiB/s | 1.79..1.81 s |
| ghostty | 2.08 s | 31 MiB/s | 2.07..2.20 s |
| mer | 1.67 s | 38 MiB/s | 1.65..1.68 s |

