//! Habits → Insights: the numbers behind the habits.
//!
//! Reading order: four headline numbers (today, this week vs last, last 30
//! days, best running streak) → completion per day for 30 days → which
//! weekdays work → one row per habit (30-day rate, strength and its 4-week
//! trend, best streak). Rows open the habit. Math lives in
//! `ottero_core::habit_insights`; this file only lays it out.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::chart::BarChart;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Corners, Div, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    TestSupportExt as _, div, px, relative, rems,
};
use ottero_core::habit::StreakUnit;
use ottero_core::habit_insights::{
    HabitHistory, day_rate, period_rate, weekday_rates, weekday_takeaway,
};
use ottero_ui::components::{empty_state, section_label};

use crate::views::habit_detail::open_habit_detail;
use crate::views::habit_widgets::add_days;
use crate::views::habits::HabitsView;

const CHART_DAYS: i64 = 30;
const WEEKDAY_WEEKS: i64 = 12;
const TREND_DAYS: i64 = 28;

fn pct(r: f64) -> i64 {
    (r * 100.).round() as i64
}

fn card(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .gap_3()
        .p_4()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
}

fn tile(label: &'static str, value: String, note: Option<String>, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_w(rems(9.))
        .gap_1()
        .p_4()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_2xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child(value),
        )
        .child(
            div()
                .h(rems(1.))
                .text_xs()
                .text_color(theme.muted_foreground)
                .truncate()
                .children(note),
        )
}

/// Thin horizontal meter for a 0..1 value.
fn meter(value: f64, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .h(rems(0.375))
        .w_full()
        .rounded_full()
        .bg(theme.muted)
        .child(
            div()
                .h_full()
                .w(relative(value.clamp(0., 1.) as f32))
                .rounded_full()
                .bg(theme.primary),
        )
}

impl HabitsView {
    pub(super) fn insights(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.habits.is_empty() {
            return empty_state(
                Lucide::ChartColumn,
                "No insights yet",
                "Add a habit and check in for a few days. Trends show up here.",
                cx,
            )
            .into_any_element();
        }
        let today = self.today;
        let hist: Vec<HabitHistory<'_>> = self
            .habits
            .iter()
            .map(|h| HabitHistory {
                habit: &h.habit,
                counts: &h.counts,
                first: h.first,
            })
            .collect();

        // ---- Headline numbers ----
        let due: Vec<_> = self.habits.iter().filter(|h| h.due_today).collect();
        let done_today = due.iter().filter(|h| h.done_today()).count();
        let monday = add_days(today, -i64::from(today.weekday().to_monday_zero_offset()));
        let this_week = period_rate(&hist, monday..add_days(today, 1), today);
        let last_week = period_rate(&hist, add_days(monday, -7)..monday, today);
        let week_note = match (this_week, last_week) {
            (Some(a), Some(b)) => {
                let delta = pct(a) - pct(b);
                Some(match delta {
                    0 => "Same as last week".to_owned(),
                    d if d > 0 => format!("Up {d} pts from last week"),
                    d => format!("Down {} pts from last week", -d),
                })
            }
            (Some(_), None) => Some("First week".to_owned()),
            (None, _) => Some("No check-ins yet this week".to_owned()),
        };
        let month = period_rate(
            &hist,
            add_days(today, -(CHART_DAYS - 1))..add_days(today, 1),
            today,
        );
        let best = self.habits.iter().max_by_key(|h| {
            (
                h.stats.current_streak,
                h.stats.streak_unit == StreakUnit::Weeks,
            )
        });
        let (best_value, best_note) = match best.filter(|h| h.stats.current_streak > 0) {
            Some(h) => (
                h.stats.streak_label(h.stats.current_streak),
                Some(h.habit.name().to_owned()),
            ),
            None => ("—".to_owned(), Some("Check in to start one".to_owned())),
        };
        let tiles = h_flex()
            .flex_wrap()
            .gap_3()
            .child(tile(
                "Today",
                format!("{done_today} of {}", due.len()),
                Some(if due.is_empty() {
                    "Nothing scheduled".to_owned()
                } else if done_today == due.len() {
                    "All done".to_owned()
                } else {
                    format!("{} to go", due.len() - done_today)
                }),
                cx,
            ))
            .child(tile(
                "This week",
                this_week.map_or("—".into(), |r| format!("{}%", pct(r))),
                week_note,
                cx,
            ))
            .child(tile(
                "Last 30 days",
                month.map_or("—".into(), |r| format!("{}%", pct(r))),
                Some("of planned check-ins".to_owned()),
                cx,
            ))
            .child(tile("Longest running streak", best_value, best_note, cx));

        // ---- Completion per day ----
        // Quiet bars; full strength only where the eye should land (today,
        // the best weekday).
        let solid = theme.primary;
        let calm = theme.primary.opacity(0.55);
        let faint = theme.muted_foreground.opacity(0.25);
        let today_label: SharedString = today.strftime("%b %-d").to_string().into();
        let daily: Vec<(SharedString, f64, bool)> = (0..CHART_DAYS)
            .rev()
            .map(|k| {
                let day = add_days(today, -k);
                let rate = day_rate(&hist, day, today);
                let label = if day.day() == 1 || k == CHART_DAYS - 1 || k == 0 {
                    day.strftime("%b %-d").to_string()
                } else {
                    day.strftime("%-d").to_string()
                };
                (label.into(), rate.map_or(0., |r| r * 100.), rate.is_some())
            })
            .collect();
        let daily_card = card(cx)
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label("Completion per day", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Last 30 days"),
                    ),
            )
            .child(
                div()
                    .id("habit-daily-chart")
                    .h(rems(9.))
                    .test_support()
                    .child(
                        BarChart::new(daily)
                            .band(|d: &(SharedString, f64, bool)| d.0.clone())
                            .value(|d: &(SharedString, f64, bool)| d.1.max(2.))
                            .fill(move |d: &(SharedString, f64, bool), _, _, _| {
                                if !d.2 || d.1 <= 0. {
                                    faint
                                } else if d.0 == today_label {
                                    solid
                                } else {
                                    calm
                                }
                            })
                            .max_band_width(px(14.))
                            .tick_margin(6)
                            .corner_radii(Corners {
                                top_left: px(2.),
                                top_right: px(2.),
                                ..Default::default()
                            })
                            .grid(false),
                    ),
            );

