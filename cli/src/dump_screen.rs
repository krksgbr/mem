use crate::{
    args::{DumpScreenArgs, ScreenTarget},
    hydration, render,
    screen_ref::{load_screen_ref, ScreenRef, ScreenRefState},
    theme::Theme,
};
use anyhow::{anyhow, bail, Result};
use crux_core::App;
use ratatui::{
    backend::TestBackend,
    widgets::{ListState, TableState},
    Terminal,
};
use shared::{
    Event, LayoutMode, Model, ProviderKind, Screen, TranscriptBrowser, ViewContent, ViewModel,
    Workspace,
};
use std::env;

pub fn dump_screen_output(
    args: &DumpScreenArgs,
    workspaces: Vec<Workspace>,
    default_now_ms: i64,
) -> Result<String> {
    let now_ms = args.now_ms.unwrap_or(default_now_ms);
    let mut model = Model::default();
    let app = TranscriptBrowser;

    let _ = app.update(Event::SetWorkspaces(workspaces, now_ms), &mut model, &());
    let render_size = if let Some(screen_ref_path) = args.screen_ref.as_deref() {
        let screen_ref = load_screen_ref(std::path::Path::new(screen_ref_path))?;
        resolve_screen_ref(&app, &mut model, &screen_ref)?;
        (screen_ref.width, screen_ref.height)
    } else {
        resolve_screen(&app, &mut model, args)?;
        (args.width, args.height)
    };
    hydration::hydrate_visible_conversation(&mut model)?;

    let view_model = app.view(&model);
    render_view_model_to_string(&view_model, render_size.0, render_size.1)
}

pub(crate) fn resolve_screen(
    app: &TranscriptBrowser,
    model: &mut Model,
    args: &DumpScreenArgs,
) -> Result<()> {
    apply_provider_filter(app, model, args.provider)?;

    match args.screen {
        ScreenTarget::Workspaces => move_to_workspace_selection(app, model, args.selected),
        ScreenTarget::Conversations => {
            let workspace = args
                .workspace
                .as_deref()
                .ok_or_else(|| anyhow!("--workspace is required for the conversations screen"))?;
            let workspace_idx = find_workspace_index(&model.workspaces, workspace)?;

            move_to_workspace(app, model, workspace_idx)?;
            let _ = app.update(Event::Select, model, &());

            hydration::hydrate_visible_conversation(model)?;
            apply_expand_all(app, model, args.expand_all)?;
            apply_conversation_selection(app, model, args.conversation.as_deref(), args.selected)?;
            hydration::hydrate_visible_conversation(model)?;
            apply_message_index(app, model, args.message_index)?;
            apply_layout(app, model, args.layout.clone())?;
            Ok(())
        }
        ScreenTarget::History => {
            let workspace = args
                .workspace
                .as_deref()
                .ok_or_else(|| anyhow!("--workspace is required for the history screen"))?;
            let conversation = args
                .conversation
                .as_deref()
                .ok_or_else(|| anyhow!("--conversation is required for the history screen"))?;

            let workspace_idx = find_workspace_index(&model.workspaces, workspace)?;
            move_to_workspace(app, model, workspace_idx)?;
            let _ = app.update(Event::Select, model, &());

            apply_conversation_selection(app, model, Some(conversation), 0)?;
            hydration::hydrate_visible_conversation(model)?;
            let _ = app.update(Event::ToggleMessage, model, &());
            hydration::hydrate_visible_conversation(model)?;
            apply_message_index(app, model, args.message_index)?;
            Ok(())
        }
        ScreenTarget::Messages => {
            let workspace = args
                .workspace
                .as_deref()
                .ok_or_else(|| anyhow!("--workspace is required for the messages screen"))?;
            let conversation = args
                .conversation
                .as_deref()
                .ok_or_else(|| anyhow!("--conversation is required for the messages screen"))?;

            let workspace_idx = find_workspace_index(&model.workspaces, workspace)?;
            move_to_workspace(app, model, workspace_idx)?;
            let _ = app.update(Event::Select, model, &());

            apply_conversation_selection(app, model, Some(conversation), 0)?;
            let _ = app.update(Event::Select, model, &());
            hydration::hydrate_visible_conversation(model)?;
            apply_message_index(app, model, args.message_index)?;
            Ok(())
        }
    }
}

