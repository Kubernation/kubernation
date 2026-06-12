//! K8sCiv GUI spike: the same observed world and pure `WorldModel`
//! geometry as the TUI, rendered as a windowed 2D game map with macroquad.
//! Flat-color tiles, shapes, and label plates — no art assets yet; the
//! point of the spike is the *experience*: smooth pan, wheel zoom, mouse
//! hover and click, and the world reading like a strategy game.
//!
//!   cargo run -p k8sciv-gui --release -- --context kind-k8sciv \
//!       --project gizmos.example.com
//!
//! `--screenshot out.png` renders until the world syncs, saves a frame,
//! and exits (used for self-verification in development).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use k8sciv_core::events::{ClusterId, WorldDelta};
use k8sciv_core::k8s::{client, watch};
use k8sciv_core::state::attention::{Severity, Target};
use k8sciv_core::state::model::{Models, NodeHealth};
use k8sciv_core::state::world::{Region, WorldModel};
use macroquad::prelude::*;

#[derive(Debug, Parser)]
#[command(name = "k8sciv-gui", version, about)]
struct Args {
    /// Kubeconfig context (defaults to current-context)
    #[arg(long)]
    context: Option<String>,
    /// Path to kubeconfig
    #[arg(long)]
    kubeconfig: Option<PathBuf>,
    /// Project a CRD's instances onto the map (repeatable)
    #[arg(long = "project", value_name = "CRD")]
    project: Vec<String>,
    /// Render until synced, save a PNG, exit (development verification)
    #[arg(long)]
    screenshot: Option<PathBuf>,
}

/// Frames are drawn from the latest snapshot the net thread published.
struct Net {
    models: Mutex<Option<Arc<Models>>>,
    status: Mutex<String>,
}

fn spawn_net(args: &Args, net: Arc<Net>) {
    let context = args.context.clone();
    let kubeconfig = args.kubeconfig.clone();
    let projections = args.project.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            *net.status.lock().unwrap() = "connecting…".into();
            let cluster = match client::connect(kubeconfig.as_deref(), context.as_deref()).await {
                Ok(c) => c,
                Err(err) => {
                    *net.status.lock().unwrap() = format!("connect failed: {err}");
                    return;
                }
            };
            let label = format!(
                "{} · {}",
                cluster.meta.context,
                cluster.meta.platform.label()
            );
            *net.status.lock().unwrap() = format!("{label} · exploring…");
            let proj = client::resolve_projections(&cluster.client, &projections).await;

            let dirty = Arc::new(AtomicBool::new(false));
            let ready = Arc::new(AtomicBool::new(false));
            let sink = {
                let dirty = dirty.clone();
                let ready = ready.clone();
                move |_id: ClusterId, delta: WorldDelta| {
                    if delta == WorldDelta::Ready {
                        ready.store(true, Ordering::Relaxed);
                    }
                    dirty.store(true, Ordering::Relaxed);
                }
            };
            let handle = watch::spawn(&cluster, ClusterId::Hot, sink, &proj);

            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tick.tick().await;
                if ready.load(Ordering::Relaxed) && dirty.swap(false, Ordering::Relaxed) {
                    let models = Models::build(&handle.world);
                    *net.status.lock().unwrap() = label.clone();
                    *net.models.lock().unwrap() = Some(Arc::new(models));
                }
            }
        });
    });
}

// World cells assume terminal-ish aspect; keep that proportion in pixels.
const CELL_W: f32 = 13.0;
const CELL_H: f32 = 19.0;

// The civ palette, in RGB.
const OCEAN: Color = Color::new(0.07, 0.19, 0.33, 1.0);
const WAVE: Color = Color::new(0.12, 0.27, 0.43, 1.0);
const PROV_BORDER: Color = Color::new(0.16, 0.27, 0.13, 1.0);
const SAND: Color = Color::new(0.77, 0.70, 0.47, 1.0);
const PARCHMENT: Color = Color::new(0.83, 0.70, 0.44, 1.0);
const PLATE: Color = Color::new(0.08, 0.09, 0.07, 0.82);
const PANEL: Color = Color::new(0.11, 0.10, 0.08, 0.92);
const INK: Color = Color::new(0.95, 0.94, 0.90, 1.0);
const DIM: Color = Color::new(0.62, 0.60, 0.55, 1.0);
const CRIT: Color = Color::new(0.83, 0.18, 0.13, 1.0);
const WARN: Color = Color::new(0.88, 0.72, 0.18, 1.0);
const ROAD: Color = Color::new(0.45, 0.33, 0.20, 1.0);
const STRUCT: Color = Color::new(0.45, 0.85, 0.90, 1.0);

