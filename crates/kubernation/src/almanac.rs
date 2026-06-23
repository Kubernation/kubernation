//! The Almanac — Kubernation's in-app field guide, drawn on the
//! window system, for the map's visual vocabulary and how to read the world.
//! The Legend draws the *actual* marks beside each definition (reusing the
//! map's painters) so it stays a true key, not prose that drifts from the
//! map.

use macroquad::prelude::*;

use kubernation_core::state::attention::Severity;
use kubernation_core::state::model::{NodeHealth, PodState};
use kubernation_core::state::world::{CoastKind, WorldModel};

use crate::draw::{draw_cronjob, draw_gate, draw_granary, draw_harbor, draw_job};
use crate::net::Snapshot;
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
    /// Fly to (and select) a live example on the map, then close.
    Locate((u16, u16)),
}

/// A live map feature a legend entry can jump to — the field-guide cross-ref.
#[derive(Clone, Copy)]
enum Locator {
    City,
    Node,
    Road,
    Harbor,
    Gate,
    Granary,
    Custom,
    Encampment,
    Job,
    CronJob,
}

/// Which live feature (if any) a legend mark points at.
fn mark_locator(m: Mark) -> Option<Locator> {
    Some(match m {
        Mark::City => Locator::City,
        Mark::Road => Locator::Road,
        Mark::Terrain(_) => Locator::Node,
        Mark::Harbor => Locator::Harbor,
        Mark::Gate => Locator::Gate,
        Mark::Granary => Locator::Granary,
        Mark::Custom => Locator::Custom,
        Mark::Camp => Locator::Encampment,
        Mark::Job => Locator::Job,
        Mark::CronJob => Locator::CronJob,
        Mark::Pod(_) | Mark::Sev(_) | Mark::Gauge => return None,
    })
}

