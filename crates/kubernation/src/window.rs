//! A reusable centered modal window — the chrome that drill-down content
//! shares (the Almanac now; richer city/node detail next). One modal is open
//! at a time; it draws a dimmed scrim, a parchment-framed panel with a title
//! bar + icon, a button/tab row, and hands back a clipped body rect the
//! caller fills (culling + scrolling its own content, since macroquad has no
//! easy scissor). Mirrors the 4X window structure (titlebar / body /
//! buttons) in the KuberNation parchment palette.

use kubernation_core::state::planned::Intervention;
use macroquad::prelude::*;

use crate::text::{text, text_bold, text_size};
use crate::theme::*;

const TITLE_H: f32 = 30.0;
const BTN_H: f32 = 26.0;
const PAD: f32 = 14.0;

/// Hit regions a drawn window hands back for this frame.
pub struct WinLayout {
    pub frame: Rect,
    /// Content region — draw here, culling rows outside it yourself.
    pub body: Rect,
    /// Bottom button/tab rects, in the order passed.
    pub buttons: Vec<Rect>,
    /// The top-right close box.
    pub close: Rect,
}

impl WinLayout {
    /// Index of the button under `p`, if any.
    pub fn button_at(&self, p: Vec2) -> Option<usize> {
        self.buttons.iter().position(|r| r.contains(p))
    }
}

/// What a frame's interaction on a drill-down window asks the caller to do.
/// Shared by the city and node windows.
#[derive(Default)]
pub struct WinAction {
    pub close: bool,
    /// A pod whose logs to tail: (namespace, pod, start-on-previous-container).
    /// `previous` is the smart crash-loop default (the view can still toggle it).
    pub log: Option<(String, String, bool)>,
    /// A pod the operator asked to evict: (namespace, pod). The caller confirms
    /// before anything is written.
    pub evict: Option<(String, String)>,
    /// A pod the operator asked to port-forward: (namespace, pod). The net
    /// thread resolves the port + binds the listener.
    pub forward: Option<(String, String)>,
    /// A live forward the operator asked to stop, by its local port.
    pub stop_forward: Option<u16>,
    /// A pod whose YAML to inspect: (namespace, pod).
    pub inspect: Option<(String, String)>,
    /// An intervention the operator staged from this window (planning turn).
    pub stage: Option<Intervention>,
    /// Toggle a staged rolling-restart for this workload (stage if absent,
    /// unstage if present — the caller has the planned world to decide).
    pub restart_toggle: Option<kubernation_core::state::model::WorkloadRef>,
    /// Set (Some(target)) or reset (None) this workload's in-session SLO target
    /// override — the treasury stepper.
    pub slo_target: Option<(kubernation_core::state::model::WorkloadRef, Option<f64>)>,
}

/// **The placement authority.** Where a window of `size` lands on a `sw` x `sh`
/// screen: capped to the screen less a margin, then centred.
///
/// PURE, unit-tested, and the ONE home for this derivation. Drawing, hit-testing
/// and scroll routing all have to agree about where a window's edge is, and
/// before this they agreed by convention — `panel_frame` and `panel_split_x`
/// each re-derived the same clamp-and-centre, with doc comments saying they
/// "mirror" `draw_window`. Nothing enforced the mirroring, so moving the
/// geometry meant a three-way edit that a reader had to know to make.
///
/// That is the shape this codebase has paid for repeatedly (`resolve_region`,
/// `derive_qos`, `worst_level`, `changed_hands`, `fresh_tier`, `slot_of_row`,
/// `terrain_order`): when two consumers must agree about a derived value, the
/// derivation gets a name and one home.
/// Where a window sits. Centred is the default and what twelve of the fourteen
/// windows use; only the two drill-downs dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Centred over the whole screen, with a dimming scrim. Right for a window
    /// that is not about a map location — the Almanac, About, the Charter.
    Centred,
    /// Against the right edge of the PLAY AREA, leaving map to its west and the
    /// docked column to its east. No scrim: the point is to keep the map
    /// legible, and dimming it would give back most of what docking bought.
    DockRightOfMap,
}

/// The placement authority: the ONE home for where a window lands.
pub fn window_rect_at(size: Vec2, sw: f32, sh: f32, place: Place) -> Rect {
    let w = size.x.min(sw - 40.0);
    let h = size.y.min(sh - 40.0);
    match place {
        Place::Centred => Rect::new(((sw - w) / 2.0).floor(), ((sh - h) / 2.0).floor(), w, h),
        Place::DockRightOfMap => {
            // Right edge flush with the play area, so the docked column stays
            // visible — it is the other context surface, and hiding it to show
            // the map would just move the problem.
            let play_w = crate::panels::play_width(sw);
            let w = w.min(play_w);
            let top = crate::panels::CHROME_H + 8.0;
            let h = h.min(sh - top - 8.0);
            Rect::new((play_w - w).floor().max(0.0), top.floor(), w, h)
        }
    }
}

