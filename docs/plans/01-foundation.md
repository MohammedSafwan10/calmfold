# 01 Foundation

Goal: a fast, good-looking shell every later feature plugs into.

## Done

- [x] Cargo workspace, pinned toolchain, strict lints, `scripts/check.sh`
- [x] gpui-kit 0.9.0 (GPUI pre 0.3.11), optimized deps in debug
- [x] Ottero Dark/Light monochrome themes, Inter + JetBrains Mono bundled
- [x] Appearance settings persisted (mode, fonts, sizes, width, motion)
- [x] Shell: custom title bar, data-driven sidebar with child links + badges,
      collapse (animated, fixed toggle spot), drag-resize, persisted layout
- [x] Command palette (⌘K): commands with shortcut hints + note search
- [x] Actions + shortcuts + macOS menus, all routed through one handler
- [x] Settings screen (searchable): Appearance, General, Keyboard, About
- [x] Feedback system: confirm destructive, undo toast, error toast
- [x] Headless UI integration tests on the real app
- [x] AGENTS.md (root + per crate), HANDOFF.md, docs

## Open

- [ ] Window size/position restore
- [ ] User-editable keybindings (load from `keymap.json`)
- [ ] Custom user themes folder (`ThemeRegistry::watch_dir`)
- [ ] App icon
- [ ] CI on GitHub Actions (macOS + Windows) running `scripts/check.sh`
