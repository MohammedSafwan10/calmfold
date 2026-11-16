# Workspace customization and settings organization

## Scope

One feature catalog drives sidebar entries, More, ordering/visibility controls
and startup choices. New installations show Today, Notes, Tasks and Calendar;
existing installations preserve all prior entries and the old Developer toggle.
Newly added catalog features appear in More until chosen. Hiding is presentation
only: preserve data, open drafts, vault locking and timers; commands still work.

Settings: General (startup), Workspace (sidebar), Appearance, Features (existing
Notes/Today/Calendar preferences), Privacy & data (vault/storage), Keyboard,
About. Native settings search reaches feature preferences even when hidden.
No unimplemented feature pages, feature-disabling system or sync changes.

Sidebar preferences belong to layout.json, with the existing layout writer,
independent of appearance reset. Keep stable catalog IDs and tolerate unknown,
duplicate or missing stored IDs. Support direct up/down controls usable by
keyboard, visibility switches and reset with Undo. More and Settings remain
reachable if every feature is hidden. Customize sidebar opens Workspace directly.

## Checklist

- [x] Catalog metadata, preference rules, migrations/defaults and persistence
- [x] Ordered sidebar, More, customization command and startup preference
- [x] Native Workspace controls with up/down reorder and reset/Undo
- [x] Settings categories and globally searchable feature groups
- [x] Regression tests for migration, reorder/hide, all-hidden, drafts/timers,
      appearance reset, persistence, More and settings navigation
- [x] Full checks, light/dark visual review, documentation and single real app

Owner feedback: remove drag handles and the ambiguous reorder menu. Use direct
up/down buttons with stable keyboard focus. More belongs directly after the
visible features, only when something is hidden; no extra footer control or
empty customization-only More menu. The footer keeps its Settings button.
