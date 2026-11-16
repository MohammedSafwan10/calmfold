//! Focus / Pomodoro domain model and deterministic timer state machine.
//!
//! All time calculations take `now: Timestamp` explicitly so the timer never
//! drifts when the system sleeps or frames drop, and every transition is
//! unit-tested without wall-clock sleeps.

use serde::{Deserialize, Serialize};

use crate::{FocusSessionId, MAX_TITLE_CHARS, TaskId, Timestamp};

/// Minimum and maximum configurable block duration in minutes.
pub const MIN_DURATION_MINS: u16 = 1;
pub const MAX_DURATION_MINS: u16 = 180;

/// Common quick-pick focus durations (minutes).
pub const FOCUS_DURATION_PRESETS: [u16; 4] = [15, 25, 50, 90];

/// Kind of focus block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FocusKind {
    Focus,
    ShortBreak,
    LongBreak,
}

impl FocusKind {
    pub const ALL: [Self; 3] = [Self::Focus, Self::ShortBreak, Self::LongBreak];

    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "short_break",
            Self::LongBreak => "long_break",
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "focus" => Some(Self::Focus),
            "short_break" => Some(Self::ShortBreak),
            "long_break" => Some(Self::LongBreak),
            _ => None,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Focus => "Focus",
            Self::ShortBreak => "Short break",
            Self::LongBreak => "Long break",
        }
    }

    #[must_use]
    pub fn is_break(self) -> bool {
        matches!(self, Self::ShortBreak | Self::LongBreak)
    }

    #[must_use]
    pub fn duration_secs(self, preset: &FocusPreset) -> u32 {
        let mins = match self {
            Self::Focus => preset.focus_mins,
            Self::ShortBreak => preset.short_break_mins,
            Self::LongBreak => preset.long_break_mins,
        };
        u32::from(mins.clamp(MIN_DURATION_MINS, MAX_DURATION_MINS)) * 60
    }
}

/// User-configurable Pomodoro / deep-work durations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusPreset {
    pub focus_mins: u16,
    pub short_break_mins: u16,
    pub long_break_mins: u16,
    pub cycles_before_long: u8,
}

impl Default for FocusPreset {
    fn default() -> Self {
        Self {
            focus_mins: 25,
            short_break_mins: 5,
            long_break_mins: 15,
            cycles_before_long: 4,
        }
    }
}

impl FocusPreset {
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            focus_mins: self.focus_mins.clamp(MIN_DURATION_MINS, MAX_DURATION_MINS),
            short_break_mins: self
                .short_break_mins
                .clamp(MIN_DURATION_MINS, MAX_DURATION_MINS),
            long_break_mins: self
                .long_break_mins
                .clamp(MIN_DURATION_MINS, MAX_DURATION_MINS),
            cycles_before_long: self.cycles_before_long.clamp(1, 12),
        }
    }
}

/// A persisted focus or break session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusSession {
    pub id: FocusSessionId,
    pub task_id: Option<TaskId>,
    pub label: String,
    pub kind: FocusKind,
    pub planned_secs: u32,
    pub actual_secs: u32,
    pub started_at: Timestamp,
    pub ended_at: Timestamp,
    pub completed: bool,
}

impl FocusSession {
    /// Display title for logs: the custom/task label, or the kind label if empty.
    #[must_use]
    pub fn display_label(&self) -> &str {
        if self.label.is_empty() {
            self.kind.label()
        } else {
            &self.label
        }
    }
}

/// Aggregated focus totals for a single task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskFocusStats {
    pub total_secs: u64,
    pub sessions: u32,
}

/// Current state of the in-memory focus timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Idle,
    Running {
        session_started_at: Timestamp,
        segment_started_at: Timestamp,
        elapsed_before_pause: u32,
    },
    Paused {
        session_started_at: Timestamp,
        elapsed_secs: u32,
    },
}

/// Deterministic focus timer state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusTimer {
    pub state: TimerState,
    pub kind: FocusKind,
    pub planned_secs: u32,
    pub task_id: Option<TaskId>,
    pub label: String,
    /// Number of completed Focus blocks in the current cycle (`0..cycles_before_long`).
    pub cycle_completed: u8,
    pub cycles_before_long: u8,
}

