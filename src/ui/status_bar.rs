use ratatui::Frame;
use ratatui::layout::Rect;

use super::RenderCtx;
use crate::util::truncate;

/// Top line: who/where/how-visible. Context, API endpoint, platform hint,
/// counts, active overlay. Rendered reversed so it reads as chrome.
pub fn render(f: &mut Frame, area: Rect, ctx: &RenderCtx, flash: Option<&str>) {
    if area.height == 0 {
        return;
    }
    let meta = &ctx.world.meta;
    let style = ctx.theme.bar();
    let buf = f.buffer_mut();

    // Paint the full bar background first.
    buf.set_string(area.x, area.y, " ".repeat(area.width as usize), style);

    let counts = if ctx.ready {
        format!(
            "nodes {} · pods {}",
            ctx.models.map.total_nodes, ctx.models.map.total_pods
        )
    } else {
        "syncing…".to_string()
    };
    let mut left = format!(
        " K8SCIV ▏{} ▏{} ▏{} ▏{}",
        truncate(&meta.context, 28),
        meta.platform.label(),
        truncate(&meta.server, 34),
        counts,
    );
    if let Some(msg) = flash {
        left.push_str(" ▏");
        left.push_str(&truncate(msg, 48));
    }
    let right = format!("overlay {} ▏? help ", ctx.overlay.label());

    buf.set_stringn(area.x, area.y, &left, area.width as usize, style);
    let rw = right.chars().count() as u16;
    if area.width > rw + 2 {
        buf.set_string(area.x + area.width - rw, area.y, &right, style);
    }
}
