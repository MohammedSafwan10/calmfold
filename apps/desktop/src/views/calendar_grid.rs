//! Calendar grids: the month view and the week/day time grid.
//!
//! Geometry is in rems so it zooms with the interface size. Overlaps and
//! multi-day bars come from `calendar_layout`; this file only draws them.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, DragMoveEvent, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, point, px, relative, rems,
};
use jiff::Span;
use keepstone_core::{Date, Occurrence, Time};
use keepstone_ui::appearance::{self, ClockFormat};

use crate::views::calendar::{CalendarDrag, CalendarView};
use crate::views::calendar_layout::{
    CalendarMode, MINUTES_PER_DAY, format_span, format_time, lay_out_timed, minutes_on, pack_spans,
    short_time,
};

/// Day-number row at the top of a month cell.
const DAY_HEADER: f32 = 1.75;
/// One line of items (bars, chips).
const ROW: f32 = 1.375;
/// One hour on the time grid.
const HOUR: f32 = 3.0;
/// Hour labels column.
const GUTTER: f32 = 3.5;
/// Shortest drawn block, in minutes (keeps 5-minute events clickable).
const MIN_BLOCK: i32 = 22;
/// Drop positions snap to quarter hours.
const SNAP: i32 = 15;
/// Room above the month grid (title bar, header, quick add, weekday row).
const MONTH_CHROME: f32 = 15.0;
/// Window width (rems) from which the month shows the day panel.
const WIDE_MONTH: f32 = 70.0;

fn add_days(day: Date, n: i64) -> Date {
    day.checked_add(Span::new().days(n)).unwrap_or(day)
}

fn day_index(first: Date, day: Date) -> usize {
    usize::try_from((day - first).get_days()).unwrap_or(0)
}

/// A multi-day bar inside a row of `cols` days starting at `first`.
struct Bar {
    occ: Occurrence,
    first_col: usize,
    last_col: usize,
}

/// Bars for a row of days: all-day occurrences and timed ones spanning days,
/// clipped to the row, in stacking order (earlier and longer first).
fn bars_for(occurrences: &[Occurrence], first: Date, cols: usize) -> Vec<Bar> {
    let end = add_days(first, cols as i64);
    let mut bars: Vec<Bar> = occurrences
        .iter()
        .filter(|o| is_bar(o))
        .filter_map(|o| {
            let days = o.days();
            if days.end <= first || days.start >= end {
                return None;
            }
            let start = days.start.max(first);
            let last = add_days(days.end.min(end), -1);
            Some(Bar {
                occ: *o,
                first_col: day_index(first, start),
                last_col: day_index(first, last),
            })
        })
        .collect();
    bars.sort_by_key(|b| (b.first_col, std::cmp::Reverse(b.last_col)));
    bars
}

/// Whether an occurrence is drawn as a bar: all-day, or a day or longer.
/// An overnight flight (10 PM – 2 AM) stays a single line on its first day.
fn is_bar(o: &Occurrence) -> bool {
    o.all_day
        || o.start
            .checked_add(Span::new().hours(24))
            .is_ok_and(|limit| o.end >= limit)
}

impl CalendarView {
    /// A drag source for an event, unless it repeats (a repeat can only be
    /// moved as a whole series, from its detail panel).
    fn event_drag(&self, occ: &Occurrence) -> Option<CalendarDrag> {
        let event = self.span.events.get(&occ.event)?;
        if event.recurrence().is_some() {
            return None;
        }
        Some(CalendarDrag::Event {
            id: event.id(),
            title: event.title().to_owned().into(),
            all_day: event.is_all_day(),
            minutes: (event.end() - event.start())
                .total(jiff::Unit::Minute)
                .map_or(60, |m| m as i64),
            grab_y: std::rc::Rc::default(),
        })
    }

    fn title_of(&self, occ: &Occurrence) -> SharedString {
        self.span
            .events
            .get(&occ.event)
            .map_or_else(SharedString::default, |e| e.title().to_owned().into())
    }