/// The map strip left west of a docked drill-down, as a screen rect.
///
/// D1's acceptance criterion is that the SUBJECT stays visible, so the camera
/// centres on this rather than on the play area — a province centred in the play
/// area would sit under the panel, which moves the keyhole rather than removing
/// it. Returns `None` when the strip is too narrow to be worth aiming at, so a
/// small window degrades to not panning rather than to panning somewhere absurd.
pub fn map_strip(sw: f32, sh: f32) -> Option<Rect> {
    let r = window_rect_at(
        crate::panels::panel_size(sw, sh),
        sw,
        sh,
        Place::DockRightOfMap,
    );
    let top = crate::panels::CHROME_H;
    (r.x >= MIN_STRIP).then(|| Rect::new(0.0, top, r.x, (sh - top).max(0.0)))
}

/// Below this the strip shows too little to locate anything in, which is one of
/// D1 §7.2's stated failure criteria — so we decline to aim at it.
const MIN_STRIP: f32 = 220.0;

/// Draw the scrim, frame, title bar (with icon), and bottom button row;
/// return the hit regions. `active` highlights that button as the current
/// tab (pass `usize::MAX` for none).
pub fn draw_window(title: &str, size: Vec2, buttons: &[&str], active: usize) -> WinLayout {
    draw_window_at(title, size, buttons, active, Place::Centred)
}

/// [`draw_window`], with the placement chosen. Only the two drill-downs pass
/// anything but [`Place::Centred`].
pub fn draw_window_at(
    title: &str,
    size: Vec2,
    buttons: &[&str],
    active: usize,
    place: Place,
) -> WinLayout {
    // The scrim belongs to a MODAL. A docked drill-down deliberately has none:
    // dimming the map would give back most of what docking it bought.
    if place == Place::Centred {
        draw_rectangle(
            0.0,
            0.0,
            screen_width(),
            screen_height(),
            Color::new(0.0, 0.0, 0.0, 0.5),
        );
    }
    let frame = window_rect_at(size, screen_width(), screen_height(), place);
    let (x, y, w, h) = (frame.x, frame.y, frame.w, frame.h);
    let mp = Vec2::from(mouse_position());

    draw_rectangle(x, y, w, h, PANEL);
    draw_rectangle_lines(x, y, w, h, 2.0, PARCHMENT);

    // Title bar.
    draw_rectangle(x, y, w, TITLE_H, darker(PANEL, 0.7));
    draw_rectangle(x, y + TITLE_H, w, 1.5, PARCHMENT);
    draw_icon(vec2(x + 8.0, y + 6.0), 18.0);
    text_bold(title, x + 34.0, y + 21.0, 18.0, PARCHMENT);

    // Close box (Esc also closes; this mirrors 4X's window button).
    let close = Rect::new(x + w - 26.0, y + 5.0, 20.0, 20.0);
    let close_bg = if close.contains(mp) {
        lighter(PLATE, 1.9)
    } else {
        PLATE
    };
    draw_rectangle(close.x, close.y, close.w, close.h, close_bg);
    draw_rectangle_lines(close.x, close.y, close.w, close.h, 1.0, PARCHMENT);
    text("x", close.x + 6.0, close.y + 15.0, 16.0, INK);

    // Bottom button / tab row.
    let row_y = y + h - BTN_H - 8.0;
    let mut rects = Vec::new();
    if !buttons.is_empty() {
        let n = buttons.len() as f32;
        let bw = (w - PAD * 2.0 - (n - 1.0) * 8.0) / n;
        for (i, label) in buttons.iter().enumerate() {
            let bx = x + PAD + i as f32 * (bw + 8.0);
            let r = Rect::new(bx, row_y, bw, BTN_H);
            let on = i == active;
            let bg = if on {
                PARCHMENT
            } else if r.contains(mp) {
                lighter(PLATE, 1.7)
            } else {
                PLATE
            };
            draw_rectangle(r.x, r.y, r.w, r.h, bg);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, PARCHMENT);
            let tc = if on { PANEL } else { INK };
            let tm = text_size(ascii(label), 14.0);
            text(
                ascii(label),
                r.x + (r.w - tm.width) / 2.0,
                r.y + 17.0,
                14.0,
                tc,
            );
            rects.push(r);
        }
    }

    let body_top = y + TITLE_H + 10.0;
    let body_bottom = if buttons.is_empty() {
        y + h - 10.0
    } else {
        row_y - 8.0
    };
    let body = Rect::new(
        x + PAD,
        body_top,
        w - PAD * 2.0,
        (body_bottom - body_top).max(0.0),
    );
    WinLayout {
        frame,
        body,
        buttons: rects,
        close,
    }
}

