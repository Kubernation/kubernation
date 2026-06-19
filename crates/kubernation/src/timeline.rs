//! The Annals — the change-timeline surface: a cluster-wide modal ("what changed
//! in the realm?") plus the per-subject section the city/node windows embed
//! (replacing their old separate CHRONICLE + HISTORY lists with one merged,
//! classified, chronological feed). The interesting logic is the pure
//! `annals_lines` draw-decision fn (unit-tested per the GUI testability policy);
//! `draw_annals` is a dumb renderer over it. Cluster-wide (hot).

use kubernation_core::jiff::Timestamp;
use kubernation_core::state::filter::NamespaceFilter;
use kubernation_core::state::timeline::{
    CLUSTER_CAP, ChangeKind, OperatorAction, TIMELINE_WINDOW_MIN, Timeline, TimelineOpts,
    TimelineScope, build_timeline, row_decisions,
};
use kubernation_core::util::format_age_opt_at;
use macroquad::prelude::*;

use crate::net::Snapshot;
use crate::text::{text, text_size};
use crate::theme::*;
use crate::window::draw_window;

/// The colour role of an Annals line — mapped to a theme colour by the renderer.
/// Keeps `annals_lines` pure + testable, and is where **colour discipline** is
/// enforced: only `Crit`/`Warn` ever map to red/yellow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRole {
    /// A critical failure / critical operator action (chaos). Red.
    Crit,
    /// A warning failure / warning operator action (evict). Yellow.
    Warn,
    /// A benign *change* — a rollout (Deploy) or a benign operator action. Cyan.
    Change,
    /// A benign event with a subject (Scale / Schedule / node Ready). Calm ink.
    Calm,
    /// Background churn / unknown events. Dim.
    Dim,
}

/// How recent an entry is — drives the relative-time grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecencyBucket {
    JustNow,
    Last5m,
    LastHour,
    Older,
    Undated,
}

/// One rendered Annals row.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnalsLine {
    pub glyph: &'static str,
    /// "{title} — {detail}" (+ "(you)" / "×N" decorations).
    pub text: String,
    pub role: LineRole,
    /// "5m" / "?" — the relative age.
    pub age: String,
    pub bucket: RecencyBucket,
    /// Draw the "trouble begins here" rule ABOVE this row (the fault line).
    pub fault_line_above: bool,
    /// A change that occurred just before the first failure — "preceded by".
    pub suspect: bool,
}

/// Glyph for a row — ascii-safe (survives `theme::ascii`), meaning carried by the
/// role colour (the established sidebar convention: `!`/`·` + colour).
fn glyph_for(kind: ChangeKind, operator: bool) -> &'static str {
    if operator {
        return "*";
    }
    match kind {
        ChangeKind::Deploy => "^",
        ChangeKind::Scale => "↔",
        ChangeKind::NodeChange => "#",
        ChangeKind::Failure => "!",
        ChangeKind::Schedule => "·",
        ChangeKind::PodChurn => "·",
        ChangeKind::Operator => "*",
        ChangeKind::Event => "·",
    }
}

/// The colour role for an entry — **trouble (Crit/Warn) tracks severity first**,
/// so a benign Info change is never painted red/yellow (colour discipline).
fn role_for(
    kind: ChangeKind,
    severity: kubernation_core::state::attention::Severity,
    operator: bool,
) -> LineRole {
    use kubernation_core::state::attention::Severity;
    match severity {
        Severity::Critical => LineRole::Crit,
        Severity::Warning => LineRole::Warn,
        Severity::Info => {
            if operator || kind == ChangeKind::Deploy {
                LineRole::Change
            } else if matches!(kind, ChangeKind::PodChurn | ChangeKind::Event) {
                LineRole::Dim
            } else {
                LineRole::Calm
            }
        }
    }
}

fn bucket_for(when: Option<&kubernation_core::Time>, now: Timestamp) -> RecencyBucket {
    match when {
        None => RecencyBucket::Undated,
        Some(t) => {
            let secs = now.duration_since(t.0).as_secs();
            if secs <= 60 {
                RecencyBucket::JustNow
            } else if secs <= 5 * 60 {
                RecencyBucket::Last5m
            } else if secs <= 60 * 60 {
                RecencyBucket::LastHour
            } else {
                RecencyBucket::Older
            }
        }
    }
}