impl FocusTimer {
    #[must_use]
    pub fn new(preset: &FocusPreset) -> Self {
        let preset = preset.clamped();
        Self {
            state: TimerState::Idle,
            kind: FocusKind::Focus,
            planned_secs: FocusKind::Focus.duration_secs(&preset),
            task_id: None,
            label: String::new(),
            cycle_completed: 0,
            cycles_before_long: preset.cycles_before_long,
        }
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self.state, TimerState::Running { .. })
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        matches!(self.state, TimerState::Paused { .. })
    }

    #[must_use]
    pub fn is_idle(&self) -> bool {
        matches!(self.state, TimerState::Idle)
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.is_idle()
    }

    /// Starts or resumes the timer at `now`.
    pub fn start(&mut self, now: Timestamp) {
        match self.state {
            TimerState::Idle => {
                self.state = TimerState::Running {
                    session_started_at: now,
                    segment_started_at: now,
                    elapsed_before_pause: 0,
                };
            }
            TimerState::Paused {
                session_started_at,
                elapsed_secs,
            } => {
                self.state = TimerState::Running {
                    session_started_at,
                    segment_started_at: now,
                    elapsed_before_pause: elapsed_secs,
                };
            }
            TimerState::Running { .. } => {}
        }
    }

    /// Pauses a running timer at `now`.
    pub fn pause(&mut self, now: Timestamp) {
        if let TimerState::Running {
            session_started_at, ..
        } = self.state
        {
            let elapsed = self.elapsed_secs(now);
            self.state = TimerState::Paused {
                session_started_at,
                elapsed_secs: elapsed,
            };
        }
    }

    /// Resets the current block to Idle without logging a session.
    pub fn reset(&mut self) {
        self.state = TimerState::Idle;
    }

    /// Selects a mode while idle. Running and paused blocks must be explicitly
    /// finished or reset first, so choosing a mode cannot discard progress.
    /// Returns whether the selection was applied.
    pub fn set_kind(&mut self, kind: FocusKind, preset: &FocusPreset) -> bool {
        if self.is_active() {
            return false;
        }
        let preset = preset.clamped();
        self.kind = kind;
        self.planned_secs = kind.duration_secs(&preset);
        self.cycles_before_long = preset.cycles_before_long;
        self.state = TimerState::Idle;
        true
    }

    /// Sets a custom duration in minutes (only applies when Idle; otherwise resets first).
    pub fn set_duration_mins(&mut self, mins: u16) {
        let mins = mins.clamp(MIN_DURATION_MINS, MAX_DURATION_MINS);
        self.planned_secs = u32::from(mins) * 60;
        self.state = TimerState::Idle;
    }

    /// Attaches or clears a linked task and display label.
    pub fn attach_task(&mut self, task_id: Option<TaskId>, label: impl AsRef<str>) {
        self.task_id = task_id;
        self.label = clamp_label(label.as_ref());
    }

    /// Seconds elapsed in the current block at `now` (capped at `planned_secs`).
    #[must_use]
    pub fn elapsed_secs(&self, now: Timestamp) -> u32 {
        let raw = match self.state {
            TimerState::Idle => 0,
            TimerState::Running {
                segment_started_at,
                elapsed_before_pause,
                ..
            } => {
                let delta = now
                    .as_second()
                    .saturating_sub(segment_started_at.as_second());
                let delta_u32 = u32::try_from(delta.max(0)).unwrap_or(u32::MAX);
                elapsed_before_pause.saturating_add(delta_u32)
            }
            TimerState::Paused { elapsed_secs, .. } => elapsed_secs,
        };
        raw.min(self.planned_secs)
    }

    /// Seconds remaining in the current block at `now`.
    #[must_use]
    pub fn remaining_secs(&self, now: Timestamp) -> u32 {
        self.planned_secs.saturating_sub(self.elapsed_secs(now))
    }

    /// Completion fraction in `0.0..=1.0`.
    #[must_use]
    pub fn progress(&self, now: Timestamp) -> f32 {
        if self.planned_secs == 0 {
            return 1.0;
        }
        (self.elapsed_secs(now) as f32 / self.planned_secs as f32).clamp(0.0, 1.0)
    }

    /// True when a running timer has reached `planned_secs`.
    #[must_use]
    pub fn has_reached_end(&self, now: Timestamp) -> bool {
        self.is_running() && self.remaining_secs(now) == 0
    }

    /// Completes or stops the current block at `now`, returning a `FocusSession`
    /// to persist if at least 1 second elapsed (or `completed` is true).
    ///
    /// Resets the timer to `Idle` and advances the Pomodoro cycle counter when
    /// a `Focus` block completes.
    pub fn finish(&mut self, now: Timestamp, completed: bool) -> Option<FocusSession> {
        let (started_at, actual_secs) = match self.state {
            TimerState::Idle => return None,
            TimerState::Running {
                session_started_at, ..
            } => (session_started_at, self.elapsed_secs(now)),
            TimerState::Paused {
                session_started_at,
                elapsed_secs,
            } => (session_started_at, elapsed_secs.min(self.planned_secs)),
        };

        self.state = TimerState::Idle;

        if actual_secs == 0 && !completed {
            return None;
        }

        if completed && self.kind == FocusKind::Focus {
            self.cycle_completed = (self.cycle_completed + 1) % self.cycles_before_long.max(1);
        }

        Some(FocusSession {
            id: FocusSessionId::new(),
            task_id: self.task_id,
            label: self.label.clone(),
            kind: self.kind,
            planned_secs: self.planned_secs,
            actual_secs,
            started_at,
            ended_at: now,
            completed,
        })
    }

    /// Suggests the next `FocusKind` after finishing a `Focus` or break block.
    #[must_use]
    pub fn next_kind_after(&self, finished_kind: FocusKind) -> FocusKind {
        match finished_kind {
            FocusKind::Focus => {
                if self.cycle_completed == 0 {
                    // `cycle_completed` wrapped back to 0 after hitting `cycles_before_long`.
                    FocusKind::LongBreak
                } else {
                    FocusKind::ShortBreak
                }
            }
            FocusKind::ShortBreak | FocusKind::LongBreak => FocusKind::Focus,
        }
    }
}