/// macroquad's built-in font is ASCII-ish; swap the TUI glyph vocabulary
/// for plain characters so nothing renders as tofu.
fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '—' | '–' => '-',
            '·' => '.',
            '×' => 'x',
            '▸' => '>',
            '‼' => '!',
            '⊘' => 'o',
            '…' => '~',
            c if c.is_ascii() => c,
            _ => '?',
        })
        .collect()
}

fn terrain(h: NodeHealth) -> Color {
    match h {
        NodeHealth::Healthy => Color::new(0.29, 0.49, 0.23, 1.0),
        NodeHealth::Cordoned => Color::new(0.55, 0.50, 0.24, 1.0),
        NodeHealth::Pressure => Color::new(0.62, 0.42, 0.18, 1.0),
        NodeHealth::NotReady => Color::new(0.42, 0.15, 0.12, 1.0),
    }
}

fn severity_color(s: Severity) -> Color {
    match s {
        Severity::Critical => CRIT,
        Severity::Warning => WARN,
        Severity::Info => DIM,
    }
}

struct Camera2 {
    pos: Vec2, // world px at top-left of screen
    zoom: f32,
}

impl Camera2 {
    fn cell_px(&self) -> (f32, f32) {
        (CELL_W * self.zoom, CELL_H * self.zoom)
    }
    fn to_screen(&self, wx: f32, wy: f32) -> Vec2 {
        let (cw, ch) = self.cell_px();
        vec2(wx * cw - self.pos.x, wy * ch - self.pos.y)
    }
    fn cell_at(&self, screen: Vec2, world: &WorldModel) -> Option<(u16, u16)> {
        let (cw, ch) = self.cell_px();
        let wx = (screen.x + self.pos.x) / cw;
        let wy = (screen.y + self.pos.y) / ch;
        (wx >= 0.0 && wy >= 0.0 && wx < world.width as f32 && wy < world.height as f32)
            .then_some((wx as u16, wy as u16))
    }
    fn center_on(&mut self, cell: (u16, u16)) {
        let (cw, ch) = self.cell_px();
        self.pos = vec2(
            cell.0 as f32 * cw - screen_width() / 2.0,
            cell.1 as f32 * ch - screen_height() / 2.0,
        );
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "K8sCiv".into(),
        window_width: 1380,
        window_height: 860,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let args = Args::parse();
    let shot = args.screenshot.clone();
    let net = Arc::new(Net {
        models: Mutex::new(None),
        status: Mutex::new("starting…".into()),
    });
    spawn_net(&args, net.clone());

    let mut cam = Camera2 {
        pos: vec2(-40.0, -30.0),
        zoom: 1.0,
    };
    let mut selected: Option<(u16, u16)> = None;
    let mut concern_idx: usize = 0;
    let mut city_idx: usize = 0;
    let mut frames_synced: u32 = 0;

    loop {
        let models = net.models.lock().unwrap().clone();
        let status = net.status.lock().unwrap().clone();

        // ---- input -----------------------------------------------------
        if is_key_pressed(KeyCode::Q) || is_key_pressed(KeyCode::Escape) {
            break;
        }
        let pan = 14.0;
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            cam.pos.x -= pan;
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            cam.pos.x += pan;
        }
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            cam.pos.y -= pan;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            cam.pos.y += pan;
        }
        let (_, wheel) = mouse_wheel();
        if wheel.abs() > 0.0 {
            let factor = if wheel > 0.0 { 1.1 } else { 1.0 / 1.1 };
            // Zoom around the mouse position.
            let m = Vec2::from(mouse_position());
            let before = (m + cam.pos) / cam.zoom;
            cam.zoom = (cam.zoom * factor).clamp(0.45, 3.0);
            cam.pos = before * cam.zoom - m;
        }