/// PURE: the Annals feed as rendered lines. The fault-line rule lands above the
/// first row older than `first_trouble`; a change within `CORRELATION_WINDOW_MIN`
/// before the first failure is flagged `suspect` ("preceded by" — never "caused
/// by"). Unit-tested (incl. the colour-discipline invariant).
pub fn annals_lines(tl: &Timeline, now: Timestamp, cap: usize) -> Vec<AnnalsLine> {
    // The fault-line + suspect decisions live in core (`row_decisions`) so the
    // on-screen Annals and the exported postmortem can never disagree.
    let decisions = row_decisions(tl, cap);

    let mut out: Vec<AnnalsLine> = Vec::new();
    for (e, d) in tl.entries.iter().take(cap).zip(decisions.iter()) {
        let mut t = if e.detail.is_empty() {
            e.title.clone()
        } else {
            format!("{} — {}", e.title, e.detail)
        };
        if e.count > 1 {
            t.push_str(&format!("  ×{}", e.count));
        }
        if e.operator {
            t.push_str("  (you)");
        }
        out.push(AnnalsLine {
            glyph: glyph_for(e.kind, e.operator),
            text: t,
            role: role_for(e.kind, e.severity, e.operator),
            age: format_age_opt_at(now, e.when.as_ref()),
            bucket: bucket_for(e.when.as_ref(), now),
            fault_line_above: d.fault_line_above,
            suspect: d.suspect,
        });
    }
    if tl.entries.len() > cap {
        out.push(AnnalsLine {
            glyph: "",
            text: format!("+{} earlier", tl.entries.len() - cap),
            role: LineRole::Dim,
            age: String::new(),
            bucket: RecencyBucket::Older,
            fault_line_above: false,
            suspect: false,
        });
    }
    out
}

/// Theme colour for a line role.
pub fn role_color(role: LineRole) -> Color {
    match role {
        LineRole::Crit => CRIT,
        LineRole::Warn => WARN,
        LineRole::Change => STRUCT,
        LineRole::Calm => INK,
        LineRole::Dim => DIM,
    }
}

// --- the cluster-wide modal -------------------------------------------------

pub enum AnnalsAction {
    None,
    Close,
    /// Write a markdown after-action report (the postmortem export).
    Export,
}

pub struct Annals {
    scroll: f32,
    max_scroll: f32,
}

impl Annals {
    pub fn new() -> Self {
        Annals {
            scroll: 0.0,
            max_scroll: 0.0,
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll - dy * 36.0).clamp(0.0, self.max_scroll);
    }

    pub fn draw(
        &mut self,
        snap: Option<&Snapshot>,
        ops: &[OperatorAction],
        filter: &NamespaceFilter,
        now: Timestamp,
        mouse: Vec2,
        click: bool,
    ) -> AnnalsAction {
        let win = draw_window(
            "The Annals — what changed in the realm",
            vec2(720.0, 540.0),
            &["Export", "Close"],
            usize::MAX,
        );
        let b = win.body;
        let mut y = b.y - self.scroll;
        let line_h = 18.0;

        if let Some(s) = snap {
            let tl = build_timeline(
                &s.hot.observed,
                &TimelineOpts {
                    scope: TimelineScope::Cluster,
                    filter,
                    window_min: TIMELINE_WINDOW_MIN,
                    cap: CLUSTER_CAP,
                },
                ops,
                now,
            );
            let lines = annals_lines(&tl, now, CLUSTER_CAP);
            if lines.is_empty() {
                y += line_h;
                if y > b.y && y < b.y + b.h {
                    text("nothing changed recently", b.x + 14.0, y, 13.0, DIM);
                }
                y += line_h;
            }
            for ln in &lines {
                if ln.fault_line_above {
                    y += 10.0;
                    if y > b.y && y < b.y + b.h {
                        draw_line(b.x + 6.0, y - 4.0, b.x + b.w - 6.0, y - 4.0, 1.0, CRIT);
                        text("— trouble begins here —", b.x + 14.0, y + 8.0, 12.0, CRIT);
                    }
                    y += 16.0;
                }
                y += line_h;
                if y > b.y - 4.0 && y < b.y + b.h {
                    // age gutter (right-aligned dim), then glyph + text.
                    if !ln.age.is_empty() {
                        let aw = text_size(&ln.age, 12.0).width;
                        text(&ln.age, b.x + b.w - aw - 6.0, y, 12.0, DIM);
                    }
                    let avail = b.w - 70.0;
                    let mut body = format!("{} {}", ln.glyph, ln.text);
                    if ln.suspect {
                        body.push_str("  (before the failure)");
                    }
                    let shown = crate::panels::fit_width(&ascii(&body), 13.0, avail);
                    text(&shown, b.x + 10.0, y, 13.0, role_color(ln.role));
                }
            }
            // Honesty footer.
            y += 22.0;
            if y > b.y && y < b.y + b.h {
                text(
                    ascii(&format!(
                        "recent changes — events (~{}m), rollout history + this session's actions. not a full audit log.",
                        tl.window_min
                    )),
                    b.x + 10.0,
                    y,
                    11.0,
                    DIM,
                );
            }
        } else {
            y += line_h;
            text("the world is not yet explored", b.x + 14.0, y, 13.0, DIM);
            y += line_h;
        }

        let content_h = y - (b.y - self.scroll);
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

        if click {
            if win.close.contains(mouse) {
                return AnnalsAction::Close;
            }
            match win.button_at(mouse) {
                Some(0) => return AnnalsAction::Export, // "Export"
                Some(_) => return AnnalsAction::Close,  // "Close"
                None => {}
            }
            if !win.frame.contains(mouse) {
                return AnnalsAction::Close;
            }
        }
        AnnalsAction::None
    }
}

