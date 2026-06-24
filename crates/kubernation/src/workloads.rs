//! The realm-wide **workload table** — a sortable, filterable list of every
//! workload (the k9s-style triage view the map's drill-downs don't cover:
//! "show me everything CrashLoopBackOff"). A modal over `window.rs`; hot cluster
//! only (like the advisors). Read-only — clicking a row opens its city window.
//!
//! The sort/filter decision is a PURE fn ([`table_rows`]) tested against
//! fixtures (the testability policy); the modal is the thin renderer.

use std::collections::HashMap;

use kubernation_core::Time;
use kubernation_core::events::ClusterId;
use kubernation_core::state::attention::Severity;
use kubernation_core::state::model::{RolloutStatus, WorkloadRef, WorkloadRow};
use kubernation_core::util::format_age_opt;
use macroquad::prelude::*;

use crate::net::Snapshot;
use crate::panels::truncate_str;
use crate::text::{text, text_bold};
use crate::textfield::TextField;
use crate::theme::*;
use crate::window::draw_window;

/// Sort column. `Health` floats trouble to the top (the default); `Ready` floats
/// the least-ready (understrength) first.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WlSort {
    Health,
    Name,
    Ready,
    Age,
}

impl WlSort {
    const ALL: [WlSort; 4] = [WlSort::Health, WlSort::Name, WlSort::Ready, WlSort::Age];
    fn idx(self) -> usize {
        match self {
            WlSort::Health => 0,
            WlSort::Name => 1,
            WlSort::Ready => 2,
            WlSort::Age => 3,
        }
    }
}

/// One display row (sort done; age formatted at draw time from `age`).
#[derive(Clone)]
pub struct WlRow {
    pub r: WorkloadRef,
    pub sev: Option<Severity>,
    pub ready: String,
    pub ready_ratio: f64,
    pub status: RolloutStatus,
    pub age: Option<Time>,
}

fn sev_rank(s: Option<Severity>) -> u8 {
    match s {
        Some(Severity::Critical) => 3,
        Some(Severity::Warning) => 2,
        Some(Severity::Info) => 1,
        None => 0,
    }
}

/// PURE: filter (case-insensitive substring over kind/ns/name) + sort the
/// workloads into display rows. Clock-free — age sorts on the raw timestamp, so
/// it's deterministic + unit-testable.
pub fn table_rows(
    workloads: &[WorkloadRow],
    severity: &HashMap<WorkloadRef, Severity>,
    sort: WlSort,
    filter: &str,
) -> Vec<WlRow> {
    let f = filter.trim().to_lowercase();
    let mut rows: Vec<WlRow> = workloads
        .iter()
        .filter(|w| {
            f.is_empty() || {
                let hay = format!("{} {} {}", w.r.kind, w.r.namespace, w.r.name).to_lowercase();
                hay.contains(&f)
            }
        })
        .map(|w| {
            let ratio = if w.desired > 0 {
                w.ready as f64 / w.desired as f64
            } else {
                1.0
            };
            WlRow {
                r: w.r.clone(),
                sev: severity.get(&w.r).copied(),
                ready: format!("{}/{}", w.ready, w.desired),
                ready_ratio: ratio,
                status: w.status,
                age: w.age.clone(),
            }
        })
        .collect();
    let by_name =
        |a: &WlRow, b: &WlRow| (&a.r.namespace, &a.r.name).cmp(&(&b.r.namespace, &b.r.name));
    match sort {
        WlSort::Health => {
            rows.sort_by(|a, b| sev_rank(b.sev).cmp(&sev_rank(a.sev)).then(by_name(a, b)))
        }
        WlSort::Name => rows.sort_by(by_name),
        WlSort::Ready => rows.sort_by(|a, b| {
            a.ready_ratio
                .partial_cmp(&b.ready_ratio)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(by_name(a, b))
        }),
        // Newest first; unknown age (None) sorts oldest (last).
        WlSort::Age => rows.sort_by(|a, b| {
            let ka = a.age.as_ref().map(|t| t.0);
            let kb = b.age.as_ref().map(|t| t.0);
            kb.cmp(&ka).then(by_name(a, b))
        }),
    }
    rows
}

