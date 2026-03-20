use crate::{Conversation, Message, ProviderKind, Workspace};
use crux_core::{App, Command};
use crux_macros::effect;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MessagePreview {
    pub participant_label: String,
    pub content: String,
    pub is_focused: bool,
    pub is_expanded: bool,
    pub relative_time: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ConversationPreview {
    pub id: String,
    pub title: String,
    pub provider_label: String,
    pub relative_time: String,
    pub snippet: String,
    pub is_selected: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ViewContent {
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    List(Vec<String>),
    MessagesList(Vec<MessagePreview>),
    Split {
        conversations: Vec<ConversationPreview>,
        right_messages: Vec<MessagePreview>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum LayoutMode {
    Table,
    Split,
}

impl Default for LayoutMode {
    fn default() -> Self {
        Self::Table
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct MessageSelectionState {
    pub focused_message_idx: usize,
    pub expanded_messages: Vec<usize>,
}

impl MessageSelectionState {
    fn reset(&mut self) {
        self.focused_message_idx = 0;
        self.expanded_messages.clear();
    }

    fn move_up(&mut self) {
        if self.focused_message_idx > 0 {
            self.focused_message_idx -= 1;
        }
    }

    fn move_down(&mut self, max_messages: usize) {
        if max_messages > 0 && self.focused_message_idx + 1 < max_messages {
            self.focused_message_idx += 1;
        }
    }

    fn toggle_current(&mut self) {
        if let Some(pos) = self
            .expanded_messages
            .iter()
            .position(|&idx| idx == self.focused_message_idx)
        {
            self.expanded_messages.remove(pos);
        } else {
            self.expanded_messages.push(self.focused_message_idx);
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Screen {
    Workspaces {
        selected_workspace: usize,
    },
    Conversations {
        workspace_idx: usize,
        selected_conversation: usize,
        layout_mode: LayoutMode,
        preview_state: MessageSelectionState,
    },
    Messages {
        workspace_idx: usize,
        conv_idx: usize,
        message_state: MessageSelectionState,
    },
}

impl Default for Screen {
    fn default() -> Self {
        Self::Workspaces {
            selected_workspace: 0,
        }
    }
}

#[derive(Default)]
pub struct TranscriptBrowser;

#[derive(Serialize, Deserialize)]
pub struct Model {
    pub workspaces: Vec<Workspace>,
    pub screen: Screen,
    pub provider_filter: Option<ProviderKind>,
    pub current_time: i64,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            workspaces: vec![],
            screen: Screen::default(),
            provider_filter: None,
            current_time: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Event {
    Up,
    Down,
    MessageUp,
    MessageDown,
    ToggleMessage,
    Select,
    Back,
    CycleFilter,
    ToggleLayout,
    SetWorkspaces(Vec<Workspace>, i64),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ViewModel {
    pub title: String,
    pub content: ViewContent,
    pub selected_index: usize,
    pub filter_text: String,
    pub breadcrumb: String,
    pub active_id: Option<String>,
}

#[effect]
pub enum Effect {}

fn format_relative_time(timestamp_ms: i64, now_ms: i64) -> String {
    if timestamp_ms == 0 {
        return "N/A".to_string();
    }

    let diff_ms = now_ms.saturating_sub(timestamp_ms);
    let diff_sec = diff_ms / 1000;

    if diff_sec < 60 {
        return "now".to_string();
    }

    let diff_min = diff_sec / 60;
    if diff_min < 60 {
        return format!("{}m", diff_min);
    }

    let diff_hour = diff_min / 60;
    if diff_hour < 24 {
        return format!("{}h", diff_hour);
    }

    let diff_day = diff_hour / 24;
    if diff_day < 7 {
        return format!("{}d", diff_day);
    }

    let diff_week = diff_day / 7;
    if diff_week < 5 {
        return format!("{}w", diff_week);
    }

    let diff_month = diff_day / 30;
    if diff_month < 12 {
        return format!("{}mo", diff_month);
    }

    let diff_year = diff_day / 365;
    format!("{}y", diff_year)
}

fn clamp_selection(selected: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        selected.min(len - 1)
    }
}

impl TranscriptBrowser {
    fn filtered_conversations<'a>(
        &self,
        workspace: &'a Workspace,
        filter: &Option<ProviderKind>,
    ) -> Vec<(usize, &'a Conversation)> {
        workspace
            .conversations
            .iter()
            .enumerate()
            .filter(|(_, c)| match filter {
                Some(provider) => &c.provider == provider,
                None => true,
            })
            .collect()
    }

    fn visible_conversation_count(&self, model: &Model, workspace_idx: usize) -> usize {
        self.filtered_conversations(&model.workspaces[workspace_idx], &model.provider_filter)
            .len()
    }

    fn visible_message_count_for_selected_conversation(
        &self,
        model: &Model,
        workspace_idx: usize,
        selected_conversation: usize,
    ) -> usize {
        let workspace = &model.workspaces[workspace_idx];
        let filtered = self.filtered_conversations(workspace, &model.provider_filter);
        filtered
            .get(selected_conversation)
            .map(|(_, conversation)| conversation.messages.len())
            .unwrap_or(0)
    }

    fn apply_filter_change(&self, model: &mut Model) {
        let screen = model.screen.clone();
        match screen {
            Screen::Workspaces { .. } => {}
            Screen::Conversations {
                workspace_idx,
                selected_conversation,
                layout_mode,
                ..
            } => {
                let count = self.visible_conversation_count(model, workspace_idx);
                model.screen = Screen::Conversations {
                    workspace_idx,
                    selected_conversation: clamp_selection(selected_conversation, count),
                    layout_mode,
                    preview_state: MessageSelectionState::default(),
                };
            }
            Screen::Messages {
                workspace_idx,
                conv_idx,
                ..
            } => {
                let workspace = &model.workspaces[workspace_idx];
                let filtered = self.filtered_conversations(workspace, &model.provider_filter);
                if let Some(pos) = filtered.iter().position(|(idx, _)| *idx == conv_idx) {
                    model.screen = Screen::Conversations {
                        workspace_idx,
                        selected_conversation: pos,
                        layout_mode: LayoutMode::Table,
                        preview_state: MessageSelectionState::default(),
                    };
                } else {
                    model.screen = Screen::Conversations {
                        workspace_idx,
                        selected_conversation: 0,
                        layout_mode: LayoutMode::Table,
                        preview_state: MessageSelectionState::default(),
                    };
                }
            }
        }
    }

    fn message_previews(
        &self,
        messages: &[Message],
        selection: &MessageSelectionState,
        current_time: i64,
    ) -> Vec<MessagePreview> {
        messages
            .iter()
            .enumerate()
            .map(|(idx, message)| MessagePreview {
                participant_label: message.participant.label(),
                content: message.content.clone(),
                is_focused: idx == selection.focused_message_idx,
                is_expanded: selection.expanded_messages.contains(&idx),
                relative_time: message
                    .timestamp
                    .map(|timestamp| format_relative_time(timestamp, current_time)),
            })
            .collect()
    }
}

impl App for TranscriptBrowser {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Capabilities = ();
    type Effect = Effect;

    fn update(
        &self,
        event: Event,
        model: &mut Model,
        _caps: &Self::Capabilities,
    ) -> Command<Self::Effect, Event> {
        match event {
            Event::Up => match &mut model.screen {
                Screen::Workspaces { selected_workspace } => {
                    if *selected_workspace > 0 {
                        *selected_workspace -= 1;
                    }
                }
                Screen::Conversations {
                    selected_conversation,
                    preview_state,
                    ..
                } => {
                    if *selected_conversation > 0 {
                        *selected_conversation -= 1;
                        preview_state.reset();
                    }
                }
                Screen::Messages { .. } => {}
            },
            Event::Down => match model.screen.clone() {
                Screen::Workspaces { selected_workspace } => {
                    let max_len = model.workspaces.len();
                    if max_len > 0 && selected_workspace + 1 < max_len {
                        model.screen = Screen::Workspaces {
                            selected_workspace: selected_workspace + 1,
                        };
                    }
                }
                Screen::Conversations {
                    workspace_idx,
                    selected_conversation,
                    layout_mode,
                    preview_state,
                } => {
                    let max_len = self.visible_conversation_count(model, workspace_idx);
                    if max_len > 0 && selected_conversation + 1 < max_len {
                        model.screen = Screen::Conversations {
                            workspace_idx,
                            selected_conversation: selected_conversation + 1,
                            layout_mode,
                            preview_state: MessageSelectionState::default(),
                        };
                    } else {
                        model.screen = Screen::Conversations {
                            workspace_idx,
                            selected_conversation,
                            layout_mode,
                            preview_state,
                        };
                    }
                }
                Screen::Messages { .. } => {}
            },
            Event::MessageUp => match &mut model.screen {
                Screen::Conversations { preview_state, .. } => preview_state.move_up(),
                Screen::Messages { message_state, .. } => message_state.move_up(),
                Screen::Workspaces { .. } => {}
            },
            Event::MessageDown => match model.screen.clone() {
                Screen::Conversations {
                    workspace_idx,
                    selected_conversation,
                    layout_mode,
                    preview_state,
                } => {
                    let max_messages = self.visible_message_count_for_selected_conversation(
                        model,
                        workspace_idx,
                        selected_conversation,
                    );
                    let mut next_preview_state = preview_state;
                    next_preview_state.move_down(max_messages);
                    model.screen = Screen::Conversations {
                        workspace_idx,
                        selected_conversation,
                        layout_mode,
                        preview_state: next_preview_state,
                    };
                }
                Screen::Messages {
                    workspace_idx,
                    conv_idx,
                    message_state,
                } => {
                    let max_messages = model.workspaces[workspace_idx].conversations[conv_idx]
                        .messages
                        .len();
                    let mut next_message_state = message_state;
                    next_message_state.move_down(max_messages);
                    model.screen = Screen::Messages {
                        workspace_idx,
                        conv_idx,
                        message_state: next_message_state,
                    };
                }
                Screen::Workspaces { .. } => {}
            },
            Event::ToggleMessage => match &mut model.screen {
                Screen::Conversations { preview_state, .. } => preview_state.toggle_current(),
                Screen::Messages { message_state, .. } => message_state.toggle_current(),
                Screen::Workspaces { .. } => {}
            },
            Event::Select => {
                let screen = model.screen.clone();
                match screen {
                    Screen::Workspaces { selected_workspace } => {
                        if !model.workspaces.is_empty() {
                            model.screen = Screen::Conversations {
                                workspace_idx: selected_workspace,
                                selected_conversation: 0,
                                layout_mode: LayoutMode::Table,
                                preview_state: MessageSelectionState::default(),
                            };
                        }
                    }
                    Screen::Conversations {
                        workspace_idx,
                        selected_conversation,
                        ..
                    } => {
                        let workspace = &model.workspaces[workspace_idx];
                        let filtered =
                            self.filtered_conversations(workspace, &model.provider_filter);
                        if let Some((actual_conv_idx, _)) = filtered.get(selected_conversation) {
                            model.screen = Screen::Messages {
                                workspace_idx,
                                conv_idx: *actual_conv_idx,
                                message_state: MessageSelectionState::default(),
                            };
                        }
                    }
                    Screen::Messages { .. } => {}
                }
            }
            Event::Back => {
                let screen = model.screen.clone();
                match screen {
                    Screen::Workspaces { .. } => {}
                    Screen::Conversations { workspace_idx, .. } => {
                        model.screen = Screen::Workspaces {
                            selected_workspace: workspace_idx,
                        };
                    }
                    Screen::Messages {
                        workspace_idx,
                        conv_idx,
                        ..
                    } => {
                        let workspace = &model.workspaces[workspace_idx];
                        let filtered =
                            self.filtered_conversations(workspace, &model.provider_filter);
                        let selected_conversation = filtered
                            .iter()
                            .position(|(idx, _)| *idx == conv_idx)
                            .unwrap_or(0);
                        model.screen = Screen::Conversations {
                            workspace_idx,
                            selected_conversation,
                            layout_mode: LayoutMode::Table,
                            preview_state: MessageSelectionState::default(),
                        };
                    }
                }
            }
            Event::CycleFilter => {
                model.provider_filter = match model.provider_filter {
                    None => Some(ProviderKind::ClaudeCode),
                    Some(ProviderKind::ClaudeCode) => Some(ProviderKind::Codex),
                    Some(ProviderKind::Codex) => None,
                };
                self.apply_filter_change(model);
            }
            Event::ToggleLayout => {
                if let Screen::Conversations { layout_mode, .. } = &mut model.screen {
                    *layout_mode = match layout_mode {
                        LayoutMode::Table => LayoutMode::Split,
                        LayoutMode::Split => LayoutMode::Table,
                    };
                }
            }
            Event::SetWorkspaces(workspaces, current_time) => {
                model.workspaces = workspaces;
                model.current_time = current_time;
                model.screen = Screen::Workspaces {
                    selected_workspace: 0,
                };
            }
        }

        Command::done()
    }

    fn view(&self, model: &Model) -> ViewModel {
        let filter_text = match &model.provider_filter {
            None => "Filter: All Providers [F to cycle]".to_string(),
            Some(provider) => format!("Filter: {} [F to cycle]", provider),
        };

        match &model.screen {
            Screen::Workspaces { selected_workspace } => ViewModel {
                title: "Workspaces".to_string(),
                breadcrumb: "Workspaces".to_string(),
                active_id: None,
                content: ViewContent::Table {
                    headers: vec!["Workspace".into(), "Convs".into(), "Last Active".into()],
                    rows: model
                        .workspaces
                        .iter()
                        .map(|workspace| {
                            let conv_count = format!("{} convs", workspace.conversations.len());
                            let rel_time =
                                format_relative_time(workspace.updated_at, model.current_time);
                            vec![workspace.display_name.clone(), conv_count, rel_time]
                        })
                        .collect(),
                },
                selected_index: *selected_workspace,
                filter_text,
            },
            Screen::Conversations {
                workspace_idx,
                selected_conversation,
                layout_mode,
                preview_state,
            } => {
                let workspace = &model.workspaces[*workspace_idx];
                let filtered = self.filtered_conversations(workspace, &model.provider_filter);
                let selected = clamp_selection(*selected_conversation, filtered.len());

                let headers = vec![
                    "Conversation".into(),
                    "Provider".into(),
                    "First Active".into(),
                    "Last Active".into(),
                ];

                let rows: Vec<Vec<String>> = filtered
                    .iter()
                    .map(|(_, conversation)| {
                        vec![
                            conversation.display_title(),
                            conversation.provider.to_string(),
                            format_relative_time(conversation.created_at, model.current_time),
                            format_relative_time(conversation.updated_at, model.current_time),
                        ]
                    })
                    .collect();

                let filter_text = format!("{} | [~] Toggle Layout", filter_text);
                let active_id = filtered
                    .get(selected)
                    .map(|(_, conversation)| conversation.id.clone());

                if *layout_mode == LayoutMode::Split {
                    let right_messages = filtered
                        .get(selected)
                        .map(|(_, conversation)| {
                            self.message_previews(
                                &conversation.messages,
                                preview_state,
                                model.current_time,
                            )
                        })
                        .unwrap_or_default();

                    let conversations = filtered
                        .iter()
                        .enumerate()
                        .map(|(idx, (_, conversation))| ConversationPreview {
                            id: conversation.id.clone(),
                            title: conversation.display_title(),
                            provider_label: conversation.provider.to_string(),
                            relative_time: format_relative_time(
                                conversation.updated_at,
                                model.current_time,
                            ),
                            snippet: conversation
                                .messages
                                .first()
                                .map(|message| {
                                    message.content.lines().next().unwrap_or("").to_string()
                                })
                                .unwrap_or_default(),
                            is_selected: idx == selected,
                        })
                        .collect();

                    ViewModel {
                        title: format!("Conversations in '{}'", workspace.display_name),
                        breadcrumb: format!("Workspaces > {}", workspace.display_name),
                        active_id: active_id.clone(),
                        content: ViewContent::Split {
                            conversations,
                            right_messages,
                        },
                        selected_index: selected,
                        filter_text,
                    }
                } else {
                    ViewModel {
                        title: format!("Conversations in '{}'", workspace.display_name),
                        breadcrumb: format!("Workspaces > {}", workspace.display_name),
                        active_id,
                        content: ViewContent::Table { headers, rows },
                        selected_index: selected,
                        filter_text,
                    }
                }
            }
            Screen::Messages {
                workspace_idx,
                conv_idx,
                message_state,
            } => {
                let workspace = &model.workspaces[*workspace_idx];
                let conversation = &workspace.conversations[*conv_idx];
                let messages = self.message_previews(
                    &conversation.messages,
                    message_state,
                    model.current_time,
                );

                ViewModel {
                    title: format!("Messages in '{}'", conversation.display_title()),
                    breadcrumb: format!(
                        "Workspaces > {} > {}",
                        workspace.display_name,
                        conversation.display_title()
                    ),
                    active_id: Some(conversation.id.clone()),
                    content: ViewContent::MessagesList(messages),
                    selected_index: message_state.focused_message_idx,
                    filter_text: format!("{} | [e] Expand message", filter_text),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Participant;
    use crux_core::testing::AppTester;

    fn sample_message(content: &str) -> Message {
        Message {
            id: None,
            participant: Participant::User,
            content: content.into(),
            timestamp: Some(1000),
        }
    }

    fn sample_conversation(id: &str, provider: ProviderKind, content: &str) -> Conversation {
        Conversation {
            id: id.into(),
            external_id: None,
            title: None,
            provider,
            created_at: 1000,
            updated_at: 1000,
            messages: vec![sample_message(content)],
        }
    }

    fn sample_workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "Test Workspace".into(),
            source_path: Some("~/test".into()),
            updated_at: 1000,
            conversations: vec![sample_conversation(
                "conv-1",
                ProviderKind::ClaudeCode,
                "Hello",
            )],
        }
    }

    #[test]
    fn test_navigation_flow() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut model = Model {
            workspaces: vec![sample_workspace()],
            current_time: 2000,
            ..Default::default()
        };

        assert_eq!(
            model.screen,
            Screen::Workspaces {
                selected_workspace: 0
            }
        );
        assert_eq!(app.view(&model).title, "Workspaces");

        let _ = app.update(Event::Select, &mut model);
        assert_eq!(
            model.screen,
            Screen::Conversations {
                workspace_idx: 0,
                selected_conversation: 0,
                layout_mode: LayoutMode::Table,
                preview_state: MessageSelectionState::default()
            }
        );
        assert_eq!(app.view(&model).title, "Conversations in 'Test Workspace'");

        let _ = app.update(Event::Select, &mut model);
        assert_eq!(
            model.screen,
            Screen::Messages {
                workspace_idx: 0,
                conv_idx: 0,
                message_state: MessageSelectionState::default()
            }
        );
        assert_eq!(app.view(&model).title, "Messages in 'Hello'");

        let _ = app.update(Event::Back, &mut model);
        assert_eq!(
            model.screen,
            Screen::Conversations {
                workspace_idx: 0,
                selected_conversation: 0,
                layout_mode: LayoutMode::Table,
                preview_state: MessageSelectionState::default()
            }
        );
    }

    #[test]
    fn test_filtering_clamps_conversation_selection() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut workspace = sample_workspace();
        workspace
            .conversations
            .push(sample_conversation("conv-2", ProviderKind::Codex, "World"));
        let mut model = Model {
            workspaces: vec![workspace],
            screen: Screen::Conversations {
                workspace_idx: 0,
                selected_conversation: 1,
                layout_mode: LayoutMode::Table,
                preview_state: MessageSelectionState::default(),
            },
            ..Default::default()
        };

        let _ = app.update(Event::CycleFilter, &mut model);
        assert_eq!(model.provider_filter, Some(ProviderKind::ClaudeCode));
        assert_eq!(
            model.screen,
            Screen::Conversations {
                workspace_idx: 0,
                selected_conversation: 0,
                layout_mode: LayoutMode::Table,
                preview_state: MessageSelectionState::default()
            }
        );

        let view = app.view(&model);
        if let ViewContent::Table { rows, .. } = view.content {
            assert_eq!(rows.len(), 1);
        } else {
            panic!("Expected Table");
        }
    }

    #[test]
    fn test_filtering_from_messages_returns_to_conversations_if_hidden() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut workspace = sample_workspace();
        workspace
            .conversations
            .push(sample_conversation("conv-2", ProviderKind::Codex, "World"));
        let mut model = Model {
            workspaces: vec![workspace],
            screen: Screen::Messages {
                workspace_idx: 0,
                conv_idx: 1,
                message_state: MessageSelectionState::default(),
            },
            ..Default::default()
        };

        let _ = app.update(Event::CycleFilter, &mut model);

        assert_eq!(model.provider_filter, Some(ProviderKind::ClaudeCode));
        assert_eq!(
            model.screen,
            Screen::Conversations {
                workspace_idx: 0,
                selected_conversation: 0,
                layout_mode: LayoutMode::Table,
                preview_state: MessageSelectionState::default()
            }
        );
    }
}
