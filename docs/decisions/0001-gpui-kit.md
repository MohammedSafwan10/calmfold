# 0001 Use GPUI via gpui-kit for the desktop UI

Date: 2026-11-08 · Status: accepted

**Context.** We want a native, GPU-rendered, very fast desktop UI in Rust for
macOS and Windows, without a webview. crates.io `gpui` is stuck at 0.2.2
(Oct 2025); Zed paused community GPUI work in Feb 2026.

**Decision.** Depend on `gpui-kit` 0.9.0 only. It pins an exact `gpui-pre`
snapshot of Zed's GPUI and provides 75+ components (sidebar, settings,
command palette, dialogs, toasts, virtual lists, charts), theming, and a
headless UI test harness.

**Consequences.** Weekly-ish upstream breakage is absorbed by upgrading
gpui-kit deliberately. UI stays a thin layer over UI-free crates, so switching
frameworks (or adding mobile with another one) does not touch domain code.
