//! The bundled Kubernation logos. The **mark** (compass medallion, alpha) is
//! the small icon-grade art — the OS window icon and the top-bar emblem; the
//! **full** scene is the splash shown on the fog-of-war screen before sync.
//! Both are compiled in (downsized from the originals) so the binary stays
//! self-contained.

use std::cell::RefCell;

use macroquad::prelude::*;

const MARK: &[u8] = include_bytes!("../assets/logo/mark.png");
const FULL: &[u8] = include_bytes!("../assets/logo/full.png");

thread_local! {
    static LOGOS: RefCell<Option<(Texture2D, Texture2D)>> = const { RefCell::new(None) };
}

/// Decode the textures (needs the GL context — call after macroquad starts).
pub fn init() {
    let mark = Texture2D::from_file_with_format(MARK, Some(ImageFormat::Png));
    let full = Texture2D::from_file_with_format(FULL, Some(ImageFormat::Png));
    mark.set_filter(FilterMode::Linear);
    full.set_filter(FilterMode::Linear);
    LOGOS.with(|c| *c.borrow_mut() = Some((mark, full)));
}

/// Draw the compass mark centered at `c`, `size` px tall (keeps aspect).
pub fn draw_mark(c: Vec2, size: f32) {
    LOGOS.with(|cell| {
        if let Some((mark, _)) = cell.borrow().as_ref() {
            let aspect = mark.width() / mark.height();
            let s = vec2(size * aspect, size);
            draw_texture_ex(
                mark,
                c.x - s.x / 2.0,
                c.y - s.y / 2.0,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(s),
                    ..Default::default()
                },
            );
        }
    });
}

/// Draw the full scene centered at `c`, fit within a `max`-px square. Returns the
/// drawn rect (zero-sized if the texture isn't loaded) so the caller can anchor
/// captions to the actual image bounds instead of a magic offset.
pub fn draw_full(c: Vec2, max: f32) -> Rect {
    LOGOS.with(|cell| {
        if let Some((_, full)) = cell.borrow().as_ref() {
            let scale = max / full.width().max(full.height());
            let s = vec2(full.width() * scale, full.height() * scale);
            let x = c.x - s.x / 2.0;
            let y = c.y - s.y / 2.0;
            draw_texture_ex(
                full,
                x,
                y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(s),
                    ..Default::default()
                },
            );
            Rect::new(x, y, s.x, s.y)
        } else {
            Rect::new(c.x, c.y, 0.0, 0.0)
        }
    })
}

/// Build the OS window icon (16/32/64 RGBA) from the bundled mark. Decodes +
/// resizes on the CPU, so it's safe to call in `window_conf` before GL is up.
pub fn window_icon() -> Option<macroquad::miniquad::conf::Icon> {
    let rgba = image::load_from_memory(MARK).ok()?.to_rgba8();
    let at = |n: u32| -> Vec<u8> {
        image::imageops::resize(&rgba, n, n, image::imageops::FilterType::Triangle).into_raw()
    };
    Some(macroquad::miniquad::conf::Icon {
        small: at(16).try_into().ok()?,
        medium: at(32).try_into().ok()?,
        big: at(64).try_into().ok()?,
    })
}