fn sev_color(s: Option<Severity>) -> Color {
    match s {
        Some(Severity::Critical) => CRIT,
        Some(Severity::Warning) => WARN,
        Some(Severity::Info) => STRUCT,
        None => good(),
    }
}

pub enum WorkloadsAction {
    None,
    Close,
    /// Open the city window for this workload.
    Open(ClusterId, WorkloadRef),
}

/// The modal. Owns the filter field (it has the keyboard while open) + the sort
/// + scroll. Hot cluster only.
pub struct Workloads {
    sort: WlSort,
    pub filter: TextField,
    scroll: f32,
    max_scroll: f32,
}

impl Default for Workloads {
    fn default() -> Self {
        Workloads {
            sort: WlSort::Health,
            filter: TextField::new("", false),
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }
}

impl Workloads {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed keyboard input to the filter (the modal owns the queue while open).
    pub fn input(&mut self) {
        if self.filter.update_focused() {
            self.scroll = 0.0; // a changed filter → back to the top
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    pub fn draw(&mut self, snap: Option<&Snapshot>, mouse: Vec2, click: bool) -> WorkloadsAction {
        let rows = snap
            .map(|s| {
                table_rows(
                    &s.hot.models.workloads,
                    &s.hot.models.workload_severity,
                    self.sort,
                    &self.filter.buf,
                )
            })
            .unwrap_or_default();

        let labels = ["by health", "by name", "by ready", "by age", "Close"];
        let title = format!("Workloads — {} shown", rows.len());
        let win = draw_window(&title, vec2(720.0, 540.0), &labels, self.sort.idx());
        let b = win.body;

        // Filter line + column header.
        let filt = if self.filter.buf.is_empty() {
            "filter: (type to narrow)".to_string()
        } else {
            format!("filter: {}_", self.filter.buf)
        };
        text(&filt, b.x + 6.0, b.y + 4.0, 13.0, PARCHMENT);
        let hy = b.y + 24.0;
        // Column x offsets within the body.
        let cx_kind = b.x + 24.0;
        let cx_name = b.x + 86.0;
        let cx_ready = b.x + b.w - 220.0;
        let cx_status = b.x + b.w - 150.0;
        let cx_age = b.x + b.w - 40.0;
        text_bold("kind", cx_kind, hy, 12.0, DIM);
        text_bold("namespace / name", cx_name, hy, 12.0, DIM);
        text_bold("ready", cx_ready, hy, 12.0, DIM);
        text_bold("status", cx_status, hy, 12.0, DIM);
        text_bold("age", cx_age, hy, 12.0, DIM);
        draw_line(
            b.x,
            hy + 5.0,
            b.x + b.w,
            hy + 5.0,
            1.0,
            darker(PARCHMENT, 0.6),
        );

        let row_h = 19.0;
        let top = hy + 12.0;
        let visible_h = (b.y + b.h) - top;
        let mut clicked: WorkloadsAction = WorkloadsAction::None;
        let mut y = top - self.scroll;
        for row in &rows {
            if y > top - row_h && y < b.y + b.h {
                let rect = Rect::new(b.x, y - 13.0, b.w, row_h);
                let hover = rect.contains(mouse) && mouse.y < b.y + b.h && mouse.y > top - row_h;
                if hover {
                    draw_rectangle(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        Color::new(1.0, 1.0, 1.0, 0.06),
                    );
                    if click {
                        clicked = WorkloadsAction::Open(ClusterId::Hot, row.r.clone());
                    }
                }
                let sc = sev_color(row.sev);
                text(
                    row.sev.map(|s| s.glyph()).unwrap_or("·"),
                    b.x + 6.0,
                    y,
                    13.0,
                    sc,
                );
                text(row.r.kind.to_string(), cx_kind, y, 12.0, INK);
                let name = truncate_str(&format!("{}/{}", row.r.namespace, row.r.name), 42);
                text(&name, cx_name, y, 13.0, sc);
                text(&row.ready, cx_ready, y, 12.0, INK);
                text(row.status.to_string(), cx_status, y, 12.0, INK);
                text(format_age_opt(row.age.as_ref()), cx_age, y, 12.0, DIM);
            }
            y += row_h;
        }
        let content_h = rows.len() as f32 * row_h;
        self.max_scroll = (content_h - visible_h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);

        if rows.is_empty() {
            let msg = if snap.is_none() {
                "the world is not yet explored"
            } else {
                "no workloads match"
            };
            text(msg, b.x + 6.0, top + 10.0, 13.0, DIM);
        }

        if let WorkloadsAction::Open(..) = clicked {
            return clicked;
        }
        if click {
            if win.close.contains(mouse) {
                return WorkloadsAction::Close;
            }
            if let Some(i) = win.button_at(mouse) {
                match WlSort::ALL.get(i) {
                    Some(s) => {
                        self.sort = *s;
                        self.scroll = 0.0;
                    }
                    None => return WorkloadsAction::Close, // the trailing "Close"
                }
            } else if !win.frame.contains(mouse) {
                return WorkloadsAction::Close;
            }
        }
        WorkloadsAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubernation_core::state::model::WorkloadKind;

    fn wref(kind: WorkloadKind, ns: &str, name: &str) -> WorkloadRef {
        WorkloadRef {
            kind,
            namespace: ns.into(),
            name: name.into(),
        }
    }
    fn row(r: WorkloadRef, ready: i32, desired: i32) -> WorkloadRow {
        WorkloadRow {
            r,
            desired,
            ready,
            available: ready,
            updated: ready,
            status: RolloutStatus::Complete,
            note: String::new(),
            age: None,
            slo_target: None,
        }
    }

    /// A small synthetic realm: a critical crashy, a warning understrength web,
    /// and a healthy db — enough to pin sort + filter without a cluster.
    fn fixture() -> (Vec<WorkloadRow>, HashMap<WorkloadRef, Severity>) {
        let crashy = wref(WorkloadKind::Deployment, "demo", "crashy");
        let web = wref(WorkloadKind::Deployment, "demo", "web");
        let db = wref(WorkloadKind::StatefulSet, "data", "db");
        let workloads = vec![
            row(web.clone(), 1, 3),    // understrength
            row(db.clone(), 2, 2),     // healthy
            row(crashy.clone(), 0, 1), // down
        ];
        let mut sev = HashMap::new();
        sev.insert(crashy, Severity::Critical);
        sev.insert(web, Severity::Warning);
        (workloads, sev)
    }

    #[test]
    fn health_sort_floats_trouble_and_filter_narrows() {
        let (workloads, sev) = fixture();
        let rows = table_rows(&workloads, &sev, WlSort::Health, "");
        assert_eq!(rows.len(), 3);
        // Descending by severity rank; the worst (crashy = Critical) is first.
        for w in rows.windows(2) {
            assert!(sev_rank(w[0].sev) >= sev_rank(w[1].sev), "not descending");
        }
        assert_eq!(rows[0].r.name, "crashy");
        assert_eq!(rows[0].sev, Some(Severity::Critical));
        // A non-matching filter empties the list; a name filter narrows to it.
        assert!(table_rows(&workloads, &sev, WlSort::Name, "zzz").is_empty());
        let web_only = table_rows(&workloads, &sev, WlSort::Name, "web");
        assert_eq!(web_only.len(), 1);
        assert_eq!(web_only[0].r.name, "web");
    }

    #[test]
    fn ready_sort_floats_least_ready_and_name_sort_is_alpha() {
        let (workloads, sev) = fixture();
        // By ready: crashy (0/1) first, db (2/2 = full) last.
        let ready = table_rows(&workloads, &sev, WlSort::Ready, "");
        assert_eq!(ready[0].r.name, "crashy");
        assert!(ready.last().unwrap().ready_ratio >= 1.0);
        // By name: (namespace, name) ascending.
        let named = table_rows(&workloads, &sev, WlSort::Name, "");
        for w in named.windows(2) {
            assert!((&w[0].r.namespace, &w[0].r.name) <= (&w[1].r.namespace, &w[1].r.name));
        }
    }
}
