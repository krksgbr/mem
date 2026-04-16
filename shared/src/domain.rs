use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClaudeScaffoldArtifactKind {
    LocalCommandCaveat,
    LocalCommandStdout,
    LocalCommandStderr,
    CommandName,
    CommandMessage,
    CommandArgs,
    BashInput,
    BashOutput,
    TaskNotification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClaudeScaffoldArtifact<'a> {
    pub kind: ClaudeScaffoldArtifactKind,
    pub body: &'a str,
}

fn parse_scaffold_tag<'a>(
    trimmed: &'a str,
    tag: &'static str,
    kind: ClaudeScaffoldArtifactKind,
) -> Option<(ClaudeScaffoldArtifact<'a>, &'a str)> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let rest = trimmed.strip_prefix(&open)?;
    let (body, suffix) = rest.split_once(&close)?;
    Some((
        ClaudeScaffoldArtifact {
            kind,
            body: body.trim(),
        },
        suffix.trim(),
    ))
}

pub fn parse_claude_scaffold_sequence(content: &str) -> Option<Vec<ClaudeScaffoldArtifact<'_>>> {
    let mut rest = content.trim();
    let mut artifacts = Vec::new();

    while !rest.is_empty() {
        let parsed = parse_scaffold_tag(
            rest,
            "local-command-caveat",
            ClaudeScaffoldArtifactKind::LocalCommandCaveat,
        )
        .or_else(|| {
            parse_scaffold_tag(
                rest,
                "local-command-stdout",
                ClaudeScaffoldArtifactKind::LocalCommandStdout,
            )
        })
        .or_else(|| {
            parse_scaffold_tag(
                rest,
                "local-command-stderr",
                ClaudeScaffoldArtifactKind::LocalCommandStderr,
            )
        })
        .or_else(|| {
            parse_scaffold_tag(
                rest,
                "command-name",
                ClaudeScaffoldArtifactKind::CommandName,
            )
        })
        .or_else(|| {
            parse_scaffold_tag(
                rest,
                "command-message",
                ClaudeScaffoldArtifactKind::CommandMessage,
            )
        })
        .or_else(|| {
            parse_scaffold_tag(
                rest,
                "command-args",
                ClaudeScaffoldArtifactKind::CommandArgs,
            )
        })
        .or_else(|| parse_scaffold_tag(rest, "bash-input", ClaudeScaffoldArtifactKind::BashInput))
        .or_else(|| parse_scaffold_tag(rest, "bash-output", ClaudeScaffoldArtifactKind::BashOutput))
        .or_else(|| {
            parse_scaffold_tag(
                rest,
                "bash-stdout",
                ClaudeScaffoldArtifactKind::LocalCommandStdout,
            )
        })
        .or_else(|| {
            parse_scaffold_tag(
                rest,
                "bash-stderr",
                ClaudeScaffoldArtifactKind::LocalCommandStderr,
            )
        })
        .or_else(|| {
            parse_scaffold_tag(
                rest,
                "task-notification",
                ClaudeScaffoldArtifactKind::TaskNotification,
            )
        })?;

        artifacts.push(parsed.0);
        rest = parsed.1;
    }

    (!artifacts.is_empty()).then_some(artifacts)
}

pub fn parse_claude_scaffold_artifact(content: &str) -> Option<ClaudeScaffoldArtifact<'_>> {
    let artifacts = parse_claude_scaffold_sequence(content)?;
    (artifacts.len() == 1).then_some(artifacts[0])
}

fn parse_codex_shell_command_text(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    let rest = trimmed.strip_prefix("<user_shell_command>")?.trim_start();
    let rest = rest.strip_suffix("</user_shell_command>")?.trim_end();
    let command_block = rest.strip_prefix("<command>")?;
    let (command, suffix) = command_block.split_once("</command>")?;
    let suffix = suffix.trim();
    let suffix_ok = suffix.is_empty()
        || (suffix.starts_with("<result>") && suffix.ends_with("</result>"));
    suffix_ok.then_some(command.trim()).filter(|command| !command.is_empty())
}

