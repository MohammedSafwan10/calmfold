//! Consistent feedback for actions: confirmations, undo, and errors.
//!
//! Policy (see `docs/design-system.md`):
//! * Reversible actions (move to trash, complete, archive) run immediately and
//!   show an [`undo_toast`]. No confirmation dialog.
//! * Permanent actions (delete forever, empty trash, wipe data) always go
//!   through [`confirm_destructive`].
//! * Failures surface as an [`error_toast`], never silently.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant};
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::{App, SharedString, Window};

/// Text for a destructive confirmation. Name the object and the consequence.
pub struct Confirm {
    pub title: SharedString,
    pub description: SharedString,
    /// The verb on the button, e.g. "Delete" or "Empty trash". Never "OK".
    pub confirm_label: SharedString,
}

impl Confirm {
    #[must_use]
    pub fn new(
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            confirm_label: confirm_label.into(),
        }
    }
}

/// Asks before a permanent action. `on_confirm` runs only if the user confirms.
/// Cancel is the default-safe choice; Escape cancels.
pub fn confirm_destructive(
    confirm: Confirm,
    window: &mut Window,
    cx: &mut App,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_confirm = Rc::new(on_confirm);
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let on_confirm = on_confirm.clone();
        dialog
            .title(confirm.title.clone())
            .description(confirm.description.clone())
            .confirm()
            .ok_text(confirm.confirm_label.clone())
            .ok_variant(ButtonVariant::Danger)
            .cancel_text("Cancel")
            .on_ok(move |_, window, cx| {
                on_confirm(window, cx);
                true
            })
    });
}

/// Confirms a reversible action with an Undo button. Hides itself after a few seconds.
pub fn undo_toast(
    message: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
    on_undo: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_undo = Rc::new(on_undo);
    let notification = Notification::new()
        .message(message.into())
        .action(move |_, _, cx| {
            let on_undo = on_undo.clone();
            Button::new("undo")
                .outline()
                .xsmall()
                .label("Undo")
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_undo(window, cx);
                    this.dismiss(window, cx);
                }))
        })
        // `action` turns autohide off; an undo offer should still go away by itself.
        .autohide(true);
    window.push_notification(notification, cx);
}

/// Shows a failure. Messages come from domain/storage errors and are user-safe.
pub fn error_toast(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    window.push_notification(
        Notification::new()
            .with_type(NotificationType::Error)
            .message(message.into()),
        cx,
    );
}

/// Shows `result`'s error, if any, as an [`error_toast`].
pub fn report<T, E: std::fmt::Display>(
    result: Result<T, E>,
    window: &mut Window,
    cx: &mut App,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(err) => {
            error_toast(format!("{err:#}"), window, cx);
            None
        }
    }
}
