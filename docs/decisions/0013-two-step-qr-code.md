# 0013 QR code for two-step verification

Status: accepted (2026-11-17)

## Context

Turning on two-step verification shows the server's `otpauth://totp/…` link
as a QR code for a phone to scan. GPUI has no QR element. The code doesn't
touch `crates/crypto`: it only draws a link the server created.

## Decision

- Encode with `qrcode` 0.14 (kennytm/qrcode-rust, MIT OR Apache-2.0) with
  **default features off**: no `image` or `svg`, no dependencies, no
  `unsafe`. Error correction M.
- Draw it in `apps/desktop/src/views/qr.rs` with a GPUI `canvas`: one
  `paint_quad` per horizontal run of dark modules, whole-pixel modules, a
  4-module quiet zone. The tile is the lighter of the theme's background and
  foreground, the modules the darker one, so it is dark-on-light (what phone
  cameras expect) in both themes without a hard-coded color.
- The setup key is shown beside it, grouped in fours, for apps that can't scan.

Rejected: rendering a PNG/SVG and loading it through the image pipeline (more
features, a temporary image in memory, blurrier scaling); `fast_qr` or
`qrcodegen` (fine, but `qrcode` is the long-standing default with the smallest
surface once its features are off).

## Consequences

- One small new dependency in `ottero-desktop` only. Tests check the matrix
  (finder patterns, every dark module painted once) and that the dialog's QR
  code encodes the link it shows; scanning with a phone was not automated.
