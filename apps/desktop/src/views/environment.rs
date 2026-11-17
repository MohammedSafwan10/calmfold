//! Shared masked environment editor used by Passwords and Developer.
//! Draft generations invalidate late native-dialog results; locking destroys
//! all editor/preview state. Plaintext export only reads the saved vault item.
use super::vault_item::copy_field;
use crate::actions::{
    CopyProjectEnvironment, ExportProjectEnvironment, ImportProjectEnvironment,
    PasteProjectEnvironment,
};
use crate::library::{Library, LibraryEvent};
use anyhow::{Result, bail};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollStrategy, StatefulInteractiveElement as _, Styled as _,
    Subscription, TestSupportExt as _, Window, div, rems,
};
use ottero_core::vault::environment::{EnvVariable, EnvironmentSecrets, MAX_ENV_VARIABLES};
use ottero_core::{ProjectId, VaultItemId};
use ottero_crypto::Zeroizing;
use ottero_ui::feedback::{self, Confirm};
use std::{collections::HashSet, ops::Range, rc::Rc};

struct VariableRow {
    seq: u64,
    key: Entity<InputState>,
    value: Entity<InputState>,
}
pub struct EnvironmentEditor {
    library: Entity<Library>,
    project: Option<ProjectId>,
    record: Option<VaultItemId>,
    environment: Entity<InputState>,
    query: Entity<InputState>,
    rows: Vec<VariableRow>,
    visible: Vec<usize>,
    revealed: HashSet<u64>,
    pending: Option<EnvironmentSecrets>,
    error: Option<String>,
    editing: bool,
    active: bool,
    exporting: bool,
    generation: u64,
    sequence: u64,
    scroll: VirtualListScrollHandle,
    _subs: Vec<Subscription>,
}
impl EnvironmentEditor {
    pub fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let environment = cx
            .new(|cx| InputState::new(window, cx).placeholder("Development, staging, production…"));
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find a variable by name"));
        let subs = vec![
            cx.subscribe_in(&library, window, |this, _, event, window, cx| {
                if *event == LibraryEvent::VaultChanged
                    && !this.library.read(cx).vault_is_unlocked()
                {
                    this.clear(window, cx);
                }
            }),
            cx.subscribe(&query, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.filter(cx);
                }
            }),
        ];
        Self {
            library,
            project: None,
            record: None,
            environment,
            query,
            rows: vec![],
            visible: vec![],
            revealed: HashSet::new(),
            pending: None,
            error: None,
            editing: false,
            active: false,
            exporting: false,
            generation: 0,
            sequence: 0,
            scroll: VirtualListScrollHandle::new(),
            _subs: subs,
        }
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        for row in self.rows.drain(..) {
            row.key.update(cx, |i, cx| i.set_value("", window, cx));
            row.value.update(cx, |i, cx| i.set_value("", window, cx));
        }
        self.environment
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.query.update(cx, |i, cx| i.set_value("", window, cx));
        self.visible.clear();
        self.revealed.clear();
        self.pending = None;
        self.error = None;
        self.project = None;
        self.record = None;
        self.editing = false;
        self.active = false;
        self.exporting = false;
        cx.notify();
    }
    pub fn fill(
        &mut self,
        env: Option<&EnvironmentSecrets>,
        record: Option<VaultItemId>,
        editing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear(window, cx);
        self.editing = editing;
        self.active = env.is_some();
        self.record = record;
        if let Some(env) = env {
            self.project = env.project_id();
            self.environment.update(cx, |i, cx| {
                i.set_value(env.environment().to_owned(), window, cx)
            });
            for variable in env.variables() {
                self.push_row(variable.key(), variable.value(), window, cx);
            }
        }
        self.filter(cx);
    }
    fn push_row(&mut self, key: &str, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.sequence = self.sequence.wrapping_add(1);
        let key = cx.new(|cx| {
            let mut i = InputState::new(window, cx).placeholder("VARIABLE_NAME");
            i.set_value(key.to_owned(), window, cx);
            i
        });
        let value = cx.new(|cx| {
            let mut i = InputState::new(window, cx)
                .masked(true)
                .placeholder("Empty value");
            i.set_value(escaped_value(value).to_string(), window, cx);
            i
        });
        self.rows.push(VariableRow {
            seq: self.sequence,
            key,
            value,
        });
    }
    fn filter(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().to_lowercase();
        self.visible = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(ix, row)| {
                row.key
                    .read(cx)
                    .value()
                    .to_lowercase()
                    .contains(&query)
                    .then_some(ix)
            })
            .collect();
        cx.notify();
    }
    pub fn document(&self, cx: &App) -> Result<EnvironmentSecrets> {
        if !self.active {
            bail!("Open a project secrets item first.");
        }
        if !self.library.read(cx).vault_is_unlocked() {
            bail!("Unlock Passwords first.");
        }
        if self.pending.is_some() {
            bail!("Apply or cancel the import preview before saving.");
        }
        let variables = self
            .rows
            .iter()
            .map(|row| {
                EnvVariable::new(
                    row.key.read(cx).value().trim(),
                    &literal_value(&row.value.read(cx).value()),
                )
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(EnvironmentSecrets::new(
            self.project,
            &self.environment.read(cx).value(),
            variables,
        )?)
    }
    pub fn is_active(&self) -> bool {
        self.active
    }
    #[doc(hidden)]
    pub fn draft_is_empty(&self, cx: &App) -> bool {
        self.rows.is_empty()
            && self.pending.is_none()
            && self.environment.read(cx).value().is_empty()
            && self.query.read(cx).value().is_empty()
    }
    pub fn preview(&mut self, text: &str, cx: &mut Context<Self>) {
        if !self.editing || !self.library.read(cx).vault_is_unlocked() {
            return;
        }
        match EnvironmentSecrets::parse(text) {
            Ok(doc) => {
                self.pending = Some(doc);
                self.error = None;
            }
            Err(err) => {
                self.error = Some(err.to_string());
            }
        }
        cx.notify();
    }
    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if !self.editing || !self.library.read(cx).vault_is_unlocked() {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            let text = Zeroizing::new(text);
            self.preview(&text, cx);
        } else {
            self.error = Some("The clipboard doesn't contain text.".into());
            cx.notify();
        }
    }
    pub fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editing || !self.library.read(cx).vault_is_unlocked() {
            return;
        }
        let generation = self.generation;
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import .env".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { Library::read_environment_file(&path) })
                .await;
            _ = this.update_in(cx, |this, _, cx| {
                if generation != this.generation
                    || !this.editing
                    || !this.library.read(cx).vault_is_unlocked()
                {
                    return;
                }
                match result {
                    Ok(text) => this.preview(&text, cx),
                    Err(_) => {
                        this.error = Some(
                            "Couldn't import. Choose a UTF-8 .env file no larger than 1 MiB."
                                .into(),
                        );
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
    pub fn apply_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editing || !self.library.read(cx).vault_is_unlocked() {
            return;
        }
        let Some(doc) = self.pending.take() else {
            return;
        };
        // Preserve association and environment metadata, replacing variables only.
        for row in self.rows.drain(..) {
            row.key.update(cx, |i, cx| i.set_value("", window, cx));
            row.value.update(cx, |i, cx| i.set_value("", window, cx));
        }
        self.revealed.clear();
        for variable in doc.variables() {
            self.push_row(variable.key(), variable.value(), window, cx);
        }
        self.filter(cx);
    }
    pub fn copy_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.document(cx).and_then(|d| Ok(d.to_env()?)) {
            Ok(text) => {
                copy_field("Environment variables", text.to_string(), true, window, cx);
                if let Some(id) = self.record {
                    self.library.update(cx, |l, cx| l.mark_vault_used(id, cx));
                }
            }
            Err(err) => {
                self.error = Some(err.to_string());
                cx.notify();
            }
        }
    }
    pub fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.exporting {
            return;
        }
        let Some(id) = self
            .record
            .filter(|_| !self.editing && self.library.read(cx).vault_is_unlocked())
        else {
            return;
        };
        let view = cx.entity().downgrade();
        let generation = self.generation;
        feedback::confirm_destructive(
            Confirm::new(
                "Export plaintext secrets?",
                "The exported .env file is not encrypted. Keep it out of Git and shared folders. Existing files will not be overwritten.",
                "Choose location…",
            ),
            window,
            cx,
            move |window, cx| {
                let view = view.clone();
                let directory = directories::UserDirs::new()
                    .map(|d| d.home_dir().to_path_buf())
                    .unwrap_or_else(std::env::temp_dir);
                let paths = cx.prompt_for_new_path(&directory, Some(".env.local"));
                window
                    .spawn(cx, async move |cx| {
                        let Ok(Ok(Some(path))) = paths.await else {
                            return;
                        };
                        let Ok(Some(payload)) = view.update_in(cx, |this, _, cx| {
                            if this.generation != generation
                                || this.editing
                                || this.exporting
                                || !this.library.read(cx).vault_is_unlocked()
                            {
                                return None;
                            }
                            this.exporting = true;
                            cx.notify();
                            Some(this.library.read(cx).environment_export_text(id))
                        }) else {
                            return;
                        };
                        let result = match payload {
                            Ok(text) => cx
                                .background_executor()
                                .spawn(async move { Library::write_environment_file(&path, &text) })
                                .await,
                            Err(error) => Err(error),
                        };
                        _ = view.update_in(cx, |this, window, cx| {
                            if this.generation != generation {
                                return;
                            }
                            this.exporting = false;
                            cx.notify();
                            match result {
                                Err(error) => feedback::error_toast(error.to_string(), window, cx),
                                Ok(()) => gpui_kit::component::WindowExt::push_notification(
                                    window,
                                    "Environment file exported. It is not encrypted.",
                                    cx,
                                ),
                            }
                        });
                    })
                    .detach();
            },
        );
    }
    fn row_height(&self, index: usize, cx: &App) -> f32 {
        let row = &self.rows[index];
        if !self.editing && self.revealed.contains(&row.seq) {
            let value = literal_value(&row.value.read(cx).value());
            (value.lines().count().min(4) as f32 * 1.25 + 1.).max(3.)
        } else {
            3.
        }
    }
    fn row(&mut self, index: usize, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let row = &self.rows[index];
        let seq = row.seq;
        let key = row.key.clone();
        let value = row.value.clone();
        let library = self.library.clone();
        let record = self.record;
        let title = key.read(cx).value();
        let revealed = self.revealed.contains(&seq);
        let editing = self.editing;
        h_flex()
            .id(("env-row", seq))
            .w_full()
            .h_full()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().w(rems(12.)).min_w_0().map(|s| {
                if editing {
                    s.child(Input::new(&key).id(("env-key", seq)).small())
                } else {
                    s.child(div().truncate().text_sm().child(title))
                }
            }))
            .child(div().flex_1().min_w_0().map(|s| {
                if editing {
                    let paste_value = value.clone();
                    s.child(
                        Input::new(&value)
                            .id(("env-value", seq))
                            .small()
                            .mask_toggle()
                            .on_paste(move |clipboard, window, cx| {
                                if let Some(text) = clipboard.text() {
                                    let text = Zeroizing::new(text);
                                    let escaped = escaped_value(&text);
                                    paste_value.update(cx, |i, cx| {
                                        i.replace(escaped.to_string(), window, cx)
                                    });
                                    true
                                } else {
                                    false
                                }
                            }),
                    )
                } else {
                    if revealed {
                        s.child(
                            div()
                                .id(("env-shown", seq))
                                .text_sm()
                                .max_h(rems(5.))
                                .overflow_scroll()
                                .child(Label::new(
                                    literal_value(&value.read(cx).value()).to_string(),
                                )),
                        )
                    } else {
                        s.child(div().truncate().text_sm().child("••••••••"))
                    }
                }
            }))
            .when(!editing, |s| {
                s.child(
                    Button::new(("env-reveal", seq))
                        .ghost()
                        .small()
                        .icon(if revealed {
                            Lucide::EyeOff
                        } else {
                            Lucide::Eye
                        })
                        .accessibility_label(if revealed {
                            "Hide value"
                        } else {
                            "Reveal value"
                        })
                        .tooltip(if revealed {
                            "Hide value"
                        } else {
                            "Reveal value"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.revealed.insert(seq) {
                                this.revealed.remove(&seq);
                            }
                            cx.notify();
                        })),
                )
            })
            .child(
                Button::new(("env-copy", seq))
                    .ghost()
                    .small()
                    .icon(Lucide::Copy)
                    .tooltip("Copy value")
                    .on_click(move |_, window, cx| {
                        if !library.read(cx).vault_is_unlocked() {
                            return;
                        }
                        copy_field(
                            "Value",
                            literal_value(&value.read(cx).value()).to_string(),
                            true,
                            window,
                            cx,
                        );
                        if let Some(id) = record {
                            library.update(cx, |l, cx| l.mark_vault_used(id, cx));
                        }
                    }),
            )
            .when(editing, |s| {
                s.child(
                    Button::new(("env-remove", seq))
                        .ghost()
                        .small()
                        .icon(Lucide::X)
                        .tooltip("Remove variable")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(index) = this.rows.iter().position(|r| r.seq == seq) {
                                let row = this.rows.remove(index);
                                row.key.update(cx, |i, cx| i.set_value("", window, cx));
                                row.value.update(cx, |i, cx| i.set_value("", window, cx));
                            }
                            this.filter(cx);
                        })),
                )
            })
            .into_any_element()
    }
}
impl Render for EnvironmentEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.library.read(cx).vault_is_unlocked() {
            return div().into_any_element();
        }
        let rem = window.rem_size();
        let sizes = Rc::new(
            self.visible
                .iter()
                .map(|ix| gpui_kit::size(gpui_kit::px(0.), rem * self.row_height(*ix, cx)))
                .collect(),
        );
        let list = v_virtual_list(
            cx.entity(),
            "environment-rows",
            sizes,
            |this, range: Range<usize>, _, cx| {
                range
                    .filter_map(|ix| this.visible.get(ix).copied())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|ix| this.row(ix, cx))
                    .collect()
            },
        )
        .track_scroll(&self.scroll)
        .h(rems(
            self.visible
                .iter()
                .map(|ix| self.row_height(*ix, cx))
                .sum::<f32>()
                .min(18.),
        ))
        .w_full();
        let editing = self.editing;
        let exporting = self.exporting;
        v_flex().id("environment-editor").test_support().gap_3()
            .child(h_flex().items_center().justify_between().gap_2()
                .child(div().text_sm().child("Environment variables"))
                .child(Button::new("env-more").ghost().small().icon(Lucide::Ellipsis).tooltip("Environment actions")
                    .dropdown_menu(move |menu,_,_| {
                        menu.item(PopupMenuItem::new("Copy as .env").on_click(move |_,window,cx|window.dispatch_action(Box::new(CopyProjectEnvironment),cx)))
                            .when(!editing, |menu| menu.item(PopupMenuItem::new(if exporting {"Exporting…"} else {"Export .env.local…"}).disabled(exporting).on_click(move |_,window,cx|window.dispatch_action(Box::new(ExportProjectEnvironment),cx))))
                            .when(editing, |menu| menu.separator()
                                .item(PopupMenuItem::new("Import .env…").on_click(move |_,window,cx|window.dispatch_action(Box::new(ImportProjectEnvironment),cx)))
                                .item(PopupMenuItem::new("Paste .env").on_click(move |_,window,cx|window.dispatch_action(Box::new(PasteProjectEnvironment),cx))))
                    })))
            .child(if self.editing { Input::new(&self.environment).id("env-environment").small().into_any_element() }
                else { div().text_xs().text_color(cx.theme().muted_foreground).child(self.environment.read(cx).value()).into_any_element() })
            .when_some(self.error.clone(),|s,error|s.child(div().text_sm().text_color(cx.theme().danger).child(error)))
            .when_some(self.pending.as_ref(),|s,pending| {
                s.child(v_flex().gap_2().p_3().border_1().border_color(cx.theme().border).rounded(cx.theme().radius)
                    .child(div().text_sm().child(format!("Import {} variables? Values remain hidden.",pending.variables().len())))
                    .child(div().id("env-preview-keys").max_h(rems(8.)).overflow_y_scroll()
                        .child(v_flex().gap_1().children(pending.variables().iter().map(|variable|
                            h_flex().justify_between().gap_3().text_sm()
                                .child(div().truncate().child(variable.key().to_owned()))
                                .child("••••••••")))))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Replaces the variables in this draft. Expressions are stored literally; nothing is executed. Comments and file layout are not retained."))
                    .child(h_flex().gap_2()
                        .child(Button::new("env-apply-import").outline().small().label("Use variables").on_click(cx.listener(|s,_,w,cx|s.apply_import(w,cx))))
                        .child(Button::new("env-cancel-import").ghost().small().label("Cancel").on_click(cx.listener(|s,_,_,cx|{s.pending=None;cx.notify();}))))
                )
            })
            .child(Input::new(&self.query).id("env-search").small())
            .when(self.editing, |s| s.child(div().text_xs().text_color(cx.theme().muted_foreground).child("Values: \\n for a line break, \\r for return, \\t for tab, \\\\ for backslash. Pasted values are escaped automatically.")))
            .child(if self.visible.is_empty() {div().py_4().text_sm().text_color(cx.theme().muted_foreground).child(if self.rows.is_empty(){if self.editing {"No variables. Add one or import a .env file."} else {"No variables. Edit this item to add or import them."}}else{"No matching variables"}).into_any_element()}else{list.into_any_element()})
            .when(self.editing,|s|s.child(Button::new("env-add-variable").ghost().small().icon(Lucide::Plus).label("Add variable")
                .on_click(cx.listener(|s,_,w,cx| {
                    if s.rows.len()>=MAX_ENV_VARIABLES {s.error=Some(format!("An environment set can contain at most {MAX_ENV_VARIABLES} variables."));cx.notify();return;}
                    s.query.update(cx,|i,cx|i.set_value("",w,cx)); s.push_row("","",w,cx); s.filter(cx);
                    s.scroll.scroll_to_item(s.rows.len()-1,ScrollStrategy::Center);
                    if let Some(row)=s.rows.last() {row.key.update(cx,|i,cx|i.focus(w,cx));}
                }))))
            .into_any_element()
    }
}

// The kit's masked input is single-line and strips raw newlines. An explicit
// reversible representation keeps multiline secrets intact without exposing
// them in an unmasked text area. Copy/export always use the literal value.
fn escaped_value(value: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::new());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}
fn literal_value(value: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::new());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masked_value_representation_is_lossless() {
        for value in [
            "",
            "line one\nline two\r\n",
            "C:\\new\\test",
            "\\n\\r\\t\\",
            "💚\t spaces  ",
        ] {
            assert_eq!(&*literal_value(&escaped_value(value)), value);
        }
    }
}
