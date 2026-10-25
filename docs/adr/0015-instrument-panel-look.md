# 15. Instrument-panel look, with a theme setting

- Status: accepted
- Date: 2026-10-24

## Context

Polish step P2 asks for a window that feels like a physical focus device. The developer picked a direction from HTML mock-ups: "field recorder" (warm aluminium keys with a solid ledge, dark LCD readouts, square switches, IBM Plex) for light mode and the "bakelite" warm-dark palette (amber accent) for dark mode. They also asked for the theme to be a setting rather than only following the system.

## Decision

- The look lives in `apps/focus-ui/src/style.rs`. Pages use its styles (`key`, `key_accent`, `key_danger`, `key_flat`, `card`, `readout`, `switch`, `dot`, ...) and never hard-code colours.
- Each palette is an iced `Palette` (so text inputs, pick lists and text styles derive warm colours) plus a `Look` with the colours iced doesn't derive: key faces, ledges, panels, readout glass.
- Keys: a top-to-bottom gradient face with a hard shadow under it as the ledge (3 px, 4 px on hover). Pressed keys lose the ledge and get a dark band at the top. iced has no inset shadows, so short gradients stand in for them, on keys, readouts and switch tracks. No motion or sound.
- Fonts: IBM Plex Sans (Regular, SemiBold) and Plex Mono (Medium) are bundled with `include_bytes!` (SIL Open Font License, `apps/focus-ui/fonts/OFL.txt`), so the window looks the same everywhere (about 580 KB).
- Theme setting: Settings → General gets System / Light / Dark, default System. It goes in `settings.toml` like other GUI settings (edited through `SettingsEditor`), so the core's editable settings carry it and the daemon reports it in the status. That touches core, `config-toml`, `ipc-proto` and the daemon, so it is its own sub-step (P2c).

## Consequences

- P2 is split: P2a shared style and fonts, P2b page touches (segmented strictness control, timer dial, section labels), P2c the theme setting.
- The timer dial is drawn with iced's `canvas` feature (pulls in `lyon` for tessellation on wgpu). Pages style text boxes, drop-downs, sliders and the scroll bar through `style.rs` functions, since a custom iced `Theme` can't change those widgets' defaults.
- A custom iced theme type (overriding every widget's default) was rejected as too much code; buttons name their key style explicitly instead.
- The bundled fonts make the binary bigger, and their licence has to ship with the release (step 10).
