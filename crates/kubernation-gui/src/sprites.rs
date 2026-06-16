//! Bundled sprite tileset (Kenney "Medieval RTS", CC0 — see
//! assets/CREDITS.md), embedded so the binary stays self-contained.
//! `--tileset <dir>` overrides any sprite by name (grass.png, house.png,
//! …) at startup, Freeciv-style.

use std::cell::RefCell;
use std::path::Path;

use macroquad::prelude::*;

pub struct Sprites {
    pub grass: Texture2D,
    pub grass2: Texture2D,
    pub sand: Texture2D,
    pub water: Texture2D,
    pub stone: Texture2D,
    pub house: Texture2D,
    pub house2: Texture2D,
    pub longhouse: Texture2D,
    pub keep: Texture2D,
    pub tent: Texture2D,
    pub tree: Texture2D,
    pub rock: Texture2D,
}

thread_local! {
    static SPRITES: RefCell<Option<Sprites>> = const { RefCell::new(None) };
}

fn load_one(dir: Option<&Path>, name: &str, fallback: &'static [u8]) -> Texture2D {
    let from_dir = dir
        .map(|d| d.join(format!("{name}.png")))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read(p).ok());
    let tex = match &from_dir {
        Some(bytes) => Texture2D::from_file_with_format(bytes, Some(ImageFormat::Png)),
        None => Texture2D::from_file_with_format(fallback, Some(ImageFormat::Png)),
    };
    tex.set_filter(FilterMode::Nearest);
    tex
}

macro_rules! sprite {
    ($dir:expr, $name:literal) => {
        load_one(
            $dir,
            $name,
            include_bytes!(concat!("../assets/sprites/", $name, ".png")),
        )
    };
}

pub fn init(tileset: Option<&Path>) {
    let s = Sprites {
        grass: sprite!(tileset, "grass"),
        grass2: sprite!(tileset, "grass2"),
        sand: sprite!(tileset, "sand"),
        water: sprite!(tileset, "water"),
        stone: sprite!(tileset, "stone"),
        house: sprite!(tileset, "house"),
        house2: sprite!(tileset, "house2"),
        longhouse: sprite!(tileset, "longhouse"),
        keep: sprite!(tileset, "keep"),
        tent: sprite!(tileset, "tent"),
        tree: sprite!(tileset, "tree"),
        rock: sprite!(tileset, "rock"),
    };
    SPRITES.with(|cell| *cell.borrow_mut() = Some(s));
}

pub fn with<T>(f: impl FnOnce(&Sprites) -> T) -> Option<T> {
    SPRITES.with(|cell| cell.borrow().as_ref().map(f))
}

/// Tile a texture across a rectangle, cropping the edge tiles so the
/// pattern never bleeds past the region.
pub fn tile_region(tex: &Texture2D, r: Rect, tint: Color, tile: f32) {
    let (tw, th) = (tex.width(), tex.height());
    let mut y = r.y;
    while y < r.y + r.h {
        let h = (r.y + r.h - y).min(tile);
        let mut x = r.x;
        while x < r.x + r.w {
            let w = (r.x + r.w - x).min(tile);
            draw_texture_ex(
                tex,
                x,
                y,
                tint,
                DrawTextureParams {
                    dest_size: Some(vec2(w, h)),
                    source: Some(Rect::new(0.0, 0.0, tw * (w / tile), th * (h / tile))),
                    ..Default::default()
                },
            );
            x += tile;
        }
        y += tile;
    }
}

/// Draw a sprite centered at `c` with the given height (width keeps the
/// texture's aspect).
pub fn sprite_at(tex: &Texture2D, c: Vec2, height: f32, tint: Color) {
    let aspect = tex.width() / tex.height();
    let size = vec2(height * aspect, height);
    draw_texture_ex(
        tex,
        c.x - size.x / 2.0,
        c.y - size.y / 2.0,
        tint,
        DrawTextureParams {
            dest_size: Some(size),
            ..Default::default()
        },
    );
}
