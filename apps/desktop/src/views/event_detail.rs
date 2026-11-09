//! Event detail panel: a right-hand sheet to edit one event.
//!
//! Title, location and notes autosave (debounced, plus on blur and when the
//! panel closes). All-day, dates, times and repeat save immediately. Moving
//! the start keeps the length (like Apple Calendar); an end before the start
//! is refused and the picker snaps back.
//!
//! A repeating event opens at the occurrence that was clicked. Changing its
//! dates or times, or deleting it, asks "only this event" or "all events"
//! (Apple Calendar's question); "only this event" records an exception
//! (`core::event::Exception`). Title, place, notes, all-day and the repeat
//! rule always apply to the whole series.

use std::time::Duration;

use chrono::{Datelike as _, Timelike as _};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::time_field::{HourCycle, TimePrecision};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Placement, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Task as GpuiTask, Window, div, px,
};
use keepstone_core::{Date, DateTime, Event, EventId, Occurrence, Recurrence, Time};
use keepstone_ui::appearance::{self, ClockFormat};
use keepstone_ui::components::section_label;
use keepstone_ui::feedback;

use crate::library::{Library, LibraryEvent};
use crate::views::calendar_layout::format_span;
use crate::views::task_row::notify_err;

const AUTOSAVE_DELAY: Duration = Duration::from_millis(500);

/// Opens the detail panel for `id`. `select_title` selects the title so a
/// just-created "New event" can be renamed by typing.
pub fn open_event_detail(
    id: EventId,
    series_date: Option<Date>,
    select_title: bool,
    library: Entity<Library>,
    window: &mut Window,
    cx: &mut App,
) {
    let detail = cx.new(|cx| EventDetail::new(id, series_date, library, window, cx));
    let title = detail.read(cx).title.clone();
    window.open_sheet_at(Placement::Right, cx, move |sheet, _, _| {
        sheet.title("Event").size(px(440.)).child(detail.clone())
    });
    if select_title {
        title.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }
}