fn resolve_screen_ref(
    app: &TranscriptBrowser,
    model: &mut Model,
    screen_ref: &ScreenRef,
) -> Result<()> {
    apply_provider_filter(app, model, screen_ref.provider_filter)?;

    match &screen_ref.screen {
        ScreenRefState::Workspaces { selected_workspace } => {
            let selected_idx = selected_workspace
                .as_deref()
                .map(|workspace| find_workspace_index(&model.workspaces, workspace))
                .transpose()?
                .unwrap_or(0);
            move_to_workspace_selection(app, model, selected_idx)
        }
        ScreenRefState::Conversations {
            selected_row_id,
            selected_row_label,
            selected_index,
            expanded_row_ids,
        } => {
            let workspace = screen_ref.workspace.as_deref().ok_or_else(|| {
                anyhow!("screen ref is missing workspace for conversations screen")
            })?;
            let workspace_idx = find_workspace_index(&model.workspaces, workspace)?;
            model.screen = Screen::Conversations {
                workspace_idx,
                selected_row: 0,
                expanded_ids: expanded_row_ids.clone(),
            };
            select_tree_row(
                app,
                model,
                selected_row_id.as_deref(),
                selected_row_label.as_deref(),
                Some(*selected_index),
            )?;
            Ok(())
        }
        ScreenRefState::Messages {
            conversation,
            focused_message_idx,
            expanded_message_indices,
        } => {
            let workspace = screen_ref
                .workspace
                .as_deref()
                .ok_or_else(|| anyhow!("screen ref is missing workspace for messages screen"))?;
            let workspace_idx = find_workspace_index(&model.workspaces, workspace)?;
            let conv_idx = find_conversation_index(
                &model.workspaces[workspace_idx],
                model.provider_filter,
                conversation,
            )?;
            model.screen = Screen::Messages {
                workspace_idx,
                conv_idx,
                message_state: shared::MessageSelectionState {
                    focused_message_idx: *focused_message_idx,
                    expanded_messages: expanded_message_indices.clone(),
                },
                return_selected_row: 0,
                return_expanded_ids: Vec::new(),
            };
            Ok(())
        }
    }
}

fn apply_provider_filter(
    app: &TranscriptBrowser,
    model: &mut Model,
    provider: Option<ProviderKind>,
) -> Result<()> {
    for _ in 0..3 {
        if model.provider_filter == provider {
            return Ok(());
        }
        let _ = app.update(Event::CycleFilter, model, &());
    }

    bail!("unable to set provider filter to {:?}", provider)
}

fn move_to_workspace(
    app: &TranscriptBrowser,
    model: &mut Model,
    workspace_idx: usize,
) -> Result<()> {
    move_to_workspace_selection(app, model, workspace_idx)
}

fn move_to_workspace_selection(
    app: &TranscriptBrowser,
    model: &mut Model,
    workspace_idx: usize,
) -> Result<()> {
    if workspace_idx >= model.workspaces.len() {
        bail!(
            "workspace index {} is out of range for {} workspaces",
            workspace_idx,
            model.workspaces.len()
        );
    }

    while let Screen::Workspaces { selected_workspace } = &model.screen {
        if *selected_workspace == workspace_idx {
            return Ok(());
        }
        let _ = app.update(Event::Down, model, &());
    }

    bail!("expected to be on the workspaces screen")
}

fn apply_conversation_selection(
    app: &TranscriptBrowser,
    model: &mut Model,
    conversation: Option<&str>,
    selected: usize,
) -> Result<()> {
    if let Some(conversation) = conversation {
        expand_conversation_ancestors(app, model, conversation)?;
    }

    let target_idx = find_target_conversation_index(model, conversation, selected)?;

    while let Screen::Conversations { selected_row, .. } = &model.screen {
        if *selected_row == target_idx {
            return Ok(());
        }
        let _ = app.update(Event::Down, model, &());
    }

    bail!("expected to be on the conversations screen")
}

