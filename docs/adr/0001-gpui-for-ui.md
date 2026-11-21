# 0001. GPUI for the UI

Status: accepted

## Context
Speed is the top priority. The UI needs a sidebar, tabs, split panes and a settings panel, plus a custom terminal grid drawn on the GPU.

## Decision
Use GPUI (Zed's framework) on Vulkan/Wayland. Pin it to a fixed version in `[workspace.dependencies]` and only upgrade deliberately.

## Consequences
+ GPU text rendering and layout primitives out of the box.
- The API changes between releases and builds are heavy. Only `mer-ui` and `mer` may depend on it.
