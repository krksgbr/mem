use crate::{Conversation, Message, MessageKind, ProviderKind, Workspace};
use crux_core::{App, Command};
use crux_macros::effect;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MessagePreview {
    pub kind: MessageKind,
    pub participant_label: String,
    pub content: String,
    pub depth: usize,
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
pub struct TreeRowPreview {
    pub id: String,
    pub label: String,
    pub secondary: Option<String>,
    pub depth: usize,
    pub is_selected: bool,
    pub is_expandable: bool,
    pub is_expanded: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ViewContent {
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    List(Vec<String>),
    TreeList(Vec<TreeRowPreview>),
    HistoryList(Vec<MessagePreview>),
    MessagesList(Vec<MessagePreview>),
    Split {
        conversations: Vec<ConversationPreview>,
        right_messages: Vec<MessagePreview>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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
        selected_row: usize,
        expanded_ids: Vec<String>,
    },
    Messages {
        workspace_idx: usize,
        conv_idx: usize,
        message_state: MessageSelectionState,
        return_selected_row: usize,
        return_expanded_ids: Vec<String>,
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

#[derive(Clone, Copy)]
enum BrowserNode<'a> {
    Conversation {
        conv_idx: usize,
        conversation: &'a Conversation,
    },
    Entry {
        conv_idx: usize,
        conversation: &'a Conversation,
        message_idx: usize,
        message: &'a Message,
    },
}

#[derive(Clone, Copy)]
struct BrowserTreeRow<'a> {
    depth: usize,
    node: BrowserNode<'a>,
    is_expandable: bool,
    is_expanded: bool,
}

pub fn format_relative_time(timestamp_ms: i64, now_ms: i64) -> String {
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

fn first_non_empty_line(message: &Message) -> Option<&str> {
    message
        .content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
}

fn browser_conversation_row_id(conversation_id: &str) -> String {
    format!("conv:{conversation_id}")
}

fn browser_entry_row_id(
    conversation_id: &str,
    message_idx: usize,
    message_id: Option<&str>,
) -> String {
    match message_id {
        Some(id) => format!("entry:{conversation_id}:{id}"),
        None => format!("entry:{conversation_id}:{message_idx}"),
    }
}

fn message_kind_label(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::UserMessage => "You",
        MessageKind::AssistantMessage => "Assistant",
        MessageKind::ToolCall => "Tool Call",
        MessageKind::ToolResult => "Tool Result",
        MessageKind::Thinking => "Thinking",
        MessageKind::Summary => "Summary",
        MessageKind::Compaction => "Compaction",
        MessageKind::Label => "Label",
        MessageKind::MetadataChange => "Metadata",
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

    fn browser_rows<'a>(
        &self,
        model: &'a Model,
        workspace_idx: usize,
        expanded_ids: &[String],
    ) -> Vec<BrowserTreeRow<'a>> {
        let workspace = &model.workspaces[workspace_idx];
        let conversations = self.filtered_conversations(workspace, &model.provider_filter);
        let visible_ids = conversations
            .iter()
            .map(|(_, conversation)| conversation.id.as_str())
            .collect::<HashSet<_>>();
        let mut children_by_parent: HashMap<Option<&str>, Vec<(usize, &'a Conversation)>> =
            HashMap::new();
        let mut children_by_anchor: HashMap<(&str, &str), Vec<(usize, &'a Conversation)>> =
            HashMap::new();

        for (conv_idx, conversation) in conversations.iter().copied() {
            let parent = conversation
                .branch_parent_id
                .as_deref()
                .filter(|parent_id| visible_ids.contains(parent_id));
            if let (Some(parent_id), Some(anchor_id)) =
                (parent, conversation.branch_anchor_message_id.as_deref())
            {
                children_by_anchor
                    .entry((parent_id, anchor_id))
                    .or_default()
                    .push((conv_idx, conversation));
            } else {
                children_by_parent
                    .entry(parent)
                    .or_default()
                    .push((conv_idx, conversation));
            }
        }

        let mut rows = Vec::new();
        if let Some(roots) = children_by_parent.get(&None) {
            for (conv_idx, conversation) in roots.iter().copied() {
                self.push_conversation_rows(
                    conv_idx,
                    conversation,
                    0,
                    expanded_ids,
                    &children_by_parent,
                    &children_by_anchor,
                    &mut rows,
                );
            }
        }

        rows
    }

    fn push_conversation_rows<'a>(
        &self,
        conv_idx: usize,
        conversation: &'a Conversation,
        depth: usize,
        expanded_ids: &[String],
        children_by_parent: &HashMap<Option<&'a str>, Vec<(usize, &'a Conversation)>>,
        children_by_anchor: &HashMap<(&'a str, &'a str), Vec<(usize, &'a Conversation)>>,
        rows: &mut Vec<BrowserTreeRow<'a>>,
    ) {
        let conv_node_id = browser_conversation_row_id(&conversation.id);
        let conv_is_expanded = expanded_ids.iter().any(|id| id == &conv_node_id);
        let child_conversations = children_by_parent
            .get(&Some(conversation.id.as_str()))
            .cloned()
            .unwrap_or_default();

        rows.push(BrowserTreeRow {
            depth,
            node: BrowserNode::Conversation {
                conv_idx,
                conversation,
            },
            is_expandable: (conversation.has_loaded_messages()
                && !conversation.messages.is_empty())
                || !child_conversations.is_empty(),
            is_expanded: conv_is_expanded,
        });

        if !conv_is_expanded {
            return;
        }

        if conversation.has_loaded_messages() {
            self.push_entry_rows(
                conversation,
                conv_idx,
                expanded_ids,
                depth + 1,
                children_by_parent,
                children_by_anchor,
                rows,
            );
        }

        for (child_idx, child_conversation) in child_conversations {
            self.push_conversation_rows(
                child_idx,
                child_conversation,
                depth + 1,
                expanded_ids,
                children_by_parent,
                children_by_anchor,
                rows,
            );
        }
    }

    fn push_entry_rows<'a>(
        &self,
        conversation: &'a Conversation,
        conv_idx: usize,
        expanded_ids: &[String],
        base_depth: usize,
        conversation_children_by_parent: &HashMap<Option<&'a str>, Vec<(usize, &'a Conversation)>>,
        conversation_children_by_anchor: &HashMap<
            (&'a str, &'a str),
            Vec<(usize, &'a Conversation)>,
        >,
        rows: &mut Vec<BrowserTreeRow<'a>>,
    ) {
        let mut message_ids = HashSet::new();
        for message in &conversation.messages {
            if let Some(id) = message.id.as_deref() {
                message_ids.insert(id.to_string());
            }
        }

        let mut children_by_parent: HashMap<Option<String>, Vec<usize>> = HashMap::new();
        let mut root_indices = Vec::new();

        for (idx, message) in conversation.messages.iter().enumerate() {
            let has_known_parent = message
                .parent_id
                .as_deref()
                .map(|id| id != message.id.as_deref().unwrap_or("") && message_ids.contains(id))
                .unwrap_or(false);

            let is_structural_child = message.depth > 0 && has_known_parent;

            if is_structural_child {
                children_by_parent
                    .entry(message.parent_id.clone())
                    .or_default()
                    .push(idx);
            } else {
                root_indices.push(idx);
            }
        }

        let mut visited = HashSet::new();
        for root_idx in root_indices {
            self.push_entry_subtree(
                conversation,
                conv_idx,
                root_idx,
                base_depth,
                expanded_ids,
                &children_by_parent,
                conversation_children_by_parent,
                conversation_children_by_anchor,
                &mut visited,
                rows,
            );
        }

        for idx in 0..conversation.messages.len() {
            if !visited.contains(&idx) {
                self.push_entry_subtree(
                    conversation,
                    conv_idx,
                    idx,
                    base_depth,
                    expanded_ids,
                    &children_by_parent,
                    conversation_children_by_parent,
                    conversation_children_by_anchor,
                    &mut visited,
                    rows,
                );
            }
        }
    }

    fn push_entry_subtree<'a>(
        &self,
        conversation: &'a Conversation,
        conv_idx: usize,
        message_idx: usize,
        depth: usize,
        expanded_ids: &[String],
        children_by_parent: &HashMap<Option<String>, Vec<usize>>,
        conversation_children_by_parent: &HashMap<Option<&'a str>, Vec<(usize, &'a Conversation)>>,
        conversation_children_by_anchor: &HashMap<
            (&'a str, &'a str),
            Vec<(usize, &'a Conversation)>,
        >,
        visited: &mut HashSet<usize>,
        rows: &mut Vec<BrowserTreeRow<'a>>,
    ) {
        if !visited.insert(message_idx) {
            return;
        }

        let message = &conversation.messages[message_idx];
        let node_id = browser_entry_row_id(&conversation.id, message_idx, message.id.as_deref());
        let child_indices = children_by_parent
            .get(&message.id)
            .cloned()
            .unwrap_or_default();
        let branch_children = message
            .id
            .as_deref()
            .and_then(|message_id| {
                conversation_children_by_anchor.get(&(conversation.id.as_str(), message_id))
            })
            .cloned()
            .unwrap_or_default();
        let is_expanded = expanded_ids.iter().any(|id| id == &node_id);

        rows.push(BrowserTreeRow {
            depth,
            node: BrowserNode::Entry {
                conv_idx,
                conversation,
                message_idx,
                message,
            },
            is_expandable: !child_indices.is_empty() || !branch_children.is_empty(),
            is_expanded,
        });

        for (child_idx, child_conversation) in branch_children.iter().copied() {
            self.push_conversation_rows(
                child_idx,
                child_conversation,
                depth + 1,
                expanded_ids,
                conversation_children_by_parent,
                conversation_children_by_anchor,
                rows,
            );
        }

        if is_expanded {
            for child_idx in child_indices {
                self.push_entry_subtree(
                    conversation,
                    conv_idx,
                    child_idx,
                    depth + 1,
                    expanded_ids,
                    children_by_parent,
                    conversation_children_by_parent,
                    conversation_children_by_anchor,
                    visited,
                    rows,
                );
            }
        }
    }

    fn browser_row_view(
        &self,
        row: BrowserTreeRow<'_>,
        is_selected: bool,
        current_time: i64,
    ) -> TreeRowPreview {
        match row.node {
            BrowserNode::Conversation { conversation, .. } => TreeRowPreview {
                id: browser_conversation_row_id(&conversation.id),
                label: conversation.display_title(),
                secondary: Some(format!(
                    "{} {}",
                    conversation.provider,
                    format_relative_time(conversation.updated_at, current_time)
                )),
                depth: row.depth,
                is_selected,
                is_expandable: row.is_expandable,
                is_expanded: row.is_expanded,
            },
            BrowserNode::Entry {
                message,
                message_idx,
                conversation,
                ..
            } => {
                let label = first_non_empty_line(message)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| message_kind_label(message.kind).to_string());

                let secondary = Some(match message.timestamp {
                    Some(timestamp) => format!(
                        "{} {}",
                        message.participant.label(),
                        format_relative_time(timestamp, current_time)
                    ),
                    None => message.participant.label(),
                });

                TreeRowPreview {
                    id: browser_entry_row_id(&conversation.id, message_idx, message.id.as_deref()),
                    label,
                    secondary,
                    depth: row.depth,
                    is_selected,
                    is_expandable: row.is_expandable,
                    is_expanded: row.is_expanded,
                }
            }
        }
    }

    fn selected_browser_row<'a>(
        &self,
        model: &'a Model,
        workspace_idx: usize,
        selected_row: usize,
        expanded_ids: &[String],
    ) -> Option<BrowserTreeRow<'a>> {
        let rows = self.browser_rows(model, workspace_idx, expanded_ids);
        rows.get(clamp_selection(selected_row, rows.len())).copied()
    }

    fn root_row_index_for_conversation(
        &self,
        model: &Model,
        workspace_idx: usize,
        conv_idx: usize,
    ) -> usize {
        let rows = self.browser_rows(model, workspace_idx, &[]);
        rows.iter()
            .position(|row| matches!(row.node, BrowserNode::Conversation { conv_idx: row_conv_idx, .. } if row_conv_idx == conv_idx))
            .unwrap_or(0)
    }

    fn apply_filter_change(&self, model: &mut Model) {
        let screen = model.screen.clone();
        match screen {
            Screen::Workspaces { .. } => {}
            Screen::Conversations {
                workspace_idx,
                selected_row,
                expanded_ids,
            } => {
                let count = self.browser_rows(model, workspace_idx, &expanded_ids).len();
                model.screen = Screen::Conversations {
                    workspace_idx,
                    selected_row: clamp_selection(selected_row, count),
                    expanded_ids,
                };
            }
            Screen::Messages {
                workspace_idx,
                conv_idx,
                return_expanded_ids,
                ..
            } => {
                model.screen = Screen::Conversations {
                    workspace_idx,
                    selected_row: self.root_row_index_for_conversation(
                        model,
                        workspace_idx,
                        conv_idx,
                    ),
                    expanded_ids: return_expanded_ids,
                };
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
                kind: message.kind,
                participant_label: message.participant.label(),
                content: message.content.clone(),
                depth: message.depth,
                is_focused: idx == selection.focused_message_idx,
                is_expanded: selection.expanded_messages.contains(&idx),
                relative_time: message
                    .timestamp
                    .map(|timestamp| format_relative_time(timestamp, current_time)),
            })
            .collect()
    }
}

