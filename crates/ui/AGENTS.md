# calmfold-ui

The design system. Feature screens compose these pieces; they do not invent
their own colors, fonts, or feedback patterns.

## Contents

- `themes/calmfold.json`: Calmfold Dark (default) and Light. Monochrome;
  color is reserved for meaning (danger, success, warning). Only file allowed
  to contain hex colors.
- `fonts.rs`: bundled Inter + JetBrains Mono (OFL in `assets/fonts/*/OFL.txt`).
- `appearance.rs`: user appearance settings (theme mode, fonts, sizes,
  content width, reduce motion). Persisted to `appearance.json`, applied via
  `Theme::update`. Add new appearance options here, with `sanitized()` bounds
  and a load/save test.
- `assets.rs`: icon asset source. List every Lucide icon you use in `AppIcons`.
- `components.rs`: `segmented`, `page_header`, `empty_state`, `section_label`.
- `feedback.rs`: `confirm_destructive`, `undo_toast`, `error_toast`, `report`.

## Rules

- Read `.kiro/skills/gpui-kit-design-guides` before changing visuals.
- Prefer rem-based helpers (`p_4`, `gap_2`, `rems(..)`) over `px(..)` so the
  interface-size setting scales everything.
- A reusable piece belongs here only once two screens need it.
