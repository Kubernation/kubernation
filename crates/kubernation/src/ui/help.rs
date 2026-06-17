use ratatui::Frame;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Padding, Paragraph};

use super::centered;
use super::theme::Theme;

const KEYMAP: &[(&str, &[(&str, &str)])] = &[
    (
        "NAVIGATE",
        &[
            ("h j k l / arrows", "explore the world / move selection"),
            ("] / [", "sail to next / previous city"),
            ("g / G", "first / last item"),
            ("PgUp / PgDn", "page the map"),
            ("Ctrl+u / Ctrl+d", "half page"),
            ("Home / End", "westmost / eastmost continent"),
            ("Enter", "open the thing under the cursor"),
            ("l", "tail logs of the selected pod (city / node)"),
            ("e", "evict the selected pod — deletes it (confirm y/n)"),
            ("Esc / Backspace", "back"),
        ],
    ),
    (
        "MAP LEGEND",
        &[
            ("◍  city", "workload (deploy/sts); pop = ready pods"),
            ("≣  road", "daemonset"),
            ("Ψ  ∏", "Service harbor / Ingress gate (coast)"),
            ("⊞  granary", "mounted PVCs (yellow = unbound)"),
            ("✦  ◌", "custom resource / encampment"),
            ("◈  ◷", "Job / CronJob (namespace islands)"),
            ("▣ ▤ ▥ ▦", "node: ok / cordon / pressure / notready"),
            ("● ◐ ○ ◌ ✗ ◆", "pod: ok/start/pend/term/fail/done"),
            ("‼  !  ·", "attention: critical / warning / info"),
        ],
    ),
    (
        "VIEWS",
        &[
            ("m", "main map"),
            ("w", "workload list"),
            ("c", "switch kube context (hot)"),
            ("h / l past the edge", "cross to the other continent (pair)"),
        ],
    ),
    (
        "ATTENTION",
        &[
            ("n", "next concern (opens its view)"),
            ("a", "expand / collapse panel"),
            ("Tab", "focus panel (j/k + Enter, Esc leaves)"),
        ],
    ),
    (
        "MAP OVERLAYS",
        &[
            ("1", "pressure (default)"),
            ("2", "replica health"),
            ("3", "namespace ownership"),
        ],
    ),
    ("GENERAL", &[("?", "this keymap"), ("q / Ctrl+C", "quit")]),
];

pub fn render(f: &mut Frame, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    for (section, entries) in KEYMAP {
        lines.push(Line::styled(*section, theme.zone()));
        for (key, desc) in *entries {
            lines.push(Line::from(format!("  {key:<18} {desc}")));
        }
        lines.push(Line::raw(""));
    }
    lines.pop(); // trailing blank

    let h = (lines.len() as u16) + 4;
    let area = centered(f.area(), 72, h);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(theme.chrome())
        .title(" KEYMAP ")
        .title_style(theme.title())
        .padding(Padding::horizontal(2));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