pub struct EventDetail {
    id: EventId,
    /// The clicked occurrence of a repeating event (its series date).
    series_date: Option<Date>,
    library: Entity<Library>,
    event: Option<Event>,
    title: Entity<InputState>,
    location: Entity<InputState>,
    notes: Entity<TextareaState>,
    /// Timed events: start and end with times.
    starts_at: Entity<DatePickerState>,
    ends_at: Entity<DatePickerState>,
    /// All-day events: first and last day.
    first_day: Entity<DatePickerState>,
    last_day: Entity<DatePickerState>,
    dirty: bool,
    pending_save: Option<GpuiTask<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventDetail {
    pub fn new(
        id: EventId,
        series_date: Option<Date>,
        library: Entity<Library>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let event = library.read(cx).event(id);
        let hour_cycle = match appearance::settings(cx).clock {
            ClockFormat::TwelveHour => HourCycle::H12,
            ClockFormat::TwentyFourHour => HourCycle::H23,
        };
        let title = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Event title")
                .validate(|text, _| text.chars().count() <= keepstone_core::MAX_TITLE_CHARS)
        });
        let location = cx.new(|cx| InputState::new(window, cx).placeholder("Add a place"));
        let notes = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 14)
                .placeholder("Notes")
        });
        let timed = |window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                DatePickerState::new(window, cx)
                    .time_precision(TimePrecision::Minute)
                    .hour_cycle(hour_cycle)
                    .date_format(match hour_cycle {
                        HourCycle::H12 => "%a, %b %-d  %-I:%M %p",
                        HourCycle::H23 => "%a, %b %-d  %H:%M",
                    })
            })
        };
        let starts_at = timed(window, cx);
        let ends_at = timed(window, cx);
        let day_picker = |window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| DatePickerState::new(window, cx).date_format("%a, %b %-d, %Y"))
        };
        let first_day = day_picker(window, cx);
        let last_day = day_picker(window, cx);

        let mut this = Self {
            id,
            series_date,
            library: library.clone(),
            event,
            title: title.clone(),
            location: location.clone(),
            notes: notes.clone(),
            starts_at: starts_at.clone(),
            ends_at: ends_at.clone(),
            first_day: first_day.clone(),
            last_day: last_day.clone(),
            dirty: false,
            pending_save: None,
            _subscriptions: Vec::new(),
        };
        if let Some(event) = this.event.clone() {
            title.update(cx, |t, cx| {
                t.set_value(event.title().to_owned(), window, cx)
            });
            location.update(cx, |t, cx| {
                t.set_value(event.location().to_owned(), window, cx)
            });
            notes.update(cx, |n, cx| {
                n.set_value(event.notes().to_owned(), window, cx)
            });
            this.sync_pickers(&event, window, cx);
        }

        let text_events = |this: &mut Self,
                           event: &InputEvent,
                           window: &mut Window,
                           cx: &mut Context<Self>| match event {
            InputEvent::Change => this.schedule_save(window, cx),
            InputEvent::Blur | InputEvent::PressEnter { .. } => this.flush(window, cx),
            _ => {}
        };
        this._subscriptions = vec![
            cx.subscribe_in(&title, window, move |this, _, e, window, cx| {
                text_events(this, e, window, cx)
            }),
            cx.subscribe_in(&location, window, move |this, _, e, window, cx| {
                text_events(this, e, window, cx)
            }),
            cx.subscribe_in(&notes, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.schedule_save(window, cx),
                InputEvent::Blur => this.flush(window, cx),
                _ => {}
            }),
            cx.subscribe_in(
                &starts_at,
                window,
                |this, _, e: &DatePickerEvent, window, cx| {
                    let DatePickerEvent::Change(v) = e;
                    if let Some(start) = v.start().and_then(from_chrono_dt) {
                        this.change_start(start, window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &ends_at,
                window,
                |this, _, e: &DatePickerEvent, window, cx| {
                    let DatePickerEvent::Change(v) = e;
                    if let Some(end) = v.start().and_then(from_chrono_dt) {
                        this.change_end(end, window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &first_day,
                window,
                |this, _, e: &DatePickerEvent, window, cx| {
                    let DatePickerEvent::Change(v) = e;
                    if let Some(day) = v.start().and_then(|d| from_chrono(d.date())) {
                        this.change_first_day(day, window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &last_day,
                window,
                |this, _, e: &DatePickerEvent, window, cx| {
                    let DatePickerEvent::Change(v) = e;
                    if let Some(day) = v.start().and_then(|d| from_chrono(d.date())) {
                        this.change_last_day(day, window, cx);
                    }
                },
            ),
            // Follow changes made elsewhere (a drag in the calendar). Text
            // fields are left alone so typing is never overwritten.
            cx.subscribe_in(&library, window, |this, library, event, window, cx| {
                if *event == LibraryEvent::EventsChanged {
                    this.event = library.read(cx).event(this.id);
                    if let Some(event) = this.event.clone() {
                        this.sync_pickers(&event, window, cx);
                    }
                    cx.notify();
                }
            }),
            // Closing the panel drops this view: save what is still pending.
            cx.on_release(|this, cx| {
                if this.dirty {
                    _ = this.save(cx);
                }
            }),
        ];
        this
    }

    /// The occurrence shown: the clicked one of a repeating event (with its
    /// exception applied), else `None` (the event itself).
    fn occurrence(&self, event: &Event) -> Option<Occurrence> {
        event.recurrence()?;
        event.occurrence_on(self.series_date?)
    }

    /// Start and end of what the panel shows.
    fn shown_span(&self, event: &Event) -> (DateTime, DateTime) {
        self.occurrence(event)
            .map_or((event.start(), event.end()), |o| (o.start, o.end))
    }

    /// Puts the shown dates into whichever pickers the event uses.
    fn sync_pickers(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        let (start, end) = self.shown_span(event);
        if event.is_all_day() {
            let last = end
                .date()
                .yesterday()
                .unwrap_or(end.date())
                .max(start.date());
            if let Some(d) = to_chrono(start.date()) {
                self.first_day.update(cx, |p, cx| p.set_date(d, window, cx));
            }
            if let Some(d) = to_chrono(last) {
                self.last_day.update(cx, |p, cx| p.set_date(d, window, cx));
            }
        } else {
            if let Some(dt) = to_chrono_dt(start) {
                self.starts_at
                    .update(cx, |p, cx| p.set_date_time(dt, window, cx));
            }
            if let Some(dt) = to_chrono_dt(end) {
                self.ends_at
                    .update(cx, |p, cx| p.set_date_time(dt, window, cx));
            }
        }
    }

    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dirty = true;
        self.pending_save = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            _ = this.update_in(cx, |this, window, cx| this.flush(window, cx));
        }));
    }

    fn flush(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_save = None;
        if !self.dirty {
            return;
        }
        if let Err(err) = self.save(cx) {
            feedback::error_toast(format!("{err:#}"), window, cx);
        }
    }

    /// Writes title, location and notes. A blank title is not saved (the
    /// field keeps it so the user can fix it).
    fn save(&mut self, cx: &mut App) -> anyhow::Result<()> {
        let id = self.id;
        let title = self.title.read(cx).value().to_string();
        let location = self.location.read(cx).value().to_string();
        let notes = self.notes.read(cx).value().to_string();
        let Some(current) = self.library.read(cx).event(id) else {
            self.dirty = false;
            return Ok(());
        };
        self.library.update(cx, |lib, cx| {
            if current.notes() != notes {
                lib.set_event_notes(id, &notes, cx)?;
            }
            if current.location() != location.trim() {
                lib.set_event_location(id, &location, cx)?;
            }
            if !title.trim().is_empty() && current.title() != title.trim() {
                lib.rename_event(id, &title, cx)?;
            }
            anyhow::Ok(())
        })?;
        self.dirty = false;
        Ok(())
    }

    fn change_start(&mut self, start: DateTime, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self.event.clone().filter(|e| !e.is_all_day()) else {
            return;
        };
        let (from, to) = self.shown_span(&event);
        if start == from {
            return;
        }
        // Keep the length: moving the start moves the end with it.
        let Ok(end) = start.checked_add(to - from) else {
            return;
        };
        self.apply_span(&event, start, end, window, cx);
    }

    fn change_end(&mut self, end: DateTime, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self.event.clone().filter(|e| !e.is_all_day()) else {
            return;
        };
        let (from, to) = self.shown_span(&event);
        if end == to {
            return;
        }
        if end <= from {
            feedback::error_toast("An event has to end after it starts.", window, cx);
            self.sync_pickers(&event, window, cx);
            return;
        }
        self.apply_span(&event, from, end, window, cx);
    }

    fn change_first_day(&mut self, first: Date, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self.event.clone().filter(Event::is_all_day) else {
            return;
        };
        let (from, to) = self.shown_span(&event);
        if first == from.date() {
            return;
        }
        // Keep the length, like a timed start.
        let shift = first - from.date();
        let (Ok(start), Ok(end)) = (from.checked_add(shift), to.checked_add(shift)) else {
            return;
        };
        self.apply_span(&event, start, end, window, cx);
    }

    fn change_last_day(&mut self, last: Date, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self.event.clone().filter(Event::is_all_day) else {
            return;
        };
        let (from, to) = self.shown_span(&event);
        if Some(last) == to.date().yesterday().ok() {
            return;
        }
        if last < from.date() {
            feedback::error_toast("An event has to end after it starts.", window, cx);
            self.sync_pickers(&event, window, cx);
            return;
        }
        let Ok(end) = last.tomorrow() else { return };
        self.apply_span(&event, from, end.to_datetime(Time::midnight()), window, cx);
    }

    /// Makes what the panel shows happen `start..end`. For an occurrence of
    /// a repeating event, asks whether that's this one or the series; the
    /// pickers show the old values until the answer (Cancel leaves them).
    fn apply_span(
        &mut self,
        event: &Event,
        start: DateTime,
        end: DateTime,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_pickers(event, window, cx);
        let id = self.id;
        let all_day = event.is_all_day();
        let library = self.library.clone();
        // The series moves by the same amount as the shown occurrence.
        let (from, _) = self.shown_span(event);
        let series_start = event.start().checked_add(start - from).ok();
        let series_end = series_start.and_then(|s| s.checked_add(end - start).ok());
        let whole = move |window: &mut Window, cx: &mut App| {
            let (Some(s), Some(e)) = (series_start, series_end) else {
                return;
            };
            let result = library.update(cx, |lib, cx| {
                if all_day {
                    let last = e.date().yesterday().unwrap_or(e.date()).max(s.date());
                    lib.set_event_all_day(id, s.date(), last, cx)
                } else {
                    lib.set_event_times(id, s, e, cx)
                }
            });
            notify_err(result, window, cx);
        };
        let Some(occ) = self.occurrence(event) else {
            whole(window, cx);
            return;
        };
        let library = self.library.clone();
        let series_date = occ.series_date;
        let had_exceptions = !event.exceptions().is_empty();
        feedback::ask_repeat_scope(
            "Change this event?",
            "Only this event",
            "All events",
            window,
            cx,
            move |only_this, window, cx| {
                if only_this {
                    let result = library.update(cx, |lib, cx| {
                        lib.move_occurrence(id, series_date, start, end, cx)
                    });
                    notify_err(result.map(|_| ()), window, cx);
                } else {
                    whole(window, cx);
                    if had_exceptions {
                        window.push_notification(
                            "All events changed. Events you'd changed one by one follow the series again.",
                            cx,
                        );
                    }
                }
            },
        );
    }

    /// All-day on: the days it touched. Off: 9–10 AM on its first day, or
    /// the next hour if that day is today.
    fn set_all_day(&mut self, all_day: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self.event.clone() else {
            return;
        };
        if all_day == event.is_all_day() {
            return;
        }
        let id = self.id;
        let result = if all_day {
            let first = event.start().date();
            let last = event.last_day().max(first);
            self.library
                .update(cx, |lib, cx| lib.set_event_all_day(id, first, last, cx))
        } else {
            let day = event.start().date();
            let start = day.to_datetime(Time::constant(9, 0, 0, 0));
            let end = day.to_datetime(Time::constant(10, 0, 0, 0));
            self.library
                .update(cx, |lib, cx| lib.set_event_times(id, start, end, cx))
        };
        notify_err(result, window, cx);
    }

    fn set_recurrence(
        &mut self,
        rule: Option<Recurrence>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.id;
        notify_err(
            self.library
                .update(cx, |lib, cx| lib.set_event_recurrence(id, rule, cx)),
            window,
            cx,
        );
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        let library = self.library.clone();
        let occurrence = self.event.as_ref().and_then(|e| self.occurrence(e));
        let Some(occ) = occurrence else {
            self.delete_series(window, cx);
            return;
        };
        let view = cx.entity().downgrade();
        let series_date = occ.series_date;
        feedback::ask_repeat_scope(
            "Delete this event?",
            "Delete only this event",
            "Delete all events",
            window,
            cx,
            move |only_this, window, cx| {
                if !only_this {
                    _ = view.update(cx, |this, cx| this.delete_series(window, cx));
                    return;
                }
                let result = library.update(cx, |l, cx| l.skip_occurrence(id, series_date, cx));
                if let Some(previous) = feedback::report(result, window, cx) {
                    _ = view.update(cx, |this, _| this.dirty = false);
                    window.close_sheet(cx);
                    let library = library.clone();
                    feedback::undo_toast("Event deleted.", window, cx, move |window, cx| {
                        notify_err(
                            library.update(cx, |l, cx| {
                                l.restore_occurrence(id, series_date, previous, cx)
                            }),
                            window,
                            cx,
                        );
                    });
                }
            },
        );
    }

    /// Deletes the event (every occurrence), with Undo.
    fn delete_series(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        let library = self.library.clone();
        if feedback::report(
            library.update(cx, |l, cx| l.trash_event(id, cx)),
            window,
            cx,
        )
        .is_some()
        {
            self.dirty = false;
            window.close_sheet(cx);
            feedback::undo_toast("Event deleted.", window, cx, move |window, cx| {
                notify_err(
                    library.update(cx, |l, cx| l.restore_event(id, cx)),
                    window,
                    cx,
                );
            });
        }
    }

    fn repeat_menu(&self, current: Option<Recurrence>, cx: &mut Context<Self>) -> impl IntoElement {
        let label: SharedString = current.map_or("Never".into(), |r| r.to_string().into());
        let view = cx.entity().downgrade();
        Button::new("event-repeat")
            .outline()
            .small()
            .icon(Lucide::Repeat)
            .label(label)
            .dropdown_menu(move |menu, _, _| {
                let choices =
                    std::iter::once(None).chain(Recurrence::PRESETS.into_iter().map(Some));
                choices.fold(menu, |menu, rule| {
                    let view = view.clone();
                    let label: SharedString = rule.map_or("Never".into(), |r| r.to_string().into());
                    menu.item(PopupMenuItem::new(label).checked(rule == current).on_click(
                        move |_, window, cx| {
                            _ = view.update(cx, |this, cx| this.set_recurrence(rule, window, cx));
                        },
                    ))
                })
            })
    }
}

impl Render for EventDetail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(event) = self.event.clone() else {
            return v_flex()
                .gap_2()
                .child(div().text_sm().child("This event no longer exists."));
        };
        if event.is_trashed() {
            return v_flex().gap_2().child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("This event was deleted."),
            );
        }
        let clock = appearance::settings(cx).clock;
        let occurrence = self.occurrence(&event);
        let summary = summary(&event, occurrence.as_ref(), clock);
        let all_day = event.is_all_day();
        let view = cx.entity().downgrade();
        let (from, to) = if all_day {
            (
                DatePicker::new(&self.first_day).into_any_element(),
                DatePicker::new(&self.last_day).into_any_element(),
            )
        } else {
            (
                DatePicker::new(&self.starts_at).into_any_element(),
                DatePicker::new(&self.ends_at).into_any_element(),
            )
        };
        let theme = cx.theme();

        v_flex()
            .gap_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Input::new(&self.title)
                            .id("event-title")
                            .large()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(summary),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(section_label("When", cx))
                            .child(
                                Switch::new("event-all-day")
                                    .label("All day")
                                    .checked(all_day)
                                    .on_click(move |on, window, cx| {
                                        _ = view.update(cx, |this, cx| {
                                            this.set_all_day(*on, window, cx)
                                        });
                                    }),
                            ),
                    )
                    .child(field_row(if all_day { "From" } else { "Starts" }, from, cx))
                    .child(field_row(if all_day { "To" } else { "Ends" }, to, cx))
                    .child(field_row(
                        "Repeat",
                        self.repeat_menu(event.recurrence(), cx).into_any_element(),
                        cx,
                    )),
            )
            .child(
                v_flex().gap_2().child(section_label("Location", cx)).child(
                    Input::new(&self.location).id("event-location").prefix(
                        Icon::new(Lucide::MapPin)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    ),
                ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_label("Notes", cx))
                    .child(Textarea::new(&self.notes)),
            )
            .child(
                h_flex().pt_2().child(
                    Button::new("event-delete")
                        .ghost()
                        .small()
                        .icon(Lucide::Trash)
                        .label("Delete event")
                        .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                ),
            )
    }
}

