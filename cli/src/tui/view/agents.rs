//! Agents screen: the agent list on the left; the selected agent's identity,
//! config path and installed MCPs on the right. The pane with focus carries
//! the accent border.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Padding, Paragraph};
use ratatui::Frame;

use super::theme::{self, glyph, Tone};
use super::widgets::{self, fit, width};
use crate::tui::model::{AgentPane, Model};

pub fn render(model: &Model, f: &mut Frame, area: Rect) {
    let cols =
        Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)]).split(area);
    render_agent_list(model, f, cols[0]);
    render_detail(model, f, cols[1]);
}

/// The dimmed capability note for agents without a global MCP config.
fn capability_note(a: &arca_core::application::agents::AgentInfo) -> Option<&'static str> {
    if a.has_global {
        None
    } else if a.skills_global_dir.is_some() {
        Some("Skills")
    } else {
        Some("非 MCP")
    }
}

fn render_agent_list(model: &Model, f: &mut Frame, area: Rect) {
    let focused = model.agents_ui.pane == AgentPane::List;
    let agents = &model.data.agents;
    let block = widgets::panel("Agents", focused);

    if agents.is_empty() {
        let inner = block.inner(area);
        f.render_widget(block, area);
        widgets::empty_state(f, inner, "无 agent", &["按 n 新建"]);
        return;
    }

    let cursor = model.agents_ui.agent_cursor.min(agents.len() - 1);
    let block = block.title_top(widgets::corner_note(format!(
        "{} / {}",
        cursor + 1,
        agents.len()
    )));
    // Notes right-align inside the pane: 2 borders + gutter + dot + gap.
    let row_w = (area.width as usize).saturating_sub(2 + 2 + 2 + 1);
    let items: Vec<ListItem> = agents
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let state = if a.enabled {
                Span::styled(format!("{} ", glyph::ON), theme::fg(Tone::Success))
            } else {
                Span::styled(format!("{} ", glyph::OFF), theme::muted())
            };
            let name_style = if i == cursor {
                theme::selected(focused)
            } else if a.enabled {
                theme::text()
            } else {
                theme::muted()
            };
            let mut spans = vec![state];
            match capability_note(a) {
                Some(note) => {
                    let name_w = row_w.saturating_sub(width(note) + 1);
                    spans.push(Span::styled(fit(&a.name, name_w), name_style));
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(note, theme::muted()));
                }
                None => spans.push(Span::styled(a.name.clone(), name_style)),
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    widgets::render_list(f, area, block, items, cursor, focused);
}

fn render_detail(model: &Model, f: &mut Frame, area: Rect) {
    let Some(agent) = model.data.agents.get(model.agents_ui.agent_cursor) else {
        return;
    };
    let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(0)]).split(area);

    let mut identity = vec![
        Span::styled(agent.id.clone(), theme::muted()),
        Span::raw(theme::GAP),
    ];
    if agent.has_global {
        identity.push(Span::styled(
            format!(" {} ", agent.format),
            theme::chip(Tone::Info),
        ));
    } else if agent.skills_global_dir.is_some() {
        identity.push(Span::styled("Skills", theme::muted()));
    } else {
        identity.push(Span::styled("非 MCP Agent", theme::muted()));
    }
    if !agent.enabled {
        identity.push(Span::raw(" "));
        identity.push(Span::styled(" 已停用 ", theme::chip(Tone::Warning)));
    }
    let path = agent
        .global
        .clone()
        .or_else(|| agent.skills_global_dir.clone())
        .unwrap_or_else(|| "无 MCP/Skills 配置路径".into());
    let head = vec![
        Line::from(identity),
        Line::from(vec![
            Span::styled("路径  ", theme::muted()),
            Span::styled(path, theme::text()),
        ]),
    ];
    let info = widgets::panel(agent.name.clone(), false).padding(Padding::horizontal(1));
    f.render_widget(Paragraph::new(head).block(info), rows[0]);

    render_installed(model, f, rows[1]);
}

fn render_installed(model: &Model, f: &mut Frame, area: Rect) {
    let focused = model.agents_ui.pane == AgentPane::Installed;
    let installed = model.installed_for_selected_agent();
    let enabled = installed.iter().filter(|i| i.enabled).count();
    let block = widgets::panel(format!("已安装 MCP（{}）", installed.len()), focused);

    if installed.is_empty() {
        let inner = block.inner(area);
        f.render_widget(block, area);
        widgets::empty_state(f, inner, "暂无安装", &["按 a 从目录添加 MCP"]);
        return;
    }

    let cursor = model.agents_ui.installed_cursor.min(installed.len() - 1);
    let block = block.title_top(widgets::corner_note(format!("{enabled} 启用")));
    let name_w = installed
        .iter()
        .map(|i| width(&i.name))
        .max()
        .unwrap_or(0)
        .clamp(10, 28);

    let items: Vec<ListItem> = installed
        .iter()
        .enumerate()
        .map(|(idx, i)| {
            let state = if i.enabled {
                Span::styled(format!("{} ", glyph::OK), theme::fg(Tone::Success))
            } else {
                Span::styled(format!("{} ", glyph::FAIL), theme::muted())
            };
            let name_style = if idx == cursor {
                theme::selected(focused)
            } else if i.enabled {
                theme::text()
            } else {
                theme::muted()
            };
            let mut spans = vec![
                state,
                Span::styled(fit(&i.name, name_w), name_style),
                Span::raw(theme::GAP),
                Span::styled(
                    fit(&i.transport, 5),
                    theme::fg(theme::transport_tone(&i.transport)),
                ),
            ];
            if i.customized {
                spans.push(Span::raw(theme::GAP));
                spans.push(Span::styled(
                    format!("{} 已改", glyph::EDITED),
                    theme::fg(Tone::Warning),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    widgets::render_list(f, area, block, items, cursor, focused);
}
