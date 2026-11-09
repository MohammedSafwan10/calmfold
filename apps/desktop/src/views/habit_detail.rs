//! Habit panel: a right-hand sheet for one habit.
//!
//! Top to bottom: name (autosaves), how often, daily goal, four numbers
//! (streak, best, strength, last 30 days), the history heatmap, then
//! Archive / Delete. Schedule and goal save immediately.

use std::time::Duration;

use calmfold_core::habit::{MAX_TARGET, Schedule, StreakUnit};
use calmfold_core::{HabitId, Weekdays, clock};
use calmfold_ui::components::{section_label, segmented};
use calmfold_ui::feedback::{self, Confirm};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::LineChart;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Placement, Selectable as _, Sizable as _, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task as GpuiTask, Window,
    div, px, rems,
};

use crate::library::{HabitSummary, Library, LibraryEvent};
use crate::views::habit_widgets::heatmap;
use crate::views::task_row::notify_err;

const AUTOSAVE_DELAY: Duration = Duration::from_millis(500);
const HEATMAP_WEEKS: i64 = 22;

pub fn open_habit_detail(id: HabitId, library: Entity<Library>, window: &mut Window, cx: &mut App) {
    let detail = cx.new(|cx| HabitDetail::new(id, library, window, cx));
    window.open_sheet_at(Placement::Right, cx, move |sheet, _, _| {
        sheet.title("Habit").size(px(460.)).child(detail.clone())
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frequency {
    Daily,
    Days,
    PerWeek,
}

pub struct HabitDetail {
    id: HabitId,
    library: Entity<Library>,
    habit: Option<HabitSummary>,
    name: Entity<InputState>,
    dirty: bool,
    pending_save: Option<GpuiTask<()>>,
    _subscriptions: Vec<Subscription>,
}

impl HabitDetail {
    pub fn new(
        id: HabitId,
        library: Entity<Library>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let habit = library.read(cx).habit(id);
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Habit name")
                .validate(|text, _| text.chars().count() <= calmfold_core::MAX_TITLE_CHARS)
        });
        if let Some(h) = &habit {
            name.update(cx, |n, cx| {
                n.set_value(h.habit.name().to_owned(), window, cx)
            });
        }
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.schedule_save(window, cx),
                InputEvent::Blur | InputEvent::PressEnter { .. } => this.flush(window, cx),
                _ => {}
            }),
            cx.subscribe(&library, |this, library, event, cx| {
                if *event == LibraryEvent::HabitsChanged {
                    this.habit = library.read(cx).habit(this.id);
                    cx.notify();
                }
            }),
            cx.on_release(|this, cx| {
                if this.dirty {
                    _ = this.save(cx);
                }
            }),
        ];
        Self {
            id,
            library,
            habit,
            name,
            dirty: false,
            pending_save: None,
            _subscriptions: subscriptions,
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
        if self.dirty
            && let Err(err) = self.save(cx)
        {
            feedback::error_toast(format!("{err:#}"), window, cx);
        }
    }

    fn save(&mut self, cx: &mut App) -> anyhow::Result<()> {
        let name = self.name.read(cx).value().to_string();
        let id = self.id;
        let current = self.library.read(cx).habit(id);
        if !name.trim().is_empty()
            && current.is_some_and(|h| !h.habit.is_archived() && h.habit.name() != name.trim())
        {
            self.library
                .update(cx, |lib, cx| lib.rename_habit(id, &name, cx))?;
        }
        self.dirty = false;
        Ok(())
    }

    fn set_schedule(&mut self, schedule: Schedule, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        notify_err(
            self.library
                .update(cx, |lib, cx| lib.set_habit_schedule(id, schedule, cx)),
            window,
            cx,
        );
    }

    fn set_target(&mut self, target: u16, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        notify_err(
            self.library
                .update(cx, |lib, cx| lib.set_habit_target(id, target, cx)),
            window,
            cx,
        );
    }

    fn archive(&mut self, archive: bool, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        let library = self.library.clone();
        let result = library.update(cx, |lib, cx| {
            if archive {
                lib.archive_habit(id, cx)
            } else {
                lib.unarchive_habit(id, cx)
            }
        });
        if feedback::report(result, window, cx).is_some() && archive {
            self.dirty = false;
            window.close_sheet(cx);
            feedback::undo_toast("Habit archived.", window, cx, move |window, cx| {
                notify_err(
                    library.update(cx, |l, cx| l.unarchive_habit(id, cx)),
                    window,
                    cx,
                );
            });
        }
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(h) = &self.habit else { return };
        let id = self.id;
        let library = self.library.clone();
        feedback::confirm_destructive(
            Confirm::new(
                format!("Delete \u{201c}{}\u{201d}?", h.habit.name()),
                "Its check-in history is deleted too. To keep the history, archive it instead.",
                "Delete habit",
            ),
            window,
            cx,
            move |window, cx| {
                if feedback::report(
                    library.update(cx, |l, cx| l.delete_habit(id, cx)),
                    window,
                    cx,
                )
                .is_some()
                {
                    window.close_sheet(cx);
                }
            },
        );
    }

    fn frequency_editor(&self, schedule: Schedule, cx: &mut Context<Self>) -> impl IntoElement {
        let freq = match schedule {
            Schedule::Daily => Frequency::Daily,
            Schedule::Days(_) => Frequency::Days,
            Schedule::PerWeek(_) => Frequency::PerWeek,
        };
        let view = cx.entity().downgrade();
        let picker = segmented(
            "habit-frequency",
            &[
                (Frequency::Daily, "Every day"),
                (Frequency::Days, "On days"),
                (Frequency::PerWeek, "Times a week"),
            ],
            freq,
            move |f, window, cx| {
                let schedule = match f {
                    Frequency::Daily => Schedule::Daily,
                    Frequency::Days => Schedule::Days(Weekdays::WORKDAYS),
                    Frequency::PerWeek => Schedule::PerWeek(3),
                };
                _ = view.update(cx, |this, cx| this.set_schedule(schedule, window, cx));
            },
        );
        let detail = match schedule {
            Schedule::Daily => None,
            Schedule::Days(days) => Some(
                h_flex()
                    .gap_1()
                    .children(Weekdays::ORDER.into_iter().map(|day| {
                        let on = days.contains(day);
                        // The last day can't be turned off: a schedule needs one.
                        let next = if on {
                            days.without(day)
                        } else {
                            days.with(day)
                        };
                        Button::new(SharedString::from(format!(
                            "habit-day-{}",
                            day.to_monday_zero_offset()
                        )))
                        .small()
                        .w(rems(2.75))
                        .label(calmfold_core::habit::short_day(day))
                        .selected(on)
                        .map(|b| if on { b.primary() } else { b.outline() })
                        .disabled(on && days.len() == 1)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.set_schedule(Schedule::Days(next), window, cx)
                            },
                        ))
                    }))
                    .into_any_element(),
            ),
            Schedule::PerWeek(n) => Some(
                stepper(
                    "habit-per-week",
                    u16::from(n),
                    1,
                    6,
                    if n == 1 {
                        "time a week"
                    } else {
                        "times a week"
                    },
                    cx.listener(|this, value: &u16, window, cx| {
                        let n = u8::try_from(*value).unwrap_or(1);
                        this.set_schedule(Schedule::PerWeek(n), window, cx)
                    }),
                    cx,
                )
                .into_any_element(),
            ),
        };
        v_flex().gap_2().child(picker).children(detail)
    }
}

