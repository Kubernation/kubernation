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
use crate::text::{text, text_bold, text_size};
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
    /// Whether this workload has a place on the map at all.
    ///
    /// **A DaemonSet has none by design** — it is rendered as a road across
    /// every province it touches, never a settlement (`world.rs` excludes it
    /// from city siting, and the island-encampment fallback is for *zero-pod*
    /// workloads only). Every cluster has several, so this is the ordinary case,
    /// not a corner.
    ///
    /// The row says so, and clicking it opens the window without claiming a
    /// position on the map. A row that highlighted nothing would be the silent
    /// version of the same refusal.
    pub placed: bool,
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
    world: &kubernation_core::state::world::WorldModel,
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
                sev: severity.get(&w.r).copied(),
                ready: format!("{}/{}", w.ready, w.desired),
                ready_ratio: ratio,
                status: w.status,
                age: w.age.clone(),
                // The same two places a selection derives its position from, so
                // the row and the map cannot disagree about whether it has one.
                placed: world
                    .city_pos(&w.r)
                    .or_else(|| world.structure_pos(&w.r))
                    .is_some(),
                r: w.r.clone(),
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
    /// Open the city window for this workload, and — if it has a place on the
    /// map — make it the selection.
    ///
    /// The SELECTION travels with the click, already decided. `main.rs` has no
    /// test module (the v0.66.0 GUI testability policy), so a `placed` flag it
    /// had to interpret would put the decision in the one file the mutation
    /// floor cannot reach — D2-fix's finding, applied before it bites.
    Open {
        cluster: ClusterId,
        r: WorkloadRef,
        select: Option<crate::draw::Selection>,
    },
}

/// What selecting this row means: the workload, or nothing if the map has
/// nowhere to put it. PURE.
pub fn row_selection(row: &WlRow) -> Option<crate::draw::Selection> {
    row.placed
        .then(|| crate::draw::Selection::Workload(ClusterId::Hot, row.r.clone()))
}

/// Whether this row is what the map currently has selected. PURE.
///
/// The table is hot-only, so a warm selection matches nothing here — stated as
/// an exhaustive match rather than left to the `WorkloadRef` comparison, which
/// would silently start matching if a warm workload ever shared a ref.
pub fn row_is_selected(row: &WlRow, selected: Option<&crate::draw::Selection>) -> bool {
    match selected {
        Some(crate::draw::Selection::Workload(ClusterId::Hot, r)) => *r == row.r,
        Some(crate::draw::Selection::Workload(ClusterId::Warm, _))
        | Some(crate::draw::Selection::Node(..))
        | None => false,
    }
}

