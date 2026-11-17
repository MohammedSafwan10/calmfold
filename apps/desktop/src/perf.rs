//! Frame and input-latency recorder for `scripts/perf.sh` (feature `perf`).
//!
//! Uses GPUI's own profiler, so the numbers are what the window really did:
//!
//! - **input latency**: from the start of each platform input that changed the
//!   window (a real keystroke or scroll) to the end of the presentation that
//!   first showed it, computed from the foreground journal;
//! - **frames**: draw time and whether each presented frame missed its refresh
//!   budget, counted from the first input on (startup is excluded);
//! - **slow views / tasks / actions**: anything on the main thread that took
//!   long enough to cost a frame, by name or source location.
//!
//! Every 500 ms the totals are written to `OTTERO_PERF_OUT` as JSON.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::profiler::journal::{ForegroundEvent, ForegroundJournalEntry, IntervalBoundary};
use gpui_kit::{AnyWindowHandle, App};
use serde_json::{Value, json};

/// Main-thread work at least this long is listed by name.
const SLOW: Duration = Duration::from_millis(4);

#[derive(Default)]
struct Totals {
    started: bool,
    kinds: HashMap<&'static str, u64>,
    /// Inputs that changed the window and haven't been shown yet (their start times).
    pending_inputs: Vec<std::time::Instant>,
    /// Input start → end of the presentation that first showed it.
    latencies: Vec<Duration>,
    draws: Vec<Duration>,
    presented: u64,
    late: u64,
    missed_refreshes: u64,
    refresh_interval: Option<Duration>,
    views: HashMap<&'static str, (u64, Duration, Duration)>,
    tasks: HashMap<String, (u64, Duration)>,
    actions: HashMap<&'static str, (u64, Duration)>,
}

impl Totals {
    fn push(&mut self, entry: ForegroundJournalEntry) {
        let kind = match &entry {
            ForegroundJournalEntry::Event(e) => match e {
                ForegroundEvent::Input(i) if i.caused_invalidation => "input+",
                ForegroundEvent::Input(_) => "input",
                ForegroundEvent::Draw(_) => "draw",
                ForegroundEvent::Present(_) => "present",
                ForegroundEvent::View(_) => "view",
                ForegroundEvent::Action(_) => "action",
                ForegroundEvent::TaskPoll(_) => "task",
                ForegroundEvent::SmallPolls(_) => "small",
            },
            ForegroundJournalEntry::Boundary(IntervalBoundary::Presented(_)) => "presented",
            ForegroundJournalEntry::Boundary(IntervalBoundary::FrameSkipped(f)) => match f.reason {
                gpui_kit::profiler::journal::FrameSkipReason::NoRenderNeeded => "skip:none_needed",
                gpui_kit::profiler::journal::FrameSkipReason::InactiveFrameRateLimit => {
                    "skip:inactive"
                }
                gpui_kit::profiler::journal::FrameSkipReason::ThermalFrameRateLimit => {
                    "skip:thermal"
                }
                gpui_kit::profiler::journal::FrameSkipReason::LowPowerFrameRateLimit => {
                    "skip:low_power"
                }
            },
            ForegroundJournalEntry::Boundary(_) => "boundary",
            ForegroundJournalEntry::Discontinuity { .. } => "lost",
            _ => "other",
        };
        *self.kinds.entry(kind).or_default() += 1;
        match entry {
            ForegroundJournalEntry::Event(ForegroundEvent::Input(input)) => {
                self.started = true;
                // Keys and scrolls are what the user waits on. GPUI's
                // `caused_invalidation` misses them: views notify after the
                // handler returns, so it can't be used as the filter.
                if matches!(input.kind, "key_down" | "scroll") {
                    self.pending_inputs.push(input.start.into());
                }
            }
            _ if !self.started => {}
            ForegroundJournalEntry::Event(ForegroundEvent::Draw(frame)) => {
                self.draws.push(frame.draw_duration());
                self.refresh_interval = frame.refresh_interval.or(self.refresh_interval);
            }
            ForegroundJournalEntry::Event(ForegroundEvent::View(view)) => {
                let e = self.views.entry(view.name).or_default();
                e.0 += 1;
                e.1 += view.self_duration;
                e.2 = e.2.max(view.self_duration);
            }
            ForegroundJournalEntry::Event(ForegroundEvent::TaskPoll(task)) => {
                let took = task.poll_duration();
                if took >= SLOW {
                    let at = format!("{}:{}", task.location.file(), task.location.line());
                    let e = self.tasks.entry(at).or_default();
                    e.0 += 1;
                    e.1 = e.1.max(took);
                }
            }
            ForegroundJournalEntry::Event(ForegroundEvent::Action(action)) => {
                let took = action.end.duration_since(action.start);
                if took >= SLOW {
                    let e = self.actions.entry(action.name).or_default();
                    e.0 += 1;
                    e.1 = e.1.max(took);
                }
            }
            ForegroundJournalEntry::Boundary(IntervalBoundary::Presented(p)) => {
                self.presented += 1;
                let shown: std::time::Instant = p.presentation.present_end.into();
                for start in self.pending_inputs.drain(..) {
                    self.latencies.push(shown.saturating_duration_since(start));
                }
                let submitted = p.presentation.present_start;
                if p.frame.is_late(submitted) == Some(true) {
                    self.late += 1;
                }
                self.missed_refreshes += p.frame.missed_refreshes(submitted).unwrap_or(0);
            }
            _ => {}
        }
    }