pub fn visible_conversation_target(model: &Model) -> Option<(usize, usize)> {
    let app = TranscriptBrowser;
    match &model.screen {
        Screen::Conversations {
            workspace_idx,
            selected_row,
            expanded_ids,
        } => app
            .selected_browser_row(model, *workspace_idx, *selected_row, expanded_ids)
            .map(|row| match row.node {
                BrowserNode::Conversation { conv_idx, .. } => (*workspace_idx, conv_idx),
                BrowserNode::Entry { conv_idx, .. } => (*workspace_idx, conv_idx),
            }),
        Screen::Messages {
            workspace_idx,
            conv_idx,
            ..
        } => Some((*workspace_idx, *conv_idx)),
        Screen::Workspaces { .. } => None,
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
                Screen::Conversations { selected_row, .. } => {
                    if *selected_row > 0 {
                        *selected_row -= 1;
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
                    selected_row,
                    expanded_ids,
                } => {
                    let max_len = self.browser_rows(model, workspace_idx, &expanded_ids).len();
                    if max_len > 0 && selected_row + 1 < max_len {
                        model.screen = Screen::Conversations {
                            workspace_idx,
                            selected_row: selected_row + 1,
                            expanded_ids,
                        };
                    }
                }
                Screen::Messages { .. } => {}
            },
            Event::MessageUp => match &mut model.screen {
                Screen::Messages { message_state, .. } => message_state.move_up(),
                Screen::Workspaces { .. } | Screen::Conversations { .. } => {}
            },
            Event::MessageDown => match model.screen.clone() {
                Screen::Messages {
                    workspace_idx,
                    conv_idx,
                    mut message_state,
                    return_selected_row,
                    return_expanded_ids,
                } => {
                    let max_messages = model.workspaces[workspace_idx].conversations[conv_idx]
                        .messages
                        .len();
                    message_state.move_down(max_messages);
                    model.screen = Screen::Messages {
                        workspace_idx,
                        conv_idx,
                        message_state,
                        return_selected_row,
                        return_expanded_ids,
                    };
                }
                Screen::Workspaces { .. } | Screen::Conversations { .. } => {}
            },
            Event::ToggleMessage => match model.screen.clone() {
                Screen::Conversations {
                    workspace_idx,
                    selected_row,
                    mut expanded_ids,
                } => {
                    if let Some(row) =
                        self.selected_browser_row(model, workspace_idx, selected_row, &expanded_ids)
                    {
                        let toggle_id = match row.node {
                            BrowserNode::Conversation { conversation, .. } => {
                                browser_conversation_row_id(&conversation.id)
                            }
                            BrowserNode::Entry {
                                conversation,
                                message_idx,
                                message,
                                ..
                            } => browser_entry_row_id(
                                &conversation.id,
                                message_idx,
                                message.id.as_deref(),
                            ),
                        };

                        if row.is_expandable {
                            if let Some(pos) = expanded_ids.iter().position(|id| id == &toggle_id) {
                                expanded_ids.remove(pos);
                            } else {
                                expanded_ids.push(toggle_id);
                            }
                        }
                    }

                    model.screen = Screen::Conversations {
                        workspace_idx,
                        selected_row,
                        expanded_ids,
                    };
                }
                Screen::Messages {
                    workspace_idx,
                    conv_idx,
                    mut message_state,
                    return_selected_row,
                    return_expanded_ids,
                } => {
                    message_state.toggle_current();
                    model.screen = Screen::Messages {
                        workspace_idx,
                        conv_idx,
                        message_state,
                        return_selected_row,
                        return_expanded_ids,
                    };
                }
                Screen::Workspaces { .. } => {}
            },
            Event::Select => {
                let screen = model.screen.clone();
                match screen {
                    Screen::Workspaces { selected_workspace } => {
                        if !model.workspaces.is_empty() {
                            model.screen = Screen::Conversations {
                                workspace_idx: selected_workspace,
                                selected_row: 0,
                                expanded_ids: Vec::new(),
                            };
                        }
                    }
                    Screen::Conversations {
                        workspace_idx,
                        selected_row,
                        expanded_ids,
                    } => {
                        if let Some(row) = self.selected_browser_row(
                            model,
                            workspace_idx,
                            selected_row,
                            &expanded_ids,
                        ) {
                            let (conv_idx, focused_message_idx) = match row.node {
                                BrowserNode::Conversation { conv_idx, .. } => (conv_idx, 0),
                                BrowserNode::Entry {
                                    conv_idx,
                                    message_idx,
                                    ..
                                } => (conv_idx, message_idx),
                            };

                            model.screen = Screen::Messages {
                                workspace_idx,
                                conv_idx,
                                message_state: MessageSelectionState {
                                    focused_message_idx,
                                    expanded_messages: Vec::new(),
                                },
                                return_selected_row: selected_row,
                                return_expanded_ids: expanded_ids,
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
                        return_selected_row,
                        return_expanded_ids,
                        ..
                    } => {
                        model.screen = Screen::Conversations {
                            workspace_idx,
                            selected_row: return_selected_row,
                            expanded_ids: return_expanded_ids,
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
            Event::ToggleLayout => {}
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
                selected_row,
                expanded_ids,
            } => {
                let workspace = &model.workspaces[*workspace_idx];
                let rows = self.browser_rows(model, *workspace_idx, expanded_ids);
                let selected_row = clamp_selection(*selected_row, rows.len());
                let previews = rows
                    .iter()
                    .enumerate()
                    .map(|(idx, row)| {
                        self.browser_row_view(*row, idx == selected_row, model.current_time)
                    })
                    .collect::<Vec<_>>();

                let active_id = rows.get(selected_row).map(|row| match row.node {
                    BrowserNode::Conversation { conversation, .. } => conversation
                        .external_id
                        .clone()
                        .unwrap_or_else(|| conversation.id.clone()),
                    BrowserNode::Entry {
                        conversation,
                        message,
                        message_idx,
                        ..
                    } => browser_entry_row_id(&conversation.id, message_idx, message.id.as_deref()),
                });

                ViewModel {
                    title: format!("Conversations in '{}'", workspace.display_name),
                    breadcrumb: format!("Workspaces > {}", workspace.display_name),
                    active_id,
                    content: ViewContent::TreeList(previews),
                    selected_index: selected_row,
                    filter_text: format!("{filter_text} | [e/l] Expand  [Enter] Read"),
                }
            }
            Screen::Messages {
                workspace_idx,
                conv_idx,
                message_state,
                ..
            } => {
                let workspace = &model.workspaces[*workspace_idx];
                let conversation = &workspace.conversations[*conv_idx];
                let mut messages = self.message_previews(
                    &conversation.messages,
                    message_state,
                    model.current_time,
                );
                for message in &mut messages {
                    message.is_focused = false;
                }

                ViewModel {
                    title: format!("Messages in '{}'", conversation.display_title()),
                    breadcrumb: format!(
                        "Workspaces > {} > {}",
                        workspace.display_name,
                        conversation.display_title()
                    ),
                    active_id: Some(
                        conversation
                            .external_id
                            .clone()
                            .unwrap_or_else(|| conversation.id.clone()),
                    ),
                    content: ViewContent::MessagesList(messages),
                    selected_index: message_state.focused_message_idx,
                    filter_text: format!("{} | [↑/↓] Scroll transcript", filter_text),
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
            kind: MessageKind::UserMessage,
            participant: Participant::User,
            content: content.into(),
            timestamp: Some(1000),
            parent_id: None,
            associated_id: None,
            depth: 0,
        }
    }

    fn sample_workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "Test Workspace".into(),
            source_path: Some("~/test".into()),
            updated_at: 1000,
            conversations: vec![Conversation {
                id: "conv-1".into(),
                external_id: Some("provider-conv-1".into()),
                branch_parent_id: None,
                branch_anchor_message_id: None,
                title: Some("Hello".into()),
                preview: Some("hello".into()),
                provider: ProviderKind::ClaudeCode,
                created_at: 1000,
                updated_at: 1000,
                segments: vec![],
                messages: vec![sample_message("hello")],
                is_hydrated: true,
                load_ref: None,
            }],
        }
    }

    fn tree_workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "Tree Workspace".into(),
            source_path: Some("~/tree".into()),
            updated_at: 1000,
            conversations: vec![Conversation {
                id: "conv-1".into(),
                external_id: None,
                branch_parent_id: None,
                branch_anchor_message_id: None,
                title: Some("Tree".into()),
                preview: Some("root".into()),
                provider: ProviderKind::ClaudeCode,
                created_at: 1000,
                updated_at: 1000,
                segments: vec![],
                messages: vec![
                    Message {
                        id: Some("root".into()),
                        kind: MessageKind::UserMessage,
                        participant: Participant::User,
                        content: "root".into(),
                        timestamp: Some(1000),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    },
                    Message {
                        id: Some("child".into()),
                        kind: MessageKind::AssistantMessage,
                        participant: Participant::Assistant {
                            provider: ProviderKind::ClaudeCode,
                        },
                        content: "child".into(),
                        timestamp: Some(1100),
                        parent_id: Some("root".into()),
                        associated_id: None,
                        depth: 1,
                    },
                ],
                is_hydrated: true,
                load_ref: None,
            }],
        }
    }

    fn linear_reply_workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "Linear Reply Workspace".into(),
            source_path: Some("~/linear".into()),
            updated_at: 1000,
            conversations: vec![Conversation {
                id: "conv-1".into(),
                external_id: None,
                branch_parent_id: None,
                branch_anchor_message_id: None,
                title: Some("Linear".into()),
                preview: Some("root".into()),
                provider: ProviderKind::ClaudeCode,
                created_at: 1000,
                updated_at: 1000,
                segments: vec![],
                messages: vec![
                    Message {
                        id: Some("user-1".into()),
                        kind: MessageKind::UserMessage,
                        participant: Participant::User,
                        content: "user root".into(),
                        timestamp: Some(1000),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    },
                    Message {
                        id: Some("assistant-1".into()),
                        kind: MessageKind::AssistantMessage,
                        participant: Participant::Assistant {
                            provider: ProviderKind::ClaudeCode,
                        },
                        content: "assistant reply".into(),
                        timestamp: Some(1100),
                        parent_id: Some("user-1".into()),
                        associated_id: None,
                        depth: 0,
                    },
                ],
                is_hydrated: true,
                load_ref: None,
            }],
        }
    }

    fn branch_workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "Branch Workspace".into(),
            source_path: Some("~/branch".into()),
            updated_at: 1000,
            conversations: vec![
                Conversation {
                    id: "parent".into(),
                    external_id: None,
                    branch_parent_id: None,
                    branch_anchor_message_id: None,
                    title: Some("Parent".into()),
                    preview: Some("parent".into()),
                    provider: ProviderKind::ClaudeCode,
                    created_at: 1000,
                    updated_at: 1000,
                    segments: vec![],
                    messages: vec![Message {
                        id: Some("root-msg".into()),
                        ..sample_message("parent root")
                    }],
                    is_hydrated: true,
                    load_ref: None,
                },
                Conversation {
                    id: "child".into(),
                    external_id: None,
                    branch_parent_id: Some("parent".into()),
                    branch_anchor_message_id: Some("root-msg".into()),
                    title: Some("Child Branch".into()),
                    preview: Some("child".into()),
                    provider: ProviderKind::ClaudeCode,
                    created_at: 1001,
                    updated_at: 1001,
                    segments: vec![],
                    messages: vec![sample_message("child root")],
                    is_hydrated: true,
                    load_ref: None,
                },
            ],
        }
    }

    #[test]
    fn navigation_flow_is_workspace_tree_messages() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut model = Model {
            workspaces: vec![sample_workspace()],
            current_time: 2000,
            ..Default::default()
        };

        let _ = app.update(Event::Select, &mut model);
        assert_eq!(
            model.screen,
            Screen::Conversations {
                workspace_idx: 0,
                selected_row: 0,
                expanded_ids: Vec::new(),
            }
        );

        let _ = app.update(Event::Select, &mut model);
        assert_eq!(
            model.screen,
            Screen::Messages {
                workspace_idx: 0,
                conv_idx: 0,
                message_state: MessageSelectionState::default(),
                return_selected_row: 0,
                return_expanded_ids: Vec::new(),
            }
        );

        let _ = app.update(Event::Back, &mut model);
        assert_eq!(
            model.screen,
            Screen::Conversations {
                workspace_idx: 0,
                selected_row: 0,
                expanded_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn tree_rows_expand_and_select_entry() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut model = Model {
            workspaces: vec![tree_workspace()],
            current_time: 2000,
            screen: Screen::Conversations {
                workspace_idx: 0,
                selected_row: 0,
                expanded_ids: Vec::new(),
            },
            ..Default::default()
        };

        let _ = app.update(Event::ToggleMessage, &mut model);
        let view = app.view(&model);

        let ViewContent::TreeList(rows) = view.content else {
            panic!("expected tree list");
        };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].label, "Tree");
        assert_eq!(rows[1].label, "root");
        assert_eq!(rows[2].label, "child");

        let _ = app.update(Event::Down, &mut model);
        let _ = app.update(Event::Select, &mut model);

        assert_eq!(
            model.screen,
            Screen::Messages {
                workspace_idx: 0,
                conv_idx: 0,
                message_state: MessageSelectionState {
                    focused_message_idx: 0,
                    expanded_messages: Vec::new(),
                },
                return_selected_row: 1,
                return_expanded_ids: vec![browser_conversation_row_id("conv-1")],
            }
        );
    }

    #[test]
    fn filtering_from_messages_returns_to_tree_browser() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut workspace = sample_workspace();
        workspace.conversations.push(Conversation {
            id: "conv-2".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: Some("Codex".into()),
            preview: Some("codex".into()),
            provider: ProviderKind::Codex,
            created_at: 1000,
            updated_at: 1000,
            segments: vec![],
            messages: vec![sample_message("codex")],
            is_hydrated: true,
            load_ref: None,
        });

        let mut model = Model {
            workspaces: vec![workspace],
            screen: Screen::Messages {
                workspace_idx: 0,
                conv_idx: 1,
                message_state: MessageSelectionState::default(),
                return_selected_row: 1,
                return_expanded_ids: Vec::new(),
            },
            ..Default::default()
        };

        let _ = app.update(Event::CycleFilter, &mut model);

        assert_eq!(model.provider_filter, Some(ProviderKind::ClaudeCode));
        assert_eq!(
            model.screen,
            Screen::Conversations {
                workspace_idx: 0,
                selected_row: 0,
                expanded_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn branch_conversations_render_under_expanded_parent() {
        let app = AppTester::<TranscriptBrowser>::default();
        let model = Model {
            workspaces: vec![branch_workspace()],
            current_time: 2000,
            screen: Screen::Conversations {
                workspace_idx: 0,
                selected_row: 0,
                expanded_ids: vec![browser_conversation_row_id("parent")],
            },
            ..Default::default()
        };

        let view = app.view(&model);
        let ViewContent::TreeList(rows) = view.content else {
            panic!("expected tree list");
        };

        assert_eq!(rows[0].label, "Parent");
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].label, "parent root");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].label, "Child Branch");
        assert_eq!(rows[2].depth, 2);
    }

    #[test]
    fn linear_reply_edges_do_not_create_entry_hierarchy() {
        let app = AppTester::<TranscriptBrowser>::default();
        let model = Model {
            workspaces: vec![linear_reply_workspace()],
            current_time: 2000,
            screen: Screen::Conversations {
                workspace_idx: 0,
                selected_row: 0,
                expanded_ids: vec![browser_conversation_row_id("conv-1")],
            },
            ..Default::default()
        };

        let view = app.view(&model);
        let ViewContent::TreeList(rows) = view.content else {
            panic!("expected tree list");
        };

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].label, "Linear");
        assert_eq!(rows[1].label, "user root");
        assert_eq!(rows[2].label, "assistant reply");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].depth, 1);
        assert!(!rows[1].is_expandable);
        assert!(!rows[2].is_expandable);
    }
}
