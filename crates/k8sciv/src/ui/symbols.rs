//! The symbol grammar — one consistent visual vocabulary, documented in
//! CLAUDE.md. Changing any glyph is a breaking change to the operator's eye.

use k8sciv_core::state::model::{NodeHealth, PodState};

pub fn node_glyph(h: NodeHealth) -> char {
    match h {
        NodeHealth::Healthy => '▣',
        NodeHealth::Cordoned => '▤',
        NodeHealth::Pressure => '▥',
        NodeHealth::NotReady => '▦',
    }
}

pub fn pod_glyph(s: PodState) -> char {
    match s {
        PodState::Ok => '●',
        PodState::Starting => '◐',
        PodState::Pending => '○',
        PodState::Terminating => '◌',
        PodState::Failing => '✗',
        PodState::Succeeded => '◆',
    }
}

/// Wargame-style gauge: `▓▓▓▓░░░░░░`.
pub fn bar(ratio: f64, width: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    let mut s = String::with_capacity(width * 3);
    for _ in 0..filled {
        s.push('▓');
    }
    for _ in filled..width {
        s.push('░');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::bar;

    #[test]
    fn bar_clamps_and_fills() {
        assert_eq!(bar(0.0, 4), "░░░░");
        assert_eq!(bar(0.5, 4), "▓▓░░");
        assert_eq!(bar(1.0, 4), "▓▓▓▓");
        assert_eq!(bar(7.3, 4), "▓▓▓▓"); // over-commit clamps
    }
}
