//! The Almanac — K8sCiv's Civilopedia. An in-app reference, drawn on the
//! window system, for the map's visual vocabulary and how to read the world.
//! The Legend draws the *actual* marks beside each definition (reusing the
//! map's painters) so it stays a true key, not prose that drifts from the
//! map.

use macroquad::prelude::*;

use k8sciv_core::state::attention::Severity;
use k8sciv_core::state::model::{NodeHealth, PodState};

use crate::draw::{draw_cronjob, draw_gate, draw_granary, draw_harbor, draw_job};
use crate::panels::pod_color;
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::draw_window;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Legend,
    World,
    Controls,
    Reading,
}

impl Page {
    const ALL: [Page; 4] = [Page::Legend, Page::World, Page::Controls, Page::Reading];
    fn idx(self) -> usize {
        Page::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

/// What a frame's interaction asks the caller to do.
pub enum AlmanacAction {
    None,
    Close,
    Page(Page),
}

/// A drawn legend mark — the same shapes the map uses.
#[derive(Clone, Copy)]
enum Mark {
    Harbor,
    Gate,
    Granary,
    Job,
    CronJob,
    City,
    Road,
    Custom,
    Camp,
    Terrain(NodeHealth),
    Pod(PodState),
    Sev(Severity),
    Gauge,
}

#[derive(Default)]
pub struct Almanac {
    pub page: Page,
    scroll: f32,
    max_scroll: f32,
}

impl Almanac {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn go(&mut self, page: Page) {
        self.page = page;
        self.scroll = 0.0;
    }

    /// Wheel scroll (positive = up).
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    /// Draw the window + current page; resolve clicks into an action.
    pub fn draw(&mut self, mouse: Vec2, click: bool) -> AlmanacAction {
        let labels = ["Legend", "World", "Controls", "Reading", "Close"];
        let win = draw_window(
            "Almanac — reading the world",
            vec2(740.0, 580.0),
            &labels,
            self.page.idx(),
        );

        let mut cx = Ctx {
            body: win.body,
            y: win.body.y - self.scroll,
        };
        match self.page {
            Page::Legend => page_legend(&mut cx),
            Page::World => page_world(&mut cx),
            Page::Controls => page_controls(&mut cx),
            Page::Reading => page_reading(&mut cx),
        }
        let content_h = cx.y - (win.body.y - self.scroll);
        self.max_scroll = (content_h - win.body.h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);

        // Scrollbar hint.
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
                return AlmanacAction::Close;
            }
            if let Some(i) = win.button_at(mouse) {
                if i >= Page::ALL.len() {
                    return AlmanacAction::Close;
                }
                return AlmanacAction::Page(Page::ALL[i]);
            }
            // Click outside the window dismisses it.
            if !win.frame.contains(mouse) {
                return AlmanacAction::Close;
            }
        }
        AlmanacAction::None
    }
}

// --- content rendering -----------------------------------------------------

struct Ctx {
    body: Rect,
    y: f32,
}

const LINE: f32 = 19.0;

impl Ctx {
    fn visible(&self, top: f32, h: f32) -> bool {
        top + h >= self.body.y && top <= self.body.y + self.body.h
    }

    fn heading(&mut self, s: &str) {
        self.y += 10.0;
        if self.visible(self.y - 16.0, 20.0) {
            text_bold(s, self.body.x, self.y, 17.0, PARCHMENT);
            draw_line(
                self.body.x,
                self.y + 4.0,
                self.body.x + self.body.w,
                self.y + 4.0,
                1.0,
                darker(PARCHMENT, 0.5),
            );
        }
        self.y += 18.0;
    }

    /// A legend row: the mark, a bold name, and a wrapped description.
    fn entry(&mut self, m: Mark, name: &str, desc: &str) {
        let text_x = self.body.x + 40.0;
        let wrap_w = self.body.w - 44.0;
        let lines = wrap(desc, wrap_w, 14.0);
        let block_h = LINE * lines.len().max(1) as f32 + 6.0;
        let top = self.y;
        if self.visible(top, block_h) {
            draw_mark(m, vec2(self.body.x + 16.0, top + block_h / 2.0 - 4.0));
            text_bold(name, text_x, top + 13.0, 15.0, INK);
            for (i, l) in lines.iter().enumerate() {
                text(l, text_x, top + 13.0 + (i as f32 + 1.0) * LINE, 14.0, DIM);
            }
        }
        // name line + desc lines.
        self.y += LINE + LINE * lines.len() as f32 + 6.0;
    }

    /// A free paragraph.
    fn para(&mut self, s: &str) {
        let lines = wrap(s, self.body.w, 15.0);
        for l in &lines {
            if self.visible(self.y, LINE) {
                text(l, self.body.x, self.y + 13.0, 15.0, INK);
            }
            self.y += LINE;
        }
        self.y += 4.0;
    }