    /// Wraps `el` so dragging it carries `drag`.
    fn draggable(
        el: gpui_kit::Stateful<gpui_kit::Div>,
        drag: Option<CalendarDrag>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        match drag {
            Some(drag) => el.on_drag(drag, |drag, offset, _, cx| {
                if let CalendarDrag::Event { grab_y, .. } = drag {
                    grab_y.set(f32::from(offset.y));
                }
                cx.new(|_| drag.clone())
            }),
            None => el,
        }
    }

    /// A bar for an all-day or multi-day event, positioned in its row.
    fn bar(
        &self,
        bar: &Bar,
        lane: usize,
        cols: usize,
        top: f32,
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let clock = appearance::settings(cx).clock;
        let id = bar.occ.event;
        let span = bar.last_col - bar.first_col + 1;
        let label: SharedString = if bar.occ.all_day {
            self.title_of(&bar.occ)
        } else {
            format!(
                "{} {}",
                format_time(bar.occ.start.time(), clock),
                self.title_of(&bar.occ)
            )
            .into()
        };
        let past = bar.occ.end <= self.now;
        let el = div()
            .id(SharedString::from(format!(
                "bar-{id}-{key}-{}",
                bar.first_col
            )))
            .absolute()
            .top(rems(top + lane as f32 * ROW))
            .left(relative(bar.first_col as f32 / cols as f32))
            .w(relative(span as f32 / cols as f32))
            .h(rems(ROW))
            .px_0p5()
            .py(rems(0.0625))
            .child(
                div()
                    .size_full()
                    .px_1p5()
                    .flex()
                    .items_center()
                    .rounded(theme.radius)
                    .bg(theme.primary.opacity(0.16))
                    .hover(|s| s.bg(theme.primary.opacity(0.24)))
                    .when(past, |s| s.opacity(0.6))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .child(div().truncate().child(label)),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_event(id, window, cx);
            }));
        let drag = self.event_drag(&bar.occ);
        Self::draggable(el, drag).into_any_element()
    }

    /// A one-line chip: a timed event ("● 9:30 Standup") or a task.
    fn event_chip(
        &self,
        occ: &Occurrence,
        day: Date,
        clock: ClockFormat,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let id = occ.event;
        let past = occ.end <= self.now;
        let el = h_flex()
            .id(SharedString::from(format!("chip-{id}-{day}")))
            .h(rems(ROW))
            .px_1()
            .gap_1()
            .items_center()
            .rounded(theme.radius)
            .hover(|s| s.bg(theme.muted))
            .when(past, |s| s.opacity(0.6))
            .text_xs()
            .child(
                div()
                    .size_1p5()
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(theme.primary),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.title_of(occ)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(theme.muted_foreground)
                    .child(short_time(occ.start.time(), clock)),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_event(id, window, cx);
            }));
        let drag = self.event_drag(occ);
        Self::draggable(el, drag).into_any_element()
    }

    fn task_chip(&self, task: &keepstone_core::Task, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let id = task.id();
        let done = !task.status().is_open();
        h_flex()
            .id(SharedString::from(format!("task-chip-{id}")))
            .h(rems(ROW))
            .px_1()
            .gap_1()
            .items_center()
            .rounded(theme.radius)
            .hover(|s| s.bg(theme.muted))
            .text_xs()
            .child(
                Icon::new(if done {
                    Lucide::CircleCheckBig
                } else {
                    Lucide::Circle
                })
                .xsmall()
                .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when(done, |s| {
                        s.line_through().text_color(theme.muted_foreground)
                    })
                    .child(task.title().to_owned()),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_task(id, window, cx);
            }))
            .on_drag(
                CalendarDrag::Task {
                    id,
                    title: task.title().to_owned().into(),
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .into_any_element()
    }

    /// The day number; today gets a filled pill.
    fn day_number(&self, day: Date, muted: bool, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_today = day == self.today;
        let label = if day.day() == 1 && self.mode == CalendarMode::Month {
            day.strftime("%b %-d").to_string()
        } else {
            day.day().to_string()
        };
        h_flex()
            .h(rems(1.375))
            .min_w(rems(1.375))
            .px_1()
            .justify_center()
            .items_center()
            .rounded_full()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .map(|s| {
                if is_today {
                    s.bg(theme.primary).text_color(theme.primary_foreground)
                } else if muted {
                    s.text_color(theme.muted_foreground)
                } else {
                    s
                }
            })
            .child(label)
    }

    // ---- Month -----------------------------------------------------------

    /// Month grid plus, on wide windows, the selected day's full list.
    ///
    /// Cells stay calm: bars for all-day and multi-day events, one line per
    /// one-off event or task (title only), and routines folded into a row of
    /// small dots at the bottom. What doesn't fit becomes "+N"; the day panel
    /// on the right always lists everything for the selected day.
    pub(super) fn month(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let clock = appearance::settings(cx).clock;
        let days = self.span.days.clone();
        let weeks: Vec<Date> = (0..)
            .map(|w| add_days(days.start, 7 * w))
            .take_while(|d| *d < days.end)
            .collect();
        // How many item lines fit in a cell at this window height (the
        // routine-dot row takes the last one).
        let rem = f32::from(window.rem_size()).max(1.);
        let grid = f32::from(window.viewport_size().height) / rem - MONTH_CHROME;
        let row_h = (grid / weeks.len().max(1) as f32).max(6.);
        let slots = (((row_h - DAY_HEADER - 0.25) / ROW).floor() as usize).clamp(1, 12);
        let wide = f32::from(window.viewport_size().width) / rem >= WIDE_MONTH;
        let month = self.anchor.month();

        let weekday_row = h_flex().children((0..7).map(|i| {
            div()
                .flex_1()
                .px_2()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(add_days(days.start, i).strftime("%a").to_string())
        }));

        let n_weeks = weeks.len();
        let occurrences: Vec<Occurrence> = self
            .span
            .occurrences
            .iter()
            .filter(|o| self.shows(o, cx))
            .copied()
            .collect();
        let rows = weeks
            .into_iter()
            .enumerate()
            .map(|(w, week)| {
                let bars = bars_for(&occurrences, week, 7);
                let lanes = pack_spans(
                    &bars
                        .iter()
                        .map(|b| (b.first_col, b.last_col))
                        .collect::<Vec<_>>(),
                );
                let max_lanes = slots.saturating_sub(1).max(1);
                let cells: Vec<_> = (0..7)
                    .map(|col| {
                        let day = add_days(week, col as i64);
                        let covering: Vec<usize> = bars
                            .iter()
                            .zip(&lanes)
                            .filter(|(b, _)| (b.first_col..=b.last_col).contains(&col))
                            .map(|(_, l)| *l)
                            .collect();
                        // Room only for the bars that cross this day.
                        let bar_lines = covering.iter().max().map_or(0, |m| (m + 1).min(max_lanes));
                        let hidden_bars = covering.iter().filter(|l| **l >= max_lanes).count();
                        let (routines, plans): (Vec<&Occurrence>, Vec<&Occurrence>) = occurrences
                            .iter()
                            .filter(|o| o.days().contains(&day) && !is_bar(o))
                            .partition(|o| self.is_routine(o));
                        let tasks: Vec<_> = self.span.tasks_on(day).collect();
                        let dot_line = usize::from(!routines.is_empty());
                        let room = slots.saturating_sub(bar_lines + dot_line);
                        let total = plans.len() + tasks.len();
                        // Keep a line for "+N" when something won't fit.
                        let visible = if total > room {
                            room.saturating_sub(1)
                        } else {
                            total
                        };
                        let more = hidden_bars + total - visible;
                        let mut items: Vec<AnyElement> = Vec::new();
                        for occ in plans.iter().take(visible) {
                            items.push(self.event_chip(occ, day, clock, cx));
                        }
                        for task in tasks.iter().take(visible.saturating_sub(plans.len())) {
                            items.push(self.task_chip(task, cx));
                        }
                        let selected = day == self.anchor;
                        let outside = day.month() != month;
                        v_flex()
                            .id(SharedString::from(format!("month-{day}")))
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px_0p5()
                            .pb_1()
                            .when(col < 6, |s| s.border_r_1().border_color(theme.border))
                            .when(outside, |s| s.bg(theme.muted.opacity(0.25)))
                            .when(selected, |s| s.bg(theme.accent))
                            .drag_over::<CalendarDrag>(|s, _, _, cx| s.bg(cx.theme().drop_target))
                            .on_drop(cx.listener(move |this, drag: &CalendarDrag, window, cx| {
                                this.drop_on_day(day, drag, false, window, cx)
                            }))
                            .on_click(cx.listener(
                                move |this, e: &gpui_kit::ClickEvent, window, cx| {
                                    window.focus(&this.focus, cx);
                                    if e.click_count() >= 2 {
                                        this.create_all_day_on(day, window, cx);
                                    } else {
                                        this.select(day, cx);
                                    }
                                },
                            ))
                            .child(
                                h_flex()
                                    .h(rems(DAY_HEADER))
                                    .px_0p5()
                                    .items_center()
                                    .child(self.day_number(day, outside, cx)),
                            )
                            .child(div().h(rems(bar_lines as f32 * ROW)).flex_shrink_0())
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .children(items)
                                    .when(more > 0, |s| {
                                        s.child(
                                            div()
                                                .h(rems(ROW))
                                                .px_1()
                                                .flex()
                                                .items_center()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(format!("+{more} more")),
                                        )
                                    }),
                            )
                            .when(!routines.is_empty(), |s| {
                                s.child(self.routine_dots(&routines, day, clock, cx))
                            })
                    })
                    .collect();
                let bar_elements: Vec<AnyElement> = bars
                    .iter()
                    .zip(&lanes)
                    .filter(|(_, lane)| **lane < max_lanes)
                    .map(|(bar, lane)| self.bar(bar, *lane, 7, DAY_HEADER, &week.to_string(), cx))
                    .collect();
                h_flex()
                    .relative()
                    .flex_1()
                    .min_h(rems(6.))
                    .when(w + 1 < n_weeks, |s| {
                        s.border_b_1().border_color(theme.border)
                    })
                    .children(cells)
                    .children(bar_elements)
            })
            .collect::<Vec<_>>();

        let grid = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(weekday_row)
            .child(
                v_flex()
                    .id("month-grid")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(theme.border)
                    .children(rows),
            );
        h_flex()
            .size_full()
            .px_6()
            .pb_4()
            .gap_5()
            .child(grid)
            .when(wide, |s| s.child(self.day_panel(clock, cx)))
    }

    /// Routines on one day as a row of small dots; the tooltip names them.
    fn routine_dots(
        &self,
        routines: &[&Occurrence],
        day: Date,
        clock: ClockFormat,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let names: Vec<String> = routines
            .iter()
            .map(|o| {
                format!(
                    "{} {}",
                    format_time(o.start.time(), clock),
                    self.title_of(o)
                )
            })
            .collect();
        let tip: SharedString = names.join(" · ").into();
        let color = theme.muted_foreground.opacity(0.6);
        h_flex()
            .id(SharedString::from(format!("routines-{day}")))
            .h(rems(0.75))
            .px_1()
            .gap(rems(0.1875))
            .items_center()
            .flex_shrink_0()
            .children(
                routines
                    .iter()
                    .take(6)
                    .map(|_| div().size(rems(0.3125)).rounded_full().bg(color)),
            )
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .into_any_element()
    }

    /// The selected day, in full: all-day, timed, routines, tasks.
    fn day_panel(&self, clock: ClockFormat, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let day = self.anchor;
        let occs: Vec<Occurrence> = self
            .span
            .on_day(day)
            .filter(|o| self.shows(o, cx))
            .copied()
            .collect();
        let (routines, plans): (Vec<&Occurrence>, Vec<&Occurrence>) =
            occs.iter().partition(|o| self.is_routine(o));
        let tasks: Vec<keepstone_core::Task> = self.span.tasks_on(day).cloned().collect();
        let relative = crate::views::relative_day(day, self.today);
        let heading = day.strftime("%A, %B %-d").to_string();

        let row = |occ: &Occurrence, muted: bool, cx: &mut Context<Self>| {
            let id = occ.event;
            let event = self.span.events.get(&id);
            let when: SharedString = if occ.all_day {
                "All day".into()
            } else if occ.start.date() < day {
                format!("until {}", format_time(occ.end.time(), clock)).into()
            } else {
                format_time(occ.start.time(), clock).into()
            };
            let location = event.map(|e| e.location().to_owned()).unwrap_or_default();
            h_flex()
                .id(SharedString::from(format!("panel-{id}-{day}")))
                .gap_3()
                .px_2()
                .py_1p5()
                .items_start()
                .rounded(theme.radius)
                .hover(|s| s.bg(theme.list_hover))
                .when(occ.end <= self.now, |s| s.opacity(0.6))
                .on_click(cx.listener(move |this, _, window, cx| this.open_event(id, window, cx)))
                .child(
                    div()
                        .w(rems(3.75))
                        .flex_shrink_0()
                        .text_xs()
                        .pt(rems(0.125))
                        .text_color(theme.muted_foreground)
                        .child(when),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .truncate()
                                .when(muted, |s| s.text_color(theme.muted_foreground))
                                .child(self.title_of(occ)),
                        )
                        .when(!location.is_empty(), |s| {
                            s.child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(theme.muted_foreground)
                                    .child(location),
                            )
                        }),
                )
                .into_any_element()
        };
        let plan_rows: Vec<AnyElement> = plans.iter().map(|o| row(o, false, cx)).collect();
        let routine_rows: Vec<AnyElement> = routines.iter().map(|o| row(o, true, cx)).collect();
        let task_rows: Vec<AnyElement> = tasks
            .iter()
            .map(|t| {
                let id = t.id();
                let done = !t.status().is_open();
                h_flex()
                    .id(SharedString::from(format!("panel-task-{id}")))
                    .gap_3()
                    .px_2()
                    .py_1p5()
                    .items_center()
                    .rounded(theme.radius)
                    .hover(|s| s.bg(theme.list_hover))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_task(id, window, cx)),
                    )
                    .child(
                        h_flex().w(rems(3.75)).flex_shrink_0().child(
                            Button::new(SharedString::from(format!("panel-check-{id}")))
                                .ghost()
                                .xsmall()
                                .icon(if done {
                                    Lucide::CircleCheckBig
                                } else {
                                    Lucide::Circle
                                })
                                .tooltip(if done { "Mark as not done" } else { "Complete" })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.toggle_task(id, window, cx);
                                })),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .when(done, |s| {
                                s.line_through().text_color(theme.muted_foreground)
                            })
                            .child(t.title().to_owned()),
                    )
                    .into_any_element()
            })
            .collect();
        let group = |label: &'static str, rows: Vec<AnyElement>, cx: &Context<Self>| {
            (!rows.is_empty()).then(|| {
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .px_2()
                            .pb_0p5()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .children(rows)
            })
        };
        let empty = plan_rows.is_empty() && routine_rows.is_empty() && task_rows.is_empty();
        let plans_group = group("Events", plan_rows, cx);
        let tasks_group = group("Tasks", task_rows, cx);
        let routines_group = group("Routines", routine_rows, cx);
        v_flex()
            .id("calendar-day-panel")
            .w(rems(19.))
            .flex_shrink_0()
            .h_full()
            .pt_1()
            .gap_4()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .px_2()
                    .gap_0p5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if day == self.today {
                                theme.primary
                            } else {
                                theme.muted_foreground
                            })
                            .child(relative),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(heading),
                    ),
            )
            .when(empty, |s| {
                s.child(
                    div()
                        .px_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("Nothing planned. Type above, or double-click the day."),
                )
            })
            .children(plans_group)
            .children(tasks_group)
            .children(routines_group)
            .child(
                h_flex().px_1().child(
                    Button::new("panel-open-day")
                        .ghost()
                        .small()
                        .label("Open in day view")
                        .on_click(cx.listener(move |this, _, _, cx| this.open_day(day, cx))),
                ),
            )
    }
}