/// The trailing note on a row that has no place on the map, or none.
///
/// Visible refusal (§5): a row that simply failed to highlight would leave the
/// operator clicking it again.
pub fn row_note(row: &WlRow) -> Option<&'static str> {
    (!row.placed).then_some(match row.r.kind {
        // The honest reason, not a generic one: a DaemonSet is drawn as a road
        // across the provinces it touches, so there is no settlement to mark.
        kubernation_core::state::model::WorkloadKind::DaemonSet => "road - not a settlement",
        _ => "not on the map",
    })
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

    pub fn draw(
        &mut self,
        snap: Option<&Snapshot>,
        selected: Option<&crate::draw::Selection>,
        mouse: Vec2,
        click: bool,
    ) -> WorkloadsAction {
        let rows = snap
            .map(|s| {
                table_rows(
                    &s.hot.models.workloads,
                    &s.hot.models.workload_severity,
                    &s.hot.models.world,
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
                // BRUSHING, the read direction: the row that IS the map's
                // current selection is marked, so the two views cannot be
                // looking at different things without saying so.
                if row_is_selected(row, selected) {
                    draw_rectangle(rect.x, rect.y, rect.w, rect.h, SEL_ROW);
                    draw_rectangle(rect.x, rect.y, 3.0, rect.h, PARCHMENT);
                }
                if hover {
                    draw_rectangle(
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        Color::new(1.0, 1.0, 1.0, 0.06),
                    );
                    if click {
                        clicked = WorkloadsAction::Open {
                            cluster: ClusterId::Hot,
                            r: row.r.clone(),
                            select: row_selection(row),
                        };
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
                // The name is shortened to leave room for the note, so a
                // refusal never has to compete with a long name for space.
                let cap = if row.placed { 42 } else { 28 };
                let name = truncate_str(&format!("{}/{}", row.r.namespace, row.r.name), cap);
                text(&name, cx_name, y, 13.0, sc);
                if let Some(note) = row_note(row) {
                    let nx = cx_name + text_size(&name, 13.0).width + 8.0;
                    text(note, nx, y, 11.0, DIM);
                }
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

        if let WorkloadsAction::Open { .. } = clicked {
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
    /// The sort/filter tests are about ORDER, not placement — an empty world
    /// reports nothing placed, which is honest and keeps them from depending on
    /// map geometry. `placed` has its own test, against a real world.
    fn empty_world() -> kubernation_core::state::world::WorldModel {
        kubernation_core::state::world::WorldModel::default()
    }

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

    /// D2 brushing: which row the map has selected, and which rows it CANNOT.
    ///
    /// Built against a real world rather than the hand-made `WorkloadRow`
    /// fixture, because the whole question is where `build_world` puts things —
    /// a fixture that asserted `placed` by construction would be asserting my
    /// own assumption back at me.
    #[test]
    fn a_row_knows_whether_the_map_can_mark_it() {
        use kubernation_core::state::fixtures as fx;
        use kubernation_core::state::model::Models;

        let (world, mut st) = fx::world();
        for n in ["n1", "n2"] {
            st.node(fx::node(n, Some("z-a")));
        }
        // A Deployment with a pod -> a city.
        st.deployment(fx::deployment("demo", "sited", 1, 1));
        st.replicaset(fx::replicaset("demo", "rs", "sited"));
        st.pod(fx::pod_owned(
            fx::pod("demo", "rs-1", Some("n1")),
            "ReplicaSet",
            "rs",
        ));
        // A Deployment with NO pods -> an island encampment, still placed.
        st.deployment(fx::deployment("demo", "encamped", 1, 0));
        // A DaemonSet -> a ROAD across the provinces it touches, never a
        // settlement. This is the ordinary refusal case: every cluster has some.
        st.daemonset(fx::daemonset("demo", "paved", 2, 2));
        st.pod(fx::pod_owned(
            fx::pod("demo", "paved-x", Some("n1")),
            "DaemonSet",
            "paved",
        ));

        let models = Models::build(&world);
        let rows = table_rows(
            &models.workloads,
            &models.workload_severity,
            &models.world,
            WlSort::Name,
            "",
        );
        let find = |n: &str| rows.iter().find(|r| r.r.name == n).expect(n).clone();

        let sited = find("sited");
        let encamped = find("encamped");
        let paved = find("paved");
        assert!(sited.placed, "a workload with a pod is sited as a city");
        assert!(encamped.placed, "a zero-pod workload gets an encampment");
        assert!(
            !paved.placed,
            "a DaemonSet is drawn as a road, so there is nothing to mark"
        );

        // The refusal is SAID, and it says the honest reason rather than a
        // generic one — a row that merely failed to highlight is the silent
        // version of the same thing.
        assert_eq!(row_note(&sited), None);
        assert_eq!(row_note(&encamped), None);
        assert_eq!(row_note(&paved), Some("road - not a settlement"));

        // And what a click MEANS is decided here, not in the render loop.
        assert_eq!(
            row_selection(&sited),
            Some(crate::draw::Selection::Workload(
                ClusterId::Hot,
                sited.r.clone()
            ))
        );
        assert_eq!(
            row_selection(&paved),
            None,
            "clicking a road must not claim a place on the map"
        );

        // The mark: exactly the selected row, and only in the hot cluster,
        // which is the only one this table shows.
        let sel = crate::draw::Selection::Workload(ClusterId::Hot, sited.r.clone());
        assert!(row_is_selected(&sited, Some(&sel)));
        assert!(!row_is_selected(&encamped, Some(&sel)));
        assert!(!row_is_selected(&sited, None));
        let warm = crate::draw::Selection::Workload(ClusterId::Warm, sited.r.clone());
        assert!(
            !row_is_selected(&sited, Some(&warm)),
            "a warm selection must not light a row in a hot-only table"
        );
        let node = crate::draw::Selection::Node(ClusterId::Hot, "n1".into());
        assert!(!row_is_selected(&sited, Some(&node)));
    }

    /// The list's answer and the map's answer to "where is this?" come from the
    /// same place. If they ever diverge, a row would offer a selection the map
    /// cannot show, or refuse one it could.
    #[test]
    fn placed_agrees_with_what_a_selection_can_resolve() {
        use kubernation_core::state::fixtures as fx;
        use kubernation_core::state::model::Models;

        let (world, mut st) = fx::world();
        for n in ["n1", "n2"] {
            st.node(fx::node(n, Some("z-a")));
        }
        st.deployment(fx::deployment("demo", "sited", 1, 1));
        st.replicaset(fx::replicaset("demo", "rs", "sited"));
        st.pod(fx::pod_owned(
            fx::pod("demo", "rs-1", Some("n1")),
            "ReplicaSet",
            "rs",
        ));
        st.daemonset(fx::daemonset("demo", "paved", 2, 2));
        st.pod(fx::pod_owned(
            fx::pod("demo", "paved-x", Some("n1")),
            "DaemonSet",
            "paved",
        ));

        let models = Models::build(&world);
        let rows = table_rows(
            &models.workloads,
            &models.workload_severity,
            &models.world,
            WlSort::Name,
            "",
        );
        assert!(rows.len() >= 2, "the fixture must produce rows to compare");

        let mut saw_placed = false;
        let mut saw_unplaced = false;
        for row in &rows {
            let sel = crate::draw::Selection::Workload(ClusterId::Hot, row.r.clone());
            // One world, so one `SceneWorld` — the same shape `main` builds.
            let sw = crate::draw::SceneWorld {
                id: ClusterId::Hot,
                off: 0,
                world: &models.world,
                label: String::new(),
                fresh: &Default::default(),
            };
            let resolvable = crate::draw::selection_pos(&[sw], &sel).is_some();
            assert_eq!(
                row.placed, resolvable,
                "{}/{} : the row says placed={} but the map says {}",
                row.r.namespace, row.r.name, row.placed, resolvable
            );
            saw_placed |= row.placed;
            saw_unplaced |= !row.placed;
        }
        assert!(saw_placed, "no placed row — the agreement is vacuous");
        assert!(saw_unplaced, "no unplaced row — the refusal is untested");
    }

    #[test]
    fn health_sort_floats_trouble_and_filter_narrows() {
        let (workloads, sev) = fixture();
        let rows = table_rows(&workloads, &sev, &empty_world(), WlSort::Health, "");
        assert_eq!(rows.len(), 3);
        // Descending by severity rank; the worst (crashy = Critical) is first.
        for w in rows.windows(2) {
            assert!(sev_rank(w[0].sev) >= sev_rank(w[1].sev), "not descending");
        }
        assert_eq!(rows[0].r.name, "crashy");
        assert_eq!(rows[0].sev, Some(Severity::Critical));
        // A non-matching filter empties the list; a name filter narrows to it.
        assert!(table_rows(&workloads, &sev, &empty_world(), WlSort::Name, "zzz").is_empty());
        let web_only = table_rows(&workloads, &sev, &empty_world(), WlSort::Name, "web");
        assert_eq!(web_only.len(), 1);
        assert_eq!(web_only[0].r.name, "web");
    }

    #[test]
    fn ready_sort_floats_least_ready_and_name_sort_is_alpha() {
        let (workloads, sev) = fixture();
        // By ready: crashy (0/1) first, db (2/2 = full) last.
        let ready = table_rows(&workloads, &sev, &empty_world(), WlSort::Ready, "");
        assert_eq!(ready[0].r.name, "crashy");
        assert!(ready.last().unwrap().ready_ratio >= 1.0);
        // By name: (namespace, name) ascending.
        let named = table_rows(&workloads, &sev, &empty_world(), WlSort::Name, "");
        for w in named.windows(2) {
            assert!((&w[0].r.namespace, &w[0].r.name) <= (&w[1].r.namespace, &w[1].r.name));
        }
    }
}
