# 16. App icon drawn in code, native frame coloured by DWM

- Status: accepted
- Date: 2026-10-25

## Context

The developer chose icon A (an amber focus ring on a dark tile) for the window and the exe, and a native frame that matches the app instead of Windows' default title bar.

## Decision

- The icon is drawn in code (`apps/focus-ui/src/icon.rs`, pure `std`). The window uses it at 64 px; `build.rs` includes the same file, writes a multi-size `.ico` and embeds it with `winresource` (the Windows SDK's `rc.exe`). No SVG rasteriser, and only one copy of the icon.
- The frame stays native. `apps/focus-ui/src/frame.rs` sets DWM's dark mode, caption, text and border colours from the theme, at startup and when the theme changes. It is Windows-only code, so it joins CLAUDE.md's `#[cfg]` list.
- A fully custom title bar (no decorations, drawn in iced) was rejected: we'd have to rebuild dragging, snap layouts and the window buttons.

## Consequences

- Building on Windows needs the Windows SDK (already needed to link).
- Windows 10 ignores the caption colours and keeps its own frame.
- The daemon's exe has no icon yet (it shows in Startup apps and Task Manager).
