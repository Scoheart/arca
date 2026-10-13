//! The only place the model changes. Pure and synchronous: mutate the model,
//! return side effects to run. No I/O here — that's what makes it unit-testable.

use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::effect::Effect;
use super::message::Msg;
use super::model::{
    bucket_of, AddMcpState, AgentForm, AgentPane, ConfirmState, Data, EditorState, EditorTransport,
    InstallWizard, LocalForm, Modal, Model, PasteState, Screen, SubscribeForm, AGENT_FIELDS,
    EDITOR_FIELDS,
};
/// The GitHub raw URL of the curated collection, offered as a remote subscribe.
const OFFICIAL_URL: &str = "https://raw.githubusercontent.com/Scoheart/mux/main/data/registry.json";

pub fn update(model: &mut Model, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Init => return vec![Effect::LoadAll],
        Msg::Tick => model.tick = model.tick.wrapping_add(1),
        Msg::Redraw => {}
        Msg::Loaded(d) => {
            model.loading = false;
            model.data = Data {
                registry: d.registry,
                custom_keys: d.custom_keys.into_iter().collect::<HashSet<_>>(),
                sources: d.sources,
                agents: d.agents,
                installed: d.installed,
            };
            clamp_cursors(model);
        }
        Msg::Mutated { label, result } => match result {
            Ok(()) => {
                model.status = Some(format!("✓ {label}"));
                return vec![Effect::LoadAll];
            }
            Err(e) => model.status = Some(format!("✗ {label}：{e}")),
        },
        Msg::Key(k) => return on_key(model, k),
    }
    vec![]
}

/// Keep every cursor within the bounds of the data it indexes.
fn clamp_cursors(model: &mut Model) {
    let reg = model.filtered_registry().len();
    model.registry_ui.cursor = clamp(model.registry_ui.cursor, reg);
    model.sources_ui.cursor = clamp(model.sources_ui.cursor, model.data.sources.len());
    model.agents_ui.agent_cursor = clamp(model.agents_ui.agent_cursor, model.data.agents.len());
    let inst = model.installed_for_selected_agent().len();
    model.agents_ui.installed_cursor = clamp(model.agents_ui.installed_cursor, inst);
}

/// Clamp `cursor` to `[0, len-1]`, or 0 when empty.
fn clamp(cursor: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        cursor.min(len - 1)
    }
}

fn on_key(model: &mut Model, k: KeyEvent) -> Vec<Effect> {
    // A modal captures all input until dismissed.
    if model.modal.is_some() {
        return modal_key(model, k);
    }
    // The full-page editor captures all input (including digits/q) while open.
    if model.editor.is_some() {
        return editor_key(model, k);
    }

    // The registry search box captures text while focused.
    if model.screen == Screen::Registry && model.registry_ui.searching {
        return registry_search_key(model, k);
    }

    // Global navigation-context keys.
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Char('c') if ctrl => model.should_quit = true,
        KeyCode::Char('q') => model.should_quit = true,
        KeyCode::Char('?') => model.modal = Some(Modal::Help),
        KeyCode::Char('1') => model.screen = Screen::Registry,
        KeyCode::Char('2') => model.screen = Screen::Sources,
        KeyCode::Char('3') => model.screen = Screen::Agents,
        KeyCode::Tab => model.screen = model.screen.next(),
        KeyCode::BackTab => model.screen = model.screen.prev(),
        KeyCode::Char('r') if ctrl => {
            model.loading = true;
            return vec![Effect::LoadAll];
        }
        _ => {
            return match model.screen {
                Screen::Registry => registry_key(model, k),
                Screen::Sources => sources_key(model, k),
                Screen::Agents => agents_key(model, k),
            }
        }
    }
    vec![]
}

fn registry_key(model: &mut Model, k: KeyEvent) -> Vec<Effect> {
    let len = model.filtered_registry().len();
    match k.code {
        KeyCode::Up | KeyCode::Char('k') => {
            model.registry_ui.cursor = model.registry_ui.cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            model.registry_ui.cursor = clamp(model.registry_ui.cursor + 1, len);
        }
        KeyCode::Char('/') => model.registry_ui.searching = true,
        KeyCode::Left | KeyCode::Char('[') => {
            model.registry_ui.filter = model.registry_ui.filter.prev();
            model.registry_ui.cursor = 0;
        }
        KeyCode::Right | KeyCode::Char(']') => {
            model.registry_ui.filter = model.registry_ui.filter.next();
            model.registry_ui.cursor = 0;
        }
        KeyCode::Enter => {
            if let Some(e) = model.filtered_registry().get(model.registry_ui.cursor) {
                let key = e.key();
                model.modal = Some(Modal::Detail { key });
            }
        }
        KeyCode::Char('i') => open_install_wizard(model),
        KeyCode::Char('n') => model.editor = Some(EditorState::new_entry()),
        KeyCode::Char('e') => open_editor_edit(model),
        KeyCode::Char('p') => model.modal = Some(Modal::Paste(PasteState::default())),
        KeyCode::Char('d') => open_forget_confirm(model),
        _ => {}
    }
    vec![]
}

/// `d`: delete the selected entry — only for user-owned (manual/discovered)
/// entries; subscription/local entries are managed on the Sources screen.
fn open_forget_confirm(model: &mut Model) {
    let sel = {
        let entries = model.filtered_registry();
        entries
            .get(model.registry_ui.cursor)
            .map(|e| (e.name.clone(), e.transport().to_string(), bucket_of(e)))
    };
    let Some((name, transport, bucket)) = sel else {
        return;
    };
    if bucket != "manual" && bucket != "discovered" {
        model.status = Some("订阅/本地来源的条目请到「来源」页管理".into());
        return;
    }
    model.modal = Some(Modal::Confirm(Box::new(ConfirmState {
        prompt: format!(
            "删除「{name}」（{transport}）？会从目录移除，并从所有 agent 卸载（有备份）。"
        ),
        effect: Effect::ForgetEntry { name, transport },
    })));
}

