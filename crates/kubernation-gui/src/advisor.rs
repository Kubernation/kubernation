//! The advisor screens — classic-4X "advisors" (Civ's F1 Berater) over the
//! pure core reports (`kubernation_core::state::advisor`). Three read-only
//! summary tabs: Health (state of the realm), Storage (granaries), Network
//! (harbors & gates). Opened from the Advisors menu; a modal window like the
//! Almanac, sharing its window/tab/scroll machinery. Cluster-wide (hot).

use kubernation_core::state::advisor::{
    HealthReport, NetworkReport, StorageReport, health_report, network_report, storage_report,
};
use macroquad::prelude::*;

use crate::net::Snapshot;
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::draw_window;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AdvisorTab {
    Health,
    Storage,
    Network,
}

impl AdvisorTab {
    pub const ALL: [AdvisorTab; 3] = [AdvisorTab::Health, AdvisorTab::Storage, AdvisorTab::Network];
    fn idx(self) -> usize {
        match self {
            AdvisorTab::Health => 0,
            AdvisorTab::Storage => 1,
            AdvisorTab::Network => 2,
        }
    }
}

pub enum AdvisorAction {
    None,
    Close,
}

pub struct Advisor {
    tab: AdvisorTab,
    scroll: f32,
    max_scroll: f32,
}

