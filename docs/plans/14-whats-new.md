# What's new

Primary task: read changes across releases without leaving Calmfold.

- Open from Settings → About or the command palette.
- Use the entire Settings content area, with a quiet back button; retain the app
  sidebar and window chrome. No new permanent sidebar feature.
- A scrollable version rail selects one release at a time; narrow windows use a
  native dropdown. Version changes reset the reading scroll position.
- A readable centered column, newest version first, date and installed marker,
  short introduction, then New / Improved / Fixed sections with small emoji
  headers and hanging bullets. Avoid cards around every paragraph.
- One bundled structured JSON source drives the app and release catalog. Works
  offline, no WebView or HTML rendering, no network/loading dependency.
- Keyboard: native button activation, command palette, Escape/back returns to
  About. No automatic popup interrupting startup or work.
- Validate bundled data and cover open/back in both themes with a UI flow.
- Review native light/dark, small window and interface zoom.

References: Raycast changelog's grouped New/Improvements sections and version/date
hierarchy; Linear changelog's clear editorial reading column. Adapt to our native
shell rather than embedding their websites.

Status: implemented. Full quality gate passes, including data validation and
native light/dark open/back/Escape, version switching and narrow dropdown tests.
Native light/dark visual review completed; media playback remains future scope.

Media research: Linear uses screenshots and demo-video entries; Notion uses
video-led releases. Future media should be optional per feature: bundled native
images with captions first, user-started short demos with a poster and no
autoplay/audio. Avoid adding browser/video decoding dependencies merely for
release notes. Any native video implementation needs separate Mac/Windows
playback, accessibility and lifecycle tests; it is not implemented in this release.