fn expand_conversation_ancestors(
    app: &TranscriptBrowser,
    model: &mut Model,
    conversation: &str,
) -> Result<()> {
    let Screen::Conversations { workspace_idx, .. } = &model.screen else {
        bail!("expected to be on the conversations screen");
    };

    let workspace = &model.workspaces[*workspace_idx];
    let filtered = filtered_conversations(workspace, model.provider_filter);
    let Some((_, target_conversation)) = filtered
        .iter()
        .find(|(_, conv)| conversation_matches(conv, conversation))
    else {
        bail!(
            "conversation '{}' was not found in workspace '{}'",
            conversation,
            workspace.display_name
        );
    };

    let by_id = filtered
        .iter()
        .map(|(_, conv)| (conv.id.as_str(), *conv))
        .collect::<std::collections::HashMap<_, _>>();

    let mut ancestor_ids = Vec::new();
    let mut cursor = target_conversation.branch_parent_id.as_deref();
    while let Some(parent_id) = cursor {
        ancestor_ids.push(parent_id.to_string());
        cursor = by_id
            .get(parent_id)
            .and_then(|conv| conv.branch_parent_id.as_deref());
    }
    ancestor_ids.reverse();

    for ancestor_id in ancestor_ids {
        select_tree_row_by_id(app, model, &format!("conv:{ancestor_id}"))?;
        let view = app_view(model);
        let ViewContent::TreeList(rows) = view.content else {
            bail!("expected tree list on conversations screen");
        };
        if rows
            .get(view.selected_index)
            .map(|row| row.is_expandable && !row.is_expanded)
            .unwrap_or(false)
        {
            let _ = app.update(Event::ToggleMessage, model, &());
        }
    }

    Ok(())
}

fn select_tree_row_by_id(
    app: &TranscriptBrowser,
    model: &mut Model,
    target_row_id: &str,
) -> Result<()> {
    select_tree_row(app, model, Some(target_row_id), None, None)
}

fn select_tree_row(
    app: &TranscriptBrowser,
    model: &mut Model,
    target_row_id: Option<&str>,
    target_label: Option<&str>,
    target_index: Option<usize>,
) -> Result<()> {
    let view = app_view(model);
    let ViewContent::TreeList(rows) = view.content else {
        bail!("expected tree list on conversations screen");
    };

    let target_idx = if let Some(target_row_id) = target_row_id {
        rows.iter()
            .position(|row| row.id == target_row_id)
            .or_else(|| fallback_tree_row_index(&rows, target_row_id, target_label))
    } else {
        target_label.and_then(|target_label| rows.iter().position(|row| row.label == target_label))
    }
    .or_else(|| {
        target_index.map(|target_index| {
            if rows.is_empty() {
                0
            } else {
                target_index.min(rows.len() - 1)
            }
        })
    })
    .ok_or_else(|| {
        anyhow!(
            "tree row '{}' is not visible",
            target_row_id.unwrap_or("<unknown>")
        )
    })?;

    loop {
        let view = app_view(model);
        let ViewContent::TreeList(_) = view.content else {
            bail!("expected tree list on conversations screen");
        };
        if view.selected_index == target_idx {
            return Ok(());
        }
        if view.selected_index < target_idx {
            let _ = app.update(Event::Down, model, &());
        } else {
            let _ = app.update(Event::Up, model, &());
        }
    }
}

fn fallback_tree_row_index(
    rows: &[shared::TreeRowPreview],
    target_row_id: &str,
    target_label: Option<&str>,
) -> Option<usize> {
    target_label
        .and_then(|target_label| rows.iter().position(|row| row.label == target_label))
        .or_else(|| fallback_summary_row_index(rows, target_row_id))
}

fn fallback_summary_row_index(
    rows: &[shared::TreeRowPreview],
    target_row_id: &str,
) -> Option<usize> {
    let rest = target_row_id.strip_prefix("summary:")?;
    let (conversation_id, _range) = rest.rsplit_once(':')?;
    let prefix = format!("summary:{conversation_id}:");
    rows.iter().position(|row| row.id.starts_with(&prefix))
}