fn clamp_label(raw: &str) -> String {
    raw.trim().chars().take(MAX_TITLE_CHARS).collect()
}

/// Formats `secs` as `MM:SS` (e.g. `"25:00"`, `"04:09"`).
#[must_use]
pub fn format_mm_ss(secs: u32) -> String {
    let mins = secs / 60;
    let rem = secs % 60;
    format!("{mins:02}:{rem:02}")
}

/// Formats a duration in seconds compactly (`"45s"`, `"25m"`, `"1h 15m"`).
#[must_use]
pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let total_mins = secs / 60;
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours == 0 {
        format!("{mins}m")
    } else if mins == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {mins}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("ts")
    }

    #[test]
    fn start_pause_resume_tracks_wall_clock_without_drift() {
        let preset = FocusPreset::default();
        let mut timer = FocusTimer::new(&preset);
        assert!(timer.is_idle());
        assert_eq!(timer.remaining_secs(ts(1_000)), 25 * 60);

        timer.start(ts(1_000));
        assert!(timer.is_running());
        assert_eq!(timer.elapsed_secs(ts(1_300)), 300);
        assert_eq!(timer.remaining_secs(ts(1_300)), 1_200);

        // Pause at +300s, wait 500s while paused, resume at 1800s.
        timer.pause(ts(1_300));
        assert!(timer.is_paused());
        assert_eq!(timer.elapsed_secs(ts(1_800)), 300);

        timer.start(ts(1_800));
        assert_eq!(timer.elapsed_secs(ts(2_100)), 600);
        assert_eq!(timer.remaining_secs(ts(2_100)), 900);

        let session = timer.finish(ts(2_100), true).expect("session");
        assert_eq!(
            session.started_at,
            ts(1_000),
            "started_at must stay at the original session start across pauses"
        );
        assert_eq!(session.actual_secs, 600);
    }

    #[test]
    fn completion_caps_at_planned_and_advances_cycle() {
        let preset = FocusPreset {
            focus_mins: 1,
            short_break_mins: 1,
            long_break_mins: 2,
            cycles_before_long: 2,
        };
        let mut timer = FocusTimer::new(&preset);
        let task_id = TaskId::new();
        timer.attach_task(Some(task_id), "Write tests");
        timer.start(ts(100));

        // Overshoot by 30s (e.g. laptop slept): caps at 60s.
        assert!(timer.has_reached_end(ts(190)));
        assert_eq!(timer.elapsed_secs(ts(190)), 60);
        assert_eq!(timer.remaining_secs(ts(190)), 0);

        let session = timer.finish(ts(190), true).expect("session");
        assert_eq!(session.actual_secs, 60);
        assert_eq!(session.task_id, Some(task_id));
        assert_eq!(session.label, "Write tests");
        assert!(session.completed);
        assert_eq!(timer.cycle_completed, 1);
        assert_eq!(
            timer.next_kind_after(FocusKind::Focus),
            FocusKind::ShortBreak
        );

        // Second focus completes -> wraps to 0 -> suggests LongBreak.
        timer.set_kind(FocusKind::Focus, &preset);
        timer.start(ts(300));
        let _ = timer.finish(ts(360), true).expect("session 2");
        assert_eq!(timer.cycle_completed, 0);
        assert_eq!(
            timer.next_kind_after(FocusKind::Focus),
            FocusKind::LongBreak
        );
    }

    #[test]
    fn mode_selection_preserves_every_active_block() {
        let preset = FocusPreset::default();
        for original_kind in FocusKind::ALL {
            for selected_kind in FocusKind::ALL {
                for paused in [false, true] {
                    let mut timer = FocusTimer::new(&preset);
                    assert!(timer.set_kind(original_kind, &preset));
                    timer.attach_task(Some(TaskId::new()), "Keep my progress");
                    timer.start(ts(100));
                    if paused {
                        timer.pause(ts(130));
                    }
                    let before = timer.clone();
                    assert!(!timer.set_kind(selected_kind, &preset));
                    assert_eq!(timer, before);
                    assert_eq!(timer.elapsed_secs(ts(160)), if paused { 30 } else { 60 });

                    timer.reset();
                    assert!(timer.set_kind(selected_kind, &preset));
                    assert_eq!(timer.kind, selected_kind);
                    assert!(timer.is_idle());
                    assert_eq!(timer.planned_secs, selected_kind.duration_secs(&preset));
                }
            }
        }
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(format_mm_ss(0), "00:00");
        assert_eq!(format_mm_ss(65), "01:05");
        assert_eq!(format_mm_ss(25 * 60), "25:00");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(25 * 60), "25m");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(4500), "1h 15m");
    }
}
