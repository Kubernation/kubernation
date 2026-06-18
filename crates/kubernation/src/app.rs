use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use color_eyre::Result;
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui_crossterm::crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::MissedTickBehavior;

use crate::config::Config;
use crate::events::{AppEvent, ClusterId, WorldDelta};
use crate::ui::attention_panel::AttentionPanel;
use crate::ui::browse::{BrowseView, ResourcePicker};
use crate::ui::city::CityView;
use crate::ui::context_picker::ContextPicker;
use crate::ui::inspect::InspectView;
use crate::ui::logs::LogsView;
use crate::ui::map::MapView;
use crate::ui::namespace_picker::NamespacePicker;
use crate::ui::node_detail::NodeDetailView;
use crate::ui::plan::{PlanCmd, PlanView};
use crate::ui::theme::Theme;
use crate::ui::workloads::WorkloadListView;
use crate::ui::{
    Action, Component, Edge, OverlayMode, RenderCtx, centered, help, sidebar, status_bar,
};
use kubernation_core::k8s::client::Cluster;
use kubernation_core::k8s::{actions, browse, client, logs, watch, watch::WorldHandle};
use kubernation_core::state::attention::{Concern, Severity};
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::model::Models;
use kubernation_core::state::pair::PairSync;
use kubernation_core::state::planned::{Intervention, PlannedWorld, plan_diff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Map,
    Workloads,
    City,
    Node,
    Logs,
    /// The read-only YAML inspector ("dossier").
    Inspect,
    /// The resource browser's generic table for one kind.
    Browse,
    /// The End-of-Turn review — the staged planning-turn diff + commit.
    Plan,
}

/// Builds a `RenderCtx` for one world out of disjoint field borrows, so a
/// sibling view can be borrowed mutably at the same time.
macro_rules! ctx {
    ($self:ident, $id:expr) => {{
        let id: ClusterId = $id;
        let (models, world, ready) = match id {
            ClusterId::Hot => (&$self.models_hot, &$self.hot.world, $self.ready_hot),
            ClusterId::Warm => (
                $self.models_warm.as_ref().expect("warm models"),
                &$self.warm.as_ref().expect("warm world").world,
                $self.ready_warm,
            ),
        };
        RenderCtx {
            models,
            world,
            theme: &$self.theme,
            overlay: $self.overlay,
            ready,
            cluster: id,
            focused: $self.focus == id,
            pair: $self.pair.as_ref(),
            cluster_label: if $self.warm.is_some() {
                Some(id.label())
            } else {
                None
            },
            attention: &$self.attention,
            planned: &$self.planned,
        }
    }};
}

pub struct App {
    cfg: Config,
    theme: Theme,
    kubeconfig: Option<PathBuf>,
    projections: Vec<String>,

    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
    hot: WorldHandle,
    warm: Option<WorldHandle>,
    /// Clusters kept past spawn so the log view can fetch on demand.
    hot_cluster: Cluster,
    warm_cluster: Option<Cluster>,
    models_hot: Models,
    models_warm: Option<Models>,
    pair: Option<PairSync>,
    /// Merged, severity-ordered concerns across both worlds.
    attention: Vec<Concern>,
    /// The staged planning turn (hot cluster only). Preview-only until the
    /// operator commits it from the End-of-Turn review.
    planned: PlannedWorld,
    plan: PlanView,

    screens: Vec<Screen>,
    focus: ClusterId,
    map_hot: MapView,
    map_warm: MapView,
    workloads: WorkloadListView,
    city: CityView,
    city_cluster: ClusterId,
    node: NodeDetailView,
    node_cluster: ClusterId,
    logs: LogsView,
    log_gen: u64,
    last_log_fetch: Instant,
    inspect: InspectView,
    inspect_cluster: ClusterId,
    /// Resource browser: discovered kinds (cached), the `:` picker, and the
    /// generic table for the kind currently being browsed (hot cluster).
    kinds: Vec<browse::KindEntry>,
    /// API groups discovery couldn't enumerate (shown in the picker).
    kinds_warnings: Vec<String>,
    kinds_loading: bool,
    resource_picker: ResourcePicker,
    browse: BrowseView,
    browse_kind: Option<browse::KindEntry>,
    /// Generation token so a slow LIST for a kind the user moved off can't
    /// clobber the current table (mirrors `log_gen`).
    browse_gen: u64,
    /// Generation token so a slow discovery for a cluster the user switched
    /// away from can't repopulate the new cluster's kind list.
    discover_gen: u64,
    attention_panel: AttentionPanel,
    picker: ContextPicker,
    ns_picker: NamespacePicker,
    /// Scope every view to these namespaces (the world is observed in full;
    /// this filters the derived models). Hot + warm share one filter.
    ns_filter: NamespaceFilter,
    help_open: bool,
    overlay: OverlayMode,