fn find_target_conversation_index(
    model: &Model,
    conversation: Option<&str>,
    selected: usize,
) -> Result<usize> {
    let Screen::Conversations { workspace_idx, .. } = &model.screen else {
        bail!("expected to be on the conversations screen");
    };

    let view_model = app_view(model);
    let ViewContent::TreeList(rows) = view_model.content else {
        bail!("expected tree list on conversations screen");
    };

    if rows.is_empty() {
        let workspace = &model.workspaces[*workspace_idx];
        bail!(
            "workspace '{}' has no conversations for the current filter",
            workspace.display_name
        );
    }

    if conversation.is_none() {
        if selected >= rows.len() {
            bail!(
                "conversation row index {} is out of range for {} visible rows",
                selected,
                rows.len()
            );
        }
        return Ok(selected);
    }

    let workspace = &model.workspaces[*workspace_idx];
    let filtered = filtered_conversations(workspace, model.provider_filter);
    let target_conversation = filtered
        .iter()
        .find(|item| conversation_matches(item.1, conversation.unwrap()))
        .map(|(_, conv)| conv)
        .ok_or_else(|| {
            anyhow!(
                "conversation '{}' was not found in workspace '{}'",
                conversation.unwrap(),
                workspace.display_name
            )
        })?;

    let target_id = format!("conv:{}", target_conversation.id);
    rows.iter()
        .position(|row| row.id == target_id)
        .ok_or_else(|| {
            anyhow!(
                "conversation row '{}' is not visible",
                target_conversation.id
            )
        })
}

fn find_conversation_index(
    workspace: &Workspace,
    provider: Option<ProviderKind>,
    conversation: &str,
) -> Result<usize> {
    workspace
        .conversations
        .iter()
        .enumerate()
        .find(|(_, candidate)| {
            provider.is_none_or(|provider| candidate.provider == provider)
                && conversation_matches(candidate, conversation)
        })
        .map(|(idx, _)| idx)
        .ok_or_else(|| {
            anyhow!(
                "conversation '{}' was not found in workspace '{}'",
                conversation,
                workspace.display_name
            )
        })
}

fn apply_layout(
    _app: &TranscriptBrowser,
    model: &mut Model,
    requested: Option<LayoutMode>,
) -> Result<()> {
    let Some(requested) = requested else {
        return Ok(());
    };

    let Screen::Conversations { .. } = &model.screen else {
        bail!("--layout is only valid on the conversations screen");
    };

    if requested != LayoutMode::Table {
        bail!("only table/tree layout is supported on the conversations screen");
    }

    Ok(())
}

fn apply_expand_all(app: &TranscriptBrowser, model: &mut Model, expand_all: bool) -> Result<()> {
    if !expand_all {
        return Ok(());
    }

    let Screen::Conversations { .. } = &model.screen else {
        bail!("--expand-all is only valid on the conversations screen");
    };

    let mut idx = 0usize;
    loop {
        let view = app_view(model);
        let ViewContent::TreeList(rows) = view.content else {
            bail!("expected tree list on conversations screen");
        };
        if idx >= rows.len() {
            break;
        }

        select_conversation_row_index(app, model, idx)?;
        let current = app_view(model);
        let ViewContent::TreeList(current_rows) = current.content else {
            bail!("expected tree list on conversations screen");
        };
        if current_rows
            .get(current.selected_index)
            .map(|row| row.is_expandable && !row.is_expanded)
            .unwrap_or(false)
        {
            let _ = app.update(Event::ToggleMessage, model, &());
        }
        idx += 1;
    }

    select_conversation_row_index(app, model, 0)?;

    Ok(())
}

fn select_conversation_row_index(
    app: &TranscriptBrowser,
    model: &mut Model,
    target_idx: usize,
) -> Result<()> {
    loop {
        let view = app_view(model);
        let ViewContent::TreeList(rows) = view.content else {
            bail!("expected tree list on conversations screen");
        };
        if target_idx >= rows.len() {
            bail!(
                "conversation row index {} is out of range for {} visible rows",
                target_idx,
                rows.len()
            );
        }
        if view.selected_index == target_idx {
            return Ok(());
        }
        if view.selected_index < target_idx {
            let _ = app.update(Event::Down, model, &());
        } else {
            let _ = app.update(Event::Up, model, &());
        }
    }
}

fn apply_message_index(
    app: &TranscriptBrowser,
    model: &mut Model,
    message_index: usize,
) -> Result<()> {
    if message_index == 0 {
        return Ok(());
    }

    match &model.screen {
        Screen::Conversations { .. } => {
            for _ in 0..message_index {
                let _ = app.update(Event::Down, model, &());
            }
            Ok(())
        }
        Screen::Messages { .. } => {
            for _ in 0..message_index {
                let _ = app.update(Event::MessageDown, model, &());
            }
            Ok(())
        }
        Screen::Workspaces { .. } => {
            bail!("--message-index is not valid on the workspaces screen")
        }
    }
}

