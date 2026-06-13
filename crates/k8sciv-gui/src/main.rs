//! K8sCiv GUI: the observed world rendered as a windowed strategy map —
//! the same `k8sciv-core` models as the TUI, painted with macroquad.
//!
//!   make gui
//!   cargo run -p k8sciv-gui --release -- --context kind-k8sciv \
//!       --project gizmos.example.com
//!
//! Controls: WASD/arrows or right-drag pan · wheel zoom · hover for
//! tooltips · click to inspect (city screen / node panel) · ]/[ sail
//! between cities · N fly to the next concern · Esc close · Q quit.

mod draw;
mod net;
mod panels;
mod theme;

use std::path::PathBuf;

use clap::Parser;
use draw::{Camera, draw_minimap, draw_world, minimap_layout};
use k8sciv_core::state::attention::Target;
use k8sciv_core::state::world::Region;
use macroquad::prelude::*;
use panels::{Panel, draw_attention_strip, draw_panel, draw_tooltip, panel_layout};
use theme::*;

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
    /// On sync, select the first city whose name contains this and open
    /// its panel (development verification)
    #[arg(long)]
    inspect: Option<String>,
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
    let inspect = args.inspect.clone();
    let net = net::Net::new();
    net::spawn(
        net::NetArgs {
            context: args.context.clone(),
            kubeconfig: args.kubeconfig.clone(),
            projections: args.project.clone(),
        },
        net.clone(),
    );

    let mut cam = Camera::new();
    let mut selected: Option<(u16, u16)> = None;
    let mut panel: Option<Panel> = None;
    let mut concern_idx: usize = 0;
    let mut city_idx: usize = 0;
    let mut frames_synced: u32 = 0;
    let mut inspected = false;
    let mut drag_anchor: Option<Vec2> = None;

    loop {
        let snap = net.snapshot();
        let status = net.status();
        let mouse = Vec2::from(mouse_position());

        // ---- input ------------------------------------------------------
        if is_key_pressed(KeyCode::Q) {
            break;
        }
        if is_key_pressed(KeyCode::Escape) {
            if panel.is_some() {
                panel = None;
            } else {
                break;
            }
        }
        let mut manual_pan = false;
        let pan = 14.0;
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            cam.pos.x -= pan;
            manual_pan = true;
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            cam.pos.x += pan;
            manual_pan = true;
        }
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            cam.pos.y -= pan;
            manual_pan = true;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            cam.pos.y += pan;
            manual_pan = true;
        }
        // Right- or middle-drag pans like grabbing the map.
        if is_mouse_button_down(MouseButton::Right) || is_mouse_button_down(MouseButton::Middle) {
            if let Some(anchor) = drag_anchor {
                let d = anchor - mouse;
                if d.length() > 0.0 {
                    cam.pos += d;
                    manual_pan = true;
                }
            }
            drag_anchor = Some(mouse);
        } else {
            drag_anchor = None;
        }
        let (_, wheel) = mouse_wheel();
        if wheel.abs() > 0.0 {
            let factor = if wheel > 0.0 { 1.1 } else { 1.0 / 1.1 };
            let before = (mouse + cam.pos) / cam.zoom;
            cam.zoom = (cam.zoom * factor).clamp(0.45, 3.0);
            cam.pos = before * cam.zoom - mouse;
        }
        cam.tick(manual_pan);

        if let Some(s) = snap.as_ref() {
            let world = &s.models.world;

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
                    cam.fly_to((c.x, c.y));
                }
            }
            if is_key_pressed(KeyCode::N) && !s.models.attention.is_empty() {
                concern_idx = (concern_idx + 1) % s.models.attention.len();
                let concern = &s.models.attention[concern_idx];
                let pos = match &concern.target {
                    Target::Workload(r) => world.city_pos(r).or_else(|| world.structure_pos(r)),
                    Target::Node(name) => world.province_pos(name),
                    Target::WorkloadList => None,
                };
                if let Some(p) = pos {
                    selected = Some(p);
                    cam.fly_to(p);
                    panel = match &concern.target {
                        Target::Workload(r) => Some(Panel::City(r.clone())),
                        Target::Node(name) => Some(Panel::Node(name.clone())),
                        Target::WorkloadList => None,
                    };
                }
            }
            if is_key_pressed(KeyCode::Enter)
                && let Some(sel) = selected
            {
                panel = panel_for(world, sel);
            }

            if is_mouse_button_pressed(MouseButton::Left) {
                let pl = panel_layout();
                let ml = minimap_layout(world);
                let over_panel = panel.is_some() && pl.frame.contains(mouse);
                if panel.is_some() && pl.close.contains(mouse) {
                    panel = None;
                } else if panel.is_none()
                    && let Some(cell) = ml.world_cell(mouse, world)
                {
                    cam.fly_to(cell);
                } else if !over_panel && mouse.y > panels::CHROME_H {
                    selected = cam.cell_at(mouse, world);
                    if let Some(sel) = selected {
                        panel = panel_for(world, sel);
                    }
                }
            }

            // Development verification: select and open something specific.
            if !inspected && let Some(needle) = &inspect {
                if let Some(c) = world.cities().find(|c| c.r.name.contains(needle.as_str())) {
                    selected = Some((c.x, c.y));
                    cam.jump_to((c.x, c.y));
                    panel = Some(Panel::City(c.r.clone()));
                }
                inspected = true;
            }
        }

        // ---- draw ---------------------------------------------------------
        clear_background(OCEAN);
        match snap.as_ref() {
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
            Some(s) => {
                frames_synced += 1;
                let world = &s.models.world;
                draw_world(world, &cam, selected);

                // Hover tooltip (suppressed while dragging / over chrome).
                let pl = panel_layout();
                let over_panel = panel.is_some() && pl.frame.contains(mouse);
                let ml = minimap_layout(world);
                let over_minimap = panel.is_none() && ml.frame.contains(mouse);
                if drag_anchor.is_none()
                    && !over_panel
                    && !over_minimap
                    && mouse.y > panels::CHROME_H
                    && mouse.y < screen_height() - panels::STRIP_H
                    && let Some(cell) = cam.cell_at(mouse, world)
                {
                    draw_tooltip(world, cell, mouse);
                }

                if let Some(p) = &panel {
                    draw_panel(p, &s.observed, &s.models, &pl);
                } else {
                    draw_minimap(world, &cam, &ml);
                }
                draw_attention_strip(&s.models, concern_idx);
            }
        }

        // Top chrome.
        draw_rectangle(0.0, 0.0, screen_width(), panels::CHROME_H - 2.0, PANEL);
        draw_rectangle(0.0, panels::CHROME_H - 2.0, screen_width(), 2.0, PARCHMENT);
        draw_text(
            ascii(&format!("K8SCIV - {status}")),
            12.0,
            21.0,
            20.0,
            PARCHMENT,
        );
        let help = "right-drag/WASD pan . wheel zoom . hover info . click inspect . ]/[ cities . N next concern . Q quit";
        let hm = measure_text(help, None, 14, 1.0);
        draw_text(help, screen_width() - hm.width - 12.0, 21.0, 14.0, DIM);

        if let Some(path) = &shot
            && frames_synced > 45
        {
            get_screen_data().export_png(&path.to_string_lossy());
            break;
        }

        next_frame().await;
    }
}

fn panel_for(world: &k8sciv_core::state::world::WorldModel, sel: (u16, u16)) -> Option<Panel> {
    match world.region_at(sel.0, sel.1) {
        Region::City(_, c) => Some(Panel::City(c.r.clone())),
        Region::Province(p) => Some(Panel::Node(p.tile.name.clone())),
        Region::Structure(_, s) => s.workload.clone().map(Panel::City),
        _ => None,
    }
}
