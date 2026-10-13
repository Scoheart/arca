//! Pure rendering: read the model, draw widgets. Never mutates. Persistent chrome
//! (tab bar + footer) wraps a per-screen body; modals overlay everything.

mod agents;
mod editor;
mod modal;
mod registry;
mod sources;
mod theme;
mod widgets;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::model::{AgentPane, Model, Screen};
use theme::{glyph, Tone};

pub fn view(model: &Model, f: &mut Frame) {
    let rows = Layout::vertical([
        Constraint::Length(2), // header: wordmark, tabs, totals
        Constraint::Min(0),    // body
        Constraint::Length(1), // footer: status + key hints
    ])
    .split(f.area());

    render_header(model, f, rows[0]);
    render_body(model, f, rows[1]);
    render_footer(model, f, rows[2]);

    if model.modal.is_some() {
        modal::render(model, f);
    }
}

fn render_header(model: &Model, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(theme::muted());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut spans: Vec<Span> = vec![
        Span::styled(" Arca ", theme::chip(Tone::Brand)),
        Span::raw(theme::GAP),
    ];
    for (i, s) in Screen::ALL.iter().enumerate() {
        if *s == model.screen {
            spans.push(Span::styled(
                format!(" {} {} ", i + 1, s.title()),
                theme::chip(Tone::Accent),
            ));
        } else {
            spans.push(Span::styled(format!(" {} ", i + 1), theme::muted()));
            spans.push(Span::styled(format!("{} ", s.title()), theme::text()));
        }
        spans.push(Span::raw(" "));
    }
    let tabs = Line::from(spans);
    let tabs_w = tabs.width();
    f.render_widget(Paragraph::new(tabs), inner);

    if model.loading {
        return;
    }
    let totals = Line::from(vec![
        Span::styled(model.data.registry.len().to_string(), theme::strong()),
        Span::styled(" MCP ", theme::muted()),
        Span::styled(glyph::DOT, theme::muted()),
        Span::styled(format!(" {}", model.data.sources.len()), theme::strong()),
        Span::styled(" 来源 ", theme::muted()),
        Span::styled(glyph::DOT, theme::muted()),
        Span::styled(format!(" {}", model.data.agents.len()), theme::strong()),
        Span::styled(" Agents ", theme::muted()),
    ])
    .right_aligned();
    if tabs_w + totals.width() + 2 <= inner.width as usize {
        f.render_widget(Paragraph::new(totals), inner);
    }
}

fn render_body(model: &Model, f: &mut Frame, area: Rect) {
    if model.editor.is_some() {
        editor::render(model, f, area);
        return;
    }
    if model.loading {
        widgets::empty_state(f, area, "加载中…", &["正在读取目录、来源与 Agent 配置"]);
        return;
    }
    match model.screen {
        Screen::Registry => registry::render(model, f, area),
        Screen::Sources => sources::render(model, f, area),
        Screen::Agents => agents::render(model, f, area),
    }
}