/// Open the full-page editor pre-filled from the entry under the cursor.
fn open_editor_edit(model: &mut Model) {
    let ed = {
        let entries = model.filtered_registry();
        let Some(e) = entries.get(model.registry_ui.cursor) else {
            return;
        };
        let is_custom = model.data.custom_keys.contains(&e.key());
        EditorState::from_entry(e, is_custom)
    };
    model.editor = Some(ed);
}

fn editor_key(model: &mut Model, k: KeyEvent) -> Vec<Effect> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && matches!(k.code, KeyCode::Char('c')) {
        model.should_quit = true;
        return vec![];
    }
    if ctrl && matches!(k.code, KeyCode::Char('s')) {
        return save_editor(model);
    }
    let Some(ed) = model.editor.as_mut() else {
        return vec![];
    };
    ed.error = None;

    if ed.editing {
        let field = ed.field;
        match k.code {
            KeyCode::Enter | KeyCode::Esc => ed.editing = false,
            KeyCode::Backspace => {
                if let Some(buf) = ed.field_mut(field) {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(buf) = ed.field_mut(field) {
                    buf.push(c);
                }
            }
            _ => {}
        }
        return vec![];
    }

    // Navigation mode.
    match k.code {
        KeyCode::Up | KeyCode::Char('k') => ed.field = ed.field.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => ed.field = clamp(ed.field + 1, EDITOR_FIELDS),
        KeyCode::Enter => {
            if ed.field == 3 {
                if ed.transport_editable() {
                    ed.transport = match ed.transport {
                        EditorTransport::Stdio => EditorTransport::Http,
                        EditorTransport::Http => EditorTransport::Stdio,
                    };
                }
            } else if ed.field_mut(ed.field).is_some() {
                ed.editing = true;
            }
        }
        KeyCode::Esc => model.editor = None, // cancel
        KeyCode::Char('r') if ed.is_custom => {
            // Revert a custom entry to its source default.
            if let Some(old) = ed.original_key.clone() {
                if let Some((name, transport)) = old.rsplit_once("::") {
                    let eff = Effect::RevertEntry {
                        name: name.to_string(),
                        transport: transport.to_string(),
                    };
                    model.editor = None;
                    return vec![eff];
                }
            }
        }
        _ => {}
    }
    vec![]
}

/// Validate the editor form, guard collisions, and emit the save effect.
fn save_editor(model: &mut Model) -> Vec<Effect> {
    let (result, original_key) = {
        let Some(ed) = model.editor.as_ref() else {
            return vec![];
        };
        let origin = ed
            .original_key
            .as_ref()
            .and_then(|k| model.data.registry.iter().find(|e| &e.key() == k))
            .and_then(|e| e.origin.clone());
        (ed.to_entry(origin), ed.original_key.clone())
    };
    match result {
        Err(msg) => {
            if let Some(ed) = model.editor.as_mut() {
                ed.error = Some(msg);
            }
            vec![]
        }
        Ok(entry) => {
            let new_key = entry.key();
            let collides = original_key.as_deref() != Some(new_key.as_str())
                && model.data.registry.iter().any(|e| e.key() == new_key);
            if collides {
                if let Some(ed) = model.editor.as_mut() {
                    ed.error = Some("已存在同名同传输的条目".into());
                }
                return vec![];
            }
            model.editor = None;
            vec![Effect::UpsertEntry {
                entry,
                existing_key: original_key,
            }]
        }
    }
}

/// Open the multi-agent install target picker for the entry under the cursor.
fn open_install_wizard(model: &mut Model) {
    let (server, transport) = {
        let entries = model.filtered_registry();
        let Some(e) = entries.get(model.registry_ui.cursor) else {
            return;
        };
        (e.name.clone(), e.transport().to_string())
    };
    let n = model.installable_agents_for(&transport).len();
    if n == 0 {
        model.status = Some("没有可安装的 agent（缺少全局配置路径）".into());
        return;
    }
    model.modal = Some(Modal::Install(InstallWizard {
        server,
        transport,
        cursor: 0,
        selected: vec![false; n],
    }));
}

fn registry_search_key(model: &mut Model, k: KeyEvent) -> Vec<Effect> {
    match k.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Down => model.registry_ui.searching = false,
        KeyCode::Backspace => {
            model.registry_ui.query.pop();
            model.registry_ui.cursor = 0;
        }
        KeyCode::Char(c) => {
            model.registry_ui.query.push(c);
            model.registry_ui.cursor = 0;
        }
        _ => {}
    }
    vec![]
}

fn sources_key(model: &mut Model, k: KeyEvent) -> Vec<Effect> {
    let len = model.data.sources.len();
    match k.code {
        KeyCode::Up | KeyCode::Char('k') => {
            model.sources_ui.cursor = model.sources_ui.cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            model.sources_ui.cursor = clamp(model.sources_ui.cursor + 1, len);
        }
        KeyCode::Char(' ') | KeyCode::Enter => return toggle_source(model),
        KeyCode::Char('r') => return refresh_source(model),
        KeyCode::Char('d') => open_remove_source_confirm(model),
        KeyCode::Char('s') => model.modal = Some(Modal::Subscribe(SubscribeForm::default())),
        KeyCode::Char('l') => model.modal = Some(Modal::AddLocal(LocalForm::default())),
        KeyCode::Char('o') => {
            model.modal = Some(Modal::Subscribe(SubscribeForm {
                url: OFFICIAL_URL.into(),
                name: "Arca 精选".into(),
                field: 0,
            }))
        }
        _ => {}
    }
    vec![]
}