    ready_hot: bool,
    ready_warm: bool,
    dirty: bool,
    quit: bool,
    flash: Option<String>,
    /// Pod awaiting evict confirmation (cluster, namespace, pod) — gated
    /// behind a y/n prompt (one of the TUI's two writes; the other is commit).
    pending_evict: Option<(ClusterId, String, String)>,
    /// RBAC cache: may the user delete pods in (cluster, namespace)?
    evict_perm: HashMap<(ClusterId, String), bool>,
    /// The staged turn awaits a y/n confirm before it commits to the cluster.
    pending_commit: bool,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: Config,
        kubeconfig: Option<PathBuf>,
        projections: Vec<String>,
        hot: WorldHandle,
        warm: Option<WorldHandle>,
        hot_cluster: Cluster,
        warm_cluster: Option<Cluster>,
        tx: Sender<AppEvent>,
        rx: Receiver<AppEvent>,
    ) -> Self {
        let theme = Theme::new(cfg.color);
        let attention_panel = AttentionPanel::new(cfg.attention_expanded);
        let models_warm = warm.as_ref().map(|_| Models::default());
        Self {
            cfg,
            theme,
            kubeconfig,
            projections,
            tx,
            rx,
            hot,
            warm,
            hot_cluster,
            warm_cluster,
            models_hot: Models::default(),
            models_warm,
            pair: None,
            attention: Vec::new(),
            planned: PlannedWorld::default(),
            plan: PlanView::default(),
            screens: vec![Screen::Map],
            focus: ClusterId::Hot,
            map_hot: MapView::default(),
            map_warm: MapView::default(),
            workloads: WorkloadListView::default(),
            city: CityView::default(),
            city_cluster: ClusterId::Hot,
            node: NodeDetailView::default(),
            node_cluster: ClusterId::Hot,
            logs: LogsView::default(),
            log_gen: 0,
            last_log_fetch: Instant::now(),
            inspect: InspectView::default(),
            inspect_cluster: ClusterId::Hot,
            kinds: Vec::new(),
            kinds_warnings: Vec::new(),
            kinds_loading: false,
            resource_picker: ResourcePicker::default(),
            browse: BrowseView::default(),
            browse_kind: None,
            browse_gen: 0,
            discover_gen: 0,
            attention_panel,
            picker: ContextPicker::default(),
            ns_picker: NamespacePicker::default(),
            ns_filter: NamespaceFilter::All,
            help_open: false,
            overlay: OverlayMode::Pressure,
            ready_hot: false,
            ready_warm: false,
            dirty: false,
            quit: false,
            flash: None,
            pending_evict: None,
            evict_perm: HashMap::new(),
            pending_commit: false,
        }
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let mut tick = tokio::time::interval(Duration::from_millis(self.cfg.tick_ms()));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        self.draw(&mut terminal)?;
        loop {
            let mut needs_draw = false;
            tokio::select! {
                maybe = self.rx.recv() => match maybe {
                    None => break,
                    Some(ev) => needs_draw |= self.handle_event(ev).await,
                },
                _ = tick.tick() => {
                    if self.dirty {
                        self.rebuild();
                        needs_draw = true;
                    }
                    // Live tail: re-fetch the open log every ~2s.
                    if self.screens.last() == Some(&Screen::Logs)
                        && self.last_log_fetch.elapsed() >= Duration::from_secs(2)
                    {
                        self.fetch_logs();
                    }
                }
            }
            // Drain any burst without redrawing per event.
            while let Ok(ev) = self.rx.try_recv() {
                needs_draw |= self.handle_event(ev).await;
            }
            if self.quit {
                break;
            }
            if needs_draw {
                self.draw(&mut terminal)?;
            }
        }
        Ok(())
    }

    /// Returns true when a redraw should happen now (input/resize); world
    /// deltas only mark dirty and wait for the coalescing tick.
    async fn handle_event(&mut self, ev: AppEvent) -> bool {
        match ev {
            AppEvent::World(id, WorldDelta::Ready) => {
                match id {
                    ClusterId::Hot => self.ready_hot = true,
                    ClusterId::Warm => self.ready_warm = true,
                }
                self.rebuild();
                true
            }
            AppEvent::World(_, _) => {
                self.dirty = true;
                false
            }
            // Drop stale tails from a previous open (generation token).
            AppEvent::Logs {
                generation,
                result,
                container,
            } if generation == self.log_gen => {
                // Cache the resolved container so the next poll skips the get.
                if self.logs.container.is_none() {
                    self.logs.container = container;
                }
                self.logs.set_result(result);
                true
            }
            AppEvent::Logs { .. } => false,
            AppEvent::Evicted { result } => {
                self.flash = Some(match result {
                    Ok(msg) => msg,
                    Err(e) => format!("evict failed: {e}"),
                });
                true
            }
            AppEvent::Committed { outcome } => {
                self.flash = Some(if outcome.applied {
                    let n_ok = outcome.rows.iter().filter(|r| r.ok).count();
                    format!("committed {n_ok}/{} change(s)", outcome.rows.len())
                } else {
                    format!(
                        "commit blocked — {} change(s) failed dry-run",
                        outcome.rows.len()
                    )
                });
                self.plan.outcome = Some(outcome);
                self.dirty = true; // the world changed under us
                true
            }
            // Drop discovery from a cluster the user already switched away from.
            AppEvent::Kinds {
                generation,
                discovered,
            } if generation == self.discover_gen => {
                self.kinds = discovered.kinds;
                self.kinds_warnings = discovered.warnings;
                self.kinds_loading = false;
                if self.resource_picker.open {
                    self.resource_picker
                        .set_kinds(self.kinds.clone(), self.kinds_warnings.clone());
                }
                true
            }
            AppEvent::Kinds { .. } => false,
            // Drop a stale LIST whose kind the user already moved off (e.g.
            // picked A — slow — then B; A's late result must not clobber B).
            AppEvent::BrowseRows { generation, result } if generation == self.browse_gen => {
                self.browse.set_result(result);
                true
            }
            AppEvent::BrowseRows { .. } => false,
            AppEvent::Term(TermEvent::Key(key)) if key.kind != KeyEventKind::Release => {
                self.on_key(key).await;
                true
            }
            AppEvent::Term(TermEvent::Resize(_, _)) => true,
            _ => false,
        }
    }

    /// Re-derive all view models from the observed worlds.
    fn rebuild(&mut self) {
        refine_platform(&mut self.hot);
        if let Some(w) = self.warm.as_mut() {
            refine_platform(w);
        }
        self.models_hot = Models::build_filtered(&self.hot.world, &self.ns_filter);
        self.models_warm = self
            .warm
            .as_ref()
            .map(|w| Models::build_filtered(&w.world, &self.ns_filter));
        self.pair = self
            .warm
            .as_ref()
            .map(|w| PairSync::build(&self.hot.world, &w.world, &self.ns_filter));

        let mut merged = self.models_hot.attention.clone();
        if let Some(mw) = &self.models_warm {
            merged.extend(mw.attention.iter().cloned().map(|mut c| {
                c.cluster = ClusterId::Warm;
                c
            }));
        }
        if let Some(c) = self.pair.as_ref().and_then(|p| p.concern()) {
            merged.push(c);
        }
        merged.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.key.cmp(&b.key))
                .then_with(|| a.cluster.cmp(&b.cluster))
        });
        self.attention = merged;
        self.dirty = false;

        {
            let ctx = ctx!(self, ClusterId::Hot);
            self.map_hot.update(&ctx);
        }
        if self.warm.is_some() {
            let ctx = ctx!(self, ClusterId::Warm);
            self.map_warm.update(&ctx);
        }
        {
            let ctx = ctx!(self, self.focus);
            self.workloads.update(&ctx);
        }
        {
            let ctx = ctx!(self, self.view_cluster(Screen::City));
            self.city.update(&ctx);
        }
        {
            let ctx = ctx!(self, self.view_cluster(Screen::Node));
            self.node.update(&ctx);
        }
        {
            let ctx = ctx!(self, ClusterId::Hot);
            self.attention_panel.update(&ctx);
        }
    }

    fn models_for(&self, id: ClusterId) -> &Models {
        match id {
            ClusterId::Hot => &self.models_hot,
            ClusterId::Warm => self.models_warm.as_ref().unwrap_or(&self.models_hot),
        }
    }

    /// The observed (raw object) world for a cluster — used by the inspector to
    /// serialize the in-store object.
    fn observed_for(&self, id: ClusterId) -> &kubernation_core::state::observed::ObservedWorld {
        match id {
            ClusterId::Warm => self
                .warm
                .as_ref()
                .map(|h| &h.world)
                .unwrap_or(&self.hot.world),
            ClusterId::Hot => &self.hot.world,
        }
    }

    fn map_for(&mut self, id: ClusterId) -> &mut MapView {
        match id {
            ClusterId::Hot => &mut self.map_hot,
            ClusterId::Warm => &mut self.map_warm,
        }
    }

    /// Which world a screen's content belongs to. Detail views remember the
    /// cluster they were opened on; list/map follow the focus.
    fn view_cluster(&self, screen: Screen) -> ClusterId {
        let id = match screen {
            Screen::City => self.city_cluster,
            Screen::Node => self.node_cluster,
            Screen::Logs => self.logs.cluster,
            Screen::Inspect => self.inspect_cluster,
            Screen::Browse => ClusterId::Hot,
            _ => self.focus,
        };
        // Never hand out Warm when no warm world exists.
        if self.warm.is_none() {
            ClusterId::Hot
        } else {
            id
        }
    }

    async fn on_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        // The evict confirm swallows input: y/Enter writes, n/Esc/q backs out.
        if let Some((cluster, ns, pod)) = self.pending_evict.clone() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.pending_evict = None;
                    self.spawn_evict(cluster, ns, pod);
                }
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
                    self.pending_evict = None;
                }
                _ => {}
            }
            return;
        }
        // The commit confirm swallows input the same way (the planning turn's
        // write gate — y/Enter applies the staged changes, n/Esc/q backs out).
        if self.pending_commit {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.pending_commit = false;
                    self.spawn_commit();
                }
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
                    self.pending_commit = false;
                }
                _ => {}
            }
            return;
        }
        if self.help_open {
            if matches!(
                key.code,
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')
            ) {
                self.help_open = false;
            }
            return;
        }
        if self.picker.open {
            if let Some(a) = self.picker.handle_key(key) {
                self.apply(a, ClusterId::Hot).await;
            }
            return;
        }
        if self.ns_picker.open {
            if let Some(a) = self.ns_picker.handle_key(key) {
                self.apply(a, ClusterId::Hot).await;
            }
            return;
        }
        // The `:` kind picker captures all keys (chars type into its filter).
        if self.resource_picker.open {
            if let Some(a) = self.resource_picker.handle_key(key) {
                self.apply(a, ClusterId::Hot).await;
            }
            return;
        }
        if self.attention_panel.focused {
            match key.code {
                KeyCode::Tab | KeyCode::Esc => self.attention_panel.focused = false,
                _ => {
                    let action = {
                        let ctx = ctx!(self, ClusterId::Hot);
                        self.attention_panel.handle_key(key, &ctx)
                    };
                    if let Some(a) = action {
                        let source = self
                            .attention_panel
                            .current(&self.attention)
                            .map(|c| c.cluster)
                            .unwrap_or(self.focus);
                        self.attention_panel.focused = false;
                        self.apply(a, source).await;
                    }
                }
            }
            return;
        }
        // While editing the log filter, every keystroke (incl. Esc/Backspace,
        // which the app otherwise treats as "back") is text input.
        if self.screens.last() == Some(&Screen::Logs) && self.logs.filtering() {
            self.logs.filter_input(key);
            return;
        }
        // Same for the city's image editor (the "set image" planning verb).
        if self.screens.last() == Some(&Screen::City) && self.city.image_editing() {
            let source = self.view_cluster(Screen::City);
            if let Some(a) = self.city.image_input(key) {
                self.apply(a, source).await;
            }
            return;
        }
        // The End-of-Turn review owns its keys (so its c/x/D don't hit the
        // global bindings); a few navigation escapes still work.
        if self.screens.last() == Some(&Screen::Plan) {
            match key.code {
                KeyCode::Esc | KeyCode::Backspace => self.pop_screen(),
                KeyCode::Char('t') => self.pop_screen(),
                KeyCode::Char('m') => self.go_home(Screen::Map),
                KeyCode::Char('w') => self.go_home(Screen::Workloads),
                KeyCode::Char('?') => self.help_open = true,
                _ => {
                    let cmd = {
                        let ctx = ctx!(self, ClusterId::Hot);
                        self.plan.handle_key(key, &ctx)
                    };
                    if let Some(cmd) = cmd {
                        self.apply_plan_cmd(cmd);
                    }
                }
            }
            return;
        }

        // On the log / inspector overlays, `c`/`w` are copy / export (handled by
        // the view in the `_` arm below), not the global context / workloads
        // bindings — so those two globals defer there.
        let text_view = matches!(self.screens.last(), Some(Screen::Logs | Screen::Inspect));
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('t') => {
                self.plan.open();
                self.push_screen(Screen::Plan);
            }
            KeyCode::Char('m') => self.go_home(Screen::Map),
            KeyCode::Char('w') if !text_view => self.go_home(Screen::Workloads),
            KeyCode::Char('n') => {
                if let Some((source, a)) = self.attention_panel.next_action(&self.attention) {
                    self.apply(a, source).await;
                }
            }
            // `L` tails the focused concern's offending pod straight away (the
            // "next unit needing orders → and here's why" jump). With nothing
            // `n`-parked yet, it acts on the top concern — the one the collapsed
            // panel already highlights with its `[L]ogs` hint.
            KeyCode::Char('L') if !self.attention.is_empty() => {
                let i = self
                    .attention_panel
                    .cycle
                    .unwrap_or(0)
                    .min(self.attention.len() - 1);
                self.attention_panel.cycle = Some(i);
                let routed = {
                    let c = &self.attention[i];
                    (c.cluster, crate::ui::attention_panel::logs_action_for(c))
                };
                match routed {
                    (source, Some(a)) => self.apply(a, source).await,
                    (_, None) => self.flash = Some("this concern has no pod to tail".into()),
                }
            }
            KeyCode::Char('a') => self.attention_panel.expanded = !self.attention_panel.expanded,
            KeyCode::Tab if self.attention_panel.expanded => self.attention_panel.focused = true,
            KeyCode::Char('c') if !text_view => self.picker.open_with(
                &self.hot.world.meta.all_contexts,
                &self.hot.world.meta.context,
            ),
            KeyCode::Char('N') => self
                .ns_picker
                .open_with(self.hot.world.namespaces(), &self.ns_filter),
            // `:` opens the resource browser — discover kinds if needed.
            KeyCode::Char(':') => self.open_browser(),
            KeyCode::Char('1') => self.overlay = OverlayMode::Pressure,
            KeyCode::Char('2') => self.overlay = OverlayMode::ReplicaHealth,
            KeyCode::Char('3') => self.overlay = OverlayMode::Namespace,
            KeyCode::Esc | KeyCode::Backspace => self.pop_screen(),
            _ => {
                let screen = self.screens.last().copied().unwrap_or(Screen::Map);
                let source = self.view_cluster(screen);
                let action = match screen {
                    Screen::Map => {
                        if self.focus == ClusterId::Warm && self.warm.is_some() {
                            let ctx = ctx!(self, ClusterId::Warm);
                            self.map_warm.handle_key(key, &ctx)
                        } else {
                            let ctx = ctx!(self, ClusterId::Hot);
                            self.map_hot.handle_key(key, &ctx)
                        }
                    }
                    Screen::Workloads => {
                        let ctx = ctx!(self, source);
                        self.workloads.handle_key(key, &ctx)
                    }
                    Screen::City => {
                        let ctx = ctx!(self, source);
                        self.city.handle_key(key, &ctx)
                    }
                    Screen::Node => {
                        let ctx = ctx!(self, source);
                        self.node.handle_key(key, &ctx)
                    }
                    Screen::Logs => {
                        let ctx = ctx!(self, source);
                        self.logs.handle_key(key, &ctx)
                    }
                    Screen::Inspect => {
                        let ctx = ctx!(self, source);
                        self.inspect.handle_key(key, &ctx)
                    }
                    Screen::Browse => {
                        let ctx = ctx!(self, ClusterId::Hot);
                        self.browse.handle_key(key, &ctx)
                    }
                    // Plan keys are intercepted above; unreachable here.
                    Screen::Plan => None,
                };
                if let Some(a) = action {
                    let source = if screen == Screen::Map {
                        self.focus
                    } else {
                        source
                    };
                    self.apply(a, source).await;
                }
            }
        }
    }

    fn go_home(&mut self, base: Screen) {
        self.screens = vec![base];
        self.city.close();
        self.node.close();
    }

    fn pop_screen(&mut self) {
        if self.screens.len() > 1 {
            match self.screens.pop() {
                Some(Screen::City) => self.city.close(),
                Some(Screen::Node) => self.node.close(),
                _ => {}
            }
        }
    }

    fn push_screen(&mut self, s: Screen) {
        if self.screens.last() != Some(&s) {
            self.screens.push(s);
            if self.screens.len() > 8 {
                self.screens.remove(0);
            }
        }
    }

    async fn apply(&mut self, action: Action, source: ClusterId) {
        let source = if self.warm.is_none() {
            ClusterId::Hot
        } else {
            source
        };
        match action {
            Action::OpenWorkloadList => {
                self.focus = source;
                self.go_home(Screen::Workloads);
            }
            Action::OpenNode(name) => {
                // Park the explorer on the province too, so returning to
                // the map lands where the attention pointed.
                if let Some(pos) = self.models_for(source).world.province_pos(&name) {
                    self.map_for(source).jump_to(pos);
                }
                self.node.open(name);
                self.node_cluster = source;
                self.focus = source;
                self.push_screen(Screen::Node);
            }
            Action::OpenWorkload(r) => {
                let world = &self.models_for(source).world;
                if let Some(pos) = world.city_pos(&r).or_else(|| world.structure_pos(&r)) {
                    self.map_for(source).jump_to(pos);
                }
                self.city.open(r);
                self.city_cluster = source;
                self.focus = source;
                self.push_screen(Screen::City);
            }
            Action::OpenLogs {
                namespace,
                pod,
                previous,
            } => {
                self.logs.open(source, namespace, pod);
                self.logs.previous = previous; // smart default for crash-loops
                self.push_screen(Screen::Logs);
                self.fetch_logs();
            }
            Action::RefetchLogs => self.fetch_logs(),
            Action::InspectPod { namespace, pod } => {
                if let Some(y) = kubernation_core::state::inspect::pod_yaml(
                    self.observed_for(source),
                    &namespace,
                    &pod,
                ) {
                    self.inspect.open(format!("pod {namespace}/{pod}"), y);
                    self.inspect_cluster = source;
                    self.push_screen(Screen::Inspect);
                } else {
                    self.flash = Some(format!("no object for pod {namespace}/{pod}"));
                }
            }
            Action::InspectWorkload(r) => {
                if let Some(y) =
                    kubernation_core::state::inspect::workload_yaml(self.observed_for(source), &r)
                {
                    self.inspect
                        .open(format!("{} {}/{}", r.kind, r.namespace, r.name), y);
                    self.inspect_cluster = source;
                    self.push_screen(Screen::Inspect);
                } else {
                    self.flash = Some(format!("no object for {}", r.name));
                }
            }
            Action::InspectNode(name) => {
                if let Some(y) =
                    kubernation_core::state::inspect::node_yaml(self.observed_for(source), &name)
                {
                    self.inspect.open(format!("node {name}"), y);
                    self.inspect_cluster = source;
                    self.push_screen(Screen::Inspect);
                } else {
                    self.flash = Some(format!("no object for node {name}"));
                }
            }
            Action::CopyText(text) => {
                // Prefer the OS clipboard tool (reliable locally — e.g. pbcopy);
                // also emit OSC 52 for remote/SSH terminals that support it.
                let n = text.lines().count();
                let ok = os_clipboard_copy(&text);
                copy_osc52(&text);
                self.flash = Some(if ok {
                    format!("copied {n} lines to clipboard")
                } else {
                    format!("sent {n} lines via OSC 52 · w exports to a file")
                });
            }
            Action::ExportText { text, filename } => {
                self.flash = Some(export_to_file(&text, &filename));
            }
            Action::ListSelectedKind => {
                if let Some(kind) = self.resource_picker.selected_kind() {
                    self.browse.open(&kind);
                    self.browse_kind = Some(kind.clone());
                    self.push_screen(Screen::Browse);
                    self.spawn_list(kind);
                }
            }
            Action::RefreshBrowse => {
                if let Some(kind) = self.browse_kind.clone() {
                    self.browse.set_loading();
                    self.spawn_list(kind);
                }
            }
            Action::InspectSelected => {
                if let Some(obj) = self.browse.selected_object() {
                    let kind = obj
                        .types
                        .as_ref()
                        .map(|t| t.kind.clone())
                        .unwrap_or_default();
                    let ns = obj.metadata.namespace.clone().unwrap_or_default();
                    let name = obj.metadata.name.clone().unwrap_or_default();
                    let title = if ns.is_empty() {
                        format!("{kind} {name}")
                    } else {
                        format!("{kind} {ns}/{name}")
                    };
                    self.inspect
                        .open(title, kubernation_core::state::inspect::dynamic_yaml(&obj));
                    self.inspect_cluster = ClusterId::Hot;
                    self.push_screen(Screen::Inspect);
                }
            }
            Action::EvictPod { namespace, pod } => {
                // RBAC gate (cached per namespace): only raise the confirm if
                // the user may delete pods there; otherwise say why.
                let key = (source, namespace.clone());
                let cached = self.evict_perm.get(&key).copied();
                let allowed = if let Some(b) = cached {
                    Some(b)
                } else {
                    let client = match source {
                        ClusterId::Warm => self
                            .warm_cluster
                            .as_ref()
                            .map(|c| c.client.clone())
                            .unwrap_or_else(|| self.hot_cluster.client.clone()),
                        ClusterId::Hot => self.hot_cluster.client.clone(),
                    };
                    match actions::can_evict_pod(client, &namespace).await {
                        Ok(b) => {
                            self.evict_perm.insert(key, b);
                            Some(b)
                        }
                        Err(e) => {
                            self.flash = Some(format!("permission check failed: {e}"));
                            None
                        }
                    }
                };
                match allowed {
                    Some(true) => self.pending_evict = Some((source, namespace, pod)),
                    Some(false) => {
                        self.flash = Some(format!("no permission to evict pods in {namespace}"))
                    }
                    None => {}
                }
            }
            Action::Stage(iv) => {
                if source == ClusterId::Hot {
                    self.planned.stage(iv);
                    self.plan.outcome = None; // the staged set changed
                } else {
                    self.flash = Some("planning applies to the hot cluster only".into());
                }
            }
            Action::ToggleRestart(r) => {
                if source == ClusterId::Hot {
                    if self.planned.restarting(&r) {
                        self.planned.unstage_restart(&r);
                    } else {
                        self.planned.stage_restart(r);
                    }
                    self.plan.outcome = None;
                } else {
                    self.flash = Some("planning applies to the hot cluster only".into());
                }
            }
            Action::SetNamespaceFilter(f) => {
                self.flash = Some(f.label());
                self.ns_filter = f;
                // rebuild() at the end of apply re-derives every view's models.
            }
            Action::SwitchContext(name) => self.switch_context(name).await,
            Action::EdgeReached(edge) => {
                if self.warm.is_some() {
                    match (edge, self.focus) {
                        (Edge::Right, ClusterId::Hot) => self.focus = ClusterId::Warm,
                        (Edge::Left, ClusterId::Warm) => self.focus = ClusterId::Hot,
                        _ => {}
                    }
                }
            }
        }
        // Detail views derive their models in update().
        self.rebuild();
    }

    async fn switch_context(&mut self, name: String) {
        tracing::info!(%name, "switching hot context");
        if let Some(w) = &self.warm
            && w.world.meta.context == name
        {
            self.flash = Some("that context is already the warm cluster".into());
            return;
        }
        match client::connect(self.kubeconfig.as_deref(), Some(&name)).await {
            Ok(cluster) => {
                // New informer set first; the old one aborts on drop.
                let proj = client::resolve_projections(&cluster.client, &self.projections).await;
                let sink = {
                    let tx = self.tx.clone();
                    move |id, delta| {
                        let _ = tx.try_send(AppEvent::World(id, delta));
                    }
                };
                self.hot = watch::spawn(&cluster, ClusterId::Hot, sink, &proj);
                self.hot_cluster = cluster; // logs fetch from the new client
                self.ready_hot = false;
                self.dirty = true;
                self.models_hot = Models::default();
                self.pair = None;
                self.evict_perm.clear(); // answers were for the old cluster
                self.pending_evict = None;
                self.ns_filter = NamespaceFilter::All; // namespaces differ
                self.kinds.clear(); // discovery was for the old cluster
                self.kinds_warnings.clear();
                self.kinds_loading = false; // re-discover on the next `:`
                self.discover_gen += 1; // drop a slow old-cluster discovery
                self.go_home(Screen::Map);
                self.focus = ClusterId::Hot;
                self.attention_panel.cycle = None;
                self.flash = Some(format!("hot context → {name}"));
            }
            Err(err) => {
                tracing::error!(%err, "context switch failed");
                self.flash = Some(format!("switch failed: {err}"));
            }
        }
    }

    /// Act on a command from the End-of-Turn review (unstage / discard /
    /// commit). Commit only raises the confirm — `spawn_commit` does the write.
    fn apply_plan_cmd(&mut self, cmd: PlanCmd) {
        match cmd {
            PlanCmd::Unstage(i) => {
                self.planned.unstage(i);
                self.plan.outcome = None;
            }
            PlanCmd::Discard => {
                self.planned.clear();
                self.plan.outcome = None;
            }
            PlanCmd::Commit => {
                let appliable = plan_diff(&self.hot.world, &self.planned)
                    .iter()
                    .filter(|c| !c.noop)
                    .count();
                if appliable > 0 {
                    self.pending_commit = true;
                } else {
                    self.flash = Some("nothing to commit".into());
                }
            }
        }
    }

    /// Commit the staged planning turn off the loop: the shared write file
    /// dry-runs every change (also enforcing RBAC) and only applies for real
    /// if all pass. The per-row outcome comes back as a `Committed` event.
    fn spawn_commit(&mut self) {
        let ivs: Vec<Intervention> = self.planned.interventions().to_vec();
        if ivs.is_empty() {
            return;
        }
        let client = self.hot_cluster.client.clone();
        self.flash = Some(format!("committing {} change(s) …", ivs.len()));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let outcome = actions::commit_interventions(client, &ivs).await;
            let _ = tx.send(AppEvent::Committed { outcome }).await;
        });
    }

    /// Run the confirmed pod eviction off the loop, reporting the outcome
    /// back as an `Evicted` event for the flash.
    fn spawn_evict(&mut self, cluster: ClusterId, namespace: String, pod: String) {
        let client = match cluster {
            ClusterId::Warm => self
                .warm_cluster
                .as_ref()
                .map(|c| c.client.clone())
                .unwrap_or_else(|| self.hot_cluster.client.clone()),
            ClusterId::Hot => self.hot_cluster.client.clone(),
        };
        self.flash = Some(format!("evicting {namespace}/{pod} …"));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = actions::evict_pod(client, &namespace, &pod)
                .await
                .map(|()| format!("evicted {namespace}/{pod}"));
            let _ = tx.send(AppEvent::Evicted { result }).await;
        });
    }

    /// Open the resource browser: ensure kind discovery is running, open the
    /// `:` picker (it fills in when discovery lands).
    fn open_browser(&mut self) {
        if self.kinds.is_empty() && !self.kinds_loading {
            self.spawn_discover();
        }
        self.resource_picker.open_with(
            self.kinds.clone(),
            self.kinds_warnings.clone(),
            self.kinds.is_empty(),
        );
    }

    fn spawn_discover(&mut self) {
        self.kinds_loading = true;
        let generation = self.discover_gen;
        let client = self.hot_cluster.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let discovered = browse::discover(&client).await;
            let _ = tx
                .send(AppEvent::Kinds {
                    generation,
                    discovered,
                })
                .await;
        });
    }

    fn spawn_list(&mut self, kind: browse::KindEntry) {
        self.browse_gen += 1;
        let generation = self.browse_gen;
        let client = self.hot_cluster.client.clone();
        let filter = self.ns_filter.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = browse::list_kind(&client, &kind, &filter).await;
            let _ = tx.send(AppEvent::BrowseRows { generation, result }).await;
        });
    }

    /// Kick off an async tail of the currently-open log target. Tagged with
    /// a generation so a stale result (after the user moves on) is dropped.
    fn fetch_logs(&mut self) {
        self.log_gen += 1;
        self.last_log_fetch = Instant::now();
        let generation = self.log_gen;
        let client = match self.logs.cluster {
            ClusterId::Warm => self
                .warm_cluster
                .as_ref()
                .map(|c| c.client.clone())
                .unwrap_or_else(|| self.hot_cluster.client.clone()),
            ClusterId::Hot => self.hot_cluster.client.clone(),
        };
        let ns = self.logs.namespace.clone();
        let pod = self.logs.pod.clone();
        let cached = self.logs.container.clone();
        let opts = logs::LogOpts {
            previous: self.logs.previous,
            timestamps: self.logs.timestamps,
            window: self.logs.window,
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            // Resolve the container once (cached on the view thereafter), so a
            // poll / p-T-s toggle doesn't re-issue an Api::get each time.
            let container = match cached {
                Some(c) => Some(c),
                None => logs::first_container(client.clone(), &ns, &pod).await,
            };
            let result = logs::tail(client, &ns, &pod, container.clone(), &opts).await;
            let _ = tx
                .send(AppEvent::Logs {
                    generation,
                    result,
                    container,
                })
                .await;
        });
    }

    fn draw(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        terminal.draw(|f| {
            let att_h = self.attention_panel.height(self.attention.len());
            let [status_a, main_a, att_a] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(att_h),
            ])
            .areas(f.area());

            {
                let ctx_hot = ctx!(self, ClusterId::Hot);
                if self.warm.is_some() {
                    let ctx_warm = ctx!(self, ClusterId::Warm);
                    status_bar::render(
                        f,
                        status_a,
                        &ctx_hot,
                        Some(&ctx_warm),
                        self.flash.as_deref(),
                        &self.ns_filter,
                    );
                } else {
                    status_bar::render(
                        f,
                        status_a,
                        &ctx_hot,
                        None,
                        self.flash.as_deref(),
                        &self.ns_filter,
                    );
                }
            }

            match self.screens.last().copied().unwrap_or(Screen::Map) {
                Screen::Map => {
                    // 4X-style sidebar (WORLD/STATUS/ORDERS) when there's
                    // room; otherwise the floating world overlay serves.
                    let paired = self.warm.is_some();
                    let min_w = if paired { 150 } else { 110 };
                    let (board_a, side_a) = if f.area().width >= min_w {
                        let [b, s] = Layout::horizontal([
                            Constraint::Min(40),
                            Constraint::Length(sidebar::SIDEBAR_W),
                        ])
                        .areas(main_a);
                        (b, Some(s))
                    } else {
                        (main_a, None)
                    };
                    self.map_hot.external_minimap =
                        side_a.is_some() && self.focus == ClusterId::Hot;
                    self.map_warm.external_minimap =
                        side_a.is_some() && self.focus == ClusterId::Warm;

                    if paired {
                        let [left_a, div_a, right_a] = Layout::horizontal([
                            Constraint::Percentage(50),
                            Constraint::Length(1),
                            Constraint::Fill(1),
                        ])
                        .areas(board_a);
                        let [lb, lmap] =
                            Layout::vertical([Constraint::Length(1), Constraint::Min(4)])
                                .areas(left_a);
                        let [rb, rmap] =
                            Layout::vertical([Constraint::Length(1), Constraint::Min(4)])
                                .areas(right_a);
                        {
                            let ctx = ctx!(self, ClusterId::Hot);
                            banner(f, lb, &ctx);
                            self.map_hot.render(f, lmap, &ctx);
                        }
                        {
                            let ctx = ctx!(self, ClusterId::Warm);
                            banner(f, rb, &ctx);
                            self.map_warm.render(f, rmap, &ctx);
                        }
                        divider(f, div_a, &self.theme);
                    } else {
                        let ctx = ctx!(self, ClusterId::Hot);
                        self.map_hot.render(f, board_a, &ctx);
                    }
                    if let Some(sa) = side_a {
                        let focus = self.view_cluster(Screen::Map);
                        let focused_map = match focus {
                            ClusterId::Hot => &self.map_hot,
                            ClusterId::Warm => &self.map_warm,
                        };
                        let ctx = ctx!(self, focus);
                        sidebar::render(f, sa, &ctx, focused_map);
                    }
                }
                Screen::Workloads => {
                    let ctx = ctx!(self, self.view_cluster(Screen::Workloads));
                    self.workloads.render(f, main_a, &ctx);
                }
                Screen::City => {
                    let ctx = ctx!(self, self.view_cluster(Screen::City));
                    self.city.render(f, main_a, &ctx);
                }
                Screen::Node => {
                    let ctx = ctx!(self, self.view_cluster(Screen::Node));
                    self.node.render(f, main_a, &ctx);
                }
                Screen::Logs => {
                    let ctx = ctx!(self, self.view_cluster(Screen::Logs));
                    self.logs.render(f, main_a, &ctx);
                }
                Screen::Inspect => {
                    let ctx = ctx!(self, self.view_cluster(Screen::Inspect));
                    self.inspect.render(f, main_a, &ctx);
                }
                Screen::Browse => {
                    let ctx = ctx!(self, ClusterId::Hot);
                    self.browse.render(f, main_a, &ctx);
                }
                Screen::Plan => {
                    // The planning turn is hot-cluster only.
                    let ctx = ctx!(self, ClusterId::Hot);
                    self.plan.render(f, main_a, &ctx);
                }
            }

            {
                let ctx = ctx!(self, ClusterId::Hot);
                self.attention_panel.render(f, att_a, &ctx);
            }
            if self.help_open {
                help::render(f, &self.theme);
            }
            if self.picker.open {
                self.picker.render(f, &self.theme);
            }
            if self.ns_picker.open {
                self.ns_picker.render(f, &self.theme);
            }
            if self.resource_picker.open {
                self.resource_picker.render(f, &self.theme);
            }
            if let Some((_, ns, pod)) = self.pending_evict.clone() {
                render_evict_confirm(f, &self.theme, &ns, &pod);
            }
            if self.pending_commit {
                let n = plan_diff(&self.hot.world, &self.planned)
                    .iter()
                    .filter(|c| !c.noop)
                    .count();
                render_commit_confirm(f, &self.theme, n);
            }
        })?;
        Ok(())
    }
}