/// The status message (toned by its leading ✓ / ✗) followed by the live key
/// hints, so a message never hides what the keys do.
fn render_footer(model: &Model, f: &mut Frame, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
    if let Some(status) = &model.status {
        let (tone, icon, body) = classify_status(status);
        spans.push(Span::styled(format!(" {icon} "), theme::chip(tone)));
        spans.push(Span::styled(format!(" {body}"), theme::fg(tone)));
        spans.push(Span::raw(theme::GAP));
        spans.push(Span::styled(glyph::DOT, theme::muted()));
        spans.push(Span::raw(theme::GAP));
    }
    spans.extend(widgets::hints(footer_hints(model)));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn classify_status(status: &str) -> (Tone, &'static str, &str) {
    if let Some(rest) = status.strip_prefix(glyph::OK) {
        (Tone::Success, glyph::OK, rest.trim_start())
    } else if let Some(rest) = status.strip_prefix(glyph::FAIL) {
        (Tone::Danger, glyph::FAIL, rest.trim_start())
    } else {
        (Tone::Warning, glyph::WARN, status)
    }
}

/// Context-sensitive key hints, so the UI documents itself.
fn footer_hints(model: &Model) -> &'static [(&'static str, &'static str)] {
    if model.editor.is_some() {
        return &[
            ("↑↓", "字段"),
            ("Enter", "编辑/切换"),
            ("Ctrl-S", "保存"),
            ("r", "恢复默认"),
            ("Esc", "取消"),
        ];
    }
    if model.screen == Screen::Registry && model.registry_ui.searching {
        return &[
            ("输入", "以搜索"),
            ("Enter/Esc", "结束"),
            ("Backspace", "删除"),
        ];
    }
    match model.screen {
        Screen::Registry => &[
            ("↑↓", "移动"),
            ("/", "搜索"),
            ("←→", "过滤"),
            ("i", "安装"),
            ("n", "新建"),
            ("e", "编辑"),
            ("d", "删除"),
            ("p", "粘贴"),
            ("?", "帮助"),
            ("q", "退出"),
        ],
        Screen::Sources => &[
            ("↑↓", "移动"),
            ("Space", "启停"),
            ("r", "刷新"),
            ("d", "删除"),
            ("s", "订阅"),
            ("l", "导入"),
            ("o", "官方"),
            ("?", "帮助"),
            ("q", "退出"),
        ],
        Screen::Agents => match model.agents_ui.pane {
            AgentPane::List => &[
                ("↑↓", "移动"),
                ("→/Enter", "进入"),
                ("a", "添加MCP"),
                ("n", "新建"),
                ("e", "编辑"),
                ("Space", "启停"),
                ("?", "帮助"),
                ("q", "退出"),
            ],
            AgentPane::Installed => &[
                ("↑↓", "移动"),
                ("←/Esc", "返回"),
                ("a", "添加MCP"),
                ("Space", "启停"),
                ("d", "删除"),
                ("?", "帮助"),
                ("q", "退出"),
            ],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::message::LoadedData;
    use crate::tui::message::Msg;
    use crate::tui::update::update;
    use arca_core::domain::types::{RegistryConfig, RegistryEntry, RegistryOrigin, StdioConfig};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn entry(name: &str, kind: &str) -> RegistryEntry {
        RegistryEntry {
            name: name.into(),
            description: String::new(),
            tags: vec![],
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "npx".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: Some(RegistryOrigin {
                kind: kind.into(),
                agent: None,
                scope: None,
                source: None,
            }),
            repo: None,
        }
    }

    /// Flatten the buffer, dropping whitespace so CJK wide-glyph continuation
    /// cells don't split needles.
    fn render(model: &Model) -> String {
        let mut terminal = Terminal::new(TestBackend::new(90, 16)).unwrap();
        terminal.draw(|f| view(model, f)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .flat_map(|c| c.symbol().chars())
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    fn loaded(model: &mut Model, entries: Vec<RegistryEntry>) {
        update(
            model,
            Msg::Loaded(Box::new(LoadedData {
                registry: entries,
                custom_keys: vec![],
                sources: vec![],
                agents: vec![],
                installed: vec![],
            })),
        );
    }

    #[test]
    fn renders_chrome_and_loading() {
        let text = render(&Model::new());
        assert!(text.contains("Arca"));
        assert!(text.contains("Registry"));
        assert!(text.contains("Agents"));
        assert!(text.contains("加载中"));
    }

    #[test]
    fn renders_registry_entries_after_load() {
        let mut m = Model::new();
        loaded(
            &mut m,
            vec![entry("filesystem", "manual"), entry("wiki", "remote")],
        );
        let text = render(&m);
        assert!(text.contains("filesystem"));
        assert!(text.contains("wiki"));
        assert!(!text.contains("加载中"));
    }

    #[test]
    fn empty_catalog_shows_hint() {
        let mut m = Model::new();
        loaded(&mut m, vec![]);
        let text = render(&m);
        assert!(text.contains("目录为空"));
    }

    #[test]
    fn status_keeps_key_hints_visible() {
        let mut m = Model::new();
        loaded(&mut m, vec![entry("filesystem", "manual")]);
        m.status = Some("✗ 安装：boom".into());
        let text = render(&m);
        assert!(text.contains("安装：boom"));
        assert!(text.contains("移动"));
    }

    #[test]
    fn status_tone_follows_leading_glyph() {
        assert!(classify_status("✓ 已安装") == (Tone::Success, glyph::OK, "已安装"));
        assert!(classify_status("✗ 失败：x") == (Tone::Danger, glyph::FAIL, "失败：x"));
        assert!(classify_status("URL 不能为空") == (Tone::Warning, glyph::WARN, "URL 不能为空"));
    }

    #[test]
    fn selected_row_carries_the_focus_gutter() {
        let mut m = Model::new();
        loaded(
            &mut m,
            vec![entry("alpha", "manual"), entry("beta", "manual")],
        );
        let text = render(&m);
        assert!(text.contains("▌alpha"));
        assert!(!text.contains("▌beta"));
    }
}