/// − value + with a unit label.
fn stepper(
    id: &'static str,
    value: u16,
    min: u16,
    max: u16,
    unit: &'static str,
    on_change: impl Fn(&u16, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let on_change = std::rc::Rc::new(on_change);
    let dec = on_change.clone();
    h_flex()
        .gap_2()
        .items_center()
        .child(
            Button::new(SharedString::from(format!("{id}-minus")))
                .outline()
                .small()
                .icon(Lucide::Minus)
                .disabled(value <= min)
                .on_click(move |_, window, cx| dec(&value.saturating_sub(1).max(min), window, cx)),
        )
        .child(
            div()
                .min_w(rems(2.))
                .text_center()
                .font_weight(FontWeight::SEMIBOLD)
                .child(value.to_string()),
        )
        .child(
            Button::new(SharedString::from(format!("{id}-plus")))
                .outline()
                .small()
                .icon(Lucide::Plus)
                .disabled(value >= max)
                .on_click(move |_, window, cx| on_change(&(value + 1).min(max), window, cx)),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(unit),
        )
}

fn strength_chart(h: &HabitSummary, today: calmfold_core::Date, cx: &App) -> impl IntoElement {
    let series: Vec<(SharedString, f64)> = h
        .habit
        .strength_series(&h.counts, h.first, today, 90)
        .into_iter()
        .map(|(d, v)| {
            (
                SharedString::from(d.strftime("%b %-d").to_string()),
                v * 100.,
            )
        })
        .collect();
    div().id("habit-strength-chart").h(rems(7.)).child(
        LineChart::new(series)
            .x(|d: &(SharedString, f64)| d.0.clone())
            .y(|d: &(SharedString, f64)| d.1)
            .stroke(cx.theme().primary)
            .natural()
            .y_domain(0., 100.)
            .tick_margin(30)
            .grid(false),
    )
}

fn stat_tile(label: &'static str, value: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_w(rems(6.))
        .gap_1()
        .p_3()
        .rounded(theme.radius_lg)
        .bg(theme.group_box)
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child(value),
        )
}