/// A small hover-revealed row button (e.g. the `yaml` affordance on pod rows),
/// in the structure-cyan ink. Returns true if clicked this frame.
pub fn row_button(r: Rect, mouse: Vec2, click: bool, label: &str) -> bool {
    let hot = r.contains(mouse);
    draw_rectangle(
        r.x,
        r.y,
        r.w,
        r.h,
        if hot { lighter(PLATE, 1.9) } else { PLATE },
    );
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, STRUCT);
    let tm = text_size(label, 12.0);
    text(
        label,
        r.x + (r.w - tm.width) / 2.0,
        r.y + r.h / 2.0 + 4.0,
        12.0,
        STRUCT,
    );
    hot && click
}

/// A per-pod evict button, revealed on row hover. RBAC-aware via `allowed`:
/// `Some(true)` = enabled (red, destructive), `Some(false)` = disabled
/// ("locked" — no delete permission), `None` = the permission probe is still
/// in flight ("…"). Returns true only when clicked while enabled.
pub fn evict_button(r: Rect, mouse: Vec2, click: bool, allowed: Option<bool>) -> bool {
    let on = r.contains(mouse);
    let (fill, border, tc, label) = match allowed {
        Some(true) => (
            if on { CRIT } else { darker(CRIT, 0.55) },
            CRIT,
            if on { INK } else { lighter(CRIT, 1.5) },
            "evict",
        ),
        Some(false) => (darker(PLATE, 1.3), DIM, DIM, "locked"),
        None => (darker(PLATE, 1.3), DIM, DIM, "..."),
    };
    draw_rectangle(r.x, r.y, r.w, r.h, fill);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, border);
    let tm = text_size(label, 12.0);
    text(
        label,
        r.x + (r.w - tm.width) / 2.0,
        r.y + r.h - 4.0,
        12.0,
        tc,
    );
    on && click && allowed == Some(true)
}

/// What a click on a per-pod forward button asks for.
pub enum ForwardBtn {
    /// Start a new forward for this pod.
    Start,
    /// Stop the live forward (its local port is known to the caller).
    Stop,
}

/// A per-pod port-forward button, revealed on row hover. State-aware:
/// `active = Some(local_port)` → a green "stop :PORT" (hover shows "stop");
/// otherwise RBAC-gated like evict (`Some(true)` enabled "fwd", `Some(false)`
/// "locked", `None` probing "…"). Returns the requested action when clicked.
pub fn forward_button(
    r: Rect,
    mouse: Vec2,
    click: bool,
    allowed: Option<bool>,
    active: Option<u16>,
) -> Option<ForwardBtn> {
    let on = r.contains(mouse);
    if let Some(port) = active {
        let fill = if on { good() } else { darker(good(), 0.5) };
        draw_rectangle(r.x, r.y, r.w, r.h, fill);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, good());
        // At rest show the port; on hover show the action.
        let label = if on {
            "stop".to_string()
        } else {
            format!(":{port}")
        };
        let tc = if on { INK } else { lighter(good(), 1.4) };
        let tm = text_size(&label, 12.0);
        text(
            &label,
            r.x + (r.w - tm.width) / 2.0,
            r.y + r.h - 4.0,
            12.0,
            tc,
        );
        return (on && click).then_some(ForwardBtn::Stop);
    }
    let (fill, border, tc, label) = match allowed {
        Some(true) => (
            if on {
                darker(good(), 0.7)
            } else {
                darker(good(), 0.45)
            },
            good(),
            if on { INK } else { lighter(good(), 1.3) },
            "fwd",
        ),
        Some(false) => (darker(PLATE, 1.3), DIM, DIM, "locked"),
        None => (darker(PLATE, 1.3), DIM, DIM, "..."),
    };
    draw_rectangle(r.x, r.y, r.w, r.h, fill);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, border);
    let tm = text_size(label, 12.0);
    text(
        label,
        r.x + (r.w - tm.width) / 2.0,
        r.y + r.h - 4.0,
        12.0,
        tc,
    );
    (on && click && allowed == Some(true)).then_some(ForwardBtn::Start)
}

/// A little book/scroll glyph for the title bar.
fn draw_icon(p: Vec2, s: f32) {
    draw_rectangle(p.x, p.y, s, s, PARCHMENT);
    draw_rectangle_lines(p.x, p.y, s, s, 1.0, darker(PARCHMENT, 0.5));
    draw_line(
        p.x + s * 0.5,
        p.y + 2.0,
        p.x + s * 0.5,
        p.y + s - 2.0,
        1.0,
        darker(PARCHMENT, 0.45),
    );
    draw_line(
        p.x + 3.0,
        p.y + s * 0.35,
        p.x + s * 0.45,
        p.y + s * 0.35,
        1.0,
        darker(PARCHMENT, 0.45),
    );
}