        if let Some(m) = models.as_ref() {
            let world = &m.world;
            if is_key_pressed(KeyCode::RightBracket) || is_key_pressed(KeyCode::LeftBracket) {
                let cities: Vec<_> = world.cities().collect();
                if !cities.is_empty() {
                    if is_key_pressed(KeyCode::RightBracket) {
                        city_idx = (city_idx + 1) % cities.len();
                    } else {
                        city_idx = (city_idx + cities.len() - 1) % cities.len();
                    }
                    let c = cities[city_idx];
                    selected = Some((c.x, c.y));
                    cam.center_on((c.x, c.y));
                }
            }
            if is_key_pressed(KeyCode::N) && !m.attention.is_empty() {
                concern_idx = (concern_idx + 1) % m.attention.len();
                let target = &m.attention[concern_idx].target;
                let pos = match target {
                    Target::Workload(r) => world.city_pos(r).or_else(|| world.structure_pos(r)),
                    Target::Node(name) => world.province_pos(name),
                    Target::WorkloadList => None,
                };
                if let Some(p) = pos {
                    selected = Some(p);
                    cam.center_on(p);
                }
            }
            if is_mouse_button_pressed(MouseButton::Left) {
                selected = cam.cell_at(Vec2::from(mouse_position()), world);
            }
        }

        // ---- draw ------------------------------------------------------
        clear_background(OCEAN);
        let (cw, ch) = cam.cell_px();

        // Sparse waves, stable per world cell so panning feels physical.
        let x0 = (cam.pos.x / cw).floor().max(0.0) as i32;
        let y0 = (cam.pos.y / ch).floor().max(0.0) as i32;
        let cols = (screen_width() / cw) as i32 + 2;
        let rows = (screen_height() / ch) as i32 + 2;
        for wy in y0..y0 + rows {
            for wx in x0..x0 + cols {
                if (wx * 7 + wy * 13).rem_euclid(31) == 0 {
                    let p = cam.to_screen(wx as f32, wy as f32);
                    draw_rectangle(p.x, p.y + ch * 0.45, cw * 0.5, 2.0, WAVE);
                }
            }
        }

