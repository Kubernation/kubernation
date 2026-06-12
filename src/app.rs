use std::path::PathBuf;
use std::time::Duration;

use color_eyre::Result;
use ratatui::DefaultTerminal;
use ratatui::layout::{Constraint, Layout};
use ratatui_crossterm::crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::MissedTickBehavior;

use crate::config::Config;
use crate::events::{AppEvent, WorldDelta};
use crate::k8s::{client, watch, watch::WorldHandle};
use crate::state::model::Models;
use crate::state::planned::PlannedWorld;
use crate::ui::attention_panel::AttentionPanel;
use crate::ui::city::CityView;
use crate::ui::context_picker::ContextPicker;
use crate::ui::map::MapView;
use crate::ui::node_detail::NodeDetailView;
use crate::ui::theme::Theme;
use crate::ui::workloads::WorkloadListView;
use crate::ui::{Action, Component, OverlayMode, RenderCtx, help, status_bar};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Map,
    Workloads,
    City,
    Node,
}

pub struct App {
    cfg: Config,
    theme: Theme,
    kubeconfig: Option<PathBuf>,

    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
    handle: WorldHandle,
    models: Models,
    /// Future planning-turn state; intentionally unused in the MVP.
    _planned: PlannedWorld,

    screens: Vec<Screen>,
    map: MapView,
    workloads: WorkloadListView,
    city: CityView,
    node: NodeDetailView,
    attention: AttentionPanel,
    picker: ContextPicker,
    help_open: bool,
    overlay: OverlayMode,

    ready: bool,
    dirty: bool,
    quit: bool,
    flash: Option<String>,
}

impl App {
    pub fn new(
        cfg: Config,
        kubeconfig: Option<PathBuf>,
        handle: WorldHandle,
        tx: Sender<AppEvent>,
        rx: Receiver<AppEvent>,
    ) -> Self {
        let theme = Theme::new(cfg.color);
        let attention = AttentionPanel::new(cfg.attention_expanded);
        Self {
            cfg,
            theme,
            kubeconfig,
            tx,
            rx,
            handle,
            models: Models::default(),
            _planned: PlannedWorld::default(),
            screens: vec![Screen::Map],
            map: MapView::default(),
            workloads: WorkloadListView::default(),
            city: CityView::default(),
            node: NodeDetailView::default(),
            attention,
            picker: ContextPicker::default(),
            help_open: false,
            overlay: OverlayMode::Pressure,
            ready: false,
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
            AppEvent::World(WorldDelta::Ready) => {
                self.ready = true;
                self.rebuild();
                true
            }
            AppEvent::World(_) => {
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

    /// Re-derive all view models from the observed world.
    fn rebuild(&mut self) {
        // A node's providerID is a stronger platform signal than kubeconfig
        // heuristics; refine once nodes are observed.
        if self.handle.world.meta.platform == client::Platform::Unknown
            && let Some(p) = self.handle.world.nodes.state().iter().find_map(|n| {
                n.spec
                    .as_ref()?
                    .provider_id
                    .as_deref()
                    .and_then(client::Platform::from_provider_id)
            })
        {
            self.handle.world.meta.platform = p;
        }
        self.models = Models::build(&self.handle.world);
        self.dirty = false;
        let ctx = RenderCtx {
            models: &self.models,
            world: &self.handle.world,
            theme: &self.theme,
            overlay: self.overlay,
            ready: self.ready,
        };
        self.map.update(&ctx);
        self.workloads.update(&ctx);
        self.city.update(&ctx);
        self.node.update(&ctx);
        self.attention.update(&ctx);
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
                self.apply(a).await;
            }
            return;
        }
        if self.attention.focused {
            match key.code {
                KeyCode::Tab | KeyCode::Esc => self.attention.focused = false,
                _ => {
                    let action = {
                        let ctx = RenderCtx {
                            models: &self.models,
                            world: &self.handle.world,
                            theme: &self.theme,
                            overlay: self.overlay,
                            ready: self.ready,
                        };
                        self.attention.handle_key(key, &ctx)
                    };
                    if let Some(a) = action {
                        self.attention.focused = false;
                        self.apply(a).await;
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
                if let Some(a) = self.attention.next_action(&self.models) {
                    self.apply(a).await;
                }
            }
            KeyCode::Char('a') => self.attention.expanded = !self.attention.expanded,
            KeyCode::Tab if self.attention.expanded => self.attention.focused = true,
            KeyCode::Char('c') => self.picker.open_with(
                &self.handle.world.meta.all_contexts,
                &self.handle.world.meta.context,
            ),
            KeyCode::Char('1') => self.overlay = OverlayMode::Pressure,
            KeyCode::Char('2') => self.overlay = OverlayMode::ReplicaHealth,
            KeyCode::Char('3') => self.overlay = OverlayMode::Namespace,
            KeyCode::Esc | KeyCode::Backspace => self.pop_screen(),
            _ => {
                let action = {
                    let ctx = RenderCtx {
                        models: &self.models,
                        world: &self.handle.world,
                        theme: &self.theme,
                        overlay: self.overlay,
                        ready: self.ready,
                    };
                    match self.screens.last().copied().unwrap_or(Screen::Map) {
                        Screen::Map => self.map.handle_key(key, &ctx),
                        Screen::Workloads => self.workloads.handle_key(key, &ctx),
                        Screen::City => self.city.handle_key(key, &ctx),
                        Screen::Node => self.node.handle_key(key, &ctx),
                    }
                };
                if let Some(a) = action {
                    self.apply(a).await;
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

    async fn apply(&mut self, action: Action) {
        match action {
            Action::OpenWorkloadList => self.go_home(Screen::Workloads),
            Action::OpenNode(name) => {
                self.node.open(name);
                self.push_screen(Screen::Node);
            }
            Action::OpenWorkload(r) => {
                self.city.open(r);
                self.push_screen(Screen::City);
            }
            Action::SwitchContext(name) => self.switch_context(name).await,
        }
        // Detail views derive their models in update().
        self.rebuild();
    }

    async fn switch_context(&mut self, name: String) {
        tracing::info!(%name, "switching context");
        match client::connect(self.kubeconfig.as_deref(), Some(&name)).await {
            Ok(cluster) => {
                // New informer set first; the old one aborts on drop.
                self.handle = watch::spawn(&cluster, self.tx.clone());
                self.ready = false;
                self.dirty = true;
                self.models = Models::default();
                self.go_home(Screen::Map);
                self.attention.cycle = None;
                self.flash = Some(format!("context → {name}"));
            }
            Err(err) => {
                tracing::error!(%err, "context switch failed");
                self.flash = Some(format!("switch failed: {err}"));
            }
        }
    }

    fn draw(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        terminal.draw(|f| {
            let ctx = RenderCtx {
                models: &self.models,
                world: &self.handle.world,
                theme: &self.theme,
                overlay: self.overlay,
                ready: self.ready,
            };
            let att_h = self.attention.height(self.models.attention.len());
            let [status_a, main_a, att_a] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(att_h),
            ])
            .areas(f.area());

            status_bar::render(f, status_a, &ctx, self.flash.as_deref());
            match self.screens.last().copied().unwrap_or(Screen::Map) {
                Screen::Map => self.map.render(f, main_a, &ctx),
                Screen::Workloads => self.workloads.render(f, main_a, &ctx),
                Screen::City => self.city.render(f, main_a, &ctx),
                Screen::Node => self.node.render(f, main_a, &ctx),
            }
            self.attention.render(f, att_a, &ctx);

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
