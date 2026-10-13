//! Sources screen: catalog sources with state, kind, server count and any
//! sync error on its own line.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;
use ratatui::Frame;

use super::theme::{self, glyph, Tone};
use super::widgets::{self, fit, width};
use crate::tui::model::Model;
use arca_core::application::mcp::sources::SourceView;

const NAME_MIN: usize = 10;
const NAME_MAX: usize = 32;

pub fn render(model: &Model, f: &mut Frame, area: Rect) {
    let enabled: u32 = model
        .data
        .sources
        .iter()
        .filter(|s| s.enabled)
        .map(|s| s.server_count)
        .sum();
    let block = widgets::panel("来源", true).title_top(widgets::corner_note(format!(
        "共 {enabled} 个 server（已启用来源）"
    )));

    if model.data.sources.is_empty() {
        let inner = block.inner(area);
        f.render_widget(block, area);
        widgets::empty_state(
            f,
            inner,
            "还没有来源。",
            &["订阅远程配置、导入本地配置，或添加 Arca 精选。"],
        );
        return;
    }

    let sources = &model.data.sources;
    let cursor = model.sources_ui.cursor.min(sources.len() - 1);
    let name_w = sources
        .iter()
        .map(|s| width(&s.name))
        .max()
        .unwrap_or(0)
        .clamp(NAME_MIN, NAME_MAX);
    let count_w = sources
        .iter()
        .map(|s| s.server_count.to_string().len())
        .max()
        .unwrap_or(1);

    let items: Vec<ListItem> = sources
        .iter()
        .enumerate()
        .map(|(i, s)| ListItem::new(source_lines(s, i == cursor, name_w, count_w)))
        .collect();
    widgets::render_list(f, area, block, items, cursor, true);
}

fn source_lines(
    s: &SourceView,
    selected: bool,
    name_w: usize,
    count_w: usize,
) -> Vec<Line<'static>> {
    let state = if s.enabled {
        Span::styled(format!("{} 启用", glyph::ON), theme::fg(Tone::Success))
    } else {
        Span::styled(format!("{} 停用", glyph::OFF), theme::muted())
    };
    let (kind, tone) = if s.managed {
        ("Arca 维护", Tone::Warning)
    } else if s.kind == "remote" {
        ("订阅", Tone::Accent)
    } else {
        ("本地", Tone::Success)
    };
    let name_style = if selected {
        theme::selected(true)
    } else if s.enabled {
        theme::text()
    } else {
        theme::muted()
    };
    let mut lines = vec![Line::from(vec![
        state,
        Span::raw(theme::GAP),
        Span::styled(fit(&s.name, name_w), name_style),
        Span::raw(theme::GAP),
        Span::styled(fit(kind, width("Arca 维护")), theme::fg(tone)),
        Span::raw(theme::GAP),
        Span::styled(format!("{:>count_w$}", s.server_count), theme::strong()),
        Span::styled(" server", theme::muted()),
    ])];
    if let Some(err) = &s.error {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(width("● 启用") + theme::GAP.len())),
            Span::styled(format!(" {} ", glyph::WARN), theme::chip(Tone::Danger)),
            Span::styled(format!(" {err}"), theme::fg(Tone::Danger)),
        ]));
    }
    lines
}