        match models.as_ref() {
            None => {
                draw_text(ascii(&status), 40.0, 60.0, 30.0, PARCHMENT);
                draw_text(
                    "the world is unexplored - fog of war",
                    40.0,
                    100.0,
                    24.0,
                    DIM,
                );
            }
            Some(m) => {
                frames_synced += 1;
                let world = &m.world;

                // --- continents ------------------------------------------
                for cont in &world.continents {
                    let p = cam.to_screen(cont.x as f32 + 1.0, cont.y as f32 - 1.0);
                    draw_text(
                        format!("{}  ({} provinces)", cont.zone, cont.provinces.len()),
                        p.x,
                        p.y + ch * 0.7,
                        20.0 * cam.zoom.max(0.8),
                        PARCHMENT,
                    );
                    for prov in &cont.provinces {
                        let tl = cam.to_screen(prov.x as f32, prov.y as f32);
                        let w = prov.w as f32 * cw;
                        let h = prov.h as f32 * ch;
                        draw_rectangle(tl.x, tl.y, w, h, terrain(prov.tile.health));
                        draw_rectangle_lines(tl.x, tl.y, w, h, 2.0, PROV_BORDER);
                        // Province name + census.
                        draw_text(
                            &prov.tile.name,
                            tl.x + 6.0,
                            tl.y + 15.0 * cam.zoom.max(0.7),
                            16.0 * cam.zoom.max(0.7),
                            INK,
                        );
                        draw_text(
                            format!("{} pods", prov.tile.pods.len()),
                            tl.x + 6.0,
                            tl.y + 30.0 * cam.zoom.max(0.7),
                            13.0 * cam.zoom.max(0.7),
                            DIM,
                        );
                        // Daemonset roads along the southern edge.
                        for i in 0..prov.infra.min(8) {
                            draw_rectangle(
                                tl.x + 8.0 + i as f32 * 14.0,
                                tl.y + h - 6.0,
                                10.0,
                                3.0,
                                ROAD,
                            );
                        }
                        // Cities.
                        for city in &prov.cities {
                            let c = cam.to_screen(city.x as f32 + 0.5, city.y as f32 + 0.5);
                            let r = (7.0 + (city.ready as f32).min(30.0) * 0.45) * cam.zoom;
                            let fill = match city.severity {
                                Some(s) => severity_color(s),
                                None => INK,
                            };
                            draw_circle(c.x, c.y, r + 2.0, PLATE);
                            draw_circle(c.x, c.y, r, fill);
                            let pop = format!("{}", city.ready);
                            let m1 = measure_text(&pop, None, (15.0 * cam.zoom) as u16, 1.0);
                            draw_text(
                                &pop,
                                c.x - m1.width / 2.0,
                                c.y + m1.height / 2.0,
                                15.0 * cam.zoom,
                                if city.severity.is_some() { INK } else { PLATE },
                            );
                            // Civ-style name plate under the city.
                            let label = &city.r.name;
                            let fs = (15.0 * cam.zoom).max(11.0);
                            let tm = measure_text(label, None, fs as u16, 1.0);
                            let lx = c.x - tm.width / 2.0;
                            let ly = c.y + r + fs * 0.9;
                            draw_rectangle(
                                lx - 4.0,
                                ly - tm.height,
                                tm.width + 8.0,
                                tm.height + 5.0,
                                PLATE,
                            );
                            draw_text(label, lx, ly, fs, INK);
                        }
                    }
                }

                // --- islands ---------------------------------------------
                for isl in &world.islands {
                    let tl = cam.to_screen(isl.x as f32, isl.y as f32);
                    let w = isl.w as f32 * cw;
                    let h = isl.h as f32 * ch;
                    draw_rectangle(tl.x, tl.y, w, h, SAND);
                    draw_rectangle_lines(tl.x, tl.y, w, h, 2.0, Color::new(0.55, 0.48, 0.30, 1.0));
                    draw_text(
                        format!("isle of {}", isl.label),
                        tl.x + 6.0,
                        tl.y + 16.0,
                        15.0 * cam.zoom.max(0.8),
                        PLATE,
                    );
                    for s in &isl.structures {
                        let p = cam.to_screen(isl.x as f32 + 1.5, s.y as f32 + 0.5);
                        let color = if s.glyph == '✦' { STRUCT } else { DIM };
                        draw_poly(p.x, p.y, 4, 6.0 * cam.zoom, 45.0, color);
                        draw_text(
                            format!("{}/{}", s.kind, s.name),
                            p.x + 12.0,
                            p.y + 5.0,
                            13.0 * cam.zoom.max(0.8),
                            PLATE,
                        );
                    }
                }

                // --- selection + ORDERS panel ------------------------------
                if let Some(sel) = selected {
                    let p = cam.to_screen(sel.0 as f32, sel.1 as f32);
                    draw_rectangle_lines(p.x - 2.0, p.y - 2.0, cw + 4.0, ch + 4.0, 2.5, INK);
                    draw_orders(world, m, sel);
                }

                // --- minimap ------------------------------------------------
                draw_minimap(world, &cam);

                // --- attention strip ----------------------------------------
                let base = screen_height() - 64.0;
                draw_rectangle(0.0, base, screen_width(), 64.0, PANEL);
                draw_rectangle(0.0, base, screen_width(), 2.0, PARCHMENT);
                if m.attention.is_empty() {
                    draw_text("all quiet — no concerns", 16.0, base + 26.0, 18.0, DIM);
                } else {
                    for (i, c) in m.attention.iter().take(3).enumerate() {
                        let marker = if i == concern_idx % m.attention.len() {
                            "> "
                        } else {
                            "  "
                        };
                        draw_text(
                            ascii(&format!("{marker}{} - {}", c.title, c.detail)),
                            16.0,
                            base + 20.0 + i as f32 * 19.0,
                            16.0,
                            severity_color(c.severity),
                        );
                    }
                }
            }
        }

        // --- chrome -----------------------------------------------------
        draw_rectangle(0.0, 0.0, screen_width(), 30.0, PANEL);
        draw_rectangle(0.0, 30.0, screen_width(), 2.0, PARCHMENT);
        draw_text(
            ascii(&format!("K8SCIV - {status}")),
            12.0,
            21.0,
            20.0,
            PARCHMENT,
        );
        let help =
            "WASD/arrows pan · wheel zoom · click inspect · ]/[ cities · N next concern · Q quit";
        let hm = measure_text(help, None, 15, 1.0);
        draw_text(help, screen_width() - hm.width - 12.0, 21.0, 15.0, DIM);

        // --- screenshot mode ---------------------------------------------
        if let Some(path) = &shot
            && frames_synced > 30
        {
            get_screen_data().export_png(&path.to_string_lossy());
            break;
        }

        next_frame().await;
    }
}