impl Advisor {
    pub fn new(tab: AdvisorTab) -> Self {
        Advisor {
            tab,
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }

    pub fn go(&mut self, tab: AdvisorTab) {
        self.tab = tab;
        self.scroll = 0.0;
    }

    pub fn cycle(&mut self, delta: i32) {
        let i = (self.tab.idx() as i32 + delta).rem_euclid(AdvisorTab::ALL.len() as i32);
        self.go(AdvisorTab::ALL[i as usize]);
    }

    /// Wheel scroll (positive = up).
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    pub fn draw(&mut self, snap: Option<&Snapshot>, mouse: Vec2, click: bool) -> AdvisorAction {
        let labels = ["Health", "Storage", "Network", "Close"];
        let win = draw_window(
            "Advisors — state of the realm",
            vec2(640.0, 540.0),
            &labels,
            self.tab.idx(),
        );

        let mut cx = Ctx {
            body: win.body,
            y: win.body.y - self.scroll,
        };
        if let Some(s) = snap {
            let obs = &s.hot.observed;
            match self.tab {
                AdvisorTab::Health => page_health(&mut cx, &health_report(obs)),
                AdvisorTab::Storage => page_storage(&mut cx, &storage_report(obs)),
                AdvisorTab::Network => page_network(&mut cx, &network_report(obs)),
            }
        } else {
            cx.note("the world is not yet explored", DIM);
        }

        let content_h = cx.y - (win.body.y - self.scroll);
        self.max_scroll = (content_h - win.body.h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);
        if self.max_scroll > 0.0 {
            let b = win.body;
            let frac = (b.h / content_h).clamp(0.05, 1.0);
            let thumb_h = b.h * frac;
            let t = self.scroll / self.max_scroll;
            let ty = b.y + t * (b.h - thumb_h);
            draw_rectangle(b.x + b.w + 2.0, b.y, 3.0, b.h, darker(PANEL, 0.6));
            draw_rectangle(b.x + b.w + 2.0, ty, 3.0, thumb_h, PARCHMENT);
        }

        if click {
            if win.close.contains(mouse) {
                return AdvisorAction::Close;
            }
            if let Some(i) = win.button_at(mouse) {
                if let Some(t) = AdvisorTab::ALL.get(i) {
                    self.go(*t);
                } else {
                    return AdvisorAction::Close; // the trailing "Close" tab
                }
            } else if !win.frame.contains(mouse) {
                return AdvisorAction::Close;
            }
        }
        AdvisorAction::None
    }
}

// --- content rendering ------------------------------------------------------

struct Ctx {
    body: Rect,
    y: f32,
}

impl Ctx {
    fn visible(&self) -> bool {
        self.y > self.body.y - 18.0 && self.y < self.body.y + self.body.h
    }
    fn heading(&mut self, s: &str) {
        // A clear gap above the heading (separates it from the prior section),
        // then it groups tightly with its own rows below.
        self.y += 24.0;
        if self.visible() {
            text_bold(s, self.body.x + 4.0, self.y, 15.0, PARCHMENT);
        }
        self.y += 6.0;
    }
    /// A "label ........ value" row, the value right-aligned and colored.
    fn stat(&mut self, label: &str, value: &str, color: Color) {
        self.y += 19.0;
        if self.visible() {
            text(label, self.body.x + 14.0, self.y, 14.0, INK);
            let vw = text_size(value, 14.0).width;
            text(
                value,
                self.body.x + self.body.w - vw - 6.0,
                self.y,
                14.0,
                color,
            );
        }
    }
    fn note(&mut self, s: &str, color: Color) {
        self.y += 18.0;
        if self.visible() {
            text(s, self.body.x + 14.0, self.y, 13.0, color);
        }
    }
}

/// Token color for a count that's bad when non-zero (else dim).
fn warn_if(n: usize, col: Color) -> Color {
    if n > 0 { col } else { DIM }
}

fn page_health(cx: &mut Ctx, r: &HealthReport) {
    cx.heading("PROVINCES (NODES)");
    cx.stat("total", &r.nodes_total.to_string(), INK);
    cx.stat("healthy", &r.nodes_healthy.to_string(), GOOD);
    cx.stat(
        "cordoned",
        &r.nodes_cordoned.to_string(),
        warn_if(r.nodes_cordoned, WARN),
    );
    cx.stat(
        "under pressure",
        &r.nodes_pressure.to_string(),
        warn_if(r.nodes_pressure, WARN),
    );
    cx.stat(
        "NotReady",
        &r.nodes_notready.to_string(),
        warn_if(r.nodes_notready, CRIT),
    );

    cx.heading("CITIZENS (PODS)");
    cx.stat("total", &r.pods_total.to_string(), INK);
    cx.stat("running", &r.pods_running.to_string(), GOOD);
    cx.stat(
        "starting",
        &r.pods_starting.to_string(),
        warn_if(r.pods_starting, STRUCT),
    );
    cx.stat(
        "pending",
        &r.pods_pending.to_string(),
        warn_if(r.pods_pending, WARN),
    );
    cx.stat(
        "failing",
        &r.pods_failing.to_string(),
        warn_if(r.pods_failing, CRIT),
    );
    cx.stat("succeeded", &r.pods_succeeded.to_string(), DIM);

    cx.heading("CITIES (WORKLOADS)");
    cx.stat("total", &r.workloads_total.to_string(), INK);
    cx.stat(
        "understrength",
        &r.workloads_degraded.to_string(),
        warn_if(r.workloads_degraded, WARN),
    );
    cx.note(
        if r.metrics_live {
            "node gauges: live usage (metrics-server)"
        } else {
            "node gauges: scheduling pressure (requests)"
        },
        DIM,
    );
}

fn page_storage(cx: &mut Ctx, r: &StorageReport) {
    cx.heading("GRANARIES (PVCs)");
    cx.stat("total", &r.total.to_string(), INK);
    cx.stat("bound", &r.bound.to_string(), GOOD);
    cx.stat("pending", &r.pending.to_string(), warn_if(r.pending, WARN));

    cx.heading("PENDING CLAIMS");
    if r.pending_claims.is_empty() {
        cx.note("all claims bound — granaries full", DIM);
    } else {
        for c in &r.pending_claims {
            cx.stat(&format!("{}/{}", c.namespace, c.name), &c.phase, WARN);
        }
    }
}

fn page_network(cx: &mut Ctx, r: &NetworkReport) {
    cx.heading("CONNECTIVITY");
    cx.stat("services (harbors)", &r.services.to_string(), INK);
    cx.stat("ingresses (gates)", &r.ingresses.to_string(), INK);

    cx.heading("ORPHAN GATES (INGRESS)");
    if r.orphan_ingresses.is_empty() {
        cx.note("every gate reaches a service", DIM);
    } else {
        for o in &r.orphan_ingresses {
            cx.stat(&format!("{}/{}", o.namespace, o.name), &o.detail, WARN);
        }
    }

    cx.heading("IDLE HARBORS (SERVICE)");
    if r.idle_services.is_empty() {
        cx.note("every harbor serves a city", DIM);
    } else {
        for s in &r.idle_services {
            cx.stat(&format!("{}/{}", s.namespace, s.name), &s.detail, STRUCT);
        }
    }
}
