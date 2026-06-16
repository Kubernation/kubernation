use ratatui::Frame;
use ratatui::layout::Rect;

use super::RenderCtx;
use k8sciv_core::util::truncate;

fn counts(ctx: &RenderCtx) -> String {
    if ctx.ready {
        format!(
            "{}n·{}p",
            ctx.models.map.total_nodes, ctx.models.map.total_pods
        )
    } else {
        "sync…".to_string()
    }
}

/// Top line: who/where/how-visible. Single cluster shows full identity; a
/// pair shows both worlds compactly. Rendered reversed so it reads as chrome.
pub fn render(
    f: &mut Frame,
    area: Rect,
    hot: &RenderCtx,
    warm: Option<&RenderCtx>,
    flash: Option<&str>,
) {
    if area.height == 0 {
        return;
    }
    let style = hot.theme.bar();
    let buf = f.buffer_mut();
    buf.set_string(area.x, area.y, " ".repeat(area.width as usize), style);

    let mut left = match warm {
        None => {
            let meta = &hot.world.meta;
            format!(
                " KUBERNATION ▏{} ▏{} ▏{} ▏{}",
                truncate(&meta.context, 28),
                meta.platform.label(),
                truncate(&meta.server, 34),
                counts(hot),
            )
        }
        Some(w) => format!(
            " KUBERNATION ▏H {} {} ▏W {} {}",
            truncate(&hot.world.meta.context, 24),
            counts(hot),
            truncate(&w.world.meta.context, 24),
            counts(w),
        ),
    };
    if let Some(msg) = flash {
        left.push_str(" ▏");
        left.push_str(&truncate(msg, 48));
    }
    let gauges = if hot.models.map.metrics_live {
        "live"
    } else {
        "req"
    };
    let right = format!("gauges {gauges} ▏overlay {} ▏? help ", hot.overlay.label());

    buf.set_stringn(area.x, area.y, &left, area.width as usize, style);
    let rw = right.chars().count() as u16;
    if area.width > rw + 2 {
        buf.set_string(area.x + area.width - rw, area.y, &right, style);
    }
}