/// The source row under the cursor.
fn current_source(model: &Model) -> Option<&arca_core::application::mcp::sources::SourceView> {
    model.data.sources.get(model.sources_ui.cursor)
}

fn toggle_source(model: &mut Model) -> Vec<Effect> {
    let Some(s) = current_source(model) else {
        return vec![];
    };
    vec![Effect::SetSourceEnabled {
        id: s.id.clone(),
        on: !s.enabled,
    }]
}

/// `r`: refresh a remote/local source, or re-scan the managed discovered source
/// without importing anything.
fn refresh_source(model: &mut Model) -> Vec<Effect> {
    let Some(s) = current_source(model) else {
        return vec![];
    };
    if s.managed {
        if s.id == "discovered" {
            model.status = Some("已重新识别外部配置；未自动导入".into());
            return vec![Effect::LoadAll];
        }
        model.status = Some("手动来源无需刷新".into());
        return vec![];
    }
    vec![Effect::RefreshSource { id: s.id.clone() }]
}

fn open_remove_source_confirm(model: &mut Model) {
    let Some(s) = current_source(model) else {
        return;
    };
    if s.managed {
        model.status = Some("Arca 维护的来源不可删除".into());
        return;
    }
    let (id, name) = (s.id.clone(), s.name.clone());
    model.modal = Some(Modal::Confirm(Box::new(ConfirmState {
        prompt: format!("删除来源「{name}」？其缓存文件会一并删除。"),
        effect: Effect::RemoveSource { id },
    })));
}

fn agents_key(model: &mut Model, k: KeyEvent) -> Vec<Effect> {
    match model.agents_ui.pane {
        AgentPane::List => {
            let len = model.data.agents.len();
            match k.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    model.agents_ui.agent_cursor = model.agents_ui.agent_cursor.saturating_sub(1);
                    model.agents_ui.installed_cursor = 0;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    model.agents_ui.agent_cursor = clamp(model.agents_ui.agent_cursor + 1, len);
                    model.agents_ui.installed_cursor = 0;
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                    if !model.installed_for_selected_agent().is_empty() {
                        model.agents_ui.pane = AgentPane::Installed;
                    }
                }
                KeyCode::Char('a') => open_add_mcp(model),
                KeyCode::Char(' ') => return toggle_agent_enabled(model),
                KeyCode::Char('e') => open_agent_edit(model),
                KeyCode::Char('n') => model.modal = Some(Modal::AddAgent(AgentForm::new_agent())),
                _ => {}
            }
        }
        AgentPane::Installed => {
            let len = model.installed_for_selected_agent().len();
            match k.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    model.agents_ui.installed_cursor =
                        model.agents_ui.installed_cursor.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    model.agents_ui.installed_cursor =
                        clamp(model.agents_ui.installed_cursor + 1, len);
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Esc => {
                    model.agents_ui.pane = AgentPane::List;
                }
                KeyCode::Char('a') => open_add_mcp(model),
                KeyCode::Char(' ') => return toggle_installed(model),
                KeyCode::Char('d') => open_delete_confirm(model),
                _ => {}
            }
        }
    }
    vec![]
}

/// The installed row currently under the cursor, as (server, transport, agent).
fn selected_installed(model: &Model) -> Option<(String, String, String)> {
    let row = model
        .installed_for_selected_agent()
        .get(model.agents_ui.installed_cursor)
        .copied()?;
    Some((row.name.clone(), row.transport.clone(), row.agent.clone()))
}

/// Space on an installed row: enable a disabled server, or disable an active one.
fn toggle_installed(model: &mut Model) -> Vec<Effect> {
    let Some(row) = model
        .installed_for_selected_agent()
        .get(model.agents_ui.installed_cursor)
        .copied()
    else {
        return vec![];
    };
    let (server, transport, agent) = (row.name.clone(), row.transport.clone(), row.agent.clone());
    if row.enabled {
        vec![Effect::Disable {
            server,
            transport,
            agent,
        }]
    } else {
        vec![Effect::Enable {
            server,
            transport,
            agent,
        }]
    }
}

/// `d` on an installed row: gate a hard delete behind a Confirm modal.
fn open_delete_confirm(model: &mut Model) {
    let Some((server, transport, agent)) = selected_installed(model) else {
        return;
    };
    model.modal = Some(Modal::Confirm(Box::new(ConfirmState {
        prompt: format!("从 {agent} 删除 {server}？此操作会写回配置文件（有备份）。"),
        effect: Effect::Delete {
            server,
            transport,
            agent,
        },
    })));
}

/// `a`: open the "add MCP to this agent" search popover for the selected agent.
fn open_add_mcp(model: &mut Model) {
    let Some(agent) = model.data.agents.get(model.agents_ui.agent_cursor) else {
        return;
    };
    if !agent.has_global {
        model.status = Some("该 agent 无全局路径，无法安装".into());
        return;
    }
    model.modal = Some(Modal::AddMcp(AddMcpState {
        agent: agent.id.clone(),
        query: String::new(),
        cursor: 0,
    }));
}

