//! A small reusable single-line text field for the immediate-mode GUI — the
//! Oracle Settings face needs four (name / URL / model / token), and the
//! codebase has a documented history of stray-char bugs every time an editor is
//! hand-rolled (the log filter, the city image editor). This centralizes the
//! gotchas:
//!
//! - macroquad's char queue is NOT cleared per frame → flush it when a field
//!   takes focus (`flush_char_queue`) or stale nav chars (w/a/s/d/:/y) leak in;
//! - Cmd/Ctrl+V enqueues the literal `v` REGARDLESS of the modifier → on a paste
//!   we read the clipboard AND drain the queue the same frame (kill the stray v);
//! - a paste source that prefers the real OS clipboard (long corporate tokens
//!   must paste) with a CLI-tool fallback mirroring `os_clipboard_copy`;
//! - the token field renders MASKED while editing the real value (so backspace /
//!   paste operate on the true string) — the plaintext flows ONLY into the
//!   LlmConfig token, never to a log (this module makes ZERO tracing calls).

use macroquad::prelude::*;

/// Which field currently owns the global char queue (exactly one at a time).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    Name,
    Url,
    Model,
    Token,
    Timeout,
}

/// A single-line editable buffer. `masked` renders bullets but keeps the real
/// value in `buf` (so editing works on the true string).
#[derive(Default, Clone)]
pub struct TextField {
    pub buf: String,
    pub masked: bool,
}

impl TextField {
    pub fn new(initial: &str, masked: bool) -> Self {
        TextField {
            buf: initial.to_string(),
            masked,
        }
    }

    /// Feed input to the FOCUSED field this frame: paste (Cmd/Ctrl+V), backspace,
    /// then literal chars. Call ONLY for the focused field (the single queue
    /// owner). Returns true if the buffer changed.
    pub fn update_focused(&mut self) -> bool {
        let before = self.buf.len();
        let paste = is_key_pressed(KeyCode::V)
            && (is_key_down(KeyCode::LeftSuper)
                || is_key_down(KeyCode::RightSuper)
                || is_key_down(KeyCode::LeftControl)
                || is_key_down(KeyCode::RightControl));
        if paste {
            if let Some(s) = paste_clipboard() {
                // A single-line field: take the first line, trimmed of trailing
                // whitespace (corporate tokens are often copied with a newline).
                let line = s.lines().next().unwrap_or("").trim_end();
                self.buf.push_str(line);
            }
            // Eat the stray 'v' (and any other chars) macroquad queued this frame.
            while get_char_pressed().is_some() {}
        } else {
            while let Some(c) = get_char_pressed() {
                if !c.is_control() {
                    self.buf.push(c);
                }
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            self.buf.pop();
        }
        self.buf.len() != before
    }

    /// What to render — bullets if masked, else the raw value.
    pub fn display(&self) -> String {
        masked_display(&self.buf, self.masked)
    }
}

/// PURE: the rendered representation. Masked ⇒ one bullet per char (length
/// preserved, value never revealed). ASCII `*` so the bundled font + the
/// `theme::ascii` sanitizer always render it.
pub fn masked_display(buf: &str, masked: bool) -> String {
    if masked {
        "*".repeat(buf.chars().count())
    } else {
        buf.to_string()
    }
}

/// Drain macroquad's char queue (call when a field acquires focus so stale nav
/// keys don't leak into the newly-focused buffer — the city.rs:267 idiom).
pub fn flush_char_queue() {
    while get_char_pressed().is_some() {}
}

/// Read the OS clipboard, preferring the windowing layer (a real NSPasteboard /
/// X11 impl) and falling back to a CLI tool (mirrors `os_clipboard_copy` in
/// reverse) so a long token always pastes. Returns None if nothing is available.
pub fn paste_clipboard() -> Option<String> {
    if let Some(s) = macroquad::miniquad::window::clipboard_get()
        && !s.is_empty()
    {
        return Some(s);
    }
    os_clipboard_paste()
}

fn os_clipboard_paste() -> Option<String> {
    use std::process::{Command, Stdio};
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbpaste", &[])]
    } else if cfg!(target_os = "windows") {
        &[("powershell", &["-NoProfile", "-Command", "Get-Clipboard"])]
    } else {
        &[
            ("wl-paste", &["--no-newline"]),
            ("xclip", &["-selection", "clipboard", "-o"]),
            ("xsel", &["--clipboard", "--output"]),
        ]
    };
    for (cmd, args) in candidates {
        if let Ok(out) = Command::new(cmd)
            .args(*args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && out.status.success()
            && let Ok(s) = String::from_utf8(out.stdout)
            && !s.is_empty()
        {
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_display_keeps_length_and_hides_value() {
        assert_eq!(masked_display("sk-SECRET", true), "*********");
        assert_eq!(masked_display("sk-SECRET", true).len(), "sk-SECRET".len());
        assert!(!masked_display("sk-SECRET", true).contains("SECRET"));
        // Unmasked is verbatim.
        assert_eq!(masked_display("gpt-4o", false), "gpt-4o");
        // Multibyte: one bullet per char, not per byte.
        assert_eq!(masked_display("café", true), "****");
    }
}