fn is_codex_shell_command_only(content: &str) -> bool {
    parse_codex_shell_command_text(content).is_some()
}

fn is_claude_scaffold_only(content: &str) -> bool {
    parse_claude_scaffold_sequence(content).is_some()
}

const CLAUDE_SCAFFOLD_PREFIXES: &[&str] = &[
    "<local-command-caveat>",
    "<local-command-stdout></local-command-stdout>",
    "<local-command-stderr></local-command-stderr>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<bash-stdout></bash-stdout>",
    "<bash-stderr></bash-stderr>",
    "<bash-input>",
    "<bash-output>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<command-name>",
    "<command-message>",
    "<command-args>",
    "<task-notification>",
];

fn starts_with_claude_scaffold_prefix(content: &str) -> bool {
    let trimmed = content.trim_start();
    CLAUDE_SCAFFOLD_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

fn displayable_truncated_scaffold_text(content: &str) -> Option<String> {
    let mut rest = content.trim();

    let mut stripped_any = false;
    loop {
        let mut matched = false;
        for prefix in CLAUDE_SCAFFOLD_PREFIXES {
            if let Some(next) = rest.strip_prefix(prefix) {
                rest = next.trim();
                stripped_any = true;
                matched = true;
                break;
            }
        }
        if !matched {
            break;
        }
    }

    (stripped_any && !rest.is_empty() && !rest.starts_with('<')).then(|| rest.to_string())
}

fn displayable_claude_scaffold_text(content: &str) -> Option<String> {
    let Some(artifacts) = parse_claude_scaffold_sequence(content) else {
        return displayable_truncated_scaffold_text(content);
    };
    if let Some(command_name) = artifacts.iter().find_map(|artifact| {
        (artifact.kind == ClaudeScaffoldArtifactKind::CommandName && !artifact.body.is_empty())
            .then(|| artifact.body.to_string())
    }) {
        return Some(command_name);
    }

    let displayable = artifacts
        .into_iter()
        .filter_map(|artifact| match artifact.kind {
            ClaudeScaffoldArtifactKind::LocalCommandCaveat
            | ClaudeScaffoldArtifactKind::LocalCommandStdout
            | ClaudeScaffoldArtifactKind::LocalCommandStderr
            | ClaudeScaffoldArtifactKind::TaskNotification => None,
            ClaudeScaffoldArtifactKind::CommandName
            | ClaudeScaffoldArtifactKind::CommandMessage
            | ClaudeScaffoldArtifactKind::CommandArgs
            | ClaudeScaffoldArtifactKind::BashInput
            | ClaudeScaffoldArtifactKind::BashOutput => {
                (!artifact.body.is_empty()).then(|| artifact.body.to_string())
            }
        })
        .collect::<Vec<_>>();

    match displayable.as_slice() {
        [] => None,
        [only] => Some(only.clone()),
        many => Some(many.join(" ")),
    }
}

fn is_bare_structural_tag_line(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
        return false;
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    let inner = inner.strip_prefix('/').unwrap_or(inner).trim();
    if inner.is_empty() || inner.contains(char::is_whitespace) {
        return false;
    }

    inner
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == ':')
}

fn is_non_substantive_status_line(line: &str) -> bool {
    matches!(
        line.trim(),
        "[Request interrupted by user for tool use]"
            | "[Request interrupted by user]"
            | "[Interrupted]"
    )
}

fn strip_leading_structural_tags(line: &str) -> &str {
    let mut rest = line.trim();

    loop {
        if !rest.starts_with('<') {
            return rest;
        }

        let Some(close_idx) = rest.find('>') else {
            return rest;
        };
        let tag = &rest[..=close_idx];
        if !is_bare_structural_tag_line(tag) {
            return rest;
        }

        rest = rest[close_idx + 1..].trim_start();
    }
}