/// Space on an agent row: flip only its enabled flag, preserving every
/// capability definition.
fn toggle_agent_enabled(model: &mut Model) -> Vec<Effect> {
    let Some(a) = model.data.agents.get(model.agents_ui.agent_cursor) else {
        return vec![];
    };
    vec![Effect::SetAgentEnabled {
        id: a.id.clone(),
        enabled: !a.enabled,
    }]
}

/// `e` on an agent row: open its config-path editor.
fn open_agent_edit(model: &mut Model) {
    let Some(a) = model.data.agents.get(model.agents_ui.agent_cursor) else {
        return;
    };
    if !a.has_global {
        return;
    }
    model.modal = Some(Modal::AddAgent(AgentForm::from_agent(a)));
}

fn agent_form_key(model: &mut Model, mut form: AgentForm, k: KeyEvent) -> Vec<Effect> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && matches!(k.code, KeyCode::Char('s')) {
        let (id, def) = form.to_def();
        let err = if id.is_empty() {
            Some("ID 不能为空")
        } else if def.key.trim().is_empty() {
            Some("配置 key 不能为空")
        } else if def.global.is_none() {
            Some("全局配置路径不能为空")
        } else {
            None
        };
        if let Some(e) = err {
            form.error = Some(e.into());
            model.modal = Some(Modal::AddAgent(form));
            return vec![];
        }
        let overwrite = form.is_edit;
        return vec![Effect::PutAgent { id, def, overwrite }]; // commit + close
    }
    form.error = None;

    if form.editing {
        let field = form.field;
        match k.code {
            KeyCode::Enter | KeyCode::Esc => form.editing = false,
            KeyCode::Backspace => {
                if let Some(buf) = form.field_mut(field) {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(buf) = form.field_mut(field) {
                    buf.push(c);
                }
            }
            _ => {}
        }
        model.modal = Some(Modal::AddAgent(form));
        return vec![];
    }

    match k.code {
        KeyCode::Esc => return vec![], // cancel
        KeyCode::Up | KeyCode::Char('k') => form.field = form.field.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => form.field = clamp(form.field + 1, AGENT_FIELDS),
        KeyCode::Enter => {
            if form.field == 1 {
                if !form.schema_locked {
                    form.format = match form.format.as_str() {
                        "json" => "toml",
                        "toml" => "yaml",
                        _ => "json",
                    }
                    .into();
                }
            } else if form.field_mut(form.field).is_some() {
                form.editing = true;
            }
        }
        _ => {}
    }
    model.modal = Some(Modal::AddAgent(form));
    vec![]
}

/// Route input to the open modal. The modal is taken out of the model; handlers
/// that keep it open put it back, ones that close it (cancel / commit) don't.
fn modal_key(model: &mut Model, k: KeyEvent) -> Vec<Effect> {
    let Some(modal) = model.modal.take() else {
        return vec![];
    };
    match modal {
        Modal::Detail { key } => {
            // `e` jumps straight to editing this entry.
            if matches!(k.code, KeyCode::Char('e')) {
                if let Some(entry) = model.data.registry.iter().find(|e| e.key() == key) {
                    let is_custom = model.data.custom_keys.contains(&key);
                    model.editor = Some(EditorState::from_entry(entry, is_custom));
                }
                return vec![];
            }
            if !matches!(k.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                model.modal = Some(Modal::Detail { key }); // ignore other keys, stay open
            }
            vec![]
        }
        Modal::Help => {
            if !matches!(k.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                model.modal = Some(Modal::Help);
            }
            vec![]
        }
        Modal::Install(w) => install_key(model, w, k),
        Modal::AddMcp(st) => add_mcp_key(model, st, k),
        Modal::Confirm(c) => match k.code {
            KeyCode::Char('y') | KeyCode::Enter => vec![c.effect], // commit + close
            _ => vec![],                                           // n / Esc / other → cancel
        },
        Modal::Paste(st) => paste_key(model, st, k),
        Modal::Subscribe(form) => subscribe_key(model, form, k),
        Modal::AddLocal(form) => local_key(model, form, k),
        Modal::AddAgent(form) => agent_form_key(model, form, k),
    }
}

/// Trim a form value to an `Option` (empty → None).
fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn subscribe_key(model: &mut Model, mut form: SubscribeForm, k: KeyEvent) -> Vec<Effect> {
    match k.code {
        KeyCode::Esc => return vec![], // cancel
        KeyCode::Tab | KeyCode::Down | KeyCode::Up => form.field = 1 - form.field,
        KeyCode::Enter => {
            if form.url.trim().is_empty() {
                model.status = Some("URL 不能为空".into());
            } else {
                return vec![Effect::Subscribe {
                    url: form.url.clone(),
                    name: opt(&form.name),
                }];
            }
        }
        KeyCode::Backspace => {
            let buf = if form.field == 0 {
                &mut form.url
            } else {
                &mut form.name
            };
            buf.pop();
        }
        KeyCode::Char(c) => {
            let buf = if form.field == 0 {
                &mut form.url
            } else {
                &mut form.name
            };
            buf.push(c);
        }
        _ => {}
    }
    model.modal = Some(Modal::Subscribe(form));
    vec![]
}

fn local_key(model: &mut Model, mut form: LocalForm, k: KeyEvent) -> Vec<Effect> {
    match k.code {
        KeyCode::Esc => return vec![], // cancel
        KeyCode::Tab | KeyCode::Down | KeyCode::Up => form.field = 1 - form.field,
        KeyCode::Enter => {
            if form.path.trim().is_empty() {
                model.status = Some("路径不能为空".into());
            } else {
                return vec![Effect::AddLocal {
                    path: form.path.clone(),
                    name: opt(&form.name),
                }];
            }
        }
        KeyCode::Backspace => {
            let buf = if form.field == 0 {
                &mut form.path
            } else {
                &mut form.name
            };
            buf.pop();
        }
        KeyCode::Char(c) => {
            let buf = if form.field == 0 {
                &mut form.path
            } else {
                &mut form.name
            };
            buf.push(c);
        }
        _ => {}
    }
    model.modal = Some(Modal::AddLocal(form));
    vec![]
}