impl Default for Annals {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubernation_core::Time;
    use kubernation_core::jiff::SignedDuration;
    use kubernation_core::state::attention::Severity;
    use kubernation_core::state::timeline::{ChangeKind, TimelineEntry};

    fn now() -> Timestamp {
        "2026-06-19T12:00:00Z".parse().unwrap()
    }
    fn ago(s: i64) -> Option<Time> {
        Some(Time(now() - SignedDuration::from_secs(s)))
    }

    fn entry(
        kind: ChangeKind,
        sev: Severity,
        operator: bool,
        when: Option<Time>,
        key: &str,
    ) -> TimelineEntry {
        TimelineEntry {
            when,
            kind,
            severity: sev,
            subject: ("demo".into(), key.into(), "Pod".into()),
            title: key.into(),
            detail: "d".into(),
            revision: None,
            count: 1,
            operator,
            key: key.into(),
        }
    }

    fn tl(entries: Vec<TimelineEntry>, first_trouble: Option<Time>) -> Timeline {
        Timeline {
            entries,
            first_trouble,
            truncated: false,
            deployment_only_note: false,
            window_min: TIMELINE_WINDOW_MIN,
        }
    }

    #[test]
    fn annals_lines_color_discipline() {
        // Only Failure/Warning+ + warning/critical operator actions get a Trouble
        // role; benign changes/churn stay calm.
        let entries = vec![
            entry(ChangeKind::Deploy, Severity::Info, false, ago(30), "deploy"),
            entry(ChangeKind::Scale, Severity::Info, false, ago(40), "scale"),
            entry(
                ChangeKind::PodChurn,
                Severity::Info,
                false,
                ago(50),
                "churn",
            ),
            entry(
                ChangeKind::Operator,
                Severity::Info,
                true,
                ago(20),
                "cordon",
            ),
            entry(
                ChangeKind::Failure,
                Severity::Critical,
                false,
                ago(10),
                "crash",
            ),
            entry(
                ChangeKind::Failure,
                Severity::Warning,
                false,
                ago(15),
                "probe",
            ),
            entry(
                ChangeKind::Operator,
                Severity::Warning,
                true,
                ago(5),
                "evict",
            ),
            entry(
                ChangeKind::Operator,
                Severity::Critical,
                true,
                ago(3),
                "chaos",
            ),
        ];
        let lines = annals_lines(&tl(entries, None), now(), 100);
        let role = |k: &str| {
            lines
                .iter()
                .find(|l| l.text.starts_with(&format!("{k} —")))
                .unwrap()
                .role
        };
        // Benign — never red/yellow.
        assert_eq!(role("deploy"), LineRole::Change);
        assert_eq!(role("scale"), LineRole::Calm);
        assert_eq!(role("churn"), LineRole::Dim);
        assert_eq!(role("cordon"), LineRole::Change);
        // Trouble.
        assert_eq!(role("crash"), LineRole::Crit);
        assert_eq!(role("probe"), LineRole::Warn);
        assert_eq!(role("evict"), LineRole::Warn);
        assert_eq!(role("chaos"), LineRole::Crit);
    }