/// A labelled row: the label in a fixed lane so the controls line up.
fn field_row(label: &'static str, control: gpui_kit::AnyElement, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_3()
        .items_center()
        .child(
            div()
                .w_16()
                .flex_shrink_0()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(control))
}

/// "Monday, Nov 9 · 9 – 10:30 AM · Every week", for the event or (for a
/// repeating event) one occurrence of it.
#[must_use]
pub fn summary(event: &Event, occurrence: Option<&Occurrence>, clock: ClockFormat) -> String {
    let (start, end) = occurrence.map_or((event.start(), event.end()), |o| (o.start, o.end));
    let first = start.date();
    let mut parts = vec![first.strftime("%A, %b %-d").to_string()];
    if event.is_all_day() {
        let last = end.date().yesterday().unwrap_or(first).max(first);
        if last == first {
            parts.push("All day".into());
        } else {
            parts[0] = format!("{} – {}", first.strftime("%b %-d"), last.strftime("%b %-d"));
            parts.push(format!("{} days", (last - first).get_days() + 1));
        }
    } else {
        parts.push(format_span(start, end, clock));
    }
    if let Some(rule) = event.recurrence() {
        parts.push(rule.to_string());
    }
    if occurrence.is_some_and(|o| o.moved) {
        parts.push("Changed for this day".into());
    }
    parts.join(" · ")
}

