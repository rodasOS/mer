# gpui_wgpu (local copy)

A copy of the `gpui_wgpu` crate from GPUI, the UI framework of the Zed editor
(<https://github.com/zed-industries/zed>, crate `crates/gpui_wgpu`), taken at the revision `mer`
pins in the root `Cargo.toml` (`4a4c8150f03ef3c5e0c4f837dc0263c137f5b0ce`). It is wired in with a
`[patch]` section there. License: Apache-2.0, see `LICENSE-APACHE` (upstream states it in that file,
not in a header on each source file).

The only change, marked `mer patch` in the code, lives in `src/wgpu_context.rs` and
`src/wgpu_renderer.rs`: try Vulkan on its own before Vulkan plus OpenGL, so Mesa's OpenGL and LLVM
stack is not loaded. The reasoning and the numbers are in
[`docs/adr/0007-vulkan-first-gpu-startup.md`](../../docs/adr/0007-vulkan-first-gpu-startup.md).

When the GPUI pin changes, take a fresh copy from the new revision and re-apply the change.
