//! Bundled-font text rendering. macroquad's built-in font is a blurry
//! bitmap with ASCII-only coverage; we ship Fira Sans (OFL, see
//! assets/fonts/OFL.txt) and route every label through these helpers.

use std::cell::RefCell;

use macroquad::prelude::*;
use macroquad::text::load_ttf_font_from_bytes;

const REGULAR: &[u8] = include_bytes!("../assets/fonts/FiraSans-Regular.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/FiraSans-SemiBold.ttf");
// A serif (Liberation Serif Bold, OFL 1.1 — Times-metric) for classic-4X
// place-name banners on the map, the way old strategy maps label cities.
const SERIF: &[u8] = include_bytes!("../assets/fonts/LiberationSerif-Bold.ttf");

thread_local! {
    static FONTS: RefCell<Option<(Font, Font)>> = const { RefCell::new(None) };
    static SERIF_FONT: RefCell<Option<Font>> = const { RefCell::new(None) };
}

/// Load the bundled fonts; falls back to the built-in font if parsing
/// ever fails (helpers treat a missing font as "use default").
pub fn init() {
    let regular = load_ttf_font_from_bytes(REGULAR).ok();
    let semibold = load_ttf_font_from_bytes(SEMIBOLD).ok();
    if let (Some(r), Some(b)) = (regular, semibold) {
        FONTS.with(|f| *f.borrow_mut() = Some((r, b)));
    }
    let serif = load_ttf_font_from_bytes(SERIF).ok();
    SERIF_FONT.with(|f| *f.borrow_mut() = serif);
}

fn with_font<T>(bold: bool, f: impl FnOnce(Option<&Font>) -> T) -> T {
    FONTS.with(|fonts| {
        let borrow = fonts.borrow();
        let font = borrow.as_ref().map(|(r, sb)| if bold { sb } else { r });
        f(font)
    })
}

pub fn text(s: impl AsRef<str>, x: f32, y: f32, size: f32, color: Color) {
    with_font(false, |font| {
        draw_text_ex(
            s.as_ref(),
            x,
            y,
            TextParams {
                font,
                font_size: size as u16,
                color,
                ..Default::default()
            },
        )
    });
}

pub fn text_bold(s: impl AsRef<str>, x: f32, y: f32, size: f32, color: Color) {
    with_font(true, |font| {
        draw_text_ex(
            s.as_ref(),
            x,
            y,
            TextParams {
                font,
                font_size: size as u16,
                color,
                ..Default::default()
            },
        )
    });
}

pub fn text_size(s: impl AsRef<str>, size: f32) -> TextDimensions {
    with_font(false, |font| {
        measure_text(s.as_ref(), font, size as u16, 1.0)
    })
}

/// Serif place-name rendering for classic-4X city banners. Falls back to the
/// semibold sans face if the serif failed to parse (still reads as a label).
pub fn name_text(s: impl AsRef<str>, x: f32, y: f32, size: f32, color: Color) {
    let drew = SERIF_FONT.with(|sf| {
        sf.borrow().as_ref().map(|font| {
            draw_text_ex(
                s.as_ref(),
                x,
                y,
                TextParams {
                    font: Some(font),
                    font_size: size as u16,
                    color,
                    ..Default::default()
                },
            )
        })
    });
    if drew.is_none() {
        text_bold(s, x, y, size, color);
    }
}

pub fn name_text_size(s: impl AsRef<str>, size: f32) -> TextDimensions {
    SERIF_FONT.with(|sf| match sf.borrow().as_ref() {
        Some(font) => measure_text(s.as_ref(), Some(font), size as u16, 1.0),
        None => measure_text(s.as_ref(), None, size as u16, 1.0),
    })
}

/// Draw sans text with a dark halo (the same glyphs drawn in `halo` at eight
/// offsets behind the main color), so a label with no backing plate reads on
/// any background — light terrain or dark sea.
pub fn text_outline(s: impl AsRef<str>, x: f32, y: f32, size: f32, color: Color, halo: Color) {
    let s = s.as_ref();
    let o = (size * 0.08).clamp(1.0, 2.0);
    for (dx, dy) in [
        (-1.0, -1.0),
        (0.0, -1.0),
        (1.0, -1.0),
        (-1.0, 0.0),
        (1.0, 0.0),
        (-1.0, 1.0),
        (0.0, 1.0),
        (1.0, 1.0),
    ] {
        text(s, x + dx * o, y + dy * o, size, halo);
    }
    text(s, x, y, size, color);
}
