//! Registry screen: search field, origin filter chips, and the catalog table.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph};
use ratatui::Frame;

use super::theme::{self, glyph, Tone};
use super::widgets::{self, fit, width};
use crate::tui::model::{bucket_of, Model, OriginFilter};
use arca_core::domain::types::RegistryEntry;

/// Name column bounds (cells): wide enough for typical server names without
/// starving the description on an 80-column terminal.
const NAME_MIN: usize = 12;
const NAME_MAX: usize = 26;

pub fn render(model: &Model, f: &mut Frame, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Length(3), // search field
        Constraint::Length(1), // filter chips
        Constraint::Min(0),    // catalog
    ])
    .split(area);

    render_search(model, f, rows[0]);
    render_filter(model, f, rows[1]);
    render_catalog(model, f, rows[2]);
}

fn render_search(model: &Model, f: &mut Frame, area: Rect) {
    let ui = &model.registry_ui;
    let block = widgets::panel("搜索", ui.searching);
    let prompt_style = if ui.searching {
        theme::key()
    } else {
        theme::muted()
    };
    let mut spans = vec![Span::styled(format!("{} ", glyph::PROMPT), prompt_style)];
    if ui.query.is_empty() && !ui.searching {
        spans.push(Span::styled("按 / 搜索", theme::muted()));
    } else {
        spans.push(Span::styled(ui.query.clone(), theme::strong()));
        if ui.searching {
            spans.push(Span::styled(glyph::CARET, theme::fg(Tone::Accent)));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

fn render_filter(model: &Model, f: &mut Frame, area: Rect) {
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for filt in OriginFilter::ALL {
        let n = model
            .data
            .registry
            .iter()
            .filter(|e| filter_matches(filt, e))
            .count();
        if filt == model.registry_ui.filter {
            spans.push(Span::styled(
                format!(" {} {n} ", filt.label()),
                theme::chip(Tone::Accent),
            ));
        } else {
            spans.push(Span::styled(format!(" {}", filt.label()), theme::text()));
            spans.push(Span::styled(format!(" {n} "), theme::muted()));
        }
        spans.push(Span::raw(" "));
    }
    spans.push(Span::raw(" "));
    spans.extend(widgets::hints(&[("←→", "切换")]));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn filter_matches(filt: OriginFilter, e: &RegistryEntry) -> bool {
    filt.bucket().is_none_or(|b| bucket_of(e) == b)
}

fn render_catalog(model: &Model, f: &mut Frame, area: Rect) {
    let entries = model.filtered_registry();
    let focused = !model.registry_ui.searching;
    let mut block = widgets::panel("MCP 目录", focused);

    if entries.is_empty() {
        let inner = block.inner(area);
        f.render_widget(block, area);
        if model.data.registry.is_empty() {
            widgets::empty_state(
                f,
                inner,
                "目录为空。",
                &[
                    "到「来源」订阅或导入；",
                    "用 arca discover mcp 查看外部配置。",
                ],
            );
        } else {
            widgets::empty_state(f, inner, "没有匹配的条目。", &["换个关键词或切换来源过滤"]);
        }
        return;
    }

    let cursor = model.registry_ui.cursor.min(entries.len() - 1);
    block = block.title_top(widgets::corner_note(format!(
        "{} / {}",
        cursor + 1,
        entries.len()
    )));

    // Optional columns appear only when some row needs them, so the common
    // case spends its width on descriptions.
    let usage: Vec<usize> = entries.iter().map(|e| model.usage_count(e)).collect();
    let custom: Vec<bool> = entries
        .iter()
        .map(|e| model.data.custom_keys.contains(&e.key()))
        .collect();
    let any_custom = custom.iter().any(|c| *c);
    let usage_w = usage
        .iter()
        .filter(|u| **u > 0)
        .map(|u| width(&format!("{u} 用")))
        .max();
    let name_w = entries
        .iter()
        .map(|e| width(&e.name))
        .max()
        .unwrap_or(0)
        .clamp(NAME_MIN, NAME_MAX);

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let name_style = if i == cursor {
                theme::selected(focused)
            } else {
                theme::text()
            };
            let transport = e.transport();
            let (origin, origin_tone) = theme::origin(e);
            let mut spans = vec![
                Span::styled(fit(&e.name, name_w), name_style),
                Span::raw(theme::GAP),
                Span::styled(
                    fit(transport, 5),
                    theme::fg(theme::transport_tone(transport)),
                ),
                Span::raw(theme::GAP),
                Span::styled(origin, theme::fg(origin_tone)),
            ];
            if any_custom {
                spans.push(Span::raw(theme::GAP));
                spans.push(if custom[i] {
                    Span::styled(
                        format!("{} 自定义", glyph::EDITED),
                        theme::fg(Tone::Warning),
                    )
                } else {
                    Span::raw(fit("", width("✎ 自定义")))
                });
            }
            if let Some(w) = usage_w {
                spans.push(Span::raw(theme::GAP));
                spans.push(if usage[i] > 0 {
                    Span::styled(fit(&format!("{} 用", usage[i]), w), theme::fg(Tone::Info))
                } else {
                    Span::raw(fit("", w))
                });
            }
            if !e.description.is_empty() {
                spans.push(Span::raw(theme::GAP));
                spans.push(Span::styled(e.description.clone(), theme::muted()));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    widgets::render_list(f, area, block, items, cursor, focused);
}
