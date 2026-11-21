# 0006. Public releases are generic; local installs stay native

Status: accepted. Amends the "only this machine matters" rule in `AGENTS.md`.

## Context
`mer` was built for one machine: `target-cpu=native`, Wayland only, installed with
`cargo xtask install`. The repository is public and other people want to try it, which needs
packages that run on other distros and other CPUs.

## Decision
- A tag `v*` (or a manual run) triggers `.github/workflows/release.yml`. It builds on
  Ubuntu 22.04 (older glibc, so newer distros run it) with
  `RUSTFLAGS="-C target-cpu=x86-64-v2"` and `--features x11`, makes a `.deb` (`cargo-deb`) and an
  `.rpm` (`cargo-generate-rpm`), and attaches them with SHA256 sums to the GitHub release.
- Releases use no PGO and a generic CPU level. The cost is a few percent in terminal painting
  and search; start-up, idle CPU and memory do not change.
- Local installs are unchanged: `.cargo/config.toml` keeps `target-cpu=native` and
  `cargo xtask install [--pgo]` never sets a generic target. Whoever builds from source
  chooses the target with `RUSTFLAGS` (documented in the README).
- X11 is a cargo feature (`x11`, off by default) so the development machine, which has no
  `-dev` packages, still builds the same way.
- Windows: `release-windows.yml` exists but runs only by hand and fails until the port is done
  (see `TODO.md`).

## Consequences
- The maintainer's machine remains the reference for performance numbers; the release binaries
  are measured by nobody until someone benchmarks them.
- X11 and non-Fedora distros are built but untested in real use.
- The release workflows have not been run yet; the first tag is their first test.
