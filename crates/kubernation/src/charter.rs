//! The Charter — a modal showing your self-scoped RBAC ("what can I do here?").
//!
//! A read-only window over the pure `kubernation_core::state::charter` grid: the
//! curated `can-i` probes resolved by the apiserver, shown as a resource×verb
//! grid (✓ allowed / ✗ denied / ? unknown) for the active namespace plus the
//! realm-wide (cluster-scoped) band. Allowed *dangerous* capabilities pop
//! (the audit finding). Mirrors advisor.rs's window/scroll machinery; the pure
//! line/banner builders are unit-tested (the GUI testability policy).

use kubernation_core::events::ClusterId;
use kubernation_core::k8s::rbac::Risk;
use kubernation_core::state::charter::{Access, Charter, CharterCell};
use macroquad::prelude::*;

use crate::net::Net;
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::draw_window;

pub enum CharterAction {
    None,
    Close,
}

/// ✓ / ✗ / ? / (not probed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Yes,
    No,
    Maybe,
}

/// Theme role for a cell (no GL — mapped to a colour at draw time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Good,
    Crit,
    Warn,
    Dim,
}

/// One rendered grid row: a resource label + its verb cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharterLine {
    pub label: String,
    pub cells: Vec<(String, Mark, Role)>, // (verb token, mark, role)
}

fn cell_label(c: &CharterCell) -> String {
    match c.probe.subresource {
        Some(sub) => format!("{}/{}", c.probe.resource, sub),
        None => c.probe.resource.to_string(),
    }
}

/// Map one cell to its (mark, role). PURE. Allowed-dangerous pops (Crit for a
/// Critical-risk grant, Warn for High); allowed-benign is calm Good; denied and
/// unknown are Dim (denied is normal for a scoped user, not an error).
fn cell_mark(c: &CharterCell) -> (Mark, Role) {
    match c.access {
        Access::Allowed => {
            let role = match (c.dangerous, c.probe.risk) {
                (true, Risk::Critical) => Role::Crit,
                (true, _) => Role::Warn,
                (false, _) => Role::Good,
            };
            (Mark::Yes, role)
        }
        Access::Denied => (Mark::No, Role::Dim),
        Access::Unknown => (Mark::Maybe, Role::Dim),
    }
}

/// Group a band's cells into per-resource rows (first-seen order). PURE +
/// unit-tested. Groups by label irrespective of position, so a probe-table edit
/// that breaks contiguity can't split one resource into two rows.
pub fn charter_lines(cells: &[CharterCell]) -> Vec<CharterLine> {
    let mut lines: Vec<CharterLine> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for c in cells {
        let label = cell_label(c);
        let (mark, role) = cell_mark(c);
        let tok = (c.probe.verb.to_string(), mark, role);
        if let Some(&i) = index.get(&label) {
            lines[i].cells.push(tok);
        } else {
            index.insert(label.clone(), lines.len());
            lines.push(CharterLine {
                label,
                cells: vec![tok],
            });
        }
    }
    lines
}

/// The header/trust banner lines. PURE + unit-tested.
pub fn charter_banner(c: &Charter) -> Vec<(String, Role)> {
    if let kubernation_core::state::charter::Trust::Unavailable(err) = &c.trust {
        return vec![(
            format!(
                "couldn't read your access — the apiserver didn't answer access reviews ({}). needs authorization.k8s.io/v1.",
                err
            ),
            Role::Crit,
        )];
    }
    let probed = c.allowed + c.denied + c.unknown;
    // Two orthogonal axes, separately coloured: capability breadth (green only
    // when you actually have access — never paint "0 of N" green) and security
    // posture (Crit when dangerous power is granted, else calm Dim).
    let mut out = vec![
        (
            format!("you can do {} of {} probed actions here", c.allowed, probed),
            if c.allowed > 0 { Role::Good } else { Role::Dim },
        ),
        (
            format!("{} dangerous capabilities granted", c.dangerous_granted),
            if c.dangerous_granted > 0 {
                Role::Crit
            } else {
                Role::Dim
            },
        ),
    ];
    if c.unknown > 0 {
        out.push((
            format!(
                "{} cell(s) the apiserver didn't answer (shown ?)",
                c.unknown
            ),
            Role::Dim,
        ));
    }
    out
}

/// The modal window. `focus` is the namespace whose grid is shown.
pub struct CharterView {
    pub focus: String,
    scroll: f32,
    max_scroll: f32,
}