fn paste_key(model: &mut Model, mut st: PasteState, k: KeyEvent) -> Vec<Effect> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Esc => return vec![], // cancel
        KeyCode::Char('s') if ctrl => {
            if st.text.trim().is_empty() {
                model.status = Some("粘贴内容为空".into());
            } else {
                return vec![Effect::ImportPaste(st.text)]; // commit + close
            }
        }
        KeyCode::Enter => st.text.push('\n'),
        KeyCode::Backspace => {
            st.text.pop();
        }
        KeyCode::Char(c) => st.text.push(c),
        _ => {}
    }
    model.modal = Some(Modal::Paste(st));
    vec![]
}

fn install_key(model: &mut Model, mut w: InstallWizard, k: KeyEvent) -> Vec<Effect> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let agents: Vec<String> = model
        .installable_agents_for(&w.transport)
        .iter()
        .map(|a| a.id.clone())
        .collect();
    match k.code {
        KeyCode::Esc => return vec![], // cancel (already taken out)
        KeyCode::Up | KeyCode::Char('k') => w.cursor = w.cursor.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => w.cursor = clamp(w.cursor + 1, agents.len()),
        KeyCode::Char(' ') => {
            if let Some(s) = w.selected.get_mut(w.cursor) {
                *s = !*s;
            }
        }
        KeyCode::Char('a') if ctrl => {
            let all_on = w.selected.iter().all(|s| *s);
            for s in w.selected.iter_mut() {
                *s = !all_on;
            }
        }
        KeyCode::Enter => {
            let chosen: Vec<String> = agents
                .iter()
                .zip(&w.selected)
                .filter(|(_, s)| **s)
                .map(|(a, _)| a.clone())
                .collect();
            if chosen.is_empty() {
                model.status = Some("请至少选择一个 agent".into());
            } else {
                return vec![Effect::Install {
                    server: w.server,
                    transport: w.transport,
                    agents: chosen,
                }]; // commit + close
            }
        }
        _ => {}
    }
    model.modal = Some(Modal::Install(w));
    vec![]
}

