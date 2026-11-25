//! Overlay dialogs drawn on top of the current screen. Every dialog shares the
//! frame from `widgets::dialog`: dimmed backdrop, rounded border in the
//! dialog's tone, a title, and key hints on the bottom border.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::theme::{self, glyph, Tone};
use super::widgets::{self, fit, width, Field};
use crate::tui::model::{AddMcpState, ConfirmState, InstallWizard, Modal, Model, PasteState};

pub fn render(model: &Model, f: &mut Frame) {
    match model.modal.as_ref() {
        Some(Modal::Detail { key }) => render_detail(model, f, key),
        Some(Modal::Help) => render_help(f),
        Some(Modal::Install(w)) => render_install(model, f, w),
        Some(Modal::AddMcp(st)) => render_add_mcp(model, f, st),
        Some(Modal::Confirm(c)) => render_confirm(f, c),
        Some(Modal::Paste(st)) => render_paste(f, st),
        Some(Modal::Subscribe(form)) => render_form(
            f,
            "订阅远程配置",
            ["配置文件 URL", "名称（可选）"],
            [&form.url, &form.name],
            form.field,
        ),
        Some(Modal::AddLocal(form)) => render_form(
            f,
            "导入本地配置",
            ["文件路径", "名称（可选）"],
            [&form.path, &form.name],
            form.field,
        ),
        Some(Modal::AddAgent(form)) => render_agent_form(f, form),
        None => {}
    }
}

/// Dialog width: `pct` of the screen, kept within `[min, max]` cells.
fn dialog_width(screen: Rect, pct: u16, min: u16, max: u16) -> u16 {
    (screen.width.saturating_mul(pct) / 100).clamp(min, max)
}

/// Height for `content` rows plus the dialog chrome (borders + top padding).
fn dialog_height(content: usize) -> u16 {
    (content as u16).saturating_add(3)
}

fn render_agent_form(f: &mut Frame, form: &crate::tui::model::AgentForm) {
    use crate::tui::model::AGENT_FIELDS;
    let labels = form.labels();
    let fields: Vec<Field> = labels
        .iter()
        .enumerate()
        .take(AGENT_FIELDS)
        .map(|(i, label)| Field {
            label,
            value: form.value(i),
            focused: i == form.field,
            editing: i == form.field && form.editing,
            locked: i == 0 && !form.id_editable(),
        })
        .collect();
    let mut lines = widgets::form_lines(&fields);
    let help = (form.field == 1).then_some("Enter 切换 json ↔ toml");
    if let Some(note) = widgets::form_note(form.error.as_deref(), help) {
        lines.push(Line::from(""));
        lines.push(note);
    }

    let title = if form.is_edit {
        "编辑 Agent"
    } else {
        "新建 Agent"
    };
    let screen = f.area();
    let area = widgets::centered(
        screen,
        dialog_width(screen, 70, 56, 96),
        dialog_height(AGENT_FIELDS + 3),
    );
    let inner = widgets::dialog(
        f,
        area,
        title,
        Tone::Accent,
        &[
            ("↑↓", "字段"),
            ("Enter", "编辑/切换"),
            ("Ctrl-S", "保存"),
            ("Esc", "取消"),
        ],
    );
    f.render_widget(Paragraph::new(lines), inner);
}

/// A small two-field form (Subscribe / AddLocal): Tab switches field, Enter submits.
fn render_form(f: &mut Frame, title: &str, labels: [&str; 2], values: [&str; 2], field: usize) {
    let fields: Vec<Field> = (0..2)
        .map(|i| Field {
            label: labels[i],
            value: values[i].to_string(),
            focused: i == field,
            editing: i == field,
            locked: false,
        })
        .collect();
    let screen = f.area();
    let area = widgets::centered(screen, dialog_width(screen, 66, 56, 96), dialog_height(3));
    let inner = widgets::dialog(
        f,
        area,
        title,
        Tone::Accent,
        &[("Tab", "切换"), ("Enter", "提交"), ("Esc", "取消")],
    );
    f.render_widget(Paragraph::new(widgets::form_lines(&fields)), inner);
}

fn render_paste(f: &mut Frame, st: &PasteState) {
    let screen = f.area();
    let area = widgets::centered(
        screen,
        dialog_width(screen, 75, 56, 120),
        screen.height.saturating_mul(3) / 4,
    );
    let inner = widgets::dialog(
        f,
        area,
        "粘贴配置",
        Tone::Success,
        &[("Ctrl-S", "识别并添加"), ("Esc", "取消")],
    );
    let mut lines: Vec<Line> = if st.text.is_empty() {
        vec![Line::from(Span::styled(
            "在此粘贴 mcpServers JSON / TOML …",
            theme::muted(),
        ))]
    } else {
        st.text.lines().map(|l| Line::from(l.to_string())).collect()
    };
    match lines.last_mut() {
        Some(last) if !st.text.is_empty() && !st.text.ends_with('\n') => {
            last.push_span(Span::styled(glyph::CARET, theme::fg(Tone::Accent)));
        }
        _ => lines.push(Line::from(Span::styled(
            glyph::CARET,
            theme::fg(Tone::Accent),
        ))),
    }
    // Keep the caret in view once the buffer outgrows the dialog.
    let overflow = lines.len().saturating_sub(inner.height as usize) as u16;
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((overflow, 0)),
        inner,
    );
}

