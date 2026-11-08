# Design system

Normative guides: `.kiro/skills/gpui-kit-design-guides` (read before UI work).
This file records Keepstone's choices on top of them.

## Principles

- Quiet, monochrome, precise. Black and white carry structure; color only
  carries meaning (danger red, success green, warning amber).
- Desktop conventions: persistent sidebar, keyboard first (every action has a
  command, most have a shortcut), command palette (⌘K / Ctrl K) reaches everything.
- One focal point per screen. Few borders, no nested cards, no decorative shadows.

## Theme

- `crates/ui/themes/keepstone.json`: **Keepstone Dark** (default via System on
  dark Macs) and **Keepstone Light**. Radius 8 / 12 (large), shadows off.
- Feature code uses tokens only: `background`, `foreground`, `muted_foreground`,
  `border`, `list_hover`, `list_active`, `group_box`, `sidebar_*`, `danger`...
- Adding a theme: add an entry to the JSON (or a new JSON in a user themes
  folder later) and expose it in Settings → Appearance.

## Appearance settings (user-controlled)

| Setting | Values | Effect |
| --- | --- | --- |
| Appearance | System / Light / Dark | Theme mode; System follows the OS live. |
| Content width | Compact / Comfortable / Wide | Max width of the note editor (38/48/72 rem). |
| Reduce motion | System / On / Off | Disables menu/dialog/toast animations. |
| Interface font | Inter / System + size 12–20 px (default 15) | Size is the `rem`, so it zooms the whole UI. |
| Code font | JetBrains Mono / System + size 10–20 px | Code in notes. |

Stored in `appearance.json`; `Restore default appearance` resets.

## Typography

Inter (UI) and JetBrains Mono (code), bundled, SIL OFL 1.1. Scale: `text_xs`
metadata, `text_sm` body/list, `text_lg` pane titles, `text_2xl` page titles.
Weights: Medium for row titles, Semibold for headings. No uppercase labels.

## Spacing and size

Use rem helpers (`p_4`, `gap_2`, `rems(48.)`), never raw `px` for layout, so
interface size scales everything. Exceptions: hairlines and the dialog width.

## Icons

Lucide (1,800+ icons, one consistent stroke style, ISC). Use
`gpui_kit::assets::IconName as Lucide`, and register each in
`crates/ui/src/assets.rs` (only listed icons are embedded, keeping the binary small).

## Sidebar

- Data-driven in `apps/desktop/src/nav.rs`: Section → Entry (icon, route,
  badge) → Child deep links (e.g. Tasks → Today/Upcoming/All/Done/Trash).
- Collapses to icons (⌘\ or the title-bar button at the same spot), animated.
- Resizable by dragging its edge (180–360 px); double-click the edge resets.
  Width and collapsed state persist.
- Settings lives in the footer.

## Feedback rules (`keepstone_ui::feedback`)

| Situation | Pattern |
| --- | --- |
| Reversible (move to trash, complete) | Do it now, `undo_toast("… moved to trash.")` |
| Permanent (delete forever, empty trash) | `confirm_destructive` with a verb button ("Delete", "Empty trash"), Cancel default, Escape cancels |
| Failure | `error_toast` with a user-safe message |

Never confirm reversible actions. Never use "OK" or "Yes" as the confirm label.

## Shared components (`crates/ui/src/components.rs`)

`segmented` (controlled segmented control), `page_header`, `empty_state`,
`section_label`. Promote a pattern here only when a second screen needs it.

## Copy

Sentence case. Short, specific, no exclamation marks. Empty states say what
the space is for and how to fill it.
