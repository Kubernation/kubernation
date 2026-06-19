//! A classic-4X dropdown menu bar for the GUI chrome. Replaces the scattered
//! chrome buttons (almanac `?`, End-Turn, namespace filter) with Game / View /
//! Orders / Advisors / World / Help menus — the iconic menu bar of the genre, in the
//! Kubernation carved-stone palette. Immediate-mode like the rest of the GUI:
//! `draw_menu_bar` both paints the bar (+ any open dropdown) and hit-tests,
//! returning the chosen action. An open menu is GUI-loop state
//! (`open: &mut Option<usize>`); the caller suspends map nav while it's set.

use macroquad::prelude::*;

use crate::advisor::AdvisorTab;
use crate::draw::Overlay;
use crate::panels::CHROME_H;
use crate::text::{text, text_bold, text_size};
use crate::theme::*;

/// A command issued by clicking a menu item. The caller maps each to existing
/// GUI state (open a picker, fit the camera, set the overlay, …).
#[derive(Clone, Copy, PartialEq)]
pub enum MenuAction {
    SwitchContext,
    Fit,
    Quit,
    SetOverlay(Overlay),
    EndTurn,
    DiscardTurn,
    ChaosOpen,
    NamespaceFilter,
    Advisor(AdvisorTab),
    Charter,
    Almanac,
    Annals,
}

/// Live state the bar reflects: the active overlay (radio mark), the staged
/// intervention count (Orders badge + enable), and whether a namespace filter
/// is active (World check).
pub struct MenuCtx {
    pub overlay: Overlay,
    pub staged: usize,
    pub ns_active: bool,
}

/// One dropdown row. A `None` action with a non-empty label is a non-clickable
/// section header / footnote; an empty label is a separator rule.
struct Item {
    label: String,
    action: Option<MenuAction>,
    checked: bool,
    enabled: bool,
}