impl CharterView {
    pub fn new(focus: String) -> Self {
        CharterView {
            focus,
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    pub fn draw(
        &mut self,
        snap: Option<&crate::net::Snapshot>,
        net: &Net,
        mouse: Vec2,
        click: bool,
    ) -> CharterAction {
        let win = draw_window(
            &ascii(&format!("Charter — your writ in {} · HOT", self.focus)),
            vec2(720.0, 560.0),
            &["Close"],
            usize::MAX,
        );
        let b = win.body;
        let id = ClusterId::Hot;

        // Namespace scope toggle (◀ ns ▶) — cycles the observed namespaces.
        let namespaces: Vec<String> = snap
            .map(|s| s.hot.observed.namespaces().into_iter().collect())
            .unwrap_or_default();
        let mut y = b.y + 4.0;
        text("namespace:", b.x, y + 12.0, 14.0, DIM);
        let nsw = text_size("namespace:", 14.0).width;
        let prev = Rect::new(b.x + nsw + 10.0, y, 18.0, 18.0);
        let nsx = prev.x + prev.w + 6.0;
        let focus_txt = ascii(&self.focus);
        text_bold(&focus_txt, nsx, y + 13.0, 14.0, PARCHMENT);
        let nmw = text_size(&focus_txt, 14.0).width;
        let next = Rect::new(nsx + nmw + 6.0, y, 18.0, 18.0);
        if namespaces.len() > 1 {
            for (r, sym) in [(prev, "<"), (next, ">")] {
                let bg = if r.contains(mouse) {
                    lighter(PLATE, 1.7)
                } else {
                    PLATE
                };
                draw_rectangle(r.x, r.y, r.w, r.h, bg);
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, PARCHMENT);
                text(sym, r.x + 6.0, r.y + 14.0, 14.0, INK);
            }
        }
        y += 24.0;

        // Resolve (or request) the grid for this scope.
        let charter = net.charter(id, &self.focus);
        if charter.is_none() {
            net.request_charter(id, self.focus.clone());
        }

        let mut cx = Ctx {
            body: Rect::new(b.x, y, b.w, b.h - (y - b.y)),
            y: y - self.scroll,
        };
        match &charter {
            None => cx.row("reading your access…", DIM, false),
            Some(c) => {
                for (line, role) in charter_banner(c) {
                    cx.row(&ascii(&line), role_color(role), false);
                }
                cx.gap();
                cx.heading(&ascii(&format!("NAMESPACE: {}", c.namespace)));
                draw_band(&mut cx, &charter_lines(&c.ns_cells));
                cx.heading("REALM-WIDE (cluster-scoped)");
                draw_band(&mut cx, &charter_lines(&c.cluster_cells));
                cx.gap();
                cx.row(
                    "read-only self-query (same API as kubectl auth can-i) — Kubernation can't grant or change RBAC; ask your cluster admin.",
                    DIM,
                    false,
                );
            }
        }

        // Scroll bookkeeping (matches advisor.rs).
        let content_h = cx.y - (y - self.scroll);
        self.max_scroll = (content_h - cx.body.h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);

        // Input: scope toggle, close.
        if click {
            if namespaces.len() > 1 && (prev.contains(mouse) || next.contains(mouse)) {
                let back = prev.contains(mouse);
                self.focus = match namespaces.iter().position(|n| *n == self.focus) {
                    // In the list → step from here (wrapping).
                    Some(i) => {
                        let delta = if back { namespaces.len() - 1 } else { 1 };
                        namespaces[(i + delta) % namespaces.len()].clone()
                    }
                    // Focus isn't an observed namespace (e.g. a manual --charter
                    // value) → land deterministically on an end, skipping nothing.
                    None => {
                        if back {
                            namespaces[namespaces.len() - 1].clone()
                        } else {
                            namespaces[0].clone()
                        }
                    }
                };
                self.scroll = 0.0;
            } else if win.close.contains(mouse)
                || win.button_at(mouse).is_some()
                || !win.frame.contains(mouse)
            {
                return CharterAction::Close;
            }
        }
        CharterAction::None
    }
}

fn role_color(role: Role) -> Color {
    match role {
        Role::Good => GOOD,
        Role::Crit => CRIT,
        Role::Warn => WARN,
        Role::Dim => DIM,
    }
}

/// Draw a band of grid rows: "resource    get ✓  list ✗  …".
fn draw_band(cx: &mut Ctx, lines: &[CharterLine]) {
    for line in lines {
        cx.y += 17.0;
        if cx.visible() {
            text(ascii(&line.label), cx.body.x + 14.0, cx.y, 13.0, INK);
            let mut tx = cx.body.x + 220.0;
            for (verb, mark, role) in &line.cells {
                let glyph = match mark {
                    Mark::Yes => "✓",
                    Mark::No => "✗",
                    Mark::Maybe => "?",
                };
                let tok = format!("{verb} {glyph}");
                text(ascii(&tok), tx, cx.y, 13.0, role_color(*role));
                tx += text_size(&tok, 13.0).width + 16.0;
            }
        }
    }
}

// Minimal scroll-aware text cursor (mirrors advisor.rs::Ctx).
struct Ctx {
    body: Rect,
    y: f32,
}

impl Ctx {
    fn visible(&self) -> bool {
        self.y > self.body.y - 18.0 && self.y < self.body.y + self.body.h
    }
    fn gap(&mut self) {
        self.y += 8.0;
    }
    fn heading(&mut self, s: &str) {
        self.y += 22.0;
        if self.visible() {
            text_bold(s, self.body.x + 4.0, self.y, 15.0, PARCHMENT);
        }
        self.y += 4.0;
    }
    fn row(&mut self, s: &str, color: Color, bold: bool) {
        self.y += 18.0;
        if self.visible() {
            if bold {
                text_bold(s, self.body.x + 14.0, self.y, 13.0, color);
            } else {
                text(s, self.body.x + 14.0, self.y, 13.0, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubernation_core::k8s::rbac::Verdict;
    use kubernation_core::state::charter::{build_charter, namespaced_probes};

    #[test]
    fn charter_lines_group_by_resource_and_color_dangerous() {
        // All allowed → dangerous grants pop; benign are Good.
        let nsv: Vec<Verdict> = namespaced_probes()
            .iter()
            .map(|_| Verdict::Allowed)
            .collect();
        let c = build_charter("demo", &nsv, &[]);
        let lines = charter_lines(&c.ns_cells);
        // pods has 4 verbs → one grouped row with 4 cells.
        let pods = lines.iter().find(|l| l.label == "pods").unwrap();
        assert_eq!(pods.cells.len(), 4);
        // pods/exec create is Critical → Crit role when allowed.
        let exec = lines.iter().find(|l| l.label == "pods/exec").unwrap();
        assert_eq!(exec.cells[0].2, Role::Crit);
        // pods/log get is Normal → Good.
        let log = lines.iter().find(|l| l.label == "pods/log").unwrap();
        assert_eq!(log.cells[0].2, Role::Good);
    }

    #[test]
    fn cell_mark_maps_access_states() {
        let nsv: Vec<Verdict> = namespaced_probes()
            .iter()
            .enumerate()
            .map(|(i, _)| {
                if i == 0 {
                    Verdict::Denied
                } else {
                    Verdict::Unknown("x".into())
                }
            })
            .collect();
        let c = build_charter("demo", &nsv, &[]);
        let lines = charter_lines(&c.ns_cells);
        let pods = lines.iter().find(|l| l.label == "pods").unwrap();
        assert_eq!(pods.cells[0].1, Mark::No); // denied
        assert_eq!(pods.cells[1].1, Mark::Maybe); // unknown
    }

    #[test]
    fn banner_reports_unavailable_and_dangerous() {
        // Unavailable → a single Crit banner line.
        let nsv = vec![Verdict::Unknown("403".into()); namespaced_probes().len()];
        let c = build_charter("demo", &nsv, &[]);
        let b = charter_banner(&c);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].1, Role::Crit);
        assert!(b[0].0.contains("couldn't read your access"));

        // All allowed → capability line is Good (you have access), and the
        // separate dangerous line is Crit (you hold dangerous power).
        let nsv: Vec<Verdict> = namespaced_probes()
            .iter()
            .map(|_| Verdict::Allowed)
            .collect();
        let c = build_charter("demo", &nsv, &[]);
        let b = charter_banner(&c);
        assert!(b[0].0.contains("you can do") && b[0].1 == Role::Good);
        let danger = b.iter().find(|(s, _)| s.contains("dangerous")).unwrap();
        assert_eq!(danger.1, Role::Crit);

        // A locked-out identity (all denied): capability line is NOT green, and
        // the dangerous line is calm Dim — green must never read as "all good"
        // when you're powerless.
        let nsv = vec![Verdict::Denied; namespaced_probes().len()];
        let c = build_charter("demo", &nsv, &[]);
        let b = charter_banner(&c);
        assert_eq!(b[0].1, Role::Dim, "0-of-N capability must not be green");
        let danger = b.iter().find(|(s, _)| s.contains("dangerous")).unwrap();
        assert_eq!(danger.1, Role::Dim);
    }
}
