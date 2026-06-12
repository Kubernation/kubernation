use ratatui::Frame;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Padding, Paragraph};

use super::centered;
use super::theme::Theme;

const KEYMAP: &[(&str, &[(&str, &str)])] = &[
    (
        "NAVIGATE",
        &[
            ("h j k l / arrows", "move cursor / selection"),
            ("g / G", "first / last item"),
            ("PgUp / PgDn", "page within a zone column"),
            ("Ctrl+u / Ctrl+d", "half page"),
            ("Home / End", "first / last zone"),
            ("Enter", "open the thing under the cursor"),
            ("Esc / Backspace", "back"),
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
    let area = centered(f.area(), 66, h);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(theme.chrome())
        .title(" KEYMAP ")
        .title_style(theme.title())
        .padding(Padding::horizontal(2));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