/// The hot world's first live instance of `loc`, as a scene cell (hot is at
/// offset 0, so a hot-world cell is already a scene cell).
fn locate(w: &WorldModel, loc: Locator) -> Option<(u16, u16)> {
    let structure = |glyph: char| {
        w.islands.iter().find_map(|isl| {
            isl.structures
                .iter()
                .find(|s| s.glyph == glyph)
                .map(|s| (isl.x + 1, s.y))
        })
    };
    let provinces = || w.continents.iter().flat_map(|c| &c.provinces);
    match loc {
        Locator::City => w.cities().next().map(|c| (c.x, c.y)),
        Locator::Granary => w.cities().find(|c| c.storage.is_some()).map(|c| (c.x, c.y)),
        Locator::Node => provinces().next().map(|p| (p.x + 2, p.y)),
        Locator::Road => provinces().find(|p| p.infra > 0).map(|p| (p.x + 2, p.y)),
        Locator::Harbor => w
            .continents
            .iter()
            .flat_map(|c| &c.coast)
            .find(|m| m.kind == CoastKind::Harbor)
            .map(|m| (m.x, m.y)),
        Locator::Gate => w
            .continents
            .iter()
            .flat_map(|c| &c.coast)
            .find(|m| m.kind == CoastKind::Gate)
            .map(|m| (m.x, m.y)),
        Locator::Custom => structure('✦'),
        Locator::Encampment => structure('◌'),
        Locator::Job => structure('◈'),
        Locator::CronJob => structure('◷'),
    }
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

    /// Jump to a page by tab index (keyboard 1-4); out-of-range is ignored.
    pub fn go_idx(&mut self, i: usize) {
        if let Some(p) = Page::ALL.get(i) {
            self.go(*p);
        }
    }

    /// Step to the previous/next page (keyboard left/right).
    pub fn cycle(&mut self, delta: i32) {
        let n = Page::ALL.len() as i32;
        let i = (self.page.idx() as i32 + delta).rem_euclid(n);
        self.go(Page::ALL[i as usize]);
    }

    /// Wheel scroll (positive = up).
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    /// Draw the window + current page; resolve clicks into an action. `snap`
    /// lets the Legend light up entries that have a live example to fly to.
    pub fn draw(&mut self, snap: Option<&Snapshot>, mouse: Vec2, click: bool) -> AlmanacAction {
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
            world: snap.map(|s| &s.hot.models.world),
            mouse,
            click,
            pending: None,
        };
        match self.page {
            Page::Legend => page_legend(&mut cx),
            Page::World => page_world(&mut cx),
            Page::Controls => page_controls(&mut cx),
            Page::Reading => page_reading(&mut cx),
        }
        let pending = cx.pending;
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

        // A click on a lit legend entry flies to its live example.
        if let Some(cell) = pending {
            return AlmanacAction::Locate(cell);
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

struct Ctx<'a> {
    body: Rect,
    y: f32,
    /// The hot world, for resolving live examples (None until first sync).
    world: Option<&'a WorldModel>,
    mouse: Vec2,
    click: bool,
    /// Set when a lit legend entry is clicked: the cell to fly to.
    pending: Option<(u16, u16)>,
}

const LINE: f32 = 19.0;

impl Ctx<'_> {
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

    /// A legend row: the mark, a bold name, and a wrapped description. If the
    /// mark points at a live map feature, the row lights up (a `›` chevron +
    /// hover highlight) and a click flies the camera to an example of it.
    fn entry(&mut self, m: Mark, name: &str, desc: &str) {
        let text_x = self.body.x + 40.0;
        let wrap_w = self.body.w - 64.0;
        let lines = wrap(desc, wrap_w, 14.0);
        let block_h = LINE * lines.len().max(1) as f32 + 6.0;
        let top = self.y;
        let target = mark_locator(m).and_then(|l| self.world.and_then(|w| locate(w, l)));
        let fully = top >= self.body.y && top + block_h <= self.body.y + self.body.h;
        let row = Rect::new(self.body.x, top, self.body.w, block_h);
        let hot = target.is_some() && fully && row.contains(self.mouse);
        if self.visible(top, block_h) {
            if hot {
                draw_rectangle(
                    row.x - 6.0,
                    row.y,
                    row.w + 10.0,
                    row.h,
                    Color::new(1.0, 1.0, 1.0, 0.07),
                );
            }
            draw_mark(m, vec2(self.body.x + 16.0, top + block_h / 2.0 - 4.0));
            text_bold(name, text_x, top + 13.0, 15.0, INK);
            for (i, l) in lines.iter().enumerate() {
                text(l, text_x, top + 13.0 + (i as f32 + 1.0) * LINE, 14.0, DIM);
            }
            if target.is_some() {
                // A chevron marks a live, clickable cross-reference.
                let col = if hot {
                    PARCHMENT
                } else {
                    darker(PARCHMENT, 0.7)
                };
                text(">", self.body.x + self.body.w - 16.0, top + 13.0, 16.0, col);
            }
        }
        if hot && self.click {
            self.pending = target;
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
    cx.para("Entries marked  >  have a live example — click to fly there.");
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
        "Kubernation renders the cluster as a living world you explore, in the grammar of classic 4X strategy games — but the nouns stay kubectl-greppable.",
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
        "An attention queue surfaces what needs focus and parks your cursor on it: 4X's \"next unit needing orders\", not a wall of dashboards.",
    );
    cx.heading("Read-first, gated writes");
    cx.para(
        "Kubernation reads by default — exploring the world never changes the cluster. A few deliberate writes exist, each confirmed and RBAC-checked: evict a pod, commit a planning turn (scale/cordon/restart/image/rollback), and Game Day chaos drills. Port-forward is active but non-mutating. The whole write surface is one small auditable file.",
    );
}

fn page_controls(cx: &mut Ctx) {
    cx.heading("Explore");
    cx.key("Drag / WASD / arrows", "pan the map");
    cx.key("Mouse wheel", "zoom (anchored at the cursor)");
    cx.key("F", "fit the whole world on screen");
    cx.key("] / [", "sail to the next / previous city");
    cx.key("N", "fly to the next concern");
    cx.key("L", "tail the focused concern's offending pod");
    cx.key(
        "B",
        "blast radius — highlight what a selected node/city (or focused concern) affects",
    );
    cx.heading("Inspect");
    cx.key("Click land / city", "open the node or city panel");
    cx.key("Click a harbor / gate", "open the city it serves");
    cx.key(
        "Click a pod row",
        "tail that pod's logs (lines tinted by severity)",
    );
    cx.key(
        "in logs: / p T s",
        "filter (AND · !excl) · previous · timestamps · window",
    );
    cx.key(
        "Hover a pod row",
        "reveal fwd (port-forward) · yaml · evict — RBAC-gated",
    );
    cx.key(
        "fwd · FORWARDS column",
        "tunnel 127.0.0.1 → the pod; x stops it",
    );
    cx.key("y · pod row yaml", "inspect YAML — workload/node, or a pod");
    cx.key(
        "c · w (logs / yaml)",
        "copy to clipboard · export to a file",
    );
    cx.key(
        ": (resource browser)",
        "any kind — pick, then click a row's YAML",
    );
    cx.key("Hover", "tooltip for whatever is under the cursor");
    cx.heading("Plan & cluster");
    cx.key("t", "open the End-of-Turn review (staged changes)");
    cx.key("C", "switch kube context");
    cx.key("? / F1", "this Almanac");
    cx.key("Esc", "close the open panel / window");
    cx.key("Q", "quit");
    cx.heading("Menu bar");
    cx.para(
        "The top bar holds the menus: Game (context, fit, export after-action report, quit), View (the map overlay — terrain health, cpu/mem pressure, replica health, namespace territory, walls, or saturation; and the Annals), Orders (end of turn, discard), Game Day (chaos drills), Advisors (Health / Storage / Network / Right-sizing / Hardening / Posture summaries), World (namespace filter), Help (Charter — your RBAC access — and the field guide). Click a title to open it.",
    );
    cx.heading("Saturation overlay (strain)");
    cx.para(
        "The 4th golden signal — how full a province is toward its hard limits: cpu/mem usage, scheduled pods vs the kubelet max-pods, and the kubelet Disk/PID/Mem-pressure conditions. Red = at/over a limit and refusing or evicting work; the province SELECTION names the binding dimension (e.g. 'pods 105/110', 'DiskPressure (pegged)'). Distinct from Pressure, which shows cpu/mem utilization only — Saturation also lights up a node at max-pods or under a kubelet condition while cpu/mem look calm (and needs no metrics-server for those axes).",
    );
    cx.heading("Oracle (BYO-LLM)");
    cx.para(
        "The Wonders menu holds the Oracle: a bring-your-own language model you consult to EXPLAIN a scope — the whole realm, a selected workload or node, or a focused concern — and which may PROPOSE a fix (scale / restart / set-image / rollback / cordon). It is advisory: the model sees a redacted, fenced summary of what Kubernation already observed (never raw cluster dumps, never Secret values) and can NEVER change the cluster itself — a proposal is validated against the live cluster and offered as a Stage button that enters the planning turn, applied only through the usual confirmed, RBAC-checked, dry-run gate. For a crash/error concern the offending pod's logs are included automatically, and after a reply an INVESTIGATE FURTHER row lets you fold in more context with one click (logs / storage / blast radius / rollout history / widen to node) and re-consult — the app gathers the data, the model only advises. The consult asks a question tailored to the scope (why a workload is unhealthy, what is straining a node, the realm's worst-first priorities) and, for a workload or concern, folds in the root-cause diagnosis Kubernation already computed (the why + the fix) so the model reasons over the failure instead of guessing. At realm or node scope the reply also offers a CONSULT NEXT row: clickable links — seeded from the attention queue (a node lists its troubled stationed workloads) plus anything the model flags, each validated against the live cluster — that jump the consult to that single object so you can drill from the whole realm down to a specific suspect. Use Preview to review exactly what will be sent before you Consult; while a consult is in flight the spinner counts up and a Cancel stops it, and once an answer is back c copies it / w exports it to a file. Configure endpoints in the Oracle window's Settings face: save named PROFILES (a local Ollama, a corporate frontier model, …) and switch between them. For a local endpoint it lists the models you've pulled (click to pick); for a remote one you enter a URL + a masked API token + model. Profiles persist to ~/.config/kubernation/oracle.json, including the token by your explicit per-profile opt-in (PLAINTEXT on disk at mode 0600 — for high-sensitivity tokens prefer the KUBERNATION_LLM_TOKEN env var, which is never persisted). A remote endpoint publishes off-laptop, so it stays off until you Arm it (switching profiles re-disarms). The default is a local Ollama at localhost:11434 with model qwen3:30b — a fast Mixture-of-Experts that answers in ~10–15s (must be pulled: `ollama pull qwen3:30b`); --llm-url / --llm-model override the active profile for one launch. A remote endpoint publishes off-laptop, so it is off until you Arm it; replies are model-generated — verify before acting.",
    );
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
    cx.key("Regional", "towns + chips; names selected when crowded");
    cx.key("World (zoom out)", "each province aggregates to one badge");
    cx.heading("Gauges");
    cx.para(
        "Node cpu / mem read live usage when metrics-server is installed, otherwise scheduling pressure (pod requests / allocatable). Calm below 70%, elevated 70-90% (yellow), high above 90% (red). With metrics-server, a trend sparkline under each gauge shows the last ~15 minutes (and the STATUS column shows the cluster trend).",
    );
    cx.heading("Treasury (error budget)");
    cx.para(
        "Each city window shows an availability SLO and the error budget it spends down — full when the workload stays up, draining when it flaps, exhausted when availability falls below the target (default 99%). Availability is derived from pod readiness (at least one replica up) over a recent window — no metrics-server needed. Set a per-workload target with the city's SLO stepper or a `kubernation.io/slo-target` annotation (e.g. \"99.9\"). A burning or exhausted budget also raises a queue concern.",
    );
    cx.heading("Game Day (chaos)");
    cx.para(
        "The Game Day menu opens a chaos drill: pick a target, choose an experiment (or a compound TIER — Skirmish / Raid / Siege), preview the dry-run steps + blast radius + budget cost, then run it — a real, confirmed failure. Workload experiments: outage (scale to 0), kill one / a percentage / all pods, scale spike (surge), broken image (unresolvable ref), and partition (deny-all / ingress / egress NetworkPolicy). Node failure cordons + drains a node; cordon-freeze cordons without draining. A scorecard shows the response (steady-state, recovery time, MTTD, budget spent); reversible drills offer Restore (manual / auto-after-60s / on exit). Control-plane / system namespaces and control-plane nodes are never targetable.",
    );
    cx.heading("Attention");
    cx.para(
        "Pod-level failures aggregate per owning workload — one \"city in trouble\", not forty pod alarms. The right column's ATTENTION section lists the worst few; N walks them, and clicking a concern flies there and opens its drill-down.",
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
