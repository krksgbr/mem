use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use shared::{Model, ProviderKind, Screen, ViewContent, ViewModel, Workspace};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenRef {
    pub version: u8,
    pub workspace: Option<String>,
    pub provider_filter: Option<ProviderKind>,
    pub width: u16,
    pub height: u16,
    pub screen: ScreenRefState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScreenRefState {
    Workspaces {
        selected_workspace: Option<String>,
    },
    Conversations {
        #[serde(default)]
        selected_row_id: Option<String>,
        #[serde(default)]
        selected_row_label: Option<String>,
        #[serde(default)]
        selected_index: usize,
        expanded_row_ids: Vec<String>,
    },
    Messages {
        conversation: String,
        focused_message_idx: usize,
        expanded_message_indices: Vec<usize>,
    },
}

pub fn capture_screen_ref(
    model: &Model,
    view_model: &ViewModel,
    width: u16,
    height: u16,
) -> Result<ScreenRef> {
    let workspace = active_workspace_key(model);
    let screen = match (&model.screen, &view_model.content) {
        (Screen::Workspaces { selected_workspace }, _) => ScreenRefState::Workspaces {
            selected_workspace: model.workspaces.get(*selected_workspace).map(workspace_key),
        },
        (
            Screen::Conversations {
                selected_row,
                expanded_ids,
                ..
            },
            ViewContent::TreeList(rows),
        ) => ScreenRefState::Conversations {
            selected_row_id: rows.get(*selected_row).map(|row| row.id.clone()),
            selected_row_label: rows.get(*selected_row).map(|row| row.label.clone()),
            selected_index: *selected_row,
            expanded_row_ids: expanded_ids.clone(),
        },
        (
            Screen::Messages {
                conv_idx,
                message_state,
                ..
            },
            _,
        ) => {
            let workspace_idx = workspace_index(model)?;
            let conversation = model.workspaces[workspace_idx]
                .conversations
                .get(*conv_idx)
                .ok_or_else(|| anyhow!("message screen references missing conversation index"))?;
            ScreenRefState::Messages {
                conversation: conversation
                    .external_id
                    .clone()
                    .unwrap_or_else(|| conversation.id.clone()),
                focused_message_idx: message_state.focused_message_idx,
                expanded_message_indices: message_state.expanded_messages.clone(),
            }
        }
        (Screen::Conversations { .. }, _) => {
            bail!("screen ref capture expected tree list content for conversations screen")
        }
    };

    Ok(ScreenRef {
        version: 1,
        workspace,
        provider_filter: model.provider_filter,
        width,
        height,
        screen,
    })
}

pub fn write_screen_ref(path: &Path, screen_ref: &ScreenRef) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(screen_ref)?)?;
    Ok(())
}

pub fn load_screen_ref(path: &Path) -> Result<ScreenRef> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn workspace_key(workspace: &Workspace) -> String {
    workspace
        .source_path
        .clone()
        .unwrap_or_else(|| workspace.display_name.clone())
}

fn active_workspace_key(model: &Model) -> Option<String> {
    workspace_index(model)
        .ok()
        .and_then(|idx| model.workspaces.get(idx))
        .map(workspace_key)
}

fn workspace_index(model: &Model) -> Result<usize> {
    match model.screen {
        Screen::Workspaces { selected_workspace } => Ok(selected_workspace),
        Screen::Conversations { workspace_idx, .. } | Screen::Messages { workspace_idx, .. } => {
            Ok(workspace_idx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{
        Conversation, ConversationSegment, Message, MessageKind, Participant, TreeRowKind,
        TreeRowPreview, ViewModel,
    };

    fn message(content: &str) -> Message {
        Message {
            id: Some("msg-1".into()),
            kind: MessageKind::UserMessage,
            participant: Participant::User,
            content: content.into(),
            timestamp: Some(1000),
            parent_id: None,
            associated_id: None,
            depth: 0,
        }
    }

    fn conversation() -> Conversation {
        Conversation {
            id: "conv-1".into(),
            external_id: Some("external-conv-1".into()),
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: Some("Test".into()),
            preview: Some("preview".into()),
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: Vec::<ConversationSegment>::new(),
            messages: vec![message("hello")],
            is_hydrated: true,
            load_ref: None,
        }
    }

    fn workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "~/test".into(),
            source_path: Some("/tmp/test".into()),
            updated_at: 0,
            conversations: vec![conversation()],
        }
    }

    #[test]
    fn capture_messages_screen_ref_prefers_workspace_source_path() {
        let model = Model {
            workspaces: vec![workspace()],
            screen: Screen::Messages {
                workspace_idx: 0,
                conv_idx: 0,
                message_state: shared::MessageSelectionState {
                    focused_message_idx: 0,
                    expanded_messages: vec![0],
                },
                return_selected_row: 0,
                return_expanded_ids: Vec::new(),
            },
            provider_filter: Some(ProviderKind::ClaudeCode),
            current_time: 0,
            status_text: None,
        };
        let view = ViewModel {
            title: "Messages".into(),
            content: ViewContent::MessagesList(Vec::new()),
            selected_index: 0,
            filter_text: String::new(),
            status_text: None,
            breadcrumb: String::new(),
            active_id: Some("external-conv-1".into()),
        };

        let captured = capture_screen_ref(&model, &view, 120, 40).unwrap();
        assert_eq!(captured.workspace, Some("/tmp/test".into()));
        assert_eq!(captured.provider_filter, Some(ProviderKind::ClaudeCode));
        assert_eq!(
            captured.screen,
            ScreenRefState::Messages {
                conversation: "external-conv-1".into(),
                focused_message_idx: 0,
                expanded_message_indices: vec![0],
            }
        );
    }

    #[test]
    fn capture_conversations_screen_ref_uses_selected_tree_row_id() {
        let model = Model {
            workspaces: vec![workspace()],
            screen: Screen::Conversations {
                workspace_idx: 0,
                selected_row: 1,
                expanded_ids: vec!["conv:conv-1".into()],
            },
            provider_filter: None,
            current_time: 0,
            status_text: None,
        };
        let view = ViewModel {
            title: "Conversations".into(),
            content: ViewContent::TreeList(vec![
                TreeRowPreview {
                    id: "conv:conv-1".into(),
                    kind: TreeRowKind::Conversation,
                    label: "Test".into(),
                    secondary: None,
                    depth: 0,
                    is_selected: false,
                    is_expandable: true,
                    is_expanded: true,
                },
                TreeRowPreview {
                    id: "summary:conv-1:0-1".into(),
                    kind: TreeRowKind::Summary,
                    label: "summary".into(),
                    secondary: None,
                    depth: 1,
                    is_selected: true,
                    is_expandable: false,
                    is_expanded: false,
                },
            ]),
            selected_index: 1,
            filter_text: String::new(),
            status_text: None,
            breadcrumb: String::new(),
            active_id: Some("summary:conv-1:0-1".into()),
        };

        let captured = capture_screen_ref(&model, &view, 120, 40).unwrap();
        assert_eq!(
            captured.screen,
            ScreenRefState::Conversations {
                selected_row_id: Some("summary:conv-1:0-1".into()),
                selected_row_label: Some("summary".into()),
                selected_index: 1,
                expanded_row_ids: vec!["conv:conv-1".into()],
            }
        );
    }
}
