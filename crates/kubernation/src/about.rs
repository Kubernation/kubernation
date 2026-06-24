//! The About window — opened from Help ▸ About. Features the splash logo, then
//! credits (Jason Olmsted + Claude), the third-party license obligations we owe
//! (the bundled SIL-OFL fonts + the Rust crate ecosystem), and the trademark
//! disclaimer (this is an unaffiliated homage, not associated with the
//! Civilization rights-holders). A modal on `window.rs`, mirroring the Almanac's
//! window/scroll machinery; the content builder is pure + unit-tested (the GUI
//! testability policy) so the legal obligations can't silently drift away.

use macroquad::prelude::*;

use crate::logo;
use crate::text::{text, text_bold, text_size};
use crate::theme::*;
use crate::window::draw_window;

pub enum AboutAction {
    None,
    Close,
}

/// A titled block of About content — pure data, no GL, so it's unit-testable.
pub struct AboutSection {
    pub heading: &'static str,
    pub lines: Vec<String>,
}

/// The full About content (credits / licenses / trademark). PURE + unit-tested:
/// the authorship credit, the SIL-OFL font notice, and the trademark disclaimer
/// naming the rights-holders are obligations that must always be present.
pub fn about_sections() -> Vec<AboutSection> {
    vec![
        AboutSection {
            heading: "Credits",
            lines: vec![
                "Created by Jason Olmsted.".into(),
                "Built in collaboration with Claude (Anthropic).".into(),
                "Compass mark + the \"KuberNation\" scene: original art by Jason Olmsted.".into(),
                "World map: original procedural geometry — no sprite assets are bundled.".into(),
            ],
        },
        AboutSection {
            heading: "Third-party licenses",
            lines: vec![
                "Fira Sans (Mozilla Foundation, Telefonica S.A.)".into(),
                "    — SIL Open Font License 1.1.".into(),
                "Liberation Serif & Liberation Mono (Red Hat; digitized data (c) Google)".into(),
                "    — SIL Open Font License 1.1.".into(),
                "Many open-source Rust crates — mostly MIT / Apache-2.0, plus some".into(),
                "    ISC, BSD-3-Clause, Zlib & Unicode-3.0 (the rustls/ring TLS stack).".into(),
                "Full third-party notices: THIRD-PARTY-NOTICES.md (+ CREDITS.md, OFL.txt).".into(),
            ],
        },
        AboutSection {
            heading: "Trademark & inspiration",
            lines: vec![
                "Kubernation is an independent, unaffiliated homage. It is not".into(),
                "associated with, endorsed by, or sponsored by Take-Two Interactive".into(),
                "Software, Inc., Firaxis Games, or the Civilization franchise.".into(),
                "\"Sid Meier's Civilization\" and \"Civ\" are trademarks of Take-Two".into(),
                "Interactive, referenced only to describe this project's design".into(),
                "inspiration.".into(),
            ],
        },
    ]
}

const LINE: f32 = 19.0;

/// The About modal: a fixed window with the splash logo, the title/version, and
/// the credits/licenses/trademark sections. Scrolls if the window is shorter
/// than the content (the logo is suppressed once scrolled above the body top so
/// it can't bleed over the titlebar — macroquad has no easy scissor).
#[derive(Default)]
pub struct About {
    scroll: f32,
    max_scroll: f32,
}