impl Item {
    fn act(label: impl Into<String>, action: MenuAction) -> Self {
        Item {
            label: label.into(),
            action: Some(action),
            checked: false,
            enabled: true,
        }
    }
    fn check(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }
    fn enable(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
    fn header(label: impl Into<String>) -> Self {
        Item {
            label: label.into(),
            action: None,
            checked: false,
            enabled: false,
        }
    }
    fn sep() -> Self {
        Item::header("")
    }
    /// A clickable row whose action is live this frame.
    fn is_live(&self) -> bool {
        self.action.is_some() && self.enabled
    }
}

struct Menu {
    title: String,
    items: Vec<Item>,
}

fn menus(ctx: &MenuCtx) -> Vec<Menu> {
    let staged = ctx.staged;
    let orders_title = if staged > 0 {
        format!("Orders ({staged})")
    } else {
        "Orders".to_string()
    };
    let end_turn = if staged > 0 {
        format!("End of turn ({staged})\u{2026}")
    } else {
        "End of turn\u{2026}".to_string()
    };
    vec![
        Menu {
            title: "Game".to_string(),
            items: vec![
                Item::act("Switch context\u{2026}", MenuAction::SwitchContext),
                Item::act("Fit view", MenuAction::Fit),
                Item::sep(),
                Item::act("Quit", MenuAction::Quit),
            ],
        },
        Menu {
            title: "View".to_string(),
            items: vec![
                Item::header("MAP OVERLAY"),
                Item::act("Terrain (health)", MenuAction::SetOverlay(Overlay::Terrain))
                    .check(ctx.overlay == Overlay::Terrain),
                Item::act(
                    "Pressure (cpu/mem)",
                    MenuAction::SetOverlay(Overlay::Pressure),
                )
                .check(ctx.overlay == Overlay::Pressure),
                Item::act(
                    "Replicas (health)",
                    MenuAction::SetOverlay(Overlay::Replicas),
                )
                .check(ctx.overlay == Overlay::Replicas),
                Item::act(
                    "Namespace (territory)",
                    MenuAction::SetOverlay(Overlay::Namespace),
                )
                .check(ctx.overlay == Overlay::Namespace),
                Item::act(
                    "Walls (segmentation)",
                    MenuAction::SetOverlay(Overlay::Coverage),
                )
                .check(ctx.overlay == Overlay::Coverage),
                Item::sep(),
                Item::act("Annals (what changed) — H", MenuAction::Annals),
            ],
        },
        Menu {
            title: orders_title,
            items: vec![
                Item::act(end_turn, MenuAction::EndTurn).enable(staged > 0),
                Item::act("Discard staged changes", MenuAction::DiscardTurn).enable(staged > 0),
            ],
        },
        Menu {
            title: "Game Day".to_string(),
            items: vec![Item::act("Chaos drill\u{2026}", MenuAction::ChaosOpen)],
        },
        Menu {
            title: "Advisors".to_string(),
            items: vec![
                Item::act(
                    "Health (state of the realm)",
                    MenuAction::Advisor(AdvisorTab::Health),
                ),
                Item::act(
                    "Storage (granaries)",
                    MenuAction::Advisor(AdvisorTab::Storage),
                ),
                Item::act(
                    "Network (harbors & gates)",
                    MenuAction::Advisor(AdvisorTab::Network),
                ),
                Item::act(
                    "Right-sizing (requests vs usage)",
                    MenuAction::Advisor(AdvisorTab::RightSizing),
                ),
                Item::act(
                    "Hardening (pod security)",
                    MenuAction::Advisor(AdvisorTab::Hardening),
                ),
                Item::act(
                    "Posture (realm defense score)",
                    MenuAction::Advisor(AdvisorTab::Posture),
                ),
            ],
        },
        Menu {
            title: "World".to_string(),
            items: vec![
                Item::act("Namespace filter\u{2026}", MenuAction::NamespaceFilter)
                    .check(ctx.ns_active),
            ],
        },
        Menu {
            title: "Help".to_string(),
            items: vec![
                Item::act("Charter (your access)", MenuAction::Charter),
                Item::act("Field guide (almanac)", MenuAction::Almanac),
                Item::sep(),
                Item::header(format!("KuberNation v{}", env!("CARGO_PKG_VERSION"))),
            ],
        },
    ]
}

const TITLE_FS: f32 = 16.0;
const ITEM_FS: f32 = 15.0;
const PAD: f32 = 12.0;
const ITEM_H: f32 = 24.0;

/// Draw the menu bar starting at `bar_x0`, handling clicks. `click` should be
/// the left-press edge for this frame, already gated off by the caller when a
/// centered modal owns input (in which case pass `false`). Returns the chosen
/// action, if any. Toggling a title open/closed, picking an item, or clicking
/// outside all updates `*open`. Returns the chosen action plus the bar's right
/// edge (x) so the caller can keep its realm readout from overlapping.
pub fn draw_menu_bar(
    bar_x0: f32,
    mouse: Vec2,
    click: bool,
    open: &mut Option<usize>,
    ctx: &MenuCtx,
) -> (Option<MenuAction>, f32) {
    let menus = menus(ctx);

    // Lay out the top-level titles left to right in the chrome bar.
    let mut x = bar_x0;
    let mut titles: Vec<Rect> = Vec::with_capacity(menus.len());
    for m in &menus {
        let w = text_size(&m.title, TITLE_FS).width + PAD * 2.0;
        titles.push(Rect::new(x, 0.0, w, CHROME_H - 2.0));
        x += w;
    }
    let bar_right = x;

    // Which menu was open coming into this frame — the toggle below keys off
    // this, NOT the post-slide value, so a *click* on a different title opens
    // that title (rather than the slide-across pre-selecting it and the toggle
    // then reading it as "already open" and closing everything).
    let was_open = *open;

    // While a menu is open, hovering a different title switches to it — the
    // classic menubar "slide across" behavior.
    if open.is_some() {
        for (i, r) in titles.iter().enumerate() {
            if r.contains(mouse) {
                *open = Some(i);
            }
        }
    }

    let mut consumed = false;
    let mut result = None;

    // Top-level titles.
    for (i, (m, r)) in menus.iter().zip(&titles).enumerate() {
        // Visual highlight reflects the (post-slide) currently-open menu.
        if *open == Some(i) {
            stone_well(r.x, r.y, r.w, r.h);
        } else if r.contains(mouse) {
            draw_rectangle(r.x, r.y, r.w, r.h, lighter(STONE, 1.06));
        }
        text_bold(&m.title, r.x + PAD, 21.0, TITLE_FS, STONE_INK);
        if click && r.contains(mouse) {
            consumed = true;
            // Toggle against the pre-slide state: clicking the same open title
            // closes it; clicking any other opens that one.
            *open = if was_open == Some(i) { None } else { Some(i) };
        }
    }

    // The open dropdown, drawn over the world (the caller suspends map input
    // while a menu is open, so a click on a row can't fall through).
    if let Some(i) = *open {
        let m = &menus[i];
        let tr = titles[i];
        let mut w: f32 = 140.0;
        for it in &m.items {
            let lw = text_size(&it.label, ITEM_FS).width + 44.0;
            if lw > w {
                w = lw;
            }
        }
        let h = m.items.len() as f32 * ITEM_H + 8.0;
        let px = tr.x.min(screen_width() - w - 4.0).max(2.0);
        let py = CHROME_H - 1.0;
        stone_panel(px, py, w, h);

        let mut iy = py + 4.0;
        for it in &m.items {
            let row = Rect::new(px + 2.0, iy, w - 4.0, ITEM_H);
            if it.label.is_empty() {
                draw_rectangle(px + 8.0, iy + ITEM_H / 2.0, w - 16.0, 1.0, STONE_EDGE);
            } else if it.action.is_none() {
                // Section header / footnote — small, dim, not interactive.
                text(&it.label, row.x + 10.0, iy + 17.0, 12.0, STONE_INK_DIM);
            } else {
                let live = it.is_live();
                let hot = live && row.contains(mouse);
                if hot {
                    draw_rectangle(row.x, row.y, row.w, row.h, lighter(STONE_DARK, 1.5));
                }
                if it.checked {
                    text_bold("\u{2022}", row.x + 10.0, iy + 17.0, 16.0, STONE_INK);
                }
                let ink = if live { STONE_INK } else { STONE_INK_DIM };
                text(&it.label, row.x + 28.0, iy + 17.0, ITEM_FS, ink);
                if hot && click {
                    consumed = true;
                    result = it.action;
                    *open = None;
                }
            }
            iy += ITEM_H;
        }
    }

    // A click anywhere else closes the menu (without acting).
    if click && !consumed {
        *open = None;
    }
    (result, bar_right)
}