fn add_mcp_key(model: &mut Model, mut st: AddMcpState, k: KeyEvent) -> Vec<Effect> {
    // The popover is a search field: printable keys type into the query; only
    // arrows navigate, Enter installs, Esc cancels.
    let matches: Vec<(String, String)> = model
        .addable_entries(&st.agent, &st.query)
        .iter()
        .map(|e| (e.name.clone(), e.transport().to_string()))
        .collect();
    match k.code {
        KeyCode::Esc => return vec![], // cancel
        KeyCode::Up => st.cursor = st.cursor.saturating_sub(1),
        KeyCode::Down => st.cursor = clamp(st.cursor + 1, matches.len()),
        KeyCode::Backspace => {
            st.query.pop();
            st.cursor = 0;
        }
        KeyCode::Enter => {
            if let Some((server, transport)) = matches.get(st.cursor).cloned() {
                return vec![Effect::Install {
                    server,
                    transport,
                    agents: vec![st.agent],
                }]; // commit + close
            }
        }
        KeyCode::Char(c) => {
            st.query.push(c);
            st.cursor = 0;
        }
        _ => {}
    }
    model.modal = Some(Modal::AddMcp(st));
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::message::LoadedData;
    use arca_core::domain::types::{
        HttpConfig, RegistryConfig, RegistryEntry, RegistryOrigin, StdioConfig,
    };

    fn key(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()))
    }
    fn code(c: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(c, KeyModifiers::empty()))
    }

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

    fn loaded(entries: Vec<RegistryEntry>) -> Msg {
        Msg::Loaded(Box::new(LoadedData {
            registry: entries,
            custom_keys: vec![],
            sources: vec![],
            agents: vec![],
            installed: vec![],
        }))
    }

    #[test]
    fn init_requests_load() {
        let mut m = Model::new();
        assert_eq!(update(&mut m, Msg::Init).len(), 1);
    }

    #[test]
    fn loaded_fills_data_and_clears_loading() {
        let mut m = Model::new();
        update(
            &mut m,
            loaded(vec![entry("git", "manual"), entry("fs", "remote")]),
        );
        assert!(!m.loading);
        assert_eq!(m.data.registry.len(), 2);
    }

    #[test]
    fn digit_keys_switch_screens() {
        let mut m = Model::new();
        update(&mut m, key('2'));
        assert!(m.screen == Screen::Sources);
        update(&mut m, key('3'));
        assert!(m.screen == Screen::Agents);
    }

    #[test]
    fn slash_enters_search_and_types_into_query() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![entry("git", "manual")]));
        update(&mut m, key('/'));
        assert!(m.registry_ui.searching);
        update(&mut m, key('g'));
        update(&mut m, key('i'));
        assert_eq!(m.registry_ui.query, "gi");
        // digits type into the query (not screen-switch) while searching
        update(&mut m, key('1'));
        assert_eq!(m.registry_ui.query, "gi1");
        assert!(m.screen == Screen::Registry);
        update(&mut m, code(KeyCode::Esc));
        assert!(!m.registry_ui.searching);
    }

    #[test]
    fn origin_filter_narrows_registry() {
        let mut m = Model::new();
        update(
            &mut m,
            loaded(vec![entry("git", "manual"), entry("fs", "remote")]),
        );
        // filter → 订阅 (remote) shows only fs
        update(&mut m, code(KeyCode::Right));
        assert_eq!(m.filtered_registry().len(), 1);
        assert_eq!(m.filtered_registry()[0].name, "fs");
    }

    #[test]
    fn enter_opens_and_dismisses_detail_modal() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![entry("git", "manual")]));
        update(&mut m, code(KeyCode::Enter));
        assert!(matches!(m.modal, Some(Modal::Detail { .. })));
        update(&mut m, code(KeyCode::Esc));
        assert!(m.modal.is_none());
    }

    #[test]
    fn cursor_clamps_to_filtered_len() {
        let mut m = Model::new();
        update(
            &mut m,
            loaded(vec![entry("a", "manual"), entry("b", "manual")]),
        );
        update(&mut m, code(KeyCode::Down));
        update(&mut m, code(KeyCode::Down)); // past end
        assert_eq!(m.registry_ui.cursor, 1);
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let mut m = Model::new();
        update(&mut m, key('q'));
        assert!(m.should_quit);
        let mut m2 = Model::new();
        update(
            &mut m2,
            Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        );
        assert!(m2.should_quit);
    }

    fn agent(id: &str, has_global: bool) -> arca_core::application::agents::AgentInfo {
        arca_core::application::agents::AgentInfo {
            id: id.into(),
            name: id.into(),
            format: "json".into(),
            key: "mcpServers".into(),
            has_global,
            has_project: false,
            enabled: true,
            supported_transports: vec!["stdio", "http"],
            global: has_global.then(|| "~/x.json".to_string()),
            project: None,
            docs: None,
            note: None,
            category: "custom".into(),
            evidence: "custom".into(),
            verified_at: None,
            builtin: false,
            skills_global_dir: None,
            skills_global_dirs: Vec::new(),
        }
    }

    fn loaded_full(
        entries: Vec<RegistryEntry>,
        agents: Vec<arca_core::application::agents::AgentInfo>,
    ) -> Msg {
        Msg::Loaded(Box::new(LoadedData {
            registry: entries,
            custom_keys: vec![],
            sources: vec![],
            agents,
            installed: vec![],
        }))
    }

    #[test]
    fn install_wizard_select_and_apply_emits_install() {
        let mut m = Model::new();
        update(
            &mut m,
            loaded_full(
                vec![entry("git", "manual")],
                vec![agent("claude", true), agent("cursor", true)],
            ),
        );
        update(&mut m, key('i')); // open wizard
        assert!(matches!(m.modal, Some(Modal::Install(_))));
        update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty())),
        ); // select claude
        let eff = update(&mut m, code(KeyCode::Enter));
        assert_eq!(eff.len(), 1);
        assert!(
            matches!(&eff[0], Effect::Install { server, agents, .. } if server == "git" && agents == &vec!["claude".to_string()])
        );
        assert!(m.modal.is_none()); // closed on commit
    }

    #[test]
    fn install_wizard_empty_selection_refuses() {
        let mut m = Model::new();
        update(
            &mut m,
            loaded_full(vec![entry("git", "manual")], vec![agent("claude", true)]),
        );
        update(&mut m, key('i'));
        let eff = update(&mut m, code(KeyCode::Enter)); // nothing selected
        assert!(eff.is_empty());
        assert!(matches!(m.modal, Some(Modal::Install(_)))); // stays open
    }

    #[test]
    fn install_needs_an_installable_agent() {
        let mut m = Model::new();
        update(
            &mut m,
            loaded_full(vec![entry("git", "manual")], vec![agent("nopath", false)]),
        );
        update(&mut m, key('i'));
        assert!(m.modal.is_none()); // no wizard — nothing installable
        assert!(m.status.is_some());
    }

    #[test]
    fn install_hides_agents_that_do_not_support_the_transport() {
        let mut desktop = agent("claude-desktop", true);
        desktop.supported_transports = vec!["stdio"];
        let mut remote = entry("remote", "manual");
        remote.config.stdio = None;
        remote.config.http = Some(HttpConfig {
            kind: "http".into(),
            url: "https://example.com/mcp".into(),
            headers: None,
        });
        let mut m = Model::new();
        update(&mut m, loaded_full(vec![remote], vec![desktop]));

        update(&mut m, key('i'));

        assert!(m.modal.is_none());
        assert!(m.status.is_some());
    }

    #[test]
    fn confirm_yes_runs_effect_no_cancels() {
        use crate::tui::model::ConfirmState;
        let mut m = Model::new();
        m.modal = Some(Modal::Confirm(Box::new(ConfirmState {
            prompt: "x".into(),
            effect: Effect::Delete {
                server: "git".into(),
                transport: "stdio".into(),
                agent: "claude".into(),
            },
        })));
        let eff = update(&mut m, key('y'));
        assert_eq!(eff.len(), 1);
        assert!(matches!(&eff[0], Effect::Delete { .. }));
        assert!(m.modal.is_none());

        // 'n' cancels without an effect
        let mut m2 = Model::new();
        m2.modal = Some(Modal::Confirm(Box::new(ConfirmState {
            prompt: "x".into(),
            effect: Effect::Delete {
                server: "git".into(),
                transport: "stdio".into(),
                agent: "claude".into(),
            },
        })));
        let eff2 = update(&mut m2, key('n'));
        assert!(eff2.is_empty());
        assert!(m2.modal.is_none());
    }

    #[test]
    fn mutated_ok_sets_status_and_reloads() {
        let mut m = Model::new();
        let eff = update(
            &mut m,
            Msg::Mutated {
                label: "安装 git".into(),
                result: Ok(()),
            },
        );
        assert_eq!(eff.len(), 1); // reload
        assert!(m.status.as_deref().unwrap().contains("✓"));
    }

    fn ctrl(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }
    fn typed(m: &mut Model, s: &str) {
        for c in s.chars() {
            update(m, key(c));
        }
    }

    #[test]
    fn new_entry_fill_and_save_emits_upsert() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![]));
        update(&mut m, key('n'));
        assert!(m.editor.is_some());
        update(&mut m, code(KeyCode::Enter)); // edit name
        typed(&mut m, "git");
        update(&mut m, code(KeyCode::Enter)); // commit name
        assert_eq!(m.editor.as_ref().unwrap().name, "git");
        for _ in 0..4 {
            update(&mut m, code(KeyCode::Down)); // to command field (index 4)
        }
        update(&mut m, code(KeyCode::Enter));
        typed(&mut m, "npx");
        update(&mut m, code(KeyCode::Enter));
        let eff = update(&mut m, ctrl('s'));
        assert_eq!(eff.len(), 1);
        assert!(
            matches!(&eff[0], Effect::UpsertEntry { entry, existing_key: None } if entry.name == "git")
        );
        assert!(m.editor.is_none());
    }

    #[test]
    fn existing_custom_entry_keeps_name_and_transport_identity_locked() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![entry("git", "manual")]));
        m.data.custom_keys.insert("git::stdio".into());
        update(&mut m, key('e'));

        let editor = m.editor.as_ref().unwrap();
        assert!(editor.is_custom);
        assert!(!editor.name_editable());
        assert!(!editor.transport_editable());

        update(&mut m, code(KeyCode::Enter));
        typed(&mut m, "-renamed");
        assert_eq!(m.editor.as_ref().unwrap().name, "git");

        for _ in 0..3 {
            update(&mut m, code(KeyCode::Down));
        }
        update(&mut m, code(KeyCode::Enter));
        assert!(matches!(
            m.editor.as_ref().unwrap().transport,
            EditorTransport::Stdio
        ));

        let eff = update(&mut m, ctrl('s'));
        assert!(matches!(
            &eff[0],
            Effect::UpsertEntry {
                entry,
                existing_key: Some(existing_key),
            } if entry.key() == "git::stdio" && existing_key == "git::stdio"
        ));
    }

    #[test]
    fn save_with_empty_name_shows_error_and_stays() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![]));
        update(&mut m, key('n'));
        let eff = update(&mut m, ctrl('s'));
        assert!(eff.is_empty());
        assert!(m.editor.as_ref().unwrap().error.is_some());
    }

    #[test]
    fn editor_preserves_repo_round_trip() {
        use crate::tui::model::EditorState;
        let mut e = entry("git", "manual");
        e.repo = Some("https://github.com/x/y".into());
        let ed = EditorState::from_entry(&e, true);
        assert_eq!(ed.repo, "https://github.com/x/y");
        let out = ed.to_entry(None).unwrap();
        assert_eq!(out.repo.as_deref(), Some("https://github.com/x/y"));
    }

    #[test]
    fn editor_preserves_structured_stdio_values_round_trip() {
        use crate::tui::model::EditorState;
        let mut e = entry("structured", "manual");
        let stdio = e.config.stdio.as_mut().unwrap();
        stdio.args = Some(vec!["--label=a,b".into(), " spaced value ".into()]);
        stdio.env = Some(std::collections::HashMap::from([
            ("CSV".into(), "a,b,c".into()),
            ("EQUALS".into(), "a=b".into()),
        ]));
        stdio.cwd = Some("~/work,bench".into());

        let ed = EditorState::from_entry(&e, true);
        let out = ed.to_entry(None).unwrap();

        assert_eq!(out.config.stdio, e.config.stdio);
    }

    #[test]
    fn editor_preserves_structured_http_headers_round_trip() {
        use crate::tui::model::EditorState;
        let mut e = entry("structured-http", "manual");
        e.config.stdio = None;
        e.config.http = Some(arca_core::domain::types::HttpConfig {
            kind: "http".into(),
            url: "https://example.invalid/mcp".into(),
            headers: Some(std::collections::HashMap::from([
                ("Cookie".into(), "a=1,b=2".into()),
                ("X-List".into(), "one,two".into()),
            ])),
        });

        let ed = EditorState::from_entry(&e, true);
        let out = ed.to_entry(None).unwrap();

        assert_eq!(out.config.http, e.config.http);
    }

    #[test]
    fn transport_toggle_swaps_field_set() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![]));
        update(&mut m, key('n'));
        for _ in 0..3 {
            update(&mut m, code(KeyCode::Down)); // to transport field (index 3)
        }
        assert!(matches!(
            m.editor.as_ref().unwrap().transport,
            EditorTransport::Stdio
        ));
        update(&mut m, code(KeyCode::Enter)); // toggle
        assert!(matches!(
            m.editor.as_ref().unwrap().transport,
            EditorTransport::Http
        ));
    }

    #[test]
    fn paste_types_and_imports() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![]));
        update(&mut m, key('p'));
        assert!(matches!(m.modal, Some(Modal::Paste(_))));
        typed(&mut m, "{}");
        let eff = update(&mut m, ctrl('s'));
        assert_eq!(eff.len(), 1);
        assert!(matches!(&eff[0], Effect::ImportPaste(t) if t == "{}"));
    }

    fn source(
        id: &str,
        enabled: bool,
        managed: bool,
    ) -> arca_core::application::mcp::sources::SourceView {
        arca_core::application::mcp::sources::SourceView {
            id: id.into(),
            kind: "remote".into(),
            name: id.into(),
            url: Some("http://x".into()),
            path: None,
            format: "json".into(),
            enabled,
            added_at: None,
            synced_at: None,
            server_count: 1,
            error: None,
            managed,
        }
    }

    fn loaded_sources(srcs: Vec<arca_core::application::mcp::sources::SourceView>) -> Msg {
        Msg::Loaded(Box::new(LoadedData {
            registry: vec![],
            custom_keys: vec![],
            sources: srcs,
            agents: vec![],
            installed: vec![],
        }))
    }

    #[test]
    fn space_toggles_source_enabled() {
        let mut m = Model::new();
        m.screen = Screen::Sources;
        update(&mut m, loaded_sources(vec![source("s1", true, false)]));
        let eff = update(&mut m, key(' '));
        assert!(matches!(&eff[0], Effect::SetSourceEnabled { id, on: false } if id == "s1"));
    }

    #[test]
    fn managed_source_cannot_be_removed() {
        let mut m = Model::new();
        m.screen = Screen::Sources;
        update(&mut m, loaded_sources(vec![source("manual", true, true)]));
        update(&mut m, key('d'));
        assert!(m.modal.is_none()); // no confirm
        assert!(m.status.is_some());
    }

    #[test]
    fn subscribe_form_submits() {
        let mut m = Model::new();
        m.screen = Screen::Sources;
        update(&mut m, loaded_sources(vec![]));
        update(&mut m, key('s'));
        assert!(matches!(m.modal, Some(Modal::Subscribe(_))));
        typed(&mut m, "http://example.com/r.json");
        let eff = update(&mut m, code(KeyCode::Enter));
        assert!(
            matches!(&eff[0], Effect::Subscribe { url, .. } if url == "http://example.com/r.json")
        );
    }

    #[test]
    fn discovered_refresh_only_reloads_detection() {
        let mut m = Model::new();
        m.screen = Screen::Sources;
        update(
            &mut m,
            loaded_sources(vec![source("discovered", true, true)]),
        );
        let eff = update(&mut m, key('r'));
        assert!(matches!(&eff[0], Effect::LoadAll));
        assert_eq!(m.status.as_deref(), Some("已重新识别外部配置；未自动导入"));
    }

    #[test]
    fn space_toggles_agent_enabled() {
        let mut m = Model::new();
        m.screen = Screen::Agents;
        update(
            &mut m,
            loaded_full(vec![], vec![agent("skill-only", false)]),
        );
        let eff = update(&mut m, key(' '));
        assert!(
            matches!(&eff[0], Effect::SetAgentEnabled { id, enabled: false }
            if id == "skill-only")
        );
    }

    #[test]
    fn new_agent_form_fill_and_save() {
        let mut m = Model::new();
        m.screen = Screen::Agents;
        update(&mut m, loaded_full(vec![], vec![agent("claude", true)]));
        update(&mut m, key('n'));
        assert!(matches!(m.modal, Some(Modal::AddAgent(_))));
        update(&mut m, code(KeyCode::Enter)); // edit id
        typed(&mut m, "myagent");
        update(&mut m, code(KeyCode::Enter));
        for _ in 0..3 {
            update(&mut m, code(KeyCode::Down)); // to global path (field 3)
        }
        update(&mut m, code(KeyCode::Enter));
        typed(&mut m, "~/x.json");
        update(&mut m, code(KeyCode::Enter));
        let eff = update(&mut m, ctrl('s'));
        assert!(
            matches!(&eff[0], Effect::PutAgent { id, overwrite: false, .. } if id == "myagent")
        );
        assert!(m.modal.is_none());
    }

    #[test]
    fn editing_agent_preserves_disabled_state() {
        let mut disabled = agent("codex", true);
        disabled.enabled = false;
        disabled.builtin = true;
        let form = AgentForm::from_agent(&disabled);
        let (_, definition) = form.to_def();
        assert!(!definition.enabled);
        assert_eq!(definition.format, "json");
        assert_eq!(definition.key, "mcpServers");
    }

    #[test]
    fn d_on_manual_entry_opens_forget_confirm() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![entry("git", "manual")]));
        update(&mut m, key('d'));
        match &m.modal {
            Some(Modal::Confirm(c)) => {
                assert!(matches!(&c.effect, Effect::ForgetEntry { name, .. } if name == "git"));
            }
            _ => panic!("expected a forget-confirm modal"),
        }
    }

    #[test]
    fn d_on_remote_entry_shows_hint_not_confirm() {
        let mut m = Model::new();
        update(&mut m, loaded(vec![entry("wiki", "remote")]));
        update(&mut m, key('d'));
        assert!(m.modal.is_none(), "remote entries aren't deletable here");
        assert!(m.status.is_some());
    }

    #[test]
    fn agent_form_requires_a_path() {
        let mut m = Model::new();
        m.screen = Screen::Agents;
        update(&mut m, loaded_full(vec![], vec![agent("claude", true)]));
        update(&mut m, key('n'));
        update(&mut m, code(KeyCode::Enter)); // edit id
        typed(&mut m, "x");
        update(&mut m, code(KeyCode::Enter));
        let eff = update(&mut m, ctrl('s')); // no path given
        assert!(eff.is_empty());
        if let Some(Modal::AddAgent(form)) = &m.modal {
            assert!(form.error.is_some());
        } else {
            panic!("form should stay open with an error");
        }
    }
}