    fn report(&self) -> Value {
        let q = |values: &[Duration], p: f64| {
            let mut v = values.to_vec();
            v.sort();
            v.get(((v.len() as f64 - 1.) * p).round() as usize)
                .map_or(0., |d| ms(*d))
        };
        let latency = json!({
            "count": self.latencies.len(),
            "p50": q(&self.latencies, 0.5), "p95": q(&self.latencies, 0.95),
            "p99": q(&self.latencies, 0.99), "max": q(&self.latencies, 1.),
            "over_16_7ms": self.latencies.iter().filter(|d| d.as_secs_f64() > 1. / 60.).count(),
        });
        let draws = &self.draws;
        let mut views: Vec<_> = self
            .views
            .iter()
            .map(|(name, (n, total, max))| (*name, *n, *total, *max))
            .collect();
        views.sort_by_key(|v| std::cmp::Reverse(v.2));
        let mut tasks: Vec<_> = self.tasks.iter().collect();
        tasks.sort_by_key(|t| std::cmp::Reverse(t.1.1));
        json!({
            "input_latency_ms": latency,
            "journal": self.kinds,
            "refresh_interval_ms": self.refresh_interval.map(ms),
            "frames": {
                "drawn": draws.len(),
                "presented": self.presented,
                "late": self.late,
                "missed_refreshes": self.missed_refreshes,
                "draw_ms": { "p50": q(draws, 0.5), "p95": q(draws, 0.95), "p99": q(draws, 0.99), "max": q(draws, 1.) },
            },
            "slow_views": views.iter().take(6).map(|(name, n, total, max)| json!({
                "view": name, "count": n, "total_ms": ms(*total), "max_ms": ms(*max),
            })).collect::<Vec<_>>(),
            "slow_tasks": tasks.iter().take(6).map(|(at, (n, max))| json!({
                "at": at, "count": n, "max_ms": ms(*max),
            })).collect::<Vec<_>>(),
            "slow_actions": self.actions.iter().map(|(name, (n, max))| json!({
                "action": name, "count": n, "max_ms": ms(*max),
            })).collect::<Vec<_>>(),
        })
    }
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1e5).round() / 100.
}

/// Starts recording if `OTTERO_PERF_OUT` is set.
pub fn install(window: AnyWindowHandle, cx: &mut App) {
    let Some(out) = std::env::var_os("OTTERO_PERF_OUT") else {
        return;
    };
    gpui_kit::profiler::set_trace_enabled(true);
    let mut collector = cx.foreground_journal().collector();
    let mut totals = Totals::default();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            for entry in collector.collect_unseen().entries {
                totals.push(entry);
            }
            // Stop when the window is gone.
            if window.update(cx, |_, _, _| ()).is_err() {
                break;
            }
            let report = totals.report();
            if let Ok(text) = serde_json::to_string_pretty(&report) {
                // A best-effort dev file: a failed write just skips this sample.
                _ = std::fs::write(&out, text);
            }
        }
    })
    .detach();
}