    #[test]
    fn annals_lines_marks_fault_line_above_anchor() {
        // newest-first: [change @30s] [failure @60s] [change @120s]; first_trouble
        // = 60s. The fault line lands on the first row OLDER than 60s.
        let entries = vec![
            entry(ChangeKind::Deploy, Severity::Info, false, ago(30), "after"),
            entry(
                ChangeKind::Failure,
                Severity::Critical,
                false,
                ago(60),
                "boom",
            ),
            entry(
                ChangeKind::Deploy,
                Severity::Info,
                false,
                ago(120),
                "before",
            ),
        ];
        let lines = annals_lines(&tl(entries, ago(60)), now(), 100);
        assert!(!lines[0].fault_line_above && !lines[1].fault_line_above);
        assert!(
            lines[2].fault_line_above,
            "rule above the first pre-trouble row"
        );
    }

    #[test]
    fn annals_lines_flags_suspect_change_before_failure() {
        // A deploy 2m before the failure is a suspect; one 30m before is not; one
        // at the exact failure instant is NOT (strictly-before).
        let entries = vec![
            entry(
                ChangeKind::Failure,
                Severity::Critical,
                false,
                ago(60),
                "boom",
            ),
            entry(
                ChangeKind::Deploy,
                Severity::Info,
                false,
                ago(180),
                "recent-deploy",
            ),
            entry(
                ChangeKind::Deploy,
                Severity::Info,
                false,
                ago(60),
                "same-instant",
            ),
            entry(
                ChangeKind::Deploy,
                Severity::Info,
                false,
                ago(60 + 30 * 60),
                "old-deploy",
            ),
        ];
        let lines = annals_lines(&tl(entries, ago(60)), now(), 100);
        let suspect = |k: &str| {
            lines
                .iter()
                .find(|l| l.text.starts_with(&format!("{k} —")))
                .unwrap()
                .suspect
        };
        assert!(suspect("recent-deploy"));
        assert!(!suspect("old-deploy"));
        assert!(
            !suspect("same-instant"),
            "a change at the failure instant isn't a precursor"
        );
        assert!(!suspect("boom"), "the failure itself is not a suspect");
    }

    #[test]
    fn annals_lines_decorations_and_buckets() {
        let mut op = entry(ChangeKind::Operator, Severity::Info, true, ago(10), "web");
        op.count = 3;
        let lines = annals_lines(&tl(vec![op], None), now(), 100);
        assert!(lines[0].text.contains("×3"));
        assert!(lines[0].text.contains("(you)"));
        assert_eq!(lines[0].bucket, RecencyBucket::JustNow);
        // `age` is computed from the passed-in `now` (deterministic, consistent
        // with `bucket`) — not the wall clock.
        assert_eq!(lines[0].age, "10s");
    }

    #[test]
    fn annals_lines_caps_with_trailer() {
        let entries: Vec<_> = (0..10)
            .map(|i| {
                entry(
                    ChangeKind::Failure,
                    Severity::Warning,
                    false,
                    ago(60 + i),
                    &format!("p{i}"),
                )
            })
            .collect();
        let lines = annals_lines(&tl(entries, ago(60)), now(), 4);
        // 4 rows + a "+6 earlier" trailer.
        assert_eq!(lines.len(), 5);
        assert!(
            lines.last().unwrap().text == "+6 earlier"
                && lines.last().unwrap().role == LineRole::Dim
        );
    }

    #[test]
    fn annals_lines_undated_bucket() {
        let lines = annals_lines(
            &tl(
                vec![entry(ChangeKind::Event, Severity::Info, false, None, "x")],
                None,
            ),
            now(),
            100,
        );
        assert_eq!(lines[0].bucket, RecencyBucket::Undated);
        assert_eq!(lines[0].age, "?");
    }
}
