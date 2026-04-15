use crate::{
    parse_claude_scaffold_artifact, Conversation, Message, MessageKind, ProviderKind, Workspace,
};
use crux_core::{App, Command};
use crux_macros::effect;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MessagePreview {
    pub source_index: usize,
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum TreeRowKind {
    Conversation,
    BranchConversation,
    BranchAnchor,
    OpeningPrompt,
    Entry,
    Delegation,
    DelegationSummary,
    Summary,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TreeRowPreview {
    pub id: String,
    pub kind: TreeRowKind,
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
    pub status_text: Option<String>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            workspaces: vec![],
            screen: Screen::default(),
            provider_filter: None,
            current_time: 0,
            status_text: None,
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
    pub status_text: Option<String>,
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
    kind: TreeRowKind,
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

fn is_noise_message(message: &Message) -> bool {
    // TODO: This is intentionally narrow. Extend the Claude scaffolding classifier deliberately
    // as new provider-emitted pseudo-XML artifacts show up rather than adding more ad hoc
    // prefix checks here.
    parse_claude_scaffold_artifact(&message.content).is_some()
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

fn display_conversation_label(conversation: &Conversation) -> String {
    let label = conversation.display_title();
    label
        .strip_suffix(" (Branch)")
        .unwrap_or(label.as_str())
        .to_string()
}

fn workspace_key(workspace: &Workspace) -> &str {
    workspace
        .source_path
        .as_deref()
        .unwrap_or(workspace.display_name.as_str())
}

fn conversation_key(conversation: &Conversation) -> &str {
    conversation
        .external_id
        .as_deref()
        .unwrap_or(conversation.id.as_str())
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
            }
            children_by_parent
                .entry(parent)
                .or_default()
                .push((conv_idx, conversation));
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
        let anchored_child_ids = conversation
            .messages
            .iter()
            .filter_map(|message| message.id.as_deref())
            .flat_map(|message_id| {
                children_by_anchor
                    .get(&(conversation.id.as_str(), message_id))
                    .into_iter()
                    .flatten()
                    .map(|(_, child)| child.id.as_str())
            })
            .collect::<HashSet<_>>();
        let has_visible_entry_children = conversation
            .messages
            .iter()
            .filter(|message| !is_noise_message(message))
            .filter_map(|message| message.id.as_deref())
            .any(|message_id| {
                children_by_anchor.contains_key(&(conversation.id.as_str(), message_id))
            });
        let is_expandable = has_visible_entry_children || !child_conversations.is_empty();

        rows.push(BrowserTreeRow {
            depth,
            node: BrowserNode::Conversation {
                conv_idx,
                conversation,
            },
            kind: if conversation.branch_parent_id.is_some() {
                TreeRowKind::BranchConversation
            } else {
                TreeRowKind::Conversation
            },
            is_expandable,
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
            if anchored_child_ids.contains(child_conversation.id.as_str()) {
                continue;
            }
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
        let branch_anchor_ids = conversation_children_by_anchor
            .keys()
            .filter(|(conversation_id, _)| *conversation_id == conversation.id.as_str())
            .map(|(_, message_id)| *message_id)
            .collect::<HashSet<_>>();
        for (message_idx, message) in conversation.messages.iter().enumerate() {
            if is_noise_message(message) {
                continue;
            }
            let Some(message_id) = message.id.as_deref() else {
                continue;
            };
            if !branch_anchor_ids.contains(message_id) {
                continue;
            }
            self.push_branch_anchor_row(
                conversation,
                conv_idx,
                message_idx,
                base_depth,
                expanded_ids,
                conversation_children_by_parent,
                conversation_children_by_anchor,
                rows,
            );
        }
    }

    fn push_branch_anchor_row<'a>(
        &self,
        conversation: &'a Conversation,
        conv_idx: usize,
        message_idx: usize,
        depth: usize,
        expanded_ids: &[String],
        conversation_children_by_parent: &HashMap<Option<&'a str>, Vec<(usize, &'a Conversation)>>,
        conversation_children_by_anchor: &HashMap<
            (&'a str, &'a str),
            Vec<(usize, &'a Conversation)>,
        >,
        rows: &mut Vec<BrowserTreeRow<'a>>,
    ) {
        let message = &conversation.messages[message_idx];
        let node_id = browser_entry_row_id(&conversation.id, message_idx, message.id.as_deref());
        let branch_children = message
            .id
            .as_deref()
            .and_then(|message_id| {
                conversation_children_by_anchor.get(&(conversation.id.as_str(), message_id))
            })
            .cloned()
            .unwrap_or_default();
        let has_branch_children = !branch_children.is_empty();
        let is_expanded = expanded_ids.iter().any(|id| id == &node_id);

        rows.push(BrowserTreeRow {
            depth,
            node: BrowserNode::Entry {
                conv_idx,
                conversation,
                message_idx,
                message,
            },
            kind: TreeRowKind::BranchAnchor,
            is_expandable: has_branch_children,
            is_expanded,
        });

        let mut anchored_child_ids = HashSet::new();
        for (child_idx, child_conversation) in branch_children.iter().copied() {
            anchored_child_ids.insert(child_conversation.id.as_str());
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

        if let Some(direct_children) =
            conversation_children_by_parent.get(&Some(conversation.id.as_str()))
        {
            for (child_idx, child_conversation) in direct_children.iter().copied() {
                if anchored_child_ids.contains(child_conversation.id.as_str()) {
                    continue;
                }
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
                kind: row.kind.clone(),
                label: display_conversation_label(conversation),
                secondary: Some(format!(
                    "{}{} {}",
                    if matches!(row.kind, TreeRowKind::BranchConversation) {
                        "Branch • "
                    } else {
                        ""
                    },
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

                let time = message
                    .timestamp
                    .map(|timestamp| format_relative_time(timestamp, current_time));
                let secondary = Some(match row.kind {
                    TreeRowKind::BranchAnchor => match time {
                        Some(time) => format!("Branch point • {time}"),
                        None => "Branch point".to_string(),
                    },
                    TreeRowKind::Delegation => match time {
                        Some(time) => format!("Delegation • {time}"),
                        None => "Delegation".to_string(),
                    },
                    _ => match time {
                        Some(time) => format!("{} {}", message.participant.label(), time),
                        None => message.participant.label(),
                    },
                });

                TreeRowPreview {
                    id: browser_entry_row_id(&conversation.id, message_idx, message.id.as_deref()),
                    kind: row.kind.clone(),
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

    fn browser_row_id(&self, row: BrowserTreeRow<'_>) -> String {
        match row.node {
            BrowserNode::Conversation { conversation, .. } => {
                browser_conversation_row_id(&conversation.id)
            }
            BrowserNode::Entry {
                conversation,
                message_idx,
                message,
                ..
            } => browser_entry_row_id(&conversation.id, message_idx, message.id.as_deref()),
        }
    }

    fn workspace_index_by_key(&self, workspaces: &[Workspace], key: &str) -> Option<usize> {
        workspaces
            .iter()
            .position(|workspace| workspace_key(workspace) == key)
    }

    fn conversation_index_by_key(&self, workspace: &Workspace, key: &str) -> Option<usize> {
        workspace
            .conversations
            .iter()
            .position(|conversation| conversation_key(conversation) == key)
    }

    fn browser_row_index_by_id(
        &self,
        model: &Model,
        workspace_idx: usize,
        expanded_ids: &[String],
        row_id: &str,
    ) -> Option<usize> {
        self.browser_rows(model, workspace_idx, expanded_ids)
            .iter()
            .position(|row| self.browser_row_id(*row) == row_id)
    }

    fn visible_message_index_by_id(&self, messages: &[Message], message_id: &str) -> Option<usize> {
        messages.iter().enumerate().find_map(|(idx, message)| {
            (!is_noise_message(message) && message.id.as_deref() == Some(message_id)).then_some(idx)
        })
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
            .filter(|(_, message)| !is_noise_message(message))
            .map(|(idx, message)| MessagePreview {
                source_index: idx,
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

    fn nearest_visible_message_index(&self, messages: &[Message], preferred_idx: usize) -> usize {
        if messages.is_empty() {
            return 0;
        }
        if preferred_idx < messages.len() && !is_noise_message(&messages[preferred_idx]) {
            return preferred_idx;
        }
        for idx in preferred_idx.saturating_add(1)..messages.len() {
            if !is_noise_message(&messages[idx]) {
                return idx;
            }
        }
        for idx in (0..preferred_idx.min(messages.len())).rev() {
            if !is_noise_message(&messages[idx]) {
                return idx;
            }
        }
        preferred_idx.min(messages.len().saturating_sub(1))
    }

    fn previous_visible_message_index(&self, messages: &[Message], current_idx: usize) -> usize {
        for idx in (0..current_idx.min(messages.len())).rev() {
            if !is_noise_message(&messages[idx]) {
                return idx;
            }
        }
        current_idx
    }

    fn next_visible_message_index(&self, messages: &[Message], current_idx: usize) -> usize {
        for idx in current_idx.saturating_add(1)..messages.len() {
            if !is_noise_message(&messages[idx]) {
                return idx;
            }
        }
        current_idx
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
        if !matches!(event, Event::SetWorkspaces(_, _)) {
            model.status_text = None;
        }
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
                Screen::Messages {
                    workspace_idx,
                    conv_idx,
                    message_state,
                    ..
                } => {
                    let messages =
                        &model.workspaces[*workspace_idx].conversations[*conv_idx].messages;
                    message_state.focused_message_idx = self.previous_visible_message_index(
                        messages,
                        message_state.focused_message_idx,
                    );
                }
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
                    let messages =
                        &model.workspaces[workspace_idx].conversations[conv_idx].messages;
                    message_state.focused_message_idx = self
                        .next_visible_message_index(messages, message_state.focused_message_idx);
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
                            let focused_message_idx = self.nearest_visible_message_index(
                                &model.workspaces[workspace_idx].conversations[conv_idx].messages,
                                focused_message_idx,
                            );

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
                let previous_screen = model.screen.clone();
                let previous_workspace_key = match &previous_screen {
                    Screen::Workspaces { selected_workspace } => model
                        .workspaces
                        .get(*selected_workspace)
                        .map(workspace_key)
                        .map(ToOwned::to_owned),
                    Screen::Conversations { workspace_idx, .. }
                    | Screen::Messages { workspace_idx, .. } => model
                        .workspaces
                        .get(*workspace_idx)
                        .map(workspace_key)
                        .map(ToOwned::to_owned),
                };
                let previous_selected_row_id = match &previous_screen {
                    Screen::Conversations {
                        workspace_idx,
                        selected_row,
                        expanded_ids,
                    } => self
                        .selected_browser_row(model, *workspace_idx, *selected_row, expanded_ids)
                        .map(|row| self.browser_row_id(row)),
                    Screen::Messages {
                        workspace_idx,
                        return_selected_row,
                        return_expanded_ids,
                        ..
                    } => self
                        .selected_browser_row(
                            model,
                            *workspace_idx,
                            *return_selected_row,
                            return_expanded_ids,
                        )
                        .map(|row| self.browser_row_id(row)),
                    Screen::Workspaces { .. } => None,
                };
                let previous_conversation_key = match &previous_screen {
                    Screen::Messages {
                        workspace_idx,
                        conv_idx,
                        ..
                    } => model
                        .workspaces
                        .get(*workspace_idx)
                        .and_then(|workspace| workspace.conversations.get(*conv_idx))
                        .map(conversation_key)
                        .map(ToOwned::to_owned),
                    Screen::Workspaces { .. } | Screen::Conversations { .. } => None,
                };
                let previous_focused_message_id = match &previous_screen {
                    Screen::Messages {
                        workspace_idx,
                        conv_idx,
                        message_state,
                        ..
                    } => model
                        .workspaces
                        .get(*workspace_idx)
                        .and_then(|workspace| workspace.conversations.get(*conv_idx))
                        .and_then(|conversation| {
                            conversation.messages.get(message_state.focused_message_idx)
                        })
                        .and_then(|message| message.id.clone()),
                    Screen::Workspaces { .. } | Screen::Conversations { .. } => None,
                };

                model.workspaces = workspaces;
                model.current_time = current_time;

                let restored_screen = previous_workspace_key
                    .as_deref()
                    .and_then(|workspace_key| {
                        self.workspace_index_by_key(&model.workspaces, workspace_key)
                    })
                    .and_then(|workspace_idx| match previous_screen {
                        Screen::Workspaces {
                            selected_workspace: _,
                        } => Some(Screen::Workspaces {
                            selected_workspace: workspace_idx,
                        }),
                        Screen::Conversations {
                            selected_row: _,
                            expanded_ids,
                            ..
                        } => {
                            let selected_row = previous_selected_row_id
                                .as_deref()
                                .and_then(|row_id| {
                                    self.browser_row_index_by_id(
                                        model,
                                        workspace_idx,
                                        &expanded_ids,
                                        row_id,
                                    )
                                })
                                .unwrap_or(0);
                            Some(Screen::Conversations {
                                workspace_idx,
                                selected_row,
                                expanded_ids,
                            })
                        }
                        Screen::Messages {
                            conv_idx: _,
                            message_state,
                            return_selected_row: _,
                            return_expanded_ids,
                            ..
                        } => {
                            let workspace = &model.workspaces[workspace_idx];
                            let conv_idx = previous_conversation_key.as_deref().and_then(
                                |conversation_key| {
                                    self.conversation_index_by_key(workspace, conversation_key)
                                },
                            )?;
                            let conversation = &workspace.conversations[conv_idx];
                            let focused_message_idx = previous_focused_message_id
                                .as_deref()
                                .and_then(|message_id| {
                                    self.visible_message_index_by_id(
                                        &conversation.messages,
                                        message_id,
                                    )
                                })
                                .unwrap_or_else(|| {
                                    self.nearest_visible_message_index(
                                        &conversation.messages,
                                        message_state.focused_message_idx,
                                    )
                                });
                            let return_selected_row = previous_selected_row_id
                                .as_deref()
                                .and_then(|row_id| {
                                    self.browser_row_index_by_id(
                                        model,
                                        workspace_idx,
                                        &return_expanded_ids,
                                        row_id,
                                    )
                                })
                                .unwrap_or_else(|| {
                                    self.root_row_index_for_conversation(
                                        model,
                                        workspace_idx,
                                        conv_idx,
                                    )
                                });
                            Some(Screen::Messages {
                                workspace_idx,
                                conv_idx,
                                message_state: MessageSelectionState {
                                    focused_message_idx,
                                    expanded_messages: message_state.expanded_messages,
                                },
                                return_selected_row,
                                return_expanded_ids,
                            })
                        }
                    });

                model.screen = restored_screen.unwrap_or(Screen::Workspaces {
                    selected_workspace: 0,
                });
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
                status_text: model.status_text.clone(),
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
                    status_text: model.status_text.clone(),
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
                    status_text: model.status_text.clone(),
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
                    title: Some("Child Branch (Branch)".into()),
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

    fn unanchored_branch_workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "Unanchored Branch Workspace".into(),
            source_path: Some("~/branch-unanchored".into()),
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
                    branch_anchor_message_id: Some("missing-msg".into()),
                    title: Some("Child Branch (Branch)".into()),
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

    fn noise_filtered_workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "Noise Filter Workspace".into(),
            source_path: Some("~/noise".into()),
            updated_at: 1000,
            conversations: vec![Conversation {
                id: "conv-1".into(),
                external_id: None,
                branch_parent_id: None,
                branch_anchor_message_id: None,
                title: Some("Noise".into()),
                preview: Some("noise".into()),
                provider: ProviderKind::ClaudeCode,
                created_at: 1000,
                updated_at: 1000,
                segments: vec![],
                messages: vec![
                    Message {
                        id: Some("noise-1".into()),
                        kind: MessageKind::MetadataChange,
                        participant: Participant::System,
                        content: "<local-command-caveat>Caveat: hidden</local-command-caveat>"
                            .into(),
                        timestamp: Some(1000),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    },
                    Message {
                        id: Some("visible-1".into()),
                        kind: MessageKind::UserMessage,
                        participant: Participant::User,
                        content: "real user message".into(),
                        timestamp: Some(1100),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    },
                    Message {
                        id: Some("noise-2".into()),
                        kind: MessageKind::MetadataChange,
                        participant: Participant::System,
                        content: "<command-name>/clear</command-name>".into(),
                        timestamp: Some(1200),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    },
                    Message {
                        id: Some("visible-2".into()),
                        kind: MessageKind::AssistantMessage,
                        participant: Participant::Assistant {
                            provider: ProviderKind::ClaudeCode,
                        },
                        content: "real assistant reply".into(),
                        timestamp: Some(1300),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    },
                ],
                is_hydrated: true,
                load_ref: None,
            }],
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
    fn selecting_conversation_row_opens_transcript_when_browser_has_no_visible_children() {
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

        let view = app.view(&model);

        let ViewContent::TreeList(rows) = view.content else {
            panic!("expected tree list");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Tree");
        assert_eq!(rows[0].kind, TreeRowKind::Conversation);
        assert!(!rows[0].is_expandable);

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
                return_selected_row: 0,
                return_expanded_ids: Vec::new(),
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
        assert_eq!(rows[1].kind, TreeRowKind::BranchAnchor);
        assert_eq!(rows[2].label, "Child Branch");
        assert_eq!(rows[2].depth, 2);
        assert_eq!(rows[2].kind, TreeRowKind::BranchConversation);
    }

    #[test]
    fn branch_conversations_fallback_to_direct_children_when_anchor_is_missing() {
        let app = AppTester::<TranscriptBrowser>::default();
        let model = Model {
            workspaces: vec![unanchored_branch_workspace()],
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
        assert_eq!(rows[1].label, "Child Branch");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[1].kind, TreeRowKind::BranchConversation);
    }

    #[test]
    fn conversations_without_forks_do_not_expand_into_entry_rows() {
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

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Linear");
        assert_eq!(rows[0].kind, TreeRowKind::Conversation);
    }

    fn delegation_workspace() -> Workspace {
        Workspace {
            id: "workspace-1".into(),
            display_name: "Delegation Workspace".into(),
            source_path: Some("~/delegation".into()),
            updated_at: 1000,
            conversations: vec![Conversation {
                id: "conv-1".into(),
                external_id: None,
                branch_parent_id: None,
                branch_anchor_message_id: None,
                title: Some("Delegation".into()),
                preview: Some("delegation".into()),
                provider: ProviderKind::ClaudeCode,
                created_at: 1000,
                updated_at: 1000,
                segments: vec![],
                messages: vec![
                    Message {
                        id: Some("root-user".into()),
                        kind: MessageKind::UserMessage,
                        participant: Participant::User,
                        content: "main prompt".into(),
                        timestamp: Some(1000),
                        parent_id: None,
                        associated_id: None,
                        depth: 0,
                    },
                    Message {
                        id: Some("delegation".into()),
                        kind: MessageKind::MetadataChange,
                        participant: Participant::System,
                        content: "Explore the codebase thoroughly".into(),
                        timestamp: Some(1100),
                        parent_id: None,
                        associated_id: None,
                        depth: 1,
                    },
                    Message {
                        id: Some("subagent-1".into()),
                        kind: MessageKind::AssistantMessage,
                        participant: Participant::Assistant {
                            provider: ProviderKind::ClaudeCode,
                        },
                        content: "I will inspect the repository".into(),
                        timestamp: Some(1200),
                        parent_id: Some("delegation".into()),
                        associated_id: None,
                        depth: 1,
                    },
                    Message {
                        id: Some("subagent-2".into()),
                        kind: MessageKind::AssistantMessage,
                        participant: Participant::Assistant {
                            provider: ProviderKind::ClaudeCode,
                        },
                        content: "I found the relevant files".into(),
                        timestamp: Some(1300),
                        parent_id: Some("subagent-1".into()),
                        associated_id: None,
                        depth: 1,
                    },
                    Message {
                        id: Some("main-assistant".into()),
                        kind: MessageKind::AssistantMessage,
                        participant: Participant::Assistant {
                            provider: ProviderKind::ClaudeCode,
                        },
                        content: "Back on the main thread".into(),
                        timestamp: Some(1400),
                        parent_id: Some("root-user".into()),
                        associated_id: None,
                        depth: 0,
                    },
                ],
                is_hydrated: true,
                load_ref: None,
            }],
        }
    }

    #[test]
    fn delegation_sidechains_do_not_appear_in_conversation_browser() {
        let app = AppTester::<TranscriptBrowser>::default();
        let model = Model {
            workspaces: vec![delegation_workspace()],
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

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Delegation");
        assert_eq!(rows[0].kind, TreeRowKind::Conversation);
    }

    #[test]
    fn set_workspaces_preserves_conversation_screen_during_refresh() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut model = Model {
            workspaces: vec![branch_workspace()],
            current_time: 2000,
            screen: Screen::Conversations {
                workspace_idx: 0,
                selected_row: 1,
                expanded_ids: vec![browser_conversation_row_id("parent")],
            },
            ..Default::default()
        };

        let mut refreshed = branch_workspace();
        refreshed.updated_at = 3000;
        refreshed.conversations[0].updated_at = 3000;

        let _ = app.update(Event::SetWorkspaces(vec![refreshed], 3000), &mut model);

        assert_eq!(
            model.screen,
            Screen::Conversations {
                workspace_idx: 0,
                selected_row: 1,
                expanded_ids: vec![browser_conversation_row_id("parent")],
            }
        );
    }

    #[test]
    fn set_workspaces_preserves_message_screen_during_refresh() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut model = Model {
            workspaces: vec![sample_workspace()],
            current_time: 2000,
            screen: Screen::Messages {
                workspace_idx: 0,
                conv_idx: 0,
                message_state: MessageSelectionState {
                    focused_message_idx: 0,
                    expanded_messages: vec![0],
                },
                return_selected_row: 0,
                return_expanded_ids: Vec::new(),
            },
            ..Default::default()
        };

        let mut refreshed = sample_workspace();
        refreshed.updated_at = 3000;
        refreshed.conversations[0].updated_at = 3000;

        let _ = app.update(Event::SetWorkspaces(vec![refreshed], 3000), &mut model);

        assert_eq!(
            model.screen,
            Screen::Messages {
                workspace_idx: 0,
                conv_idx: 0,
                message_state: MessageSelectionState {
                    focused_message_idx: 0,
                    expanded_messages: vec![0],
                },
                return_selected_row: 0,
                return_expanded_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn noise_messages_are_hidden_and_navigation_skips_them() {
        let app = AppTester::<TranscriptBrowser>::default();
        let mut model = Model {
            workspaces: vec![noise_filtered_workspace()],
            current_time: 2000,
            screen: Screen::Conversations {
                workspace_idx: 0,
                selected_row: 0,
                expanded_ids: Vec::new(),
            },
            ..Default::default()
        };

        let _ = app.update(Event::Select, &mut model);
        assert_eq!(
            model.screen,
            Screen::Messages {
                workspace_idx: 0,
                conv_idx: 0,
                message_state: MessageSelectionState {
                    focused_message_idx: 1,
                    expanded_messages: Vec::new(),
                },
                return_selected_row: 0,
                return_expanded_ids: Vec::new(),
            }
        );

        let view = app.view(&model);
        let ViewContent::MessagesList(messages) = view.content else {
            panic!("expected messages list");
        };

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].source_index, 1);
        assert_eq!(messages[0].content, "real user message");
        assert_eq!(messages[1].source_index, 3);
        assert_eq!(messages[1].content, "real assistant reply");

        let _ = app.update(Event::MessageDown, &mut model);
        assert_eq!(
            model.screen,
            Screen::Messages {
                workspace_idx: 0,
                conv_idx: 0,
                message_state: MessageSelectionState {
                    focused_message_idx: 3,
                    expanded_messages: Vec::new(),
                },
                return_selected_row: 0,
                return_expanded_ids: Vec::new(),
            }
        );
    }
}
