# 0007. Vulkan first, OpenGL only as a fallback

Status: accepted

## Context
GPUI creates its wgpu instance with `VULKAN | GL`. Even when Vulkan is the backend that ends up
used, the GL backend makes Mesa load EGL, gallium and LLVM: about 25 MB more RSS in an idle
terminal. Environment variables cannot fix it: an empty `__EGL_VENDOR_LIBRARY_FILENAMES` leaks
into the shells and breaks OpenGL programs started from mer.

## Decision
Keep a local copy of `gpui_wgpu` in `vendor/gpui_wgpu`, wired with `[patch]` in the root
`Cargo.toml`. The only change (marked `mer patch`): the first window tries a Vulkan-only instance
that must find a real GPU (`new_rejecting_software`); if that fails, it creates the original
`VULKAN | GL` instance, so machines without Vulkan or with only a software adapter behave as before.

The process also turns off transparent huge pages for its heap (`crates/mer/src/main.rs`), which
saved about 35 MB.

## Consequences
+ About 60 MB less RSS in an idle window, no change in speed.
- The copy must be refreshed whenever the GPUI pin changes (diff `vendor/gpui_wgpu` against the
  new revision and re-apply the `mer patch` hunks). Drop it if GPUI makes the backends configurable.
- Processes started from mer inherit the "THP disabled" flag.