fn app_view(model: &Model) -> ViewModel {
    let app = TranscriptBrowser;
    app.view(model)
}

fn filtered_conversations(
    workspace: &Workspace,
    provider_filter: Option<ProviderKind>,
) -> Vec<(usize, &shared::Conversation)> {
    workspace
        .conversations
        .iter()
        .enumerate()
        .filter(|(_, conversation)| match provider_filter {
            Some(provider) => conversation.provider == provider,
            None => true,
        })
        .collect()
}

fn find_workspace_index(workspaces: &[Workspace], target: &str) -> Result<usize> {
    let home_dir = env::var("HOME").unwrap_or_default();
    let normalized_target = normalize_workspace_value(target, &home_dir);

    workspaces
        .iter()
        .enumerate()
        .find(|(_, workspace)| workspace_matches(workspace, target, &normalized_target, &home_dir))
        .map(|(idx, _)| idx)
        .ok_or_else(|| anyhow!("workspace '{}' was not found", target))
}

fn workspace_matches(
    workspace: &Workspace,
    raw_target: &str,
    normalized_target: &str,
    home_dir: &str,
) -> bool {
    if workspace.display_name == raw_target {
        return true;
    }

    if normalize_workspace_value(&workspace.display_name, home_dir) == normalized_target {
        return true;
    }

    if let Some(source_path) = workspace.source_path.as_deref() {
        if source_path == raw_target {
            return true;
        }

        if normalize_workspace_value(source_path, home_dir) == normalized_target {
            return true;
        }
    }

    false
}

fn normalize_workspace_value(value: &str, home_dir: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/") {
        if home_dir.is_empty() {
            value.to_string()
        } else {
            format!("{home_dir}/{rest}")
        }
    } else {
        value.to_string()
    }
}

fn conversation_matches(conversation: &shared::Conversation, target: &str) -> bool {
    conversation.id == target
        || conversation.external_id.as_deref() == Some(target)
        || conversation.display_title() == target
}

pub(crate) fn render_view_model_to_string(
    view_model: &ViewModel,
    width: u16,
    height: u16,
) -> Result<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    let theme = Theme::default();
    let mut list_state = ListState::default();
    let mut table_state = TableState::default();
    list_state.select(Some(view_model.selected_index));
    table_state.select(Some(view_model.selected_index));

    terminal.draw(|f| {
        render::render_ui(f, view_model, &mut list_state, &mut table_state, &theme);
    })?;

    Ok(buffer_to_string(terminal.backend().buffer()))
}

fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area();
    let mut result = String::new();

    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = buffer.get(x, y);
            result.push_str(cell.symbol());
        }
        result.push('\n');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{Conversation, ConversationSegment, Message, MessageKind, Participant};

    fn sample_workspace() -> Workspace {
        Workspace {
            id: "ws-1".into(),
            display_name: "~/projects/transcript-browser".into(),
            source_path: Some("/Users/gaborkerekes/projects/transcript-browser".into()),
            updated_at: 1_000_000,
            conversations: vec![
                Conversation {
                    id: "codex-1".into(),
                    external_id: Some("codex-external".into()),
                    branch_parent_id: None,
                    branch_anchor_message_id: None,
                    title: None,
                    preview: Some("hello from user".into()),
                    provider: ProviderKind::Codex,
                    created_at: 880_000,
                    updated_at: 999_000,
                    segments: vec![
                        ConversationSegment {
                            id: "segment-1".into(),
                            label: "main session".into(),
                            created_at: 880_000,
                            updated_at: 990_000,
                            message_start_idx: 0,
                            message_count: 1,
                        },
                        ConversationSegment {
                            id: "segment-2".into(),
                            label: "rollout 019d1665...".into(),
                            created_at: 995_000,
                            updated_at: 999_000,
                            message_start_idx: 1,
                            message_count: 1,
                        },
                    ],
                    messages: vec![
                        Message {
                            id: None,
                            kind: MessageKind::UserMessage,
                            participant: Participant::User,
                            content: "hello from user".into(),
                            timestamp: Some(880_000),
                            parent_id: None,
                            associated_id: None,
                            depth: 0,
                        },
                        Message {
                            id: None,
                            kind: MessageKind::AssistantMessage,
                            participant: Participant::Assistant {
                                provider: ProviderKind::Codex,
                            },
                            content: "assistant response".into(),
                            timestamp: Some(999_000),
                            parent_id: None,
                            associated_id: None,
                            depth: 0,
                        },
                    ],
                    is_hydrated: true,
                    load_ref: None,
                },
                Conversation {
                    id: "claude-1".into(),
                    external_id: Some("claude-external".into()),
                    branch_parent_id: None,
                    branch_anchor_message_id: None,
                    title: Some("Claude Session".into()),
                    preview: Some("claude content".into()),
                    provider: ProviderKind::ClaudeCode,
                    created_at: 700_000,
                    updated_at: 800_000,
                    segments: vec![],
                    messages: vec![Message {
                        id: None,
                        kind: MessageKind::AssistantMessage,
                        participant: Participant::Assistant {
                            provider: ProviderKind::ClaudeCode,
                        },
                        content: "claude content".into(),
                        timestamp: Some(800_000),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    }],
                    is_hydrated: true,
                    load_ref: None,
                },
            ],
        }
    }

    fn anchored_branch_workspace() -> Workspace {
        Workspace {
            id: "ws-branches".into(),
            display_name: "~/projects/branches".into(),
            source_path: Some("/Users/gaborkerekes/projects/branches".into()),
            updated_at: 1_000_000,
            conversations: vec![
                Conversation {
                    id: "parent".into(),
                    external_id: Some("parent-external".into()),
                    branch_parent_id: None,
                    branch_anchor_message_id: None,
                    title: Some("Parent".into()),
                    preview: Some("parent root".into()),
                    provider: ProviderKind::ClaudeCode,
                    created_at: 900_000,
                    updated_at: 999_000,
                    segments: vec![],
                    messages: vec![
                        Message {
                            id: Some("anchor-msg".into()),
                            kind: MessageKind::UserMessage,
                            participant: Participant::User,
                            content: "anchor".into(),
                            timestamp: Some(900_000),
                            parent_id: None,
                            associated_id: None,
                            depth: 0,
                        },
                        Message {
                            id: Some("reply-msg".into()),
                            kind: MessageKind::AssistantMessage,
                            participant: Participant::Assistant {
                                provider: ProviderKind::ClaudeCode,
                            },
                            content: "reply".into(),
                            timestamp: Some(999_000),
                            parent_id: Some("anchor-msg".into()),
                            associated_id: None,
                            depth: 0,
                        },
                    ],
                    is_hydrated: true,
                    load_ref: None,
                },
                Conversation {
                    id: "child".into(),
                    external_id: Some("child-external".into()),
                    branch_parent_id: Some("parent".into()),
                    branch_anchor_message_id: Some("anchor-msg".into()),
                    title: Some("Child Branch".into()),
                    preview: Some("child root".into()),
                    provider: ProviderKind::ClaudeCode,
                    created_at: 950_000,
                    updated_at: 998_000,
                    segments: vec![],
                    messages: vec![Message {
                        id: Some("child-root".into()),
                        kind: MessageKind::AssistantMessage,
                        participant: Participant::Assistant {
                            provider: ProviderKind::ClaudeCode,
                        },
                        content: "child root".into(),
                        timestamp: Some(998_000),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    }],
                    is_hydrated: true,
                    load_ref: None,
                },
            ],
        }
    }

    #[test]
    fn dump_conversations_table_uses_real_view_model() {
        let output = dump_screen_output(
            &DumpScreenArgs {
                screen_ref: None,
                screen: ScreenTarget::Conversations,
                workspace: Some("~/projects/transcript-browser".into()),
                conversation: None,
                provider: Some(ProviderKind::Codex),
                layout: Some(LayoutMode::Table),
                width: 88,
                height: 10,
                now_ms: Some(1_000_000),
                selected: 0,
                message_index: 0,
                expand_all: false,
            },
            vec![sample_workspace()],
            1_000_000,
        )
        .unwrap();

        assert!(output.contains("Codex"));
        assert!(output.contains("hello from user"));
    }

    #[test]
    fn dump_conversations_tree_can_expand_all_entries() {
        let output = dump_screen_output(
            &DumpScreenArgs {
                screen_ref: None,
                screen: ScreenTarget::Conversations,
                workspace: Some("~/projects/transcript-browser".into()),
                conversation: None,
                provider: Some(ProviderKind::Codex),
                layout: Some(LayoutMode::Table),
                width: 100,
                height: 16,
                now_ms: Some(1_000_000),
                selected: 0,
                message_index: 0,
                expand_all: true,
            },
            vec![sample_workspace()],
            1_000_000,
        )
        .unwrap();

        assert!(output.contains("hello from user"));
        assert!(!output.contains("assistant response"));
    }

    #[test]
    fn dump_conversations_tree_expand_all_reaches_anchored_branch_conversations() {
        let output = dump_screen_output(
            &DumpScreenArgs {
                screen_ref: None,
                screen: ScreenTarget::Conversations,
                workspace: Some("~/projects/branches".into()),
                conversation: Some("child".into()),
                provider: Some(ProviderKind::ClaudeCode),
                layout: Some(LayoutMode::Table),
                width: 100,
                height: 16,
                now_ms: Some(1_000_000),
                selected: 0,
                message_index: 0,
                expand_all: true,
            },
            vec![anchored_branch_workspace()],
            1_000_000,
        )
        .unwrap();

        assert!(output.contains("Parent"));
        assert!(output.contains("anchor"));
        assert!(output.contains("Child Branch"));
    }

    #[test]
    fn dump_conversations_tree_expand_all_uses_visible_row_selection_not_flat_conversation_order() {
        let output = dump_screen_output(
            &DumpScreenArgs {
                screen_ref: None,
                screen: ScreenTarget::Conversations,
                workspace: Some("~/projects/branches".into()),
                conversation: None,
                provider: Some(ProviderKind::ClaudeCode),
                layout: Some(LayoutMode::Table),
                width: 100,
                height: 16,
                now_ms: Some(1_000_000),
                selected: 0,
                message_index: 0,
                expand_all: true,
            },
            vec![anchored_branch_workspace()],
            1_000_000,
        )
        .unwrap();

        assert!(output.contains("Parent"));
        assert!(output.contains("Child Branch"));
    }

    #[test]
    fn dump_messages_screen_renders_message_content() {
        let output = dump_screen_output(
            &DumpScreenArgs {
                screen_ref: None,
                screen: ScreenTarget::Messages,
                workspace: Some("~/projects/transcript-browser".into()),
                conversation: Some("codex-1".into()),
                provider: Some(ProviderKind::Codex),
                layout: None,
                width: 80,
                height: 12,
                now_ms: Some(1_000_000),
                selected: 0,
                message_index: 1,
                expand_all: false,
            },
            vec![sample_workspace()],
            1_000_000,
        )
        .unwrap();

        assert!(output.contains("hello from user"));
        assert!(output.contains("assistant response"));
        assert!(output.contains("Codex"));
    }

    #[test]
    fn dump_screen_rejects_unknown_workspace() {
        let err = dump_screen_output(
            &DumpScreenArgs {
                screen_ref: None,
                screen: ScreenTarget::Conversations,
                workspace: Some("~/projects/missing".into()),
                conversation: None,
                provider: None,
                layout: None,
                width: 80,
                height: 12,
                now_ms: Some(1_000_000),
                selected: 0,
                message_index: 0,
                expand_all: false,
            },
            vec![sample_workspace()],
            1_000_000,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("workspace '~/projects/missing' was not found"));
    }

    #[test]
    fn resolve_screen_ref_falls_back_when_summary_row_id_drifts() {
        let app = TranscriptBrowser;
        let mut model = Model::default();
        let _ = app.update(
            Event::SetWorkspaces(vec![sample_workspace()], 1_000_000),
            &mut model,
            &(),
        );

        resolve_screen_ref(
            &app,
            &mut model,
            &ScreenRef {
                version: 1,
                workspace: Some("~/projects/transcript-browser".into()),
                provider_filter: Some(ProviderKind::Codex),
                width: 100,
                height: 16,
                screen: ScreenRefState::Conversations {
                    selected_row_id: Some("summary:codex-1:99-100".into()),
                    selected_row_label: None,
                    selected_index: 0,
                    expanded_row_ids: vec!["conv:codex-1".into()],
                },
            },
        )
        .unwrap();

        let view = app_view(&model);
        let ViewContent::TreeList(rows) = view.content else {
            panic!("expected tree list");
        };

        assert_eq!(view.selected_index, 0);
        assert_eq!(rows[0].kind, shared::TreeRowKind::Conversation);
    }
}
