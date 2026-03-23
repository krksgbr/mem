use crate::{
    args::{DumpScreenArgs, ScreenTarget},
    hydration, render,
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
    Event, LayoutMode, Model, ProviderKind, Screen, TranscriptBrowser, ViewModel, Workspace,
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
    resolve_screen(&app, &mut model, args)?;
    hydration::hydrate_visible_conversation(&mut model)?;

    let view_model = app.view(&model);
    render_view_model_to_string(&view_model, args.width, args.height)
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

            apply_expand_all(app, model, args.expand_all)?;
            apply_conversation_selection(app, model, args.conversation.as_deref(), args.selected)?;
            apply_layout(app, model, args.layout.clone())?;
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
    let target_idx = find_target_conversation_index(model, conversation, selected)?;

    while let Screen::Conversations {
        selected_conversation,
        ..
    } = &model.screen
    {
        if *selected_conversation == target_idx {
            return Ok(());
        }
        let _ = app.update(Event::Down, model, &());
    }

    bail!("expected to be on the conversations screen")
}

fn find_target_conversation_index(
    model: &Model,
    conversation: Option<&str>,
    selected: usize,
) -> Result<usize> {
    let Screen::Conversations { workspace_idx, .. } = &model.screen else {
        bail!("expected to be on the conversations screen");
    };

    let workspace = &model.workspaces[*workspace_idx];
    let filtered = filtered_conversations(workspace, model.provider_filter);

    if filtered.is_empty() {
        bail!(
            "workspace '{}' has no conversations for the current filter",
            workspace.display_name
        );
    }

    if let Some(conversation) = conversation {
        return filtered
            .iter()
            .enumerate()
            .find(|(_, item)| conversation_matches(item.1, conversation))
            .map(|(idx, _)| idx)
            .ok_or_else(|| {
                anyhow!(
                    "conversation '{}' was not found in workspace '{}'",
                    conversation,
                    workspace.display_name
                )
            });
    }

    if selected < filtered.len() {
        Ok(selected)
    } else {
        bail!(
            "conversation index {} is out of range for {} visible conversations",
            selected,
            filtered.len()
        )
    }
}

fn apply_layout(
    app: &TranscriptBrowser,
    model: &mut Model,
    requested: Option<LayoutMode>,
) -> Result<()> {
    let Some(requested) = requested else {
        return Ok(());
    };

    let Screen::Conversations { layout_mode, .. } = &model.screen else {
        bail!("--layout is only valid on the conversations screen");
    };

    if *layout_mode != requested {
        let _ = app.update(Event::ToggleLayout, model, &());
    }

    Ok(())
}

fn apply_expand_all(app: &TranscriptBrowser, model: &mut Model, expand_all: bool) -> Result<()> {
    if !expand_all {
        return Ok(());
    }

    let Screen::Conversations { workspace_idx, .. } = &model.screen else {
        bail!("--expand-all is only valid on the conversations screen");
    };

    let conversation_count =
        filtered_conversations(&model.workspaces[*workspace_idx], model.provider_filter).len();
    for _ in 0..conversation_count {
        let _ = app.update(Event::ToggleMessage, model, &());
        let _ = app.update(Event::Down, model, &());
    }

    while let Screen::Conversations {
        selected_conversation,
        ..
    } = &model.screen
    {
        if *selected_conversation == 0 {
            break;
        }
        let _ = app.update(Event::Up, model, &());
    }

    Ok(())
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
                let _ = app.update(Event::MessageDown, model, &());
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

    #[test]
    fn dump_conversations_table_uses_real_view_model() {
        let output = dump_screen_output(
            &DumpScreenArgs {
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

        assert!(output.contains("Conversation"));
        assert!(output.contains("Provider"));
        assert!(output.contains("Codex"));
        assert!(output.contains("hello from user"));
    }

    #[test]
    fn dump_conversations_table_can_expand_all_segments() {
        let output = dump_screen_output(
            &DumpScreenArgs {
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

        assert!(output.contains("main session"));
        assert!(output.contains("rollout 019d1665..."));
    }

    #[test]
    fn dump_messages_screen_renders_message_content() {
        let output = dump_screen_output(
            &DumpScreenArgs {
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
}