    /// A key/value control row.
    fn key(&mut self, k: &str, v: &str) {
        if self.visible(self.y, LINE) {
            text_bold(k, self.body.x + 8.0, self.y + 13.0, 14.0, PARCHMENT);
            text(v, self.body.x + 168.0, self.y + 13.0, 14.0, INK);
        }
        self.y += LINE + 2.0;
    }
}

fn wrap(s: &str, max_w: f32, size: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let trial = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if text_size(&trial, size).width > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = trial;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn page_legend(cx: &mut Ctx) {
    cx.heading("Land & settlements");
    cx.entry(
        Mark::City,
        "City",
        "A workload (Deployment or StatefulSet), sited on the province holding most of its pods. Its size grows with ready replicas; it migrates only when its pods do.",
    );
    cx.entry(
        Mark::Road,
        "Road",
        "A DaemonSet — paved across every province its pods run on, never a city.",
    );
    cx.entry(
        Mark::Terrain(NodeHealth::Healthy),
        "Province",
        "A node. Terrain colour is its health: green healthy, olive cordoned, brown under pressure, dark-red NotReady.",
    );

    cx.heading("The coast — how traffic enters");
    cx.entry(
        Mark::Harbor,
        "Harbor (Service)",
        "A Service fronting the city, moored on its east coast — the shoreline is the network boundary.",
    );
    cx.entry(
        Mark::Gate,
        "Gate (Ingress)",
        "An Ingress routing to the city from outside, beside its harbor.",
    );

    cx.heading("The interior — persistent state");
    cx.entry(
        Mark::Granary,
        "Granary (PVC)",
        "Persistent volume claims the city mounts, inland of it. Cyan when every claim is Bound, yellow when one is still pending.",
    );

    cx.heading("The southern islands — abstract & transient");
    cx.entry(
        Mark::Custom,
        "Structure (custom resource)",
        "A projected custom-resource instance (via --project), on its namespace island.",
    );
    cx.entry(
        Mark::Camp,
        "Encampment",
        "A workload with no pods on any land yet — it has nowhere to settle.",
    );
    cx.entry(
        Mark::Job,
        "Expedition (Job)",
        "A batch Job — one-shot work. Its status rides the label (done, active, or failed in yellow).",
    );
    cx.entry(
        Mark::CronJob,
        "CronJob",
        "A scheduled job; the label shows its cron schedule.",
    );

    cx.heading("Pods");
    cx.entry(Mark::Pod(PodState::Ok), "Running & ready", "Healthy.");
    cx.entry(
        Mark::Pod(PodState::Starting),
        "Starting",
        "Running, not yet ready.",
    );
    cx.entry(
        Mark::Pod(PodState::Pending),
        "Pending",
        "Not scheduled / waiting.",
    );
    cx.entry(
        Mark::Pod(PodState::Failing),
        "Failing",
        "Crash, image, or config trouble.",
    );
    cx.entry(Mark::Pod(PodState::Succeeded), "Succeeded", "Completed.");

    cx.heading("Attention & gauges");
    cx.entry(
        Mark::Sev(Severity::Critical),
        "Critical",
        "Crashes, image/config failure, stalled rollout.",
    );
    cx.entry(
        Mark::Sev(Severity::Warning),
        "Warning",
        "Replica gaps, unschedulable, OOM, flapping, pressure, pending PVC.",
    );
    cx.entry(
        Mark::Sev(Severity::Info),
        "Info",
        "Cordons and grouped notes.",
    );
    cx.entry(
        Mark::Gauge,
        "Gauge",
        "cpu / mem per node: live usage when metrics-server is present, else scheduling pressure (requests / allocatable).",
    );
}

fn page_world(cx: &mut Ctx) {
    cx.heading("The world is your cluster");
    cx.para(
        "K8sCiv renders the cluster as a living world you explore, in the grammar of Civilization — but the nouns stay kubectl-greppable.",
    );
    cx.para("Zones are continents of solid land, separated by ocean.");
    cx.para(
        "Nodes are provinces — patches of health-textured terrain. A zone's nodes stack into one landmass with an irregular, noise-carved coastline.",
    );
    cx.para(
        "Workloads are cities, sited on the province hosting the plurality of their pods. DaemonSets are roads instead of cities.",
    );
    cx.para(
        "A city's network exposure is moored on its east coast (Service harbors, Ingress gates); its persistent storage sits inland (PVC granaries).",
    );
    cx.para(
        "Things with no place on the land — custom resources, zero-pod workloads, Jobs and CronJobs — live on namespace islands in the southern sea.",
    );
    cx.para(
        "An attention queue surfaces what needs focus and parks your cursor on it: Civ's \"next unit needing orders\", not a wall of dashboards.",
    );
    cx.heading("Observe-only");
    cx.para(
        "K8sCiv only watches. There are no mutation paths anywhere — exploring the world never changes the cluster.",
    );
}

fn page_controls(cx: &mut Ctx) {
    cx.heading("Explore");
    cx.key("Drag / WASD / arrows", "pan the map");
    cx.key("Mouse wheel", "zoom (anchored at the cursor)");
    cx.key("F", "fit the whole world on screen");
    cx.key("] / [", "sail to the next / previous city");
    cx.key("N", "fly to the next concern");
    cx.heading("Inspect");
    cx.key("Click land / city", "open the node or city panel");
    cx.key("Click a harbor / gate", "open the city it serves");
    cx.key("Click a pod row", "tail that pod's logs");
    cx.key("Hover", "tooltip for whatever is under the cursor");
    cx.heading("Cluster & windows");
    cx.key("C", "switch kube context");
    cx.key("? / F1", "this Almanac");
    cx.key("Esc", "close the open panel / window");
    cx.key("Q", "quit");
}

fn page_reading(cx: &mut Ctx) {
    cx.heading("Cartographic scale");
    cx.para(
        "What the map shows thins as you zoom out (after Monmonier's generalization-by-scale):",
    );
    cx.key(
        "Local (zoom in)",
        "everything — full names, every settlement",
    );
    cx.key("Regional", "sprites + chips; names selected when crowded");
    cx.key("World (zoom out)", "each province aggregates to one badge");
    cx.heading("Gauges");
    cx.para(
        "Node cpu / mem read live usage when metrics-server is installed, otherwise scheduling pressure (pod requests / allocatable). Calm below 70%, elevated 70-90% (yellow), high above 90% (red).",
    );
    cx.heading("Attention");
    cx.para(
        "Pod-level failures aggregate per owning workload — one \"city in trouble\", not forty pod alarms. The strip along the bottom shows the worst few; N walks them.",
    );
    cx.heading("The pair (with --warm)");
    cx.para(
        "A warm standby appears as a second archipelago east of the hot one. Cities carry a sync chip: = in sync, ≠r replica drift, ≠i image drift, −w missing on warm, +w only on warm.",
    );
}

// --- mark painting (reuses the map's shapes / palette) ---------------------

fn draw_mark(m: Mark, c: Vec2) {
    match m {
        Mark::Harbor => draw_harbor(c, 2.0),
        Mark::Gate => draw_gate(c, 2.0),
        Mark::Granary => draw_granary(c, 1.9, STRUCT),
        Mark::Job => draw_job(c, 1.8, STRUCT),
        Mark::CronJob => draw_cronjob(c, 1.9, STRUCT),
        Mark::City => {
            let w = 18.0;
            let h = 10.0;
            draw_rectangle(c.x - w / 2.0, c.y - h / 2.0, w, h, HOUSE);
            draw_triangle(
                vec2(c.x - w / 2.0 - 2.0, c.y - h / 2.0),
                vec2(c.x + w / 2.0 + 2.0, c.y - h / 2.0),
                vec2(c.x, c.y - h / 2.0 - 7.0),
                ROOF,
            );
        }
        Mark::Road => {
            draw_line(c.x - 11.0, c.y + 2.0, c.x + 11.0, c.y + 2.0, 3.0, ROAD);
            for i in -2..=2 {
                let x = c.x + i as f32 * 5.0;
                draw_line(x, c.y - 1.0, x, c.y + 5.0, 1.0, darker(ROAD, 0.65));
            }
        }
        Mark::Custom => {
            draw_poly(c.x, c.y, 4, 7.0, 45.0, STRUCT);
            draw_poly_lines(c.x, c.y, 4, 7.0, 45.0, 1.5, darker(STRUCT, 0.5));
        }
        Mark::Camp => {
            draw_triangle(
                vec2(c.x - 8.0, c.y + 6.0),
                vec2(c.x + 8.0, c.y + 6.0),
                vec2(c.x, c.y - 6.0),
                DIM,
            );
        }
        Mark::Terrain(h) => {
            draw_rectangle(c.x - 9.0, c.y - 7.0, 18.0, 14.0, terrain(h));
            draw_rectangle_lines(
                c.x - 9.0,
                c.y - 7.0,
                18.0,
                14.0,
                1.0,
                darker(terrain(h), 0.6),
            );
        }
        Mark::Pod(s) => {
            draw_circle(c.x, c.y, 6.0, pod_color(s));
        }
        Mark::Sev(s) => {
            draw_circle(c.x, c.y, 6.0, severity_color(s));
        }
        Mark::Gauge => {
            let bw = 24.0;
            draw_rectangle(c.x - bw / 2.0, c.y - 4.0, bw, 8.0, darker(PANEL, 0.6));
            draw_rectangle(
                c.x - bw / 2.0,
                c.y - 4.0,
                bw * 0.62,
                8.0,
                Color::new(0.35, 0.60, 0.30, 1.0),
            );
            draw_rectangle_lines(
                c.x - bw / 2.0,
                c.y - 4.0,
                bw,
                8.0,
                1.0,
                darker(PARCHMENT, 0.6),
            );
        }
    }
}