// ---- Week and day ------------------------------------------------------

impl CalendarView {
    pub(super) fn time_grid(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let clock = appearance::settings(cx).clock;
        let first = self.span.days.start;
        let days: Vec<Date> = (0..)
            .map(|i| add_days(first, i))
            .take_while(|d| *d < self.span.days.end)
            .collect();
        let cols = days.len().max(1);
        let now_minute = i32::from(self.now.hour()) * 60 + i32::from(self.now.minute());

        // Scroll to the working day once per period: around now if today is
        // shown, else 8 AM.
        let key = (self.mode, first);
        if self.scrolled_for != Some(key) {
            self.scrolled_for = Some(key);
            let minute = if days.contains(&self.today) {
                (now_minute - 90).max(0)
            } else {
                8 * 60
            };
            let y = minute as f32 / 60. * HOUR * f32::from(window.rem_size());
            self.time_scroll.set_offset(point(px(0.), px(-y)));
        }

        // Header: weekday + date per column.
        let header = h_flex()
            .border_b_1()
            .border_color(theme.border)
            .child(div().w(rems(GUTTER)).flex_shrink_0())
            .children(days.iter().map(|&day| {
                let weekend = matches!(
                    day.weekday(),
                    jiff::civil::Weekday::Saturday | jiff::civil::Weekday::Sunday
                );
                h_flex()
                    .id(SharedString::from(format!("col-head-{day}")))
                    .flex_1()
                    .min_w_0()
                    .py_1p5()
                    .gap_1()
                    .justify_center()
                    .items_center()
                    .border_l_1()
                    .border_color(theme.border)
                    .text_xs()
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(day.strftime("%a").to_string()),
                    )
                    .child(self.day_number(day, weekend && day != self.today, cx))
                    .when(self.mode == CalendarMode::Week, |s| {
                        s.hover(|s| s.bg(theme.muted))
                            .on_click(cx.listener(move |this, _, _, cx| this.open_day(day, cx)))
                    })
            }));