impl Render for HabitDetail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(h) = self.habit.clone() else {
            return v_flex().child(div().text_sm().child("This habit no longer exists."));
        };
        let today = clock::today();
        let theme = cx.theme().clone();
        let archived = h.habit.is_archived();
        let s = h.stats;
        let unit = |n: usize| match (s.streak_unit, n) {
            (StreakUnit::Days, 1) => "1 day".to_owned(),
            (StreakUnit::Days, n) => format!("{n} days"),
            (StreakUnit::Weeks, 1) => "1 week".to_owned(),
            (StreakUnit::Weeks, n) => format!("{n} weeks"),
        };
        let rate = s.rate_30.map_or("—".to_owned(), |r| {
            format!("{}%", (r * 100.).round() as i64)
        });
        let strength_word = match s.strength {
            x if x >= 0.85 => "Rock solid",
            x if x >= 0.6 => "Strong",
            x if x >= 0.3 => "Building",
            _ => "Getting started",
        };

        v_flex()
            .gap_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Input::new(&self.name)
                            .id("habit-name")
                            .large()
                            .disabled(archived)
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(if archived {
                                "Archived · history kept".to_owned()
                            } else {
                                format!("{} · {strength_word}", h.habit.schedule())
                            }),
                    ),
            )
            .when(!archived, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .child(section_label("How often", cx))
                        .child(self.frequency_editor(h.habit.schedule(), cx)),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(section_label("Daily goal", cx))
                        .child(stepper(
                            "habit-target",
                            h.habit.target(),
                            1,
                            MAX_TARGET,
                            if h.habit.target() == 1 {
                                "time a day"
                            } else {
                                "times a day"
                            },
                            cx.listener(|this, value: &u16, window, cx| {
                                this.set_target(*value, window, cx)
                            }),
                            cx,
                        )),
                )
            })
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(stat_tile("Current streak", unit(s.current_streak), cx))
                    .child(stat_tile("Best streak", unit(s.best_streak), cx))
                    .child(stat_tile(
                        "Strength",
                        format!("{}%", (s.strength * 100.).round() as i64),
                        cx,
                    ))
                    .child(stat_tile("Last 30 days", rate, cx)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_label("Strength, last 90 days", cx))
                    .child(strength_chart(&h, today, cx)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_label(
                        format!("History · {} days done", s.total),
                        cx,
                    ))
                    .child(heatmap(&h, today, HEATMAP_WEEKS, cx)),
            )
            .child(
                h_flex()
                    .pt_2()
                    .gap_2()
                    .child(
                        Button::new("habit-archive")
                            .ghost()
                            .small()
                            .icon(if archived {
                                Lucide::ArchiveRestore
                            } else {
                                Lucide::Archive
                            })
                            .label(if archived { "Restore" } else { "Archive" })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.archive(!archived, window, cx)
                            })),
                    )
                    .child(
                        Button::new("habit-delete")
                            .ghost()
                            .small()
                            .icon(Lucide::Trash)
                            .label("Delete…")
                            .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                    ),
            )
    }
}
