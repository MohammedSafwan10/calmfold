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
        .filter(|o| o.all_day || o.days().end > add_days(o.days().start, 1))
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

/// Whether an occurrence is drawn as a bar (vs. a chip / time block).
fn is_bar(o: &Occurrence) -> bool {
    o.all_day || o.days().end > add_days(o.days().start, 1)
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
                    .flex_shrink_0()
                    .text_color(theme.muted_foreground)
                    .child(format_time(occ.start.time(), clock)),
            )
            .child(div().min_w_0().truncate().child(self.title_of(occ)))
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
        // How many item lines fit in a cell at this window height.
        let rem = f32::from(window.rem_size()).max(1.);
        let grid = f32::from(window.viewport_size().height) / rem - MONTH_CHROME;
        let row_h = (grid / weeks.len().max(1) as f32).max(6.);
        let slots = (((row_h - DAY_HEADER - 0.25) / ROW).floor() as usize).clamp(1, 12);
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
        let rows = weeks
            .into_iter()
            .enumerate()
            .map(|(w, week)| {
                let bars = bars_for(&self.span.occurrences, week, 7);
                let lanes = pack_spans(
                    &bars
                        .iter()
                        .map(|b| (b.first_col, b.last_col))
                        .collect::<Vec<_>>(),
                );
                let lane_rows = lanes.iter().max().map_or(0, |m| m + 1);
                let shown_lanes = lane_rows.min(slots);
                let cells: Vec<_> = (0..7)
                    .map(|col| {
                        let day = add_days(week, col as i64);
                        let hidden_bars = bars
                            .iter()
                            .zip(&lanes)
                            .filter(|(b, lane)| {
                                **lane >= shown_lanes && (b.first_col..=b.last_col).contains(&col)
                            })
                            .count();
                        let chips: Vec<&Occurrence> =
                            self.span.on_day(day).filter(|o| !is_bar(o)).collect();
                        let tasks: Vec<_> = self.span.tasks_on(day).collect();
                        let room = slots - shown_lanes;
                        let total = chips.len() + tasks.len();
                        let visible = total.min(room);
                        let more = hidden_bars + total - visible;
                        let mut items: Vec<AnyElement> = Vec::new();
                        for occ in chips.iter().take(visible) {
                            items.push(self.event_chip(occ, day, clock, cx));
                        }
                        for task in tasks.iter().take(visible.saturating_sub(chips.len())) {
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
                            .when(col < 6, |s| s.border_r_1().border_color(theme.border))
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
                                    .justify_between()
                                    .child(self.day_number(day, outside, cx))
                                    .when(more > 0, |s| {
                                        s.child(
                                            Button::new(SharedString::from(format!("more-{day}")))
                                                .ghost()
                                                .xsmall()
                                                .label(format!("{more} more"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.open_day(day, cx);
                                                })),
                                        )
                                    }),
                            )
                            // Room for the bars drawn over the row.
                            .child(div().h(rems(shown_lanes as f32 * ROW)).flex_shrink_0())
                            .child(v_flex().min_h_0().overflow_hidden().children(items))
                    })
                    .collect();
                let bar_elements: Vec<AnyElement> = bars
                    .iter()
                    .zip(&lanes)
                    .filter(|(_, lane)| **lane < shown_lanes)
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

        v_flex().size_full().px_6().pb_4().child(weekday_row).child(
            v_flex()
                .id("month-grid")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .border_t_1()
                .border_color(theme.border)
                .children(rows),
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
        let bars = bars_for(&self.span.occurrences, first, cols);
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
                let timed: Vec<&Occurrence> =
                    self.span.on_day(day).filter(|o| !is_bar(o)).collect();
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
                                .h_0p5()
                                .bg(theme.primary)
                                .child(
                                    div()
                                        .absolute()
                                        .left(rems(-0.25))
                                        .top(rems(-0.1875))
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
        let tall = minutes >= 45;
        let event = self.span.events.get(&id);
        let location = event.map(|e| e.location().to_owned()).unwrap_or_default();
        let repeats = event.is_some_and(|e| e.recurrence().is_some());
        let past = occ.end <= self.now;
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
                    .bg(theme.primary.opacity(0.16))
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
                            }),
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
