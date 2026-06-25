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

/// Held-erase auto-repeat: wait this long after the initial press, then erase a
/// char every `ERASE_INTERVAL` (≈28/sec) — matching a typical OS key-repeat.
const ERASE_DELAY: f64 = 0.40;
const ERASE_INTERVAL: f64 = 0.035;

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
    /// Held-erase bookkeeping: when the current Backspace/Delete hold began and
    /// when we last emitted an erase (`None` ⇒ not currently held).
    erase_since: Option<f64>,
    erase_last: f64,
}

impl TextField {
    pub fn new(initial: &str, masked: bool) -> Self {
        TextField {
            buf: initial.to_string(),
            masked,
            ..Default::default()
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
        // Erase = Backspace or forward Delete, with held-key auto-repeat: one
        // erase on the initial press, then repeats after a delay while held (so
        // holding the key clears a long value instead of nibbling one char).
        let erase_pressed = is_key_pressed(KeyCode::Backspace) || is_key_pressed(KeyCode::Delete);
        let erase_down = is_key_down(KeyCode::Backspace) || is_key_down(KeyCode::Delete);
        let now = get_time();
        let mut erases = 0u32;
        if erase_pressed {
            erases = 1;
            self.erase_since = Some(now);
            self.erase_last = now;
        } else if erase_down {
            if let Some(since) = self.erase_since {
                let (n, last) = erase_repeats(now, since, self.erase_last);
                erases = n;
                self.erase_last = last;
            }
        } else {
            self.erase_since = None;
        }
        for _ in 0..erases {
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

/// PURE: what to render in a fixed-width (~`cap` chars) single-line field. When
/// the value overflows: focused ⇒ show the TAIL (leading `…`) so the caret and
/// what you're typing stay visible; unfocused ⇒ show the HEAD (trailing `…`) so
/// the value is recognizable by its start. Result is at most `cap` chars.
pub fn field_view(disp: &str, cap: usize, focused: bool) -> String {
    let n = disp.chars().count();
    if cap == 0 || n <= cap {
        return disp.to_string();
    }
    let keep = cap - 1; // leave a column for the ellipsis
    if focused {
        let tail: String = disp.chars().skip(n - keep).collect();
        format!("…{tail}")
    } else {
        let head: String = disp.chars().take(keep).collect();
        format!("{head}…")
    }
}

/// PURE: held-erase auto-repeat. Given the current time, when the hold began, and
/// when the last erase was emitted, return how many repeat erases to apply this
/// frame and the updated `last` time. The initial press is counted by the caller;
/// this adds only the repeats that come after `ERASE_DELAY`. Capped so a huge
/// frame gap (e.g. a debugger pause) can't erase the whole buffer at once.
pub fn erase_repeats(now: f64, since: f64, last: f64) -> (u32, f64) {
    let mut last = last;
    let mut count = 0u32;
    loop {
        let next = (last + ERASE_INTERVAL).max(since + ERASE_DELAY);
        if now + 1e-9 >= next {
            last = next;
            count += 1;
            if count >= 1000 {
                break;
            }
        } else {
            break;
        }
    }
    (count, last)
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

    #[test]
    fn field_view_follows_caret_when_focused() {
        // Fits → rendered whole.
        assert_eq!(
            field_view("http://localhost:11434/v1", 48, true),
            "http://localhost:11434/v1"
        );
        let long = "https://my-resource.openai.azure.com/openai/deployments/gpt-4";
        // Focused + overflow → tail, leading ellipsis, exactly `cap` chars: you
        // can see what you're typing at the end.
        let v = field_view(long, 20, true);
        assert!(v.starts_with('…'));
        assert!(v.ends_with("gpt-4"));
        assert_eq!(v.chars().count(), 20);
        // Unfocused + overflow → head, trailing ellipsis: recognizable by its start.
        let v = field_view(long, 20, false);
        assert!(v.starts_with("https://"));
        assert!(v.ends_with('…'));
        assert_eq!(v.chars().count(), 20);
    }

    #[test]
    fn erase_repeats_respects_delay_then_repeats() {
        // Before the initial delay: no repeats yet (the press itself is the
        // caller's job).
        assert_eq!(erase_repeats(0.1, 0.0, 0.0), (0, 0.0));
        // At the delay: the first repeat fires.
        let (n, last) = erase_repeats(ERASE_DELAY, 0.0, 0.0);
        assert_eq!(n, 1);
        assert!((last - ERASE_DELAY).abs() < 1e-9);
        // Held to delay + 4.5 intervals → the at-delay one + 4 more = 5 (the .5
        // avoids a float boundary).
        let (n2, _) = erase_repeats(ERASE_DELAY + 4.5 * ERASE_INTERVAL, 0.0, 0.0);
        assert_eq!(n2, 5);
        // A huge frame gap is capped, not a full-buffer wipe.
        let (n3, _) = erase_repeats(10_000.0, 0.0, 0.0);
        assert_eq!(n3, 1000);
    }
}
