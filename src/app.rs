use std::path::PathBuf;
use std::time::Duration;

use color_eyre::Result;
use ratatui::DefaultTerminal;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui_crossterm::crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::MissedTickBehavior;

use crate::config::Config;
use crate::events::{AppEvent, ClusterId, WorldDelta};
use crate::k8s::{client, watch, watch::WorldHandle};
use crate::state::attention::Concern;
use crate::state::model::Models;
use crate::state::pair::PairSync;
use crate::state::planned::PlannedWorld;
use crate::ui::attention_panel::AttentionPanel;
use crate::ui::city::CityView;
use crate::ui::context_picker::ContextPicker;
use crate::ui::map::MapView;
use crate::ui::node_detail::NodeDetailView;
use crate::ui::theme::Theme;
use crate::ui::workloads::WorkloadListView;
use crate::ui::{Action, Component, Edge, OverlayMode, RenderCtx, help, sidebar, status_bar};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Map,
    Workloads,
    City,
    Node,
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
    models_hot: Models,
    models_warm: Option<Models>,
    pair: Option<PairSync>,
    /// Merged, severity-ordered concerns across both worlds.
    attention: Vec<Concern>,
    /// Future planning-turn state; intentionally unused in the MVP.
    _planned: PlannedWorld,

    screens: Vec<Screen>,
    focus: ClusterId,
    map_hot: MapView,
    map_warm: MapView,
    workloads: WorkloadListView,
    city: CityView,
    city_cluster: ClusterId,
    node: NodeDetailView,
    node_cluster: ClusterId,
    attention_panel: AttentionPanel,
    picker: ContextPicker,
    help_open: bool,
    overlay: OverlayMode,

    ready_hot: bool,
    ready_warm: bool,
    dirty: bool,
    quit: bool,
    flash: Option<String>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: Config,
        kubeconfig: Option<PathBuf>,
        projections: Vec<String>,
        hot: WorldHandle,
        warm: Option<WorldHandle>,
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
            models_hot: Models::default(),
            models_warm,
            pair: None,
            attention: Vec::new(),
            _planned: PlannedWorld::default(),
            screens: vec![Screen::Map],
            focus: ClusterId::Hot,
            map_hot: MapView::default(),
            map_warm: MapView::default(),
            workloads: WorkloadListView::default(),
            city: CityView::default(),
            city_cluster: ClusterId::Hot,
            node: NodeDetailView::default(),
            node_cluster: ClusterId::Hot,
            attention_panel,
            picker: ContextPicker::default(),
            help_open: false,
            overlay: OverlayMode::Pressure,
            ready_hot: false,
            ready_warm: false,
            dirty: false,
            quit: false,
            flash: None,
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
        self.models_hot = Models::build(&self.hot.world);
        self.models_warm = self.warm.as_ref().map(|w| Models::build(&w.world));
        self.pair = self
            .warm
            .as_ref()
            .map(|w| PairSync::build(&self.hot.world, &w.world));

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

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('m') => self.go_home(Screen::Map),
            KeyCode::Char('w') => self.go_home(Screen::Workloads),
            KeyCode::Char('n') => {
                if let Some((source, a)) = self.attention_panel.next_action(&self.attention) {
                    self.apply(a, source).await;
                }
            }
            KeyCode::Char('a') => self.attention_panel.expanded = !self.attention_panel.expanded,
            KeyCode::Tab if self.attention_panel.expanded => self.attention_panel.focused = true,
            KeyCode::Char('c') => self.picker.open_with(
                &self.hot.world.meta.all_contexts,
                &self.hot.world.meta.context,
            ),
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
                self.hot = watch::spawn(&cluster, ClusterId::Hot, self.tx.clone(), &proj);
                self.ready_hot = false;
                self.dirty = true;
                self.models_hot = Models::default();
                self.pair = None;
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
                    );
                } else {
                    status_bar::render(f, status_a, &ctx_hot, None, self.flash.as_deref());
                }
            }

            match self.screens.last().copied().unwrap_or(Screen::Map) {
                Screen::Map => {
                    // Civ-style sidebar (WORLD/STATUS/ORDERS) when there's
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
        })?;
        Ok(())
    }
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
    let mut padded: String = crate::util::truncate(&text, area.width as usize);
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