/// The TUI evict confirm — a small centered red-framed prompt; the only write
/// gate in the terminal client (y writes, n cancels).
fn render_evict_confirm(f: &mut Frame, theme: &Theme, ns: &str, pod: &str) {
    let crit = theme.severity(Severity::Critical);
    let lines = vec![
        Line::raw(""),
        Line::from(format!("  {ns}/{pod}")),
        Line::raw(""),
        Line::raw("  Deletes the pod from the cluster now."),
        Line::raw("  A managed pod is recreated; a bare pod is gone."),
        Line::raw(""),
        Line::styled("  [y] evict     [n] cancel", crit),
    ];
    let area = centered(f.area(), 60, lines.len() as u16 + 2);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(crit)
        .title(" Evict pod? ")
        .title_style(crit);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// The TUI commit confirm — the planning turn's write gate. `n` changes apply
/// to the cluster on `y`, each server-side dry-run validated first.
fn render_commit_confirm(f: &mut Frame, theme: &Theme, n: usize) {
    let warn = theme.severity(Severity::Warning);
    let lines = vec![
        Line::raw(""),
        Line::from(format!("  Commit {n} staged change(s) to the cluster?")),
        Line::raw(""),
        Line::raw("  Each is server-side dry-run validated first (which also"),
        Line::raw("  enforces RBAC); only if all pass are they applied."),
        Line::raw(""),
        Line::styled("  [y] commit     [n] cancel", warn),
    ];
    let area = centered(f.area(), 62, lines.len() as u16 + 2);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(warn)
        .title(" Commit planning turn? ")
        .title_style(warn);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// A node's providerID is a stronger platform signal than kubeconfig
/// heuristics; refine once nodes are observed.
fn refine_platform(handle: &mut WorldHandle) {
    if handle.world.meta.platform != client::Platform::Unknown {
        return;
    }
    if let Some(p) = handle.world.nodes.state().iter().find_map(|n| {
        n.spec
            .as_ref()?
            .provider_id
            .as_deref()
            .and_then(client::Platform::from_provider_id)
    }) {
        handle.world.meta.platform = p;
    }
}

/// Continent banner above each half of the paired map; the focused side
/// carries the cursor.
fn banner(f: &mut ratatui::Frame, area: Rect, ctx: &RenderCtx) {
    if area.height == 0 {
        return;
    }
    let label = ctx.cluster_label.unwrap_or("");
    let marker = if ctx.focused { "▶" } else { " " };
    let text = format!(" {marker} {label} — {}", ctx.world.meta.context);
    let style = if ctx.focused {
        ctx.theme.selection()
    } else {
        ctx.theme.dim()
    };
    let buf = f.buffer_mut();
    let mut padded: String = kubernation_core::util::truncate(&text, area.width as usize);
    while (padded.chars().count() as u16) < area.width {
        padded.push(' ');
    }
    buf.set_stringn(area.x, area.y, padded, area.width as usize, style);
}

fn divider(f: &mut ratatui::Frame, area: Rect, theme: &Theme) {
    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        buf.set_string(area.x, y, "║", theme.dim());
    }
}

/// Write `text` to `filename` in the working directory; returns a flash message.
fn export_to_file(text: &str, filename: &str) -> String {
    let path = std::env::current_dir()
        .unwrap_or_else(|_| ".".into())
        .join(filename);
    match std::fs::write(&path, text) {
        Ok(()) => format!("exported → {}", path.display()),
        Err(e) => format!("export failed: {e}"),
    }
}

/// Copy `text` to the OS clipboard by piping to the platform tool (macOS
/// `pbcopy`, Linux `wl-copy`/`xclip`/`xsel`, Windows `clip`). Returns true on
/// success — reliable locally where OSC 52 is unsupported (e.g. Terminal.app).
fn os_clipboard_copy(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(target_os = "windows") {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    for (cmd, args) in candidates {
        let Ok(mut child) = Command::new(cmd)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
            // Drop stdin to send EOF before waiting.
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// Copy `text` to the terminal's clipboard via OSC 52 (works over SSH on
/// supporting terminals; not all terminals enable it). Written straight to the
/// tty — a self-contained escape, so interleaving with ratatui's output is safe.
fn copy_osc52(text: &str) {
    use std::io::Write;
    let seq = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// Minimal standard base64 (no padding-free variants) — avoids a dependency for
/// the one OSC 52 use.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render_evict_confirm;
    use crate::config::ColorMode;
    use crate::ui::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// The evict confirm overlay shows the pod and the y/n prompt.
    #[test]
    fn evict_confirm_renders() {
        let theme = Theme::new(ColorMode::Auto);
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| render_evict_confirm(f, &theme, "demo", "web-7d4b-2"))
            .unwrap();
        let buf = term.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("Evict pod?"), "title missing:\n{text}");
        assert!(text.contains("demo/web-7d4b-2"), "pod missing:\n{text}");
        assert!(text.contains("[y] evict"), "prompt missing:\n{text}");
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        use super::base64;
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
