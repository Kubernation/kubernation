//! Pure, UI-free helpers for working with raw log lines, so the logic lives in
//! one tested place rather than the renderer. (Originally shared by the TUI and
//! GUI log views; the TUI is gone but the split still earns its keep — it's
//! unit-tested without a UI.) Tier-0 scope:
//!
//! - [`classify`] — a cheap severity guess (ERROR/WARN/…) for coloring, from
//!   klog headers, structured `level=`/`"level":` fields, and plaintext markers.
//! - [`FilterExpr`] — the log filter: space-separated AND of substrings, with a
//!   leading `!` marking a term to *exclude* (subtractive triage).
//! - [`split_ts`] — peel a server-prepended RFC3339 timestamp off a line so it
//!   can render in a dim gutter (only present when logs are fetched with
//!   timestamps on).
//!
//! Everything here is a pure function of a `&str` — no clock, no client, no UI.
//! (Structured-field columns / JSON pretty-printing are a later tier.)

/// A line's guessed severity. A *hint* for coloring only — the raw text is never
/// altered, so a mis-guess is harmless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    /// No recognizable level marker.
    Plain,
}

/// Guess a log line's severity from common conventions. Cheap and best-effort
/// (one lowercase pass). Order matters — the *precise* signals (klog headers,
/// structured `level=`/`"level":`/`"severity":` fields) are consulted before
/// the loose uppercase plaintext scan, so an explicit `level=info` line whose
/// message merely mentions "ERROR" stays Info. A leading server timestamp
/// (when logs are fetched with timestamps on) is peeled first so it can't
/// mask the klog header.
pub fn classify(line: &str) -> Level {
    // Classify the message, not any server-prepended RFC3339 timestamp.
    let (_, line) = split_ts(line);

    // klog/glog header (precise): a level letter followed by a 4-digit MMDD,
    // e.g. `E0617 12:34:56.789  1 file.go:10] msg` — ubiquitous in k8s.
    let t = line.trim_start();
    let tb = t.as_bytes();
    if tb.len() >= 5 && tb[1..5].iter().all(u8::is_ascii_digit) {
        match tb[0] {
            b'E' | b'F' => return Level::Error,
            b'W' => return Level::Warn,
            b'I' => return Level::Info,
            _ => {}
        }
    }

    // Structured level fields (precise, logfmt / JSON), case-insensitive —
    // BEFORE the loose plaintext scan.
    let lower = line.to_ascii_lowercase();
    for kw in ["error", "fatal", "panic"] {
        if has_level(&lower, kw) {
            return Level::Error;
        }
    }
    if has_level(&lower, "warn") || has_level(&lower, "warning") {
        return Level::Warn;
    }
    if has_level(&lower, "info") {
        return Level::Info;
    }
    if has_level(&lower, "debug") || has_level(&lower, "trace") {
        return Level::Debug;
    }

    // Loose uppercase plaintext markers (only when no structured level was
    // present — many loggers just print the level uppercased).
    if line.contains("ERROR") || line.contains("FATAL") || line.contains("PANIC") {
        return Level::Error;
    }
    if line.contains("WARNING") || line.contains("WARN") {
        return Level::Warn;
    }
    Level::Plain
}

/// Does `lower` (an already-lowercased line) carry a structured level marker for
/// `kw`? Matches `level=kw`, `level="kw"`, `"level":"kw"`, `"severity":"kw"`,
/// and `[kw]` — the precise forms, so a `msg` that merely mentions the word
/// isn't mistaken for a level.
fn has_level(lower: &str, kw: &str) -> bool {
    lower.contains(&format!("level={kw}"))
        || lower.contains(&format!("level=\"{kw}\""))
        || lower.contains(&format!("\"level\":\"{kw}\""))
        || lower.contains(&format!("\"severity\":\"{kw}\""))
        || lower.contains(&format!("[{kw}]"))
}

/// A parsed log filter expression: space-separated terms, each a
/// case-insensitive substring; a leading `!` makes a term *exclude* matching
/// lines. A line is shown when it contains every include term and none of the
/// exclude terms. An empty expression matches everything.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FilterExpr {
    includes: Vec<String>,
    excludes: Vec<String>,
}

impl FilterExpr {
    pub fn parse(s: &str) -> Self {
        let mut includes = Vec::new();
        let mut excludes = Vec::new();
        for tok in s.split_whitespace() {
            if let Some(neg) = tok.strip_prefix('!') {
                if !neg.is_empty() {
                    excludes.push(neg.to_ascii_lowercase());
                }
            } else {
                includes.push(tok.to_ascii_lowercase());
            }
        }
        FilterExpr { includes, excludes }
    }

    pub fn is_empty(&self) -> bool {
        self.includes.is_empty() && self.excludes.is_empty()
    }