fn strip_trailing_structural_tags(line: &str) -> &str {
    let mut rest = line.trim();

    loop {
        if !rest.ends_with('>') {
            return rest;
        }

        let Some(open_idx) = rest.rfind('<') else {
            return rest;
        };
        let tag = &rest[open_idx..];
        if !is_bare_structural_tag_line(tag) {
            return rest;
        }

        rest = rest[..open_idx].trim_end();
    }
}

fn first_meaningful_line(content: &str) -> Option<&str> {
    if is_codex_shell_command_only(content) {
        return None;
    }
    content
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !is_claude_scaffold_only(line)
                && !is_non_substantive_status_line(line)
        })
        .map(strip_leading_structural_tags)
        .map(strip_trailing_structural_tags)
        .find(|line| {
            !line.is_empty()
                && !is_bare_structural_tag_line(line)
                && !is_non_substantive_status_line(line)
        })
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    ClaudeCode,
    Codex,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderKind::ClaudeCode => write!(f, "Claude Code"),
            ProviderKind::Codex => write!(f, "Codex"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Participant {
    User,
    Assistant { provider: ProviderKind },
    System,
    Tool { name: Option<String> },
    Unknown { raw_role: String },
}

impl Participant {
    pub fn from_role(role: &str, provider: ProviderKind) -> Self {
        match role.to_ascii_lowercase().as_str() {
            "user" => Self::User,
            "assistant" => Self::Assistant { provider },
            "system" => Self::System,
            "tool" => Self::Tool { name: None },
            other => Self::Unknown {
                raw_role: other.to_string(),
            },
        }
    }

    pub fn label(&self) -> String {
        match self {
            Participant::User => "You".to_string(),
            Participant::Assistant { provider } => provider.to_string(),
            Participant::System => "System".to_string(),
            Participant::Tool { name: Some(name) } => name.clone(),
            Participant::Tool { name: None } => "Tool".to_string(),
            Participant::Unknown { raw_role } => raw_role.clone(),
        }
    }

    pub fn is_user(&self) -> bool {
        matches!(self, Participant::User)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageKind {
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    Thinking,
    Summary,
    Compaction,
    Label,
    MetadataChange,
}

impl MessageKind {
    pub fn is_searchable_by_default(self) -> bool {
        !matches!(self, Self::Thinking)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: Option<String>,
    pub kind: MessageKind,
    pub participant: Participant,
    pub content: String,
    pub timestamp: Option<i64>,
    pub parent_id: Option<String>,
    pub associated_id: Option<String>,
    pub depth: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ConversationLoadRef {
    ClaudeFile { path: String },
    CodexFiles { paths: Vec<String> },
    Indexed { conversation_id: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub external_id: Option<String>,
    pub branch_parent_id: Option<String>,
    pub branch_anchor_message_id: Option<String>,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub provider: ProviderKind,
    pub created_at: i64,
    pub updated_at: i64,
    pub segments: Vec<ConversationSegment>,
    pub messages: Vec<Message>,
    pub is_hydrated: bool,
    pub load_ref: Option<ConversationLoadRef>,
}

impl Conversation {
    pub fn has_segments(&self) -> bool {
        self.segments.len() > 1
    }

    pub fn has_loaded_messages(&self) -> bool {
        self.is_hydrated
    }

    pub fn preview_line(&self) -> Option<&str> {
        self.messages
            .iter()
            .filter(|message| message.kind == MessageKind::UserMessage)
            .find_map(first_non_empty_line)
            .or_else(|| self.messages.iter().find_map(first_non_empty_line))
            .or_else(|| {
                self.preview.as_deref().and_then(|preview| {
                    if is_claude_scaffold_only(preview)
                        || starts_with_claude_scaffold_prefix(preview)
                        || is_bare_structural_tag_line(preview.trim())
                    {
                        None
                    } else {
                        first_meaningful_line(preview)
                    }
                })
            })
    }

    pub fn display_title(&self) -> String {
        if let Some(title) = &self.title {
            if !title.trim().is_empty() {
                if let Some(displayable) = displayable_claude_scaffold_text(title) {
                    return displayable;
                }
                if !is_claude_scaffold_only(title)
                    && !starts_with_claude_scaffold_prefix(title)
                    && !is_bare_structural_tag_line(title.trim())
                {
                    if let Some(line) = first_meaningful_line(title) {
                        return line.to_string();
                    }
                }
            }
        }

        self.preview_line()
            .map(truncate_preview_line)
            .or_else(|| {
                self.messages
                    .iter()
                    .filter(|message| message.kind == MessageKind::UserMessage)
                    .find_map(|message| displayable_claude_scaffold_text(&message.content))
            })
            .or_else(|| {
                self.messages
                    .iter()
                    .filter(|message| message.kind == MessageKind::UserMessage)
                    .find_map(|message| parse_codex_shell_command_text(&message.content))
                    .map(str::to_string)
            })
            .or_else(|| {
                self.preview
                    .as_deref()
                    .and_then(displayable_claude_scaffold_text)
            })
            .unwrap_or_else(|| self.id.clone())
    }

    pub fn opening_prompt_text(&self) -> Option<String> {
        self.meaningful_user_lines().next().map(str::to_string)
    }

    pub fn early_user_context_text(&self) -> Option<String> {
        let lines = self.meaningful_user_lines().take(3).collect::<Vec<_>>();
        (!lines.is_empty()).then(|| lines.join(" "))
    }

    pub fn latest_activity_line(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find_map(first_non_empty_line)
            .or_else(|| self.preview_line())
    }

    fn meaningful_user_lines(&self) -> impl Iterator<Item = &str> {
        self.messages
            .iter()
            .filter(|message| message.kind == MessageKind::UserMessage)
            .filter_map(first_non_empty_line)
            .scan(None, |last: &mut Option<String>, line| {
                if last.as_deref() == Some(line) {
                    Some(None)
                } else {
                    *last = Some(line.to_string());
                    Some(Some(line))
                }
            })
            .flatten()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConversationSegment {
    pub id: String,
    pub label: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_start_idx: usize,
    pub message_count: usize,
}

fn first_non_empty_line(message: &Message) -> Option<&str> {
    if let Some(line) = first_meaningful_line(&message.content) {
        return Some(line);
    }
    if is_claude_scaffold_only(&message.content) {
        return None;
    }
    if is_codex_shell_command_only(&message.content) {
        return None;
    }
    None
}

fn truncate_preview_line(line: &str) -> String {
    if line.len() > 60 {
        format!("{}...", &line[..57])
    } else {
        line.to_string()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub display_name: String,
    pub source_path: Option<String>,
    pub updated_at: i64,
    pub conversations: Vec<Conversation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(participant: Participant, content: &str) -> Message {
        Message {
            id: None,
            kind: match participant {
                Participant::User => MessageKind::UserMessage,
                Participant::Assistant { .. } => MessageKind::AssistantMessage,
                Participant::Tool { .. } => MessageKind::ToolResult,
                Participant::System | Participant::Unknown { .. } => MessageKind::MetadataChange,
            },
            participant,
            content: content.into(),
            timestamp: None,
            parent_id: None,
            associated_id: None,
            depth: 0,
        }
    }

    #[test]
    fn display_title_prefers_first_user_message() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::Codex,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(Participant::System, "<collaboration_mode># Collaboration"),
                message(Participant::User, "Actual user prompt"),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.display_title(), "Actual user prompt");
        assert_eq!(conversation.preview_line(), Some("Actual user prompt"));
    }

    #[test]
    fn display_title_falls_back_when_no_user_message_exists() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![message(Participant::System, "Only system content")],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.display_title(), "Only system content");
        assert_eq!(conversation.preview_line(), Some("Only system content"));
    }

    #[test]
    fn parse_claude_scaffold_artifact_extracts_known_tags() {
        let artifact =
            parse_claude_scaffold_artifact("<command-name>/clear</command-name>").unwrap();

        assert_eq!(artifact.kind, ClaudeScaffoldArtifactKind::CommandName);
        assert_eq!(artifact.body, "/clear");
    }

    #[test]
    fn parse_claude_scaffold_artifact_extracts_extended_tags() {
        let artifact =
            parse_claude_scaffold_artifact("<bash-input>just backend check</bash-input>").unwrap();

        assert_eq!(artifact.kind, ClaudeScaffoldArtifactKind::BashInput);
        assert_eq!(artifact.body, "just backend check");
    }

    #[test]
    fn display_title_skips_claude_scaffolding_messages() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(
                    Participant::System,
                    "<local-command-caveat>Caveat: hidden</local-command-caveat>",
                ),
                message(Participant::User, "Actual prompt"),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.display_title(), "Actual prompt");
        assert_eq!(conversation.preview_line(), Some("Actual prompt"));
    }

    #[test]
    fn display_title_sanitizes_scaffold_preview_text() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: Some("<command-name>/clear</command-name>".into()),
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![],
            is_hydrated: false,
            load_ref: None,
        };

        assert_eq!(conversation.preview_line(), None);
        assert_eq!(conversation.display_title(), "/clear");
    }

    #[test]
    fn display_title_sanitizes_truncated_scaffold_preview_text() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: Some("<bash-stdout></bash-stdout><bash-stderr>cargo fmt".into()),
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![],
            is_hydrated: false,
            load_ref: None,
        };

        assert_eq!(conversation.preview_line(), None);
        assert_eq!(conversation.display_title(), "cargo fmt");
    }

    #[test]
    fn display_title_hides_local_command_stdout_preview_text() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: Some("<local-command-stdout>Login successful</local-command-stdout>".into()),
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![],
            is_hydrated: false,
            load_ref: None,
        };

        assert_eq!(conversation.preview_line(), None);
        assert_eq!(conversation.display_title(), "conv-1");
    }

    #[test]
    fn display_title_skips_raw_scaffold_sequence_title_and_uses_meaningful_message() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: Some("<bash-stdout></bash-stdout><bash-stderr>cargo fmt</bash-stderr>".into()),
            preview: Some("<bash-stdout></bash-stdout><bash-stderr>cargo fmt</bash-stderr>".into()),
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(
                    Participant::User,
                    "<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>",
                ),
                message(Participant::Assistant { provider: ProviderKind::ClaudeCode }, "No response requested."),
                message(Participant::User, "let's fix these issues."),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.preview_line(), Some("let's fix these issues."));
        assert_eq!(conversation.display_title(), "let's fix these issues.");
    }

    #[test]
    fn display_title_falls_back_to_command_name_for_command_only_session() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(
                    Participant::User,
                    "<command-name>/login</command-name>\n<command-message>login</command-message>\n<command-args></command-args>",
                ),
                message(
                    Participant::User,
                    "<local-command-stdout>Login successful</local-command-stdout>",
                ),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.preview_line(), None);
        assert_eq!(conversation.display_title(), "/login");
    }

    #[test]
    fn preview_line_skips_bare_structural_tags() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: Some("<role>\nYou are Codex.\n</role>".into()),
            provider: ProviderKind::Codex,
            created_at: 0,
            updated_at: 0,
            segments: Vec::new(),
            messages: vec![message(
                Participant::User,
                "<role>\nYou are Codex.\n</role>",
            )],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.preview_line(), Some("You are Codex."));
        assert_eq!(conversation.display_title(), "You are Codex.");
    }

    #[test]
    fn display_title_ignores_bare_tag_only_title() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: Some("<role>".into()),
            preview: Some("<role>\nReview the diff.\n</role>".into()),
            provider: ProviderKind::Codex,
            created_at: 0,
            updated_at: 0,
            segments: Vec::new(),
            messages: vec![message(
                Participant::User,
                "<role>\nReview the diff.\n</role>",
            )],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.display_title(), "Review the diff.");
    }

    #[test]
    fn preview_line_strips_leading_structural_tags_from_inline_text() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: Some("<context>User initiated a review task.</context>".into()),
            provider: ProviderKind::Codex,
            created_at: 0,
            updated_at: 0,
            segments: Vec::new(),
            messages: vec![message(
                Participant::User,
                "<context>User initiated a review task.</context>",
            )],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(
            conversation.preview_line(),
            Some("User initiated a review task.")
        );
        assert_eq!(
            conversation.display_title(),
            "User initiated a review task."
        );
    }

    #[test]
    fn preview_line_skips_multiline_scaffold_message_and_uses_next_user_message() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: Some("<command-name>/clear</command-name>".into()),
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(
                    Participant::User,
                    "<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>",
                ),
                message(Participant::User, "Actual prompt"),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.preview_line(), Some("Actual prompt"));
        assert_eq!(conversation.display_title(), "Actual prompt");
    }

    #[test]
    fn preview_line_skips_request_interrupted_status_lines() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: Some("lightdash-weekly-analytics-dashboard".into()),
            preview: None,
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(Participant::User, "[Request interrupted by user for tool use]"),
                message(
                    Participant::User,
                    "We need programmatic access to a Lightdash instance running on Cloud Run behind IAP.",
                ),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(
            conversation.preview_line(),
            Some("We need programmatic access to a Lightdash instance running on Cloud Run behind IAP.")
        );
        assert_eq!(
            conversation.display_title(),
            "lightdash-weekly-analytics-dashboard"
        );
    }

    #[test]
    fn display_title_skips_codex_shell_command_wrapper_when_prose_follows() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::Codex,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(
                    Participant::User,
                    "<user_shell_command>\n<command>\nwhich lightdash\n</command>\n<result>\nExit code: 1\n</result>\n</user_shell_command>",
                ),
                message(
                    Participant::User,
                    "let's debug your environment. lightdash is available in my shell.",
                ),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(
            conversation.preview_line(),
            Some("let's debug your environment. lightdash is available in my shell.")
        );
        assert_eq!(
            conversation.display_title(),
            "let's debug your environment. lightdash is available in m..."
        );
        assert_eq!(
            conversation.opening_prompt_text().as_deref(),
            Some("let's debug your environment. lightdash is available in my shell.")
        );
    }

    #[test]
    fn display_title_falls_back_to_codex_shell_command_for_command_only_session() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::Codex,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![message(
                Participant::User,
                "<user_shell_command>\n<command>\nwhich lightdash\n</command>\n<result>\nExit code: 0\n</result>\n</user_shell_command>",
            )],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.preview_line(), None);
        assert_eq!(conversation.display_title(), "which lightdash");
    }

    #[test]
    fn latest_activity_line_prefers_last_meaningful_message() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: Some("Initial topic".into()),
            preview: Some("Initial topic".into()),
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: Vec::new(),
            messages: vec![
                message(Participant::User, "Initial topic"),
                message(
                    Participant::Assistant {
                        provider: ProviderKind::ClaudeCode,
                    },
                    "First response",
                ),
                message(Participant::User, "Most recent follow-up"),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.latest_activity_line(), Some("Most recent follow-up"));
    }

    #[test]
    fn opening_prompt_and_early_context_use_first_meaningful_user_lines() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: Some("sticky-note".into()),
            preview: None,
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(Participant::User, "<command-name>/clear</command-name>"),
                message(
                    Participant::User,
                    "Some websites seem to get classified as thought.",
                ),
                message(
                    Participant::Assistant {
                        provider: ProviderKind::ClaudeCode,
                    },
                    "I’ll investigate.",
                ),
                message(Participant::User, "Check item 123."),
                message(Participant::User, "Check item 123."),
                message(Participant::User, "Compare it to sticky-note behavior."),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(
            conversation.opening_prompt_text().as_deref(),
            Some("Some websites seem to get classified as thought.")
        );
        assert_eq!(
            conversation.early_user_context_text().as_deref(),
            Some(
                "Some websites seem to get classified as thought. Check item 123. Compare it to sticky-note behavior."
            )
        );
    }
}
