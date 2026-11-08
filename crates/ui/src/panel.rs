//! `SidePanel`: the one resizable, collapsible side panel used everywhere
//! (the notes list today; inspectors and secondary lists later).
//!
//! Behaviour contract (same for every panel):
//! * Drag the inner edge to resize, clamped to `min..max`; double-click the edge resets.
//! * Collapse/expand animates width (≈180 ms, ease-out). Reduced motion snaps.
//! * Content keeps its full width while animating, so text never re-wraps mid-animation.
//! * The owner stores width/collapsed (see `LayoutStore` in the desktop app)
//!   and keeps the toggle button in a fixed spot that does not move when collapsed.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::InteractiveElementExt as _;
use gpui_kit::component::resizable::{HandleEdge, resize_handle};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, Axis, Context, DragMoveEvent,
    ElementId, InteractiveElement as _, IntoElement, ParentElement as _, Render, RenderOnce,
    SharedString, Styled as _, Window, div, ease_out_quint, px,
};

const TOGGLE_DURATION: Duration = Duration::from_millis(180);

/// Which side of the window the panel sits on. The resize edge is the inner side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
}

/// Drag payload for a panel's resize edge, carrying the panel id.
#[derive(Clone)]
pub struct ResizePanel(pub SharedString);

impl Render for ResizePanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

type ResizeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct SidePanel {
    id: SharedString,
    side: PanelSide,
    width: f32,
    default_width: f32,
    collapsed: bool,
    /// Bumped by the owner on every collapse/expand; drives one animation per toggle.
    toggles: u32,
    content: Option<AnyElement>,
    on_resize: Option<ResizeHandler>,
}

impl SidePanel {
    #[must_use]
    pub fn new(id: impl Into<SharedString>, side: PanelSide) -> Self {
        Self {
            id: id.into(),
            side,
            width: 280.,
            default_width: 280.,
            collapsed: false,
            toggles: 0,
            content: None,
            on_resize: None,
        }
    }

    /// Current expanded width in logical pixels, and the double-click reset width.
    #[must_use]
    pub fn width(mut self, width: f32, default_width: f32) -> Self {
        self.width = width;
        self.default_width = default_width;
        self
    }

    #[must_use]
    pub fn collapsed(mut self, collapsed: bool, toggles: u32) -> Self {
        self.collapsed = collapsed;
        self.toggles = toggles;
        self
    }

    /// Called with the requested width while dragging (unclamped; the owner clamps).
    #[must_use]
    pub fn on_resize(mut self, f: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_resize = Some(Rc::new(f));
        self
    }

    #[must_use]
    pub fn child(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }
}

impl RenderOnce for SidePanel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            side,
            width,
            default_width,
            collapsed,
            toggles,
            content,
            on_resize,
        } = self;
        // (pointer x, width) at mouse-down on the edge. Width follows the
        // pointer's movement from there, so grabbing anywhere on the edge never
        // makes the panel jump and the drag threshold doesn't lose pixels.
        let drag_start = window
            .use_keyed_state(
                SharedString::from(format!("{id}-drag-start")),
                cx,
                |_, _| Rc::new(Cell::new(None::<(f32, f32)>)),
            )
            .read(cx)
            .clone();
        let target = if collapsed { 0. } else { width };
        let from = if collapsed { width } else { 0. };

        let resize = on_resize.clone();
        let drag_id = id.clone();
        let move_start = drag_start.clone();
        let panel = div()
            .id(ElementId::Name(SharedString::from(format!("{id}-panel"))))
            .relative()
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .on_drag_move(move |e: &DragMoveEvent<ResizePanel>, window, cx| {
                if e.drag(cx).0 != drag_id {
                    return;
                }
                let x = f32::from(e.event.position.x);
                let (start_x, start_width) = move_start.get().unwrap_or_else(|| {
                    move_start.set(Some((x, width)));
                    (x, width)
                });
                let moved = x - start_x;
                let requested = match side {
                    PanelSide::Left => start_width + moved,
                    PanelSide::Right => start_width - moved,
                };
                if let Some(resize) = &resize {
                    resize(requested, window, cx);
                }
            })
            .child(
                // Full-width content, aligned to the outer edge, clipped by the animating box.
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(px(width))
                    .map(|this| match side {
                        PanelSide::Left => this.right_0(),
                        PanelSide::Right => this.left_0(),
                    })
                    .children(content),
            )
            .when(!collapsed, |this| {
                let reset = on_resize.clone();
                let drag_start = drag_start.clone();
                this.child(
                    resize_handle(SharedString::from(format!("{id}-edge")), Axis::Horizontal)
                        .inside(match side {
                            PanelSide::Left => HandleEdge::Trailing,
                            PanelSide::Right => HandleEdge::Leading,
                        })
                        // A new press starts a new drag measurement.
                        .on_mouse_down(gpui_kit::MouseButton::Left, move |e, _, _| {
                            drag_start.set(Some((f32::from(e.position.x), width)));
                        })
                        .on_drag(ResizePanel(id.clone()), |drag, _, _, cx| {
                            cx.new(|_| drag.clone())
                        })
                        .on_double_click(move |_, window, cx| {
                            if let Some(reset) = &reset {
                                reset(default_width, window, cx);
                            }
                        }),
                )
            });

        // First render (toggles == 0) and reduced motion show the final width directly.
        if toggles == 0 || cx.reduce_motion() {
            return panel.w(px(target)).into_any_element();
        }
        panel
            .with_animation(
                ElementId::Name(SharedString::from(format!("{id}-anim-{toggles}"))),
                Animation::new(TOGGLE_DURATION).with_easing(ease_out_quint()),
                move |this, t| this.w(px(from + (target - from) * t)),
            )
            .into_any_element()
    }
}