    /// Does this line pass the filter? (All includes present, no excludes.)
    pub fn matches(&self, line: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        let l = line.to_ascii_lowercase();
        self.includes.iter().all(|t| l.contains(t)) && !self.excludes.iter().any(|t| l.contains(t))
    }
}

/// Peel a leading RFC3339(Nano) timestamp off a log line, returning
/// `(Some(timestamp_token), rest)` when present, else `(None, line)`. The
/// Kubernetes API prepends such a timestamp + a space to every line when logs
/// are fetched with timestamps enabled; the cheap shape check guards against
/// mis-splitting an ordinary line that merely begins with a space-delimited
/// token.
pub fn split_ts(line: &str) -> (Option<&str>, &str) {
    if let Some((head, rest)) = line.split_once(' ')
        && looks_like_rfc3339(head)
    {
        return (Some(head), rest);
    }
    (None, line)
}

fn looks_like_rfc3339(s: &str) -> bool {
    let b = s.as_bytes();
    // `2024-06-18T03:40:11...` — date, a `T`, and a time separator.
    b.len() >= 20
        && b[0..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b.get(10) == Some(&b'T')
        && s[11..].contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recognizes_common_forms() {
        // klog headers
        assert_eq!(
            classify("E0617 12:34:56.789  1 x.go:10] boom"),
            Level::Error
        );
        assert_eq!(classify("W0617 12:34:56.789  1 x.go:10] hmm"), Level::Warn);
        assert_eq!(classify("I0617 12:34:56.789  1 x.go:10] ok"), Level::Info);
        // structured
        assert_eq!(classify(r#"{"level":"error","msg":"x"}"#), Level::Error);
        assert_eq!(classify("ts=... level=warn msg=slow"), Level::Warn);
        assert_eq!(classify(r#"{"severity":"ERROR"}"#), Level::Error);
        // plaintext markers
        assert_eq!(classify("2024 ERROR connection refused"), Level::Error);
        assert_eq!(classify("WARNING: disk almost full"), Level::Warn);
        // plain
        assert_eq!(classify("just a normal line"), Level::Plain);
        // a message that merely mentions a word is NOT a false level
        assert_eq!(classify(r#"{"msg":"no errors found"}"#), Level::Plain);
    }

    #[test]
    fn classify_structured_level_beats_uppercase_in_message() {
        // An explicit low severity must win over an uppercase keyword in the
        // *message* text (the precise structured field is consulted first).
        assert_eq!(
            classify(r#"{"level":"info","msg":"connection ERROR recovered"}"#),
            Level::Info
        );
        assert_eq!(
            classify("level=info msg=\"ERROR rate is low\""),
            Level::Info
        );
        assert_eq!(
            classify("level=warn msg=\"all ERROR cleared\""),
            Level::Warn
        );
    }

    #[test]
    fn classify_sees_through_a_leading_timestamp() {
        // With timestamps on, the server prepends an RFC3339 token; the klog
        // header / structured level must still be detected behind it.
        assert_eq!(
            classify("2024-06-18T03:40:11.123Z E0617 12:00:00 1 x.go:1] boom"),
            Level::Error
        );
        assert_eq!(
            classify("2024-06-18T03:40:11.123Z level=warn msg=slow"),
            Level::Warn
        );
    }

    #[test]
    fn filter_includes_excludes_and_empty() {
        let all = FilterExpr::parse("");
        assert!(all.is_empty());
        assert!(all.matches("anything"));

        // case-insensitive include
        let f = FilterExpr::parse("Error");
        assert!(f.matches("an ERROR happened"));
        assert!(!f.matches("all good"));

        // AND of two includes
        let f = FilterExpr::parse("error web");
        assert!(f.matches("web ERROR boom"));
        assert!(!f.matches("db ERROR boom"));

        // leading ! excludes (subtractive triage)
        let f = FilterExpr::parse("!readiness");
        assert!(f.matches("real work"));
        assert!(!f.matches("GET /readiness 200"));

        // include + exclude together
        let f = FilterExpr::parse("error !readiness");
        assert!(f.matches("ERROR in handler"));
        assert!(!f.matches("ERROR on /readiness probe"));

        // a bare `!` is ignored (no empty exclude term)
        assert!(FilterExpr::parse("!").is_empty());
    }

    #[test]
    fn split_ts_peels_only_real_timestamps() {
        let (ts, rest) = split_ts("2024-06-18T03:40:11.123456789Z hello world");
        assert_eq!(ts, Some("2024-06-18T03:40:11.123456789Z"));
        assert_eq!(rest, "hello world");

        // an ordinary line is left intact
        let (ts, rest) = split_ts("hello world");
        assert_eq!(ts, None);
        assert_eq!(rest, "hello world");

        // a line starting with a non-timestamp token isn't mis-split
        let (ts, _) = split_ts("2024 not a timestamp");
        assert_eq!(ts, None);
    }
}