fn to_chrono(date: Date) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_ymd_opt(
        i32::from(date.year()),
        u32::try_from(date.month()).ok()?,
        u32::try_from(date.day()).ok()?,
    )
}

fn from_chrono(date: chrono::NaiveDate) -> Option<Date> {
    Date::new(
        i16::try_from(date.year()).ok()?,
        i8::try_from(date.month()).ok()?,
        i8::try_from(date.day()).ok()?,
    )
    .ok()
}

fn to_chrono_dt(dt: DateTime) -> Option<chrono::NaiveDateTime> {
    let time = chrono::NaiveTime::from_hms_opt(
        u32::try_from(dt.hour()).ok()?,
        u32::try_from(dt.minute()).ok()?,
        0,
    )?;
    Some(to_chrono(dt.date())?.and_time(time))
}

fn from_chrono_dt(dt: chrono::NaiveDateTime) -> Option<DateTime> {
    let time = Time::new(
        i8::try_from(dt.hour()).ok()?,
        i8::try_from(dt.minute()).ok()?,
        0,
        0,
    )
    .ok()?;
    Some(from_chrono(dt.date())?.to_datetime(time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_times_round_trip_through_the_picker() {
        for s in ["2026-11-08T09:30", "2024-02-29T23:59", "2026-01-01T00:00"] {
            let dt: DateTime = s.parse().expect("datetime");
            assert_eq!(to_chrono_dt(dt).and_then(from_chrono_dt), Some(dt));
        }
    }

    #[test]
    fn summaries_read_naturally() {
        let now = keepstone_core::Timestamp::from_second(1).expect("ts");
        let d = |s: &str| s.parse::<Date>().expect("date");
        let dt = |s: &str| s.parse::<DateTime>().expect("datetime");
        let mut e = Event::new_timed("Call", dt("2026-11-09T09:00"), dt("2026-11-09T10:30"), now)
            .expect("event");
        e.set_recurrence(Some(Recurrence::WEEKLY), now)
            .expect("repeat");
        assert_eq!(
            summary(&e, None, ClockFormat::TwelveHour),
            "Monday, Nov 9 · 9 – 10:30 AM · Every week"
        );
        let trip = Event::new_all_day("Trip", d("2026-11-20"), d("2026-11-22"), now).expect("trip");
        assert_eq!(
            summary(&trip, None, ClockFormat::TwelveHour),
            "Nov 20 – Nov 22 · 3 days"
        );
    }
}