fn draw_orders(world: &WorldModel, m: &Models, sel: (u16, u16)) {
    let mut lines: Vec<(String, Color)> = Vec::new();
    match world.region_at(sel.0, sel.1) {
        Region::City(p, c) => {
            lines.push((c.r.name.clone(), INK));
            lines.push((format!("{} {}/{}", c.r.kind, c.r.namespace, c.r.name), DIM));
            let gap = if c.ready < c.desired { WARN } else { DIM };
            lines.push((format!("pop {} of {} desired", c.ready, c.desired), gap));
            if let Some(row) = m.workloads.iter().find(|w| w.r == c.r) {
                lines.push((format!("rollout {}", row.status), DIM));
            }
            if let Some(sev) = c.severity {
                lines.push(("needs attention".into(), severity_color(sev)));
            }
            lines.push((format!("on {}", p.tile.name), DIM));
        }
        Region::Province(p) => {
            lines.push((p.tile.name.clone(), INK));
            lines.push((format!("province of {}", p.tile.zone), PARCHMENT));
            lines.push((
                format!(
                    "cpu {:>3.0}%  mem {:>3.0}%  ·  {} pods",
                    p.tile.cpu_ratio * 100.0,
                    p.tile.mem_ratio * 100.0,
                    p.tile.pods.len()
                ),
                DIM,
            ));
            if !p.tile.ready {
                lines.push(("NotReady".into(), CRIT));
            }
            if p.tile.cordoned {
                lines.push(("cordoned".into(), WARN));
            }
            if p.infra > 0 {
                lines.push((format!("{} daemonset roads", p.infra), DIM));
            }
        }
        Region::Structure(isl, s) => {
            lines.push((format!("{}/{}", s.kind, s.name), INK));
            lines.push((format!("isle of {}", isl.label), PARCHMENT));
            if s.workload.is_some() {
                lines.push(("no pods on any land".into(), WARN));
            }
        }
        Region::Island(isl) => {
            lines.push((format!("isle of {}", isl.label), INK));
            lines.push((
                format!("{} structures", isl.structures.len() + isl.more),
                DIM,
            ));
        }
        Region::Ocean => {
            lines.push(("open sea".into(), INK));
            lines.push((format!("sector {},{}", sel.0, sel.1), DIM));
        }
    }
    let h = 26.0 + lines.len() as f32 * 19.0;
    let y0 = screen_height() - 64.0 - h - 10.0;
    draw_rectangle(10.0, y0, 330.0, h, PANEL);
    draw_rectangle_lines(10.0, y0, 330.0, h, 2.0, PARCHMENT);
    draw_text("ORDERS", 20.0, y0 + 18.0, 15.0, PARCHMENT);
    for (i, (text, color)) in lines.iter().enumerate() {
        draw_text(ascii(text), 20.0, y0 + 38.0 + i as f32 * 19.0, 16.0, *color);
    }
}

fn draw_minimap(world: &WorldModel, cam: &Camera2) {
    let scale = 150.0 / world.width.max(1) as f32;
    let mw = world.width as f32 * scale;
    let mh = (world.height as f32 * scale * (CELL_H / CELL_W)).min(170.0);
    let x0 = screen_width() - mw - 14.0;
    let y0 = 42.0;
    draw_rectangle(x0 - 4.0, y0 - 4.0, mw + 8.0, mh + 8.0, PANEL);
    draw_rectangle_lines(x0 - 4.0, y0 - 4.0, mw + 8.0, mh + 8.0, 2.0, PARCHMENT);
    draw_rectangle(x0, y0, mw, mh, OCEAN);
    let sy = mh / world.height.max(1) as f32;
    for cont in &world.continents {
        for p in &cont.provinces {
            draw_rectangle(
                x0 + p.x as f32 * scale,
                y0 + p.y as f32 * sy,
                p.w as f32 * scale,
                p.h as f32 * sy,
                terrain(p.tile.health),
            );
        }
    }
    for isl in &world.islands {
        draw_rectangle(
            x0 + isl.x as f32 * scale,
            y0 + isl.y as f32 * sy,
            isl.w as f32 * scale,
            isl.h as f32 * sy,
            SAND,
        );
    }
    // Viewport rectangle.
    let (cw, ch) = cam.cell_px();
    let vx = (cam.pos.x / cw).max(0.0) * scale;
    let vy = (cam.pos.y / ch).max(0.0) * sy;
    let vw = (screen_width() / cw) * scale;
    let vh = (screen_height() / ch) * sy;
    draw_rectangle_lines(x0 + vx, y0 + vy, vw.min(mw), vh.min(mh), 2.0, INK);
}
