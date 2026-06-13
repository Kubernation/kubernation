//! Bundled-font text rendering. macroquad's built-in font is a blurry
//! bitmap with ASCII-only coverage; we ship Fira Sans (OFL, see
//! assets/fonts/OFL.txt) and route every label through these helpers.

use std::cell::RefCell;

use macroquad::prelude::*;
use macroquad::text::load_ttf_font_from_bytes;

const REGULAR: &[u8] = include_bytes!("../assets/fonts/FiraSans-Regular.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/FiraSans-SemiBold.ttf");

thread_local! {
    static FONTS: RefCell<Option<(Font, Font)>> = const { RefCell::new(None) };
}

/// Load the bundled fonts; falls back to the built-in font if parsing
/// ever fails (helpers treat a missing font as "use default").
pub fn init() {
    let regular = load_ttf_font_from_bytes(REGULAR).ok();
    let semibold = load_ttf_font_from_bytes(SEMIBOLD).ok();
    if let (Some(r), Some(b)) = (regular, semibold) {
        FONTS.with(|f| *f.borrow_mut() = Some((r, b)));
    }
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