/// The header lines of a list dialog sit above a scrolling list; returns the
/// list's area.
fn split_header(inner: Rect, header_rows: u16) -> (Rect, Rect) {
    let [head, list] =
        Layout::vertical([Constraint::Length(header_rows), Constraint::Min(0)]).areas(inner);
    (head, list)
}

fn render_install(model: &Model, f: &mut Frame, w: &InstallWizard) {
    let agents = model.installable_agents_for(&w.transport);
    let picked = w.selected.iter().filter(|s| **s).count();
    let screen = f.area();
    let area = widgets::centered(
        screen,
        dialog_width(screen, 60, 52, 84),
        dialog_height(agents.len().max(1) + 4),
    );
    let inner = widgets::dialog(
        f,
        area,
        "安装到 agent",
        Tone::Accent,
        &[
            ("Space", "选择"),
            ("Ctrl-A", "全选"),
            ("Enter", "应用"),
            ("Esc", "取消"),
        ],
    );
    let (head, list) = split_header(inner, 3);
    let header = vec![
        Line::from(vec![
            Span::styled("安装 ", theme::text()),
            Span::styled(w.server.clone(), theme::strong()),
            Span::raw(theme::GAP),
            theme::transport_span(&w.transport),
        ]),
        Line::from(vec![
            Span::styled("选择要安装到的 agent（全局）：", theme::muted()),
            Span::raw(theme::GAP),
            Span::styled(
                format!("已选 {picked} / {}", agents.len()),
                theme::fg(Tone::Info),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(header), head);

    if agents.is_empty() {
        widgets::empty_state(f, list, "没有支持该传输的 agent", &[]);
        return;
    }
    let items: Vec<ListItem> = agents
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let checked = w.selected.get(i).copied().unwrap_or(false);
            let cbox = if checked {
                Span::styled(format!("{} ", glyph::PICKED), theme::fg(Tone::Success))
            } else {
                Span::styled(format!("{} ", glyph::OFF), theme::muted())
            };
            let name_style = if i == w.cursor {
                theme::selected(true)
            } else {
                theme::text()
            };
            let mut spans = vec![cbox, Span::styled(a.id.clone(), name_style)];
            if a.name != a.id {
                spans.push(Span::raw(theme::GAP));
                spans.push(Span::styled(a.name.clone(), theme::muted()));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    widgets::render_list(f, list, Block::default(), items, w.cursor, true);
}

fn render_add_mcp(model: &Model, f: &mut Frame, st: &AddMcpState) {
    let entries = model.addable_entries(&st.agent, &st.query);
    let screen = f.area();
    let area = widgets::centered(
        screen,
        dialog_width(screen, 70, 56, 110),
        screen.height.saturating_mul(7) / 10,
    );
    let inner = widgets::dialog(
        f,
        area,
        &format!("添加 MCP {} {}", glyph::ARROW, st.agent),
        Tone::Success,
        &[
            ("输入", "搜索"),
            ("↑↓", "选择"),
            ("Enter", "安装"),
            ("Esc", "取消"),
        ],
    );
    let (head, list) = split_header(inner, 2);
    let mut search = vec![Span::styled(format!("{} ", glyph::PROMPT), theme::key())];
    if st.query.is_empty() {
        search.push(Span::styled(glyph::CARET, theme::fg(Tone::Accent)));
        search.push(Span::styled("输入以搜索", theme::muted()));
    } else {
        search.push(Span::styled(st.query.clone(), theme::strong()));
        search.push(Span::styled(glyph::CARET, theme::fg(Tone::Accent)));
    }
    search.push(Span::raw(theme::GAP));
    search.push(Span::styled(
        format!("{} 个", entries.len()),
        theme::muted(),
    ));
    f.render_widget(Paragraph::new(Line::from(search)), head);

    if entries.is_empty() {
        widgets::empty_state(f, list, "无可添加的条目", &[]);
        return;
    }
    let name_w = entries
        .iter()
        .map(|e| width(&e.name))
        .max()
        .unwrap_or(0)
        .clamp(10, 24);
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let name_style = if i == st.cursor {
                theme::selected(true)
            } else {
                theme::text()
            };
            let mut spans = vec![
                Span::styled(fit(&e.name, name_w), name_style),
                Span::raw(theme::GAP),
                Span::styled(
                    fit(e.transport(), 5),
                    theme::fg(theme::transport_tone(e.transport())),
                ),
            ];
            if !e.description.is_empty() {
                spans.push(Span::raw(theme::GAP));
                spans.push(Span::styled(e.description.clone(), theme::muted()));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    widgets::render_list(f, list, Block::default(), items, st.cursor, true);
}

fn render_confirm(f: &mut Frame, c: &ConfirmState) {
    let screen = f.area();
    let w = dialog_width(screen, 50, 48, 72);
    let text_w = w.saturating_sub(6).max(1) as usize;
    let prompt_rows = width(&c.prompt).div_ceil(text_w).max(1);
    let area = widgets::centered(screen, w, dialog_height(prompt_rows + 3));
    let inner = widgets::dialog(f, area, "确认", Tone::Danger, &[]);
    let lines = vec![
        Line::from(Span::styled(c.prompt.clone(), theme::text())),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y 确认 ", theme::chip(Tone::Danger)),
            Span::raw(theme::GAP),
            Span::styled("n / Esc 取消", theme::muted()),
        ]),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// Pretty JSON with keys, strings and literals toned; punctuation muted.
fn json_line(line: &str) -> Line<'static> {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, rest) = line.split_at(indent_len);
    let mut spans = vec![Span::raw(indent.to_string())];
    let (key, value) = match rest.split_once("\": ") {
        Some((k, v)) if rest.starts_with('"') => (Some(format!("{k}\"")), v),
        _ => (None, rest),
    };
    if let Some(k) = key {
        spans.push(Span::styled(k, theme::fg(Tone::Accent)));
        spans.push(Span::styled(": ", theme::muted()));
    }
    let (body, comma) = match value.strip_suffix(',') {
        Some(b) => (b, ","),
        None => (value, ""),
    };
    let style = if body.starts_with('"') {
        theme::fg(Tone::Success)
    } else if matches!(body, "{" | "}" | "[" | "]" | "{}" | "[]") {
        theme::muted()
    } else {
        theme::fg(Tone::Warning)
    };
    spans.push(Span::styled(body.to_string(), style));
    if !comma.is_empty() {
        spans.push(Span::styled(comma, theme::muted()));
    }
    Line::from(spans)
}

fn render_detail(model: &Model, f: &mut Frame, key: &str) {
    let Some(entry) = model.data.registry.iter().find(|e| e.key() == key) else {
        return;
    };
    let (origin, origin_tone) = theme::origin(entry);
    let label = |s: &str| Span::styled(fit(s, 6), theme::muted());

    let mut lines = vec![
        Line::from(vec![
            Span::styled(entry.name.clone(), theme::strong()),
            Span::raw(theme::GAP),
            theme::transport_span(entry.transport()),
            Span::raw(theme::GAP),
            Span::styled(origin, theme::fg(origin_tone)),
        ]),
        Line::from(""),
    ];
    if !entry.tags.is_empty() {
        lines.push(Line::from(vec![
            label("标签"),
            Span::styled(entry.tags.join(", "), theme::text()),
        ]));
    }
    if let Some(source_url) = &entry.source_url {
        lines.push(Line::from(vec![
            label("文档"),
            Span::styled(source_url.clone(), theme::link()),
        ]));
    }
    if let Some(repo) = &entry.repo {
        if entry.source_url.as_ref() != Some(repo) {
            lines.push(Line::from(vec![
                label("仓库"),
                Span::styled(repo.clone(), theme::link()),
            ]));
        }
    }
    if !entry.description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            entry.description.clone(),
            theme::text(),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("配置", theme::strong())));
    if let Ok(json) = serde_json::to_string_pretty(&entry.config) {
        lines.extend(json.lines().map(json_line));
    }

    let screen = f.area();
    let area = widgets::centered(
        screen,
        dialog_width(screen, 70, 56, 100),
        dialog_height(lines.len() + 1),
    );
    let inner = widgets::dialog(
        f,
        area,
        "详情",
        Tone::Accent,
        &[("Esc", "关闭"), ("y", "复制"), ("e", "编辑")],
    );
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_help(f: &mut Frame) {
    let groups: [(&str, &[(&str, &str)]); 3] = [
        (
            "导航",
            &[
                ("1 / 2 / 3", "切换 Registry / 来源 / Agents"),
                ("Tab / ⇧Tab", "循环切换屏幕"),
                ("↑ ↓ / k j", "移动光标"),
                ("Enter", "打开详情 / 进入面板"),
            ],
        ),
        (
            "Registry",
            &[
                ("/", "在 Registry 搜索（Esc 退出）"),
                ("← →", "切换来源过滤（Registry）"),
                ("d", "删除选中条目（手动/探索，并从 agent 卸载）"),
            ],
        ),
        (
            "全局",
            &[
                ("Ctrl-R", "重新加载全部"),
                ("?", "帮助"),
                ("q / Ctrl-C", "退出"),
            ],
        ),
    ];
    let key_w = groups
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|(k, _)| width(k))
        .max()
        .unwrap_or(0)
        + 3;
    let mut lines: Vec<Line> = Vec::new();
    for (gi, (title, rows)) in groups.iter().enumerate() {
        if gi > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(title.to_string(), theme::strong())));
        for (k, d) in rows.iter() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(fit(k, key_w), theme::key()),
                Span::styled(d.to_string(), theme::text()),
            ]));
        }
    }
    let screen = f.area();
    let area = widgets::centered(
        screen,
        dialog_width(screen, 60, 56, 76),
        dialog_height(lines.len() + 1),
    );
    let inner = widgets::dialog(f, area, "快捷键", Tone::Accent, &[("Esc", "关闭")]);
    f.render_widget(Paragraph::new(lines), inner);
}
