//! App-wide focus timer controller.
//!
//! Lives in `AppShell` so the timer keeps running when the user switches to
//! Notes, Tasks, or Calendar, and only spawns a 1-second tick while `Running`.

use std::time::Duration;

use calmfold_core::{FocusKind, FocusPreset, FocusSession, FocusTimer, TaskId, clock};
use gpui_kit::{App, Context, Entity, EventEmitter, Global, Task as GpuiTask, Window};

use crate::library::Library;

const TICK_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusEvent {
    Changed,
    Completed(FocusSession),
}

#[derive(Clone)]
struct GlobalFocus(Entity<FocusController>);

impl Global for GlobalFocus {}

/// Registers the app-wide focus controller so any screen (e.g. task rows) can start a session.
pub fn install(focus: &Entity<FocusController>, cx: &mut App) {
    cx.set_global(GlobalFocus(focus.clone()));
}

/// Starts a focus session attached to `task_id`.
pub fn start_task_focus(task_id: TaskId, title: &str, window: &mut Window, cx: &mut App) {
    if let Some(focus) = cx.try_global::<GlobalFocus>().map(|g| g.0.clone()) {
        focus.update(cx, |f, cx| f.start_task_focus(task_id, title, window, cx));
    }
}

pub struct FocusController {
    library: Entity<Library>,
    timer: FocusTimer,
    preset: FocusPreset,
    just_finished: Option<FocusSession>,
    save_error: Option<String>,
    _tick: Option<GpuiTask<()>>,
}

impl EventEmitter<FocusEvent> for FocusController {}

impl FocusController {
    #[must_use]
    pub fn new(library: Entity<Library>) -> Self {
        let preset = FocusPreset::default();
        Self {
            library,
            timer: FocusTimer::new(&preset),
            preset,
            just_finished: None,
            save_error: None,
            _tick: None,
        }
    }

    #[must_use]
    pub fn timer(&self) -> &FocusTimer {
        &self.timer
    }

    #[must_use]
    pub fn preset(&self) -> FocusPreset {
        self.preset
    }

    #[must_use]
    pub fn just_finished(&self) -> Option<&FocusSession> {
        self.just_finished.as_ref()
    }

    #[must_use]
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    pub fn dismiss_finished(&mut self, cx: &mut Context<Self>) {
        if self.just_finished.take().is_some() {
            cx.emit(FocusEvent::Changed);
            cx.notify();
        }
    }

    pub fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.just_finished = None;
        self.save_error = None;
        self.timer.start(clock::now());
        self.ensure_ticking(window, cx);
        cx.emit(FocusEvent::Changed);
        cx.notify();
    }

    pub fn pause(&mut self, cx: &mut Context<Self>) {
        self.timer.pause(clock::now());
        self._tick = None;
        cx.emit(FocusEvent::Changed);
        cx.notify();
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.timer.is_running() {
            self.pause(cx);
        } else {
            self.start(window, cx);
        }
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.timer.reset();
        self._tick = None;
        self.just_finished = None;
        self.save_error = None;
        cx.emit(FocusEvent::Changed);
        cx.notify();
    }

    pub fn set_kind(&mut self, kind: FocusKind, cx: &mut Context<Self>) {
        if !self.timer.set_kind(kind, &self.preset) {
            return;
        }
        self._tick = None;
        self.just_finished = None;
        cx.emit(FocusEvent::Changed);
        cx.notify();
    }

    pub fn set_duration_mins(&mut self, mins: u16, cx: &mut Context<Self>) {
        self.timer.set_duration_mins(mins);
        self._tick = None;
        self.just_finished = None;
        cx.emit(FocusEvent::Changed);
        cx.notify();
    }

    pub fn attach_task(
        &mut self,
        task_id: Option<TaskId>,
        label: impl AsRef<str>,
        cx: &mut Context<Self>,
    ) {
        self.timer.attach_task(task_id, label);
        cx.emit(FocusEvent::Changed);
        cx.notify();
    }

    pub fn start_task_focus(
        &mut self,
        task_id: TaskId,
        title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.timer.set_kind(FocusKind::Focus, &self.preset) {
            return;
        }
        self.timer.attach_task(Some(task_id), title);
        self.start(window, cx);
    }

    pub fn finish_now(&mut self, completed: bool, cx: &mut Context<Self>) -> anyhow::Result<()> {
        self._tick = None;
        let now = clock::now();
        let prev_timer = self.timer.clone();
        let session = self.timer.finish(now, completed);
        if let Some(session) = session {
            if let Err(err) = self
                .library
                .update(cx, |lib, cx| lib.record_focus_session(&session, cx))
            {
                // Keep the session in a paused state so the user never loses it on a failed save.
                self.timer = prev_timer;
                if self.timer.is_running() {
                    self.timer.pause(now);
                }
                self.save_error = Some(format!("Couldn't save focus session: {err}"));
                cx.emit(FocusEvent::Changed);
                cx.notify();
                return Err(err);
            }
            self.save_error = None;
            if completed {
                self.just_finished = Some(session.clone());
            }
            cx.emit(FocusEvent::Completed(session));
        }
        cx.emit(FocusEvent::Changed);
        cx.notify();
        Ok(())
    }

    pub fn start_suggested_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = self
            .just_finished
            .as_ref()
            .map_or(FocusKind::ShortBreak, |s| {
                self.timer.next_kind_after(s.kind)
            });
        self.timer.set_kind(next, &self.preset);
        self.just_finished = None;
        self.start(window, cx);
    }

    fn ensure_ticking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self._tick.is_some() {
            return;
        }
        self._tick = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK_INTERVAL).await;
                let keep_going = this.update(cx, |this, cx| this.on_tick(cx));
                match keep_going {
                    Ok(true) => {}
                    _ => break,
                }
            }
        }));
    }

    fn on_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.timer.is_running() {
            self._tick = None;
            return false;
        }
        let now = clock::now();
        if self.timer.has_reached_end(now) {
            if let Err(err) = self.finish_now(true, cx) {
                eprintln!("calmfold: couldn't record focus session: {err:#}");
            }
            return false;
        }
        cx.emit(FocusEvent::Changed);
        cx.notify();
        true
    }
}