impl About {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wheel scroll (positive = up).
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    pub fn draw(&mut self, mouse: Vec2, click: bool) -> AboutAction {
        // Sized so all content — including the trademark disclaimer — fits without
        // scrolling on the default window (the disclaimer is the point; it shouldn't
        // be below the fold). Scroll remains a fallback for a short window.
        let win = draw_window("About KuberNation", vec2(640.0, 740.0), &["Close"], 0);
        let b = win.body;
        let top0 = b.y - self.scroll;
        let mut y = top0;
        let vis = |yy: f32, h: f32| yy + h >= b.y && yy <= b.y + b.h;
        let center = |s: &str, yy: f32, fs: f32, bold: bool, col: Color| {
            let w = text_size(s, fs).width;
            let x = b.x + (b.w - w) / 2.0;
            if bold {
                text_bold(s, x, yy, fs, col);
            } else {
                text(s, x, yy, fs, col);
            }
        };

        // The splash scene, centered. Only drawn when it sits fully below the body
        // top (no scissor → suppress rather than let it bleed over the titlebar).
        let logo_h = 150.0;
        if y >= b.y - 1.0 && vis(y, logo_h) {
            logo::draw_full(vec2(b.x + b.w / 2.0, y + logo_h / 2.0), logo_h);
        }
        y += logo_h + 6.0;

        if vis(y, 26.0) {
            center(
                &format!("KuberNation v{}", env!("CARGO_PKG_VERSION")),
                y + 20.0,
                22.0,
                true,
                PARCHMENT,
            );
        }
        y += 30.0;
        if vis(y, 18.0) {
            center(
                "A 4X-inspired Kubernetes explorer",
                y + 13.0,
                14.0,
                false,
                INK,
            );
        }
        y += 24.0;

        for sec in about_sections() {
            y += 10.0;
            if vis(y, 18.0) {
                text_bold(sec.heading, b.x, y + 14.0, 16.0, PARCHMENT);
                draw_rectangle(b.x, y + 19.0, b.w, 1.0, darker(PARCHMENT, 0.5));
            }
            y += 24.0;
            for line in &sec.lines {
                if vis(y, LINE) {
                    text(line, b.x + 4.0, y + 13.0, 14.0, INK);
                }
                y += LINE;
            }
        }

        // Scroll bookkeeping + a hint bar (mirrors the Almanac).
        let content_h = y - top0;
        self.max_scroll = (content_h - b.h).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);
        if self.max_scroll > 0.0 {
            let frac = (b.h / content_h).clamp(0.05, 1.0);
            let thumb_h = b.h * frac;
            let t = self.scroll / self.max_scroll;
            let ty = b.y + t * (b.h - thumb_h);
            draw_rectangle(b.x + b.w + 2.0, b.y, 3.0, b.h, darker(PANEL, 0.6));
            draw_rectangle(b.x + b.w + 2.0, ty, 3.0, thumb_h, PARCHMENT);
        }

        if click
            && (win.close.contains(mouse)
                || win.button_at(mouse).is_some()
                || !win.frame.contains(mouse))
        {
            return AboutAction::Close;
        }
        AboutAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The legal obligations must always be present in the About content (the
    /// reason this builder is pure + tested): authorship credit for both
    /// authors, the SIL-OFL font notice, and the trademark disclaimer naming the
    /// rights-holders and stating non-association.
    #[test]
    fn about_carries_the_required_credits_and_disclaimer() {
        let all: String = about_sections()
            .iter()
            .flat_map(|s| std::iter::once(s.heading.to_string()).chain(s.lines.clone()))
            .collect::<Vec<_>>()
            .join("\n");

        // Authorship credit (both authors).
        assert!(all.contains("Jason Olmsted"), "credits Jason Olmsted");
        assert!(all.contains("Claude"), "credits Claude");

        // The bundled-font license obligation.
        assert!(
            all.contains("SIL Open Font License"),
            "names the SIL-OFL the bundled fonts require"
        );
        assert!(all.contains("Fira Sans") && all.contains("Liberation"));

        // The crate-license line must NOT claim the binary is MIT/Apache-only — the
        // shipped TLS stack bundles ISC + BSD-3-Clause crates (a review finding). Pin
        // that the non-permissive-default licenses + the full-notice pointer are named.
        assert!(all.contains("ISC") && all.contains("BSD-3-Clause"));
        assert!(all.contains("THIRD-PARTY-NOTICES"));

        // The trademark disclaimer: non-association + the named rights-holders.
        assert!(
            all.contains("unaffiliated") || all.contains("not\nassociated"),
            "states it's an unaffiliated homage"
        );
        assert!(all.contains("Take-Two") && all.contains("Firaxis"));
        assert!(all.contains("Civilization"));
    }
}