        // All-day row: bars over the columns, then each day's tasks.
        let occurrences: Vec<Occurrence> = self
            .span
            .occurrences
            .iter()
            .filter(|o| self.shows(o, cx))
            .copied()
            .collect();
        let bars = bars_for(&occurrences, first, cols);
        let lanes = pack_spans(
            &bars
                .iter()
                .map(|b| (b.first_col, b.last_col))
                .collect::<Vec<_>>(),
        );
        let lane_rows = lanes.iter().max().map_or(0, |m| m + 1);
        let max_tasks = days
            .iter()
            .map(|d| self.span.tasks_on(*d).count())
            .max()
            .unwrap_or(0);
        let all_day_rows = (lane_rows + max_tasks).max(1);
        let all_day_cells: Vec<_> = days
            .iter()
            .map(|&day| {
                let tasks: Vec<AnyElement> = self
                    .span
                    .tasks_on(day)
                    .map(|t| self.task_chip(t, cx))
                    .collect::<Vec<_>>();
                v_flex()
                    .id(SharedString::from(format!("all-day-{day}")))
                    .flex_1()
                    .min_w_0()
                    .px_0p5()
                    .pb_1()
                    .border_l_1()
                    .border_color(theme.border)
                    .drag_over::<CalendarDrag>(|s, _, _, cx| s.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(move |this, drag: &CalendarDrag, window, cx| {
                        this.drop_on_day(day, drag, true, window, cx)
                    }))
                    .on_click(
                        cx.listener(move |this, e: &gpui_kit::ClickEvent, window, cx| {
                            if e.click_count() >= 2 {
                                this.create_all_day_on(day, window, cx);
                            }
                        }),
                    )
                    .child(div().h(rems(lane_rows as f32 * ROW)).flex_shrink_0())
                    .children(tasks)
            })
            .collect();
        let all_day_bars: Vec<AnyElement> = bars
            .iter()
            .zip(&lanes)
            .map(|(bar, lane)| self.bar(bar, *lane, cols, 0., "week", cx))
            .collect();
        let all_day = h_flex()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .w(rems(GUTTER))
                    .flex_shrink_0()
                    .pt_1()
                    .pr_2()
                    .text_right()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("all-day"),
            )
            .child(
                div()
                    .id("all-day-row")
                    .flex_1()
                    .min_w_0()
                    .max_h(rems(6. * ROW))
                    .overflow_y_scroll()
                    .child(
                        h_flex()
                            .relative()
                            .min_h(rems(all_day_rows as f32 * ROW + 0.25))
                            .children(all_day_cells)
                            .children(all_day_bars),
                    ),
            );

        // Hour labels.
        let gutter = div()
            .relative()
            .w(rems(GUTTER))
            .flex_shrink_0()
            .h_full()
            .children((1..24).map(|h| {
                div()
                    .absolute()
                    .top(rems(h as f32 * HOUR - 0.5))
                    .right_0()
                    .pr_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format_time(Time::constant(h, 0, 0, 0), clock))
            }));

        // Day columns: half-hour slots (double-click creates), then blocks.
        let hour_line = theme.border;
        let columns: Vec<_> = days
            .iter()
            .map(|&day| {
                let slots: Vec<_> = (0..48)
                    .map(|i| {
                        let minute = i * 30;
                        div()
                            .id(SharedString::from(format!("slot-{day}-{i}")))
                            .h(rems(HOUR / 2.))
                            .when(i > 0 && i % 2 == 0, |s| {
                                s.border_t_1().border_color(hour_line)
                            })
                            .on_click(cx.listener(
                                move |this, e: &gpui_kit::ClickEvent, window, cx| {
                                    window.focus(&this.focus, cx);
                                    if e.click_count() >= 2 {
                                        let start = day
                                            .to_datetime(Time::midnight())
                                            .checked_add(Span::new().minutes(i64::from(minute)));
                                        if let Ok(start) = start {
                                            this.create_at(start, window, cx);
                                        }
                                    } else {
                                        this.select(day, cx);
                                    }
                                },
                            ))
                    })
                    .collect();
                let timed: Vec<&Occurrence> = occurrences
                    .iter()
                    .filter(|o| o.days().contains(&day) && !is_bar(o))
                    .collect();
                let spans: Vec<(i32, i32)> = timed
                    .iter()
                    .map(|o| minutes_on(day, o.start, o.end))
                    .collect();
                let placed = lay_out_timed(&spans, MIN_BLOCK);
                let blocks: Vec<AnyElement> = timed
                    .iter()
                    .zip(spans.iter().zip(&placed))
                    .map(|(occ, (&(from, to), p))| {
                        self.block(occ, day, from, to, p.lane, p.lanes, clock, cx)
                    })
                    .collect();
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .border_l_1()
                    .border_color(theme.border)
                    .when(day == self.anchor && self.mode == CalendarMode::Week, |s| {
                        s.bg(theme.accent.opacity(0.5))
                    })
                    .children(slots)
                    .children(blocks)
                    .when(day == self.today, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(rems(now_minute as f32 / 60. * HOUR))
                                // A hairline: the now line must not hide text under it.
                                .h(px(1.5))
                                .bg(theme.primary)
                                .child(
                                    div()
                                        .absolute()
                                        .left(rems(-0.25))
                                        // Centered on the 1.5 px line.
                                        .top(rems(-0.21875))
                                        .size_2()
                                        .rounded_full()
                                        .bg(theme.primary),
                                ),
                        )
                    })
            })
            .collect();

        // Where a drag would land.
        let ghost =
            self.drop_target
                .filter(|_| cx.has_active_drag())
                .and_then(|(day, minute, minutes)| {
                    let col = days.iter().position(|d| *d == day)?;
                    Some(
                        div()
                            .absolute()
                            .left(relative(col as f32 / cols as f32))
                            .w(relative(1. / cols as f32))
                            .top(rems(minute as f32 / 60. * HOUR))
                            .h(rems(minutes.max(MIN_BLOCK) as f32 / 60. * HOUR))
                            .px_0p5()
                            .child(
                                div()
                                    .size_full()
                                    .rounded(theme.radius)
                                    .border_1()
                                    .border_color(theme.primary)
                                    .bg(theme.primary.opacity(0.08))
                                    .px_1p5()
                                    .text_xs()
                                    .text_color(theme.primary)
                                    .child(format_time(
                                        Time::constant(
                                            (minute / 60) as i8,
                                            (minute % 60) as i8,
                                            0,
                                            0,
                                        ),
                                        clock,
                                    )),
                            ),
                    )
                });

        let n = days.clone();
        let body = h_flex().h(rems(24. * HOUR)).child(gutter).child(
            h_flex()
                .id("time-columns")
                .relative()
                .flex_1()
                .min_w_0()
                .h_full()
                .on_drag_move(
                    cx.listener(move |this, e: &DragMoveEvent<CalendarDrag>, _, cx| {
                        let (b, p) = (e.bounds, e.event.position);
                        let target = b.contains(&p).then(|| {
                            let x = f32::from(p.x - b.left()) / f32::from(b.size.width).max(1.);
                            let col = ((x * n.len() as f32).floor() as usize).min(n.len() - 1);
                            let (grab, length) = match e.drag(cx) {
                                CalendarDrag::Event {
                                    grab_y,
                                    all_day: false,
                                    minutes,
                                    ..
                                } => (grab_y.get(), i32::try_from(*minutes).unwrap_or(60)),
                                _ => (0., 60),
                            };
                            let y = (f32::from(p.y - b.top()) - grab)
                                / f32::from(b.size.height).max(1.);
                            let raw =
                                (y * MINUTES_PER_DAY as f32 / SNAP as f32).round() as i32 * SNAP;
                            (n[col], raw.clamp(0, MINUTES_PER_DAY - SNAP), length)
                        });
                        if this.drop_target != target {
                            this.drop_target = target;
                            cx.notify();
                        }
                    }),
                )
                .on_drop(cx.listener(|this, drag: &CalendarDrag, window, cx| {
                    this.drop_on_grid(drag, window, cx)
                }))
                .children(columns)
                .children(ghost),
        );

        v_flex().size_full().px_6().pb_4().child(
            v_flex()
                .size_full()
                .border_t_1()
                .border_color(theme.border)
                .child(header)
                .child(all_day)
                .child(
                    div()
                        .id("time-grid")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.time_scroll)
                        .child(body),
                ),
        )
    }

    /// One timed block in a day column.
    #[allow(clippy::too_many_arguments)]
    fn block(
        &self,
        occ: &Occurrence,
        day: Date,
        from: i32,
        to: i32,
        lane: usize,
        lanes: usize,
        clock: ClockFormat,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let id = occ.event;
        let minutes = (to - from).max(MIN_BLOCK);
        // Two lines need an hour; shorter blocks put the start time inline.
        let tall = minutes >= 60;
        let inline_time = (!tall).then(|| short_time(occ.start.time(), clock));
        let event = self.span.events.get(&id);
        let location = event.map(|e| e.location().to_owned()).unwrap_or_default();
        let repeats = event.is_some_and(|e| e.recurrence().is_some());
        let past = occ.end <= self.now;
        // Routines sit back so one-off plans stand out.
        let fill = if self.is_routine(occ) { 0.08 } else { 0.16 };
        let el = div()
            .id(SharedString::from(format!("block-{id}-{day}")))
            .absolute()
            .top(rems(from as f32 / 60. * HOUR))
            .h(rems(minutes as f32 / 60. * HOUR))
            .left(relative(lane as f32 / lanes as f32))
            .w(relative(1. / lanes as f32))
            .px_0p5()
            .pb(rems(0.0625))
            .child(
                v_flex()
                    .size_full()
                    .overflow_hidden()
                    .px_1p5()
                    .py_0p5()
                    .rounded(theme.radius)
                    .bg(theme.primary.opacity(fill))
                    .hover(|s| s.bg(theme.primary.opacity(0.24)))
                    .when(past, |s| s.opacity(0.6))
                    .text_xs()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.title_of(occ)),
                            )
                            .when(repeats, |s| {
                                s.child(
                                    Icon::new(Lucide::Repeat)
                                        .xsmall()
                                        .text_color(theme.muted_foreground),
                                )
                            })
                            .children(inline_time.map(|t| {
                                div()
                                    .flex_shrink_0()
                                    .text_color(theme.muted_foreground)
                                    .child(t)
                            })),
                    )
                    .when(tall, |s| {
                        s.child(
                            div()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(format_span(occ.start, occ.end, clock)),
                        )
                    })
                    .when(tall && !location.is_empty(), |s| {
                        s.child(
                            div()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(location),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_event(id, window, cx);
            }));
        let drag = self.event_drag(occ);
        Self::draggable(el, drag).into_any_element()
    }
}