        // ---- Weekdays ----
        let rates = weekday_rates(&hist, today, WEEKDAY_WEEKS);
        let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let best_day = rates
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.map(|r| (i, r)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| SharedString::from(names[i]));
        let week_data: Vec<(SharedString, f64, bool)> = names
            .iter()
            .zip(rates)
            .map(|(n, r)| {
                (
                    SharedString::from(*n),
                    r.map_or(0., |r| r * 100.),
                    r.is_some(),
                )
            })
            .collect();
        let weekday_card =
            card(cx)
                .flex_1()
                .min_w(rems(18.))
                .child(section_label("By weekday", cx))
                .child(
                    div().id("habit-weekday-chart").h(rems(7.)).child(
                        BarChart::new(week_data)
                            .band(|d: &(SharedString, f64, bool)| d.0.clone())
                            .value(|d: &(SharedString, f64, bool)| d.1.max(2.))
                            .fill(move |d: &(SharedString, f64, bool), _, _, _| {
                                if !d.2 || d.1 <= 0. {
                                    faint
                                } else if best_day.as_ref() == Some(&d.0) {
                                    solid
                                } else {
                                    calm
                                }
                            })
                            .max_band_width(px(28.))
                            .corner_radii(Corners {
                                top_left: px(3.),
                                top_right: px(3.),
                                ..Default::default()
                            })
                            .grid(false),
                    ),
                )
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    weekday_takeaway(&rates).unwrap_or_else(|| {
                        "A few more weeks of check-ins show your best days.".into()
                    }),
                ));

        // ---- Strength ranking (the steadiest habits) ----
        let mut by_strength: Vec<_> = self.habits.iter().collect();
        by_strength.sort_by(|a, b| b.stats.strength.total_cmp(&a.stats.strength));
        let strength_card = card(cx)
            .flex_1()
            .min_w(rems(18.))
            .child(section_label("Strength", cx))
            .children(by_strength.iter().take(5).map(|h| {
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .text_sm()
                            .child(div().truncate().child(h.habit.name().to_owned()))
                            .child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{}%", pct(h.stats.strength))),
                            ),
                    )
                    .child(meter(h.stats.strength, cx))
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Grows with every check-in; one missed day barely moves it."),
            );

        // ---- Each habit ----
        let head =
            |w: f32, text: &'static str| div().w(rems(w)).flex_shrink_0().text_right().child(text);
        let table_rows = self.habits.iter().map(|h| {
            let id = h.habit.id();
            let series = h
                .habit
                .strength_series(&h.counts, h.first, today, TREND_DAYS + 1);
            let trend = series
                .first()
                .zip(series.last())
                .map(|(a, b)| pct(b.1) - pct(a.1))
                .unwrap_or(0);
            let rate = h.stats.rate_30;
            let library = self.library.clone();
            h_flex()
                .id(SharedString::from(format!("insight-{id}")))
                .gap_4()
                .px_4()
                .py_2p5()
                .items_center()
                .border_t_1()
                .border_color(theme.border)
                .hover(|s| s.bg(theme.list_hover))
                .on_click(move |_, window, cx| open_habit_detail(id, library.clone(), window, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().text_sm().child(h.habit.name().to_owned()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(h.habit.schedule().to_string()),
                        ),
                )
                .child(
                    h_flex()
                        .w(rems(9.))
                        .flex_shrink_0()
                        .gap_2()
                        .child(div().flex_1().child(meter(rate.unwrap_or(0.), cx)))
                        .child(
                            div()
                                .w(rems(2.5))
                                .text_right()
                                .text_sm()
                                .child(rate.map_or("—".into(), |r| format!("{}%", pct(r)))),
                        ),
                )
                .child(
                    h_flex()
                        .w(rems(5.5))
                        .flex_shrink_0()
                        .gap_1()
                        .justify_end()
                        .text_sm()
                        .child(format!("{}%", pct(h.stats.strength)))
                        .child(
                            h_flex()
                                .w(rems(2.75))
                                .gap_0p5()
                                .justify_end()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .when(trend != 0, |s| {
                                    s.child(
                                        Icon::new(if trend > 0 {
                                            Lucide::TrendingUp
                                        } else {
                                            Lucide::TrendingDown
                                        })
                                        .xsmall(),
                                    )
                                    .child(format!("{}", trend.abs()))
                                }),
                        ),
                )
                .child(
                    div()
                        .w(rems(5.))
                        .flex_shrink_0()
                        .text_right()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(h.stats.streak_label(h.stats.best_streak)),
                )
        });
        let table = v_flex()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .child(
                h_flex()
                    .gap_4()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().child("Habit"))
                    .child(head(9., "Last 30 days"))
                    .child(head(5.5, "Strength · 4 wk"))
                    .child(head(5., "Best streak")),
            )
            .children(table_rows);

        v_flex()
            .gap_4()
            .child(tiles)
            .child(daily_card)
            .child(
                h_flex()
                    .flex_wrap()
                    .items_start()
                    .gap_4()
                    .child(weekday_card)
                    .child(strength_card),
            )
            .child(table)
            .into_any_element()
    }
}
