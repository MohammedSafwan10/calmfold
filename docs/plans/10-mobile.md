# 10 Mobile

Reuse `core`, `storage`, `crypto` unchanged. Options, decide when we get here:

- GPUI mobile forks (mesa-hills-research/gpui, Glass-HQ/gpui): same UI code style, experimental.
- Native SwiftUI / Kotlin UI over a UniFFI bridge to the Rust crates: most native feel, two UIs.
- Slint: official Android/iOS support, one more UI framework.

Requires sync (plan 06) first so phone and desktop share data.
