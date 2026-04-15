use crate::theme::Theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState},
    Frame,
};
use shared::{MessageKind, MessagePreview, TreeRowKind, TreeRowPreview, ViewContent, ViewModel};

struct RenderedMessageBlock {
    lines: Vec<Line<'static>>,
}

impl RenderedMessageBlock {
    fn line_count(&self) -> usize {
        self.lines.len()
    }
}

fn selected_message_idx(
    messages: &[MessagePreview],
    selected_item_index: Option<usize>,
    show_focus: bool,
) -> usize {
    if let Some(idx) = selected_item_index {
        if let Some((preview_idx, _)) = messages
            .iter()
            .enumerate()
            .find(|(_, msg)| msg.source_index == idx)
        {
            return preview_idx;
        }
        return messages.len().saturating_sub(1);
    }

    if show_focus {
        if let Some((idx, _)) = messages.iter().enumerate().find(|(_, msg)| msg.is_focused) {
            return idx;
        }
    }

    0
}

fn render_message_block(
    msg: &MessagePreview,
    wrap_width: usize,
    theme: &Theme,
    is_selected_message: bool,
    show_focus: bool,
    tree_mode: bool,
) -> RenderedMessageBlock {
    let item_style = if is_selected_message {
        Style::default().bg(theme.selected_bg)
    } else {
        Style::default()
    };

    let (color, prefix) = match msg.kind {
        MessageKind::UserMessage => (theme.user_msg, msg.participant_label.as_str()),
        MessageKind::AssistantMessage => (theme.assistant_msg, msg.participant_label.as_str()),
        MessageKind::ToolCall => (Color::Yellow, "Tool Call"),
        MessageKind::ToolResult => (Color::LightYellow, "Tool Result"),
        MessageKind::Thinking => (theme.dim, "Thinking"),
        MessageKind::Summary => (Color::LightBlue, "Summary"),
        MessageKind::Compaction => (Color::Magenta, "Compaction"),
        MessageKind::Label => (Color::Green, "Label"),
        MessageKind::MetadataChange => (theme.dim, msg.participant_label.as_str()),
    };

    let indent = if tree_mode && msg.depth > 0 {
        format!("{}↳ ", "  ".repeat(msg.depth.saturating_sub(1)))
    } else {
        "  ".repeat(msg.depth)
    };
    let focus_prefix = if show_focus && msg.is_focused {
        "▎ "
    } else {
        "  "
    };
    let focus_style = if show_focus && msg.is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let mut lines = Vec::new();
    let mut header_spans = vec![
        Span::styled(focus_prefix.to_string(), focus_style),
        Span::raw(indent.clone()),
        Span::styled(
            prefix.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(time) = &msg.relative_time {
        header_spans.push(Span::styled(
            format!(" {}", time),
            Style::default().fg(theme.dim),
        ));
    }

    lines.push(Line::from(header_spans).style(item_style));
    lines.push(
        Line::from(vec![Span::styled(focus_prefix.to_string(), focus_style)]).style(item_style),
    );

    let wrapped_lines = textwrap::wrap(&msg.content, wrap_width);
    let total_lines = wrapped_lines.len();

    if total_lines <= 12 || msg.is_expanded {
        for line in wrapped_lines {
            lines.push(
                Line::from(vec![
                    Span::styled(focus_prefix.to_string(), focus_style),
                    Span::raw(indent.clone()),
                    Span::raw(line.to_string()),
                ])
                .style(item_style),
            );
        }
    } else {
        for line in &wrapped_lines[0..6] {
            lines.push(
                Line::from(vec![
                    Span::styled(focus_prefix.to_string(), focus_style),
                    Span::raw(indent.clone()),
                    Span::raw(line.to_string()),
                ])
                .style(item_style),
            );
        }

        let hidden = total_lines - 11;
        lines.push(
            Line::from(vec![
                Span::styled(focus_prefix.to_string(), focus_style),
                Span::raw(indent.clone()),
                Span::styled(
                    format!("... ({} more lines)", hidden),
                    Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
                ),
            ])
            .style(item_style),
        );

        for line in &wrapped_lines[total_lines - 5..] {
            lines.push(
                Line::from(vec![
                    Span::styled(focus_prefix.to_string(), focus_style),
                    Span::raw(indent.clone()),
                    Span::raw(line.to_string()),
                ])
                .style(item_style),
            );
        }
    }

    lines.push(
        Line::from(vec![Span::styled(focus_prefix.to_string(), focus_style)]).style(item_style),
    );
    RenderedMessageBlock { lines }
}

fn visible_message_blocks(
    messages: &[MessagePreview],
    selected_item_index: Option<usize>,
    show_focus: bool,
    area: Rect,
    theme: &Theme,
    tree_mode: bool,
) -> Vec<RenderedMessageBlock> {
    if messages.is_empty() || area.height == 0 {
        return Vec::new();
    }

    let wrap_width = area.width.saturating_sub(4) as usize;
    let wrap_width = if wrap_width == 0 { 80 } else { wrap_width };
    let selected_idx = selected_message_idx(messages, selected_item_index, show_focus);

    let mut start = selected_idx;
    let mut end = selected_idx + 1;
    let selected_block = render_message_block(
        &messages[selected_idx],
        wrap_width,
        theme,
        selected_item_index == Some(messages[selected_idx].source_index),
        show_focus,
        tree_mode,
    );
    let mut total_lines = selected_block.line_count();
    let target_lines = area.height as usize;
    let mut blocks_after = vec![selected_block];
    let mut blocks_before = Vec::new();

    while total_lines < target_lines && (start > 0 || end < messages.len()) {
        if start > 0 {
            let prev_idx = start - 1;
            let block = render_message_block(
                &messages[prev_idx],
                wrap_width,
                theme,
                selected_item_index == Some(messages[prev_idx].source_index),
                show_focus,
                tree_mode,
            );
            total_lines += block.line_count();
            blocks_before.push(block);
            start = prev_idx;
        }

        if total_lines >= target_lines {
            break;
        }

        if end < messages.len() {
            let next_idx = end;
            let block = render_message_block(
                &messages[next_idx],
                wrap_width,
                theme,
                selected_item_index == Some(messages[next_idx].source_index),
                show_focus,
                tree_mode,
            );
            total_lines += block.line_count();
            blocks_after.push(block);
            end += 1;
        }
    }

    blocks_before.reverse();
    blocks_before.extend(blocks_after);
    blocks_before
}

pub fn render_messages_list(
    f: &mut Frame,
    area: Rect,
    messages: &[MessagePreview],
    theme: &Theme,
    selected_item_index: Option<usize>,
    show_focus: bool,
    tree_mode: bool,
) {
    let visible_blocks = visible_message_blocks(
        messages,
        selected_item_index,
        show_focus,
        area,
        theme,
        tree_mode,
    );
    let visible_lines = visible_blocks
        .into_iter()
        .flat_map(|block| block.lines)
        .take(area.height as usize)
        .collect::<Vec<_>>();

    let paragraph = Paragraph::new(visible_lines).block(Block::default());
    f.render_widget(paragraph, area);
}

fn render_tree_list(
    f: &mut Frame,
    area: Rect,
    rows: &[TreeRowPreview],
    selected_index: usize,
    theme: &Theme,
) {
    fn truncate_for_width(value: &str, width: usize) -> String {
        if width == 0 {
            return String::new();
        }

        let char_count = value.chars().count();
        if char_count <= width {
            return value.to_string();
        }

        if width == 1 {
            return "…".to_string();
        }

        let mut truncated = value.chars().take(width - 1).collect::<String>();
        truncated.push('…');
        truncated
    }

    fn split_tree_row_meta(row: &TreeRowPreview) -> (String, String) {
        let Some(secondary) = row.secondary.as_deref() else {
            return (String::new(), String::new());
        };

        match row.kind {
            TreeRowKind::Conversation | TreeRowKind::BranchConversation => {
                let without_branch = secondary.strip_prefix("Branch • ").unwrap_or(secondary);
                let mut parts = without_branch.rsplitn(2, ' ');
                let time = parts.next().unwrap_or_default().to_string();
                let meta = parts.next().unwrap_or_default().to_string();
                (meta, time)
            }
            _ => {
                if let Some((meta, time)) = secondary.rsplit_once(" • ") {
                    (meta.to_string(), time.to_string())
                } else {
                    (secondary.to_string(), String::new())
                }
            }
        }
    }

    let (meta_width, time_width) = if area.width < 72 {
        (12usize, 9usize)
    } else {
        (16usize, 12usize)
    };
    let label_width = area
        .width
        .saturating_sub((meta_width + time_width + 2) as u16) as usize;

    let items = rows
        .iter()
        .map(|row| {
            let marker = if row.is_expandable {
                if row.is_expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };
            let indent = "  ".repeat(row.depth);
            let mut spans = vec![Span::raw(format!("{indent}{marker}"))];
            let kind_prefix = match row.kind {
                TreeRowKind::Conversation => "",
                TreeRowKind::BranchConversation => "⎇ ",
                TreeRowKind::BranchAnchor => "⤴ ",
                TreeRowKind::OpeningPrompt => "• ",
                TreeRowKind::Entry => "",
                TreeRowKind::Delegation => "↳ ",
                TreeRowKind::DelegationSummary => "… ",
                TreeRowKind::Summary => "… ",
            };
            let row_style = match row.kind {
                TreeRowKind::Conversation => Style::default().add_modifier(if row.depth == 0 {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
                TreeRowKind::BranchConversation => Style::default().fg(theme.assistant_msg),
                TreeRowKind::BranchAnchor => Style::default()
                    .fg(theme.assistant_msg)
                    .add_modifier(Modifier::BOLD),
                TreeRowKind::OpeningPrompt => Style::default().add_modifier(Modifier::BOLD),
                TreeRowKind::Entry => Style::default(),
                TreeRowKind::Delegation => Style::default().fg(theme.dim),
                TreeRowKind::DelegationSummary => Style::default().fg(theme.dim),
                TreeRowKind::Summary => Style::default().fg(theme.dim),
            };
            let label = truncate_for_width(&format!("{kind_prefix}{}", row.label), label_width);
            spans.push(Span::styled(label, row_style));

            let (meta, time) = split_tree_row_meta(row);
            let meta = truncate_for_width(&meta, meta_width);
            let time = truncate_for_width(&time, time_width);

            let cells = vec![
                Cell::from(Line::from(spans)),
                Cell::from(Line::from(vec![Span::styled(
                    meta,
                    Style::default().fg(theme.dim),
                )])),
                Cell::from(Line::from(vec![Span::styled(
                    time,
                    Style::default().fg(theme.dim),
                )])),
            ];

            Row::new(cells)
        })
        .collect::<Vec<_>>();

    let header = Row::new(vec![
        Cell::from(Span::styled("Node", Style::default().fg(theme.dim))),
        Cell::from(Span::styled("Agent", Style::default().fg(theme.dim))),
        Cell::from(Span::styled("Last Active", Style::default().fg(theme.dim))),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let table = Table::new(
        items,
        [
            Constraint::Min(10),
            Constraint::Length(meta_width as u16),
            Constraint::Length(time_width as u16),
        ],
    )
    .header(header)
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .column_spacing(1);

    let mut state = TableState::default();
    state.select(Some(selected_index));
    f.render_stateful_widget(table, area, &mut state);
}

pub fn render_ui(
    f: &mut Frame,
    view_model: &ViewModel,
    list_state: &mut ListState,
    table_state: &mut TableState,
    theme: &Theme,
) {
    let area = f.size();

    f.render_widget(Block::default(), area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let padded_content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(chunks[1]);

    let header_spans = vec![
        Span::raw("   "),
        Span::styled(
            view_model.breadcrumb.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];
    let header = Paragraph::new(Line::from(header_spans)).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border)),
    );
    f.render_widget(header, chunks[0]);

    match &view_model.content {
        ViewContent::Table { headers, rows } => {
            let header_cells = headers
                .iter()
                .map(|h| Cell::from(h.as_str()).style(Style::default().fg(theme.dim)));
            let header_row = Row::new(header_cells)
                .style(Style::default().add_modifier(Modifier::BOLD))
                .height(1)
                .bottom_margin(1);

            let items: Vec<Row> = rows
                .iter()
                .enumerate()
                .map(|(idx, row_data)| {
                    let is_selected = Some(idx) == table_state.selected();
                    let style = if is_selected {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };

                    let cells = row_data.iter().map(|s| Cell::from(format!(" {} ", s)));
                    Row::new(cells).style(style)
                })
                .collect();

            let widths = if headers.len() == 4 {
                vec![
                    Constraint::Percentage(40),
                    Constraint::Percentage(20),
                    Constraint::Percentage(20),
                    Constraint::Percentage(20),
                ]
            } else {
                vec![
                    Constraint::Percentage(60),
                    Constraint::Percentage(20),
                    Constraint::Percentage(20),
                ]
            };

            let table = Table::new(items, widths)
                .header(header_row)
                .block(Block::default());

            f.render_stateful_widget(table, padded_content[1], table_state);
        }
        ViewContent::Split {
            conversations,
            right_messages,
        } => {
            let split_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Length(2),
                    Constraint::Percentage(60),
                ])
                .split(padded_content[1]);

            let mut left_list_items = Vec::new();
            let snippet_width = split_layout[0].width.saturating_sub(6) as usize;
            let snippet_width = if snippet_width == 0 {
                80
            } else {
                snippet_width
            };

            for c in conversations {
                let is_selected = c.is_selected;

                let header_style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };

                let bold_style = if is_selected {
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                };

                let provider_color = if c.provider_label.to_lowercase().contains("claude") {
                    theme.assistant_msg
                } else {
                    theme.user_msg
                };

                let dim_style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().fg(theme.dim)
                };

                let provider_style = if is_selected {
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .fg(provider_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(provider_color)
                };

                let header_line = Line::from(vec![
                    Span::styled(" ", dim_style),
                    Span::styled(format!("{} ", c.title), bold_style),
                    Span::styled(" ● ", provider_style),
                    Span::styled(format!("{} ", c.provider_label), dim_style),
                    Span::styled(format!(" {} ", c.relative_time), dim_style),
                ]);

                let raw_snippet = c.snippet.replace('\n', " ");
                let clean_snippet = raw_snippet.trim();
                let wrapped_snippet = textwrap::wrap(clean_snippet, snippet_width);
                let snippet_str = wrapped_snippet
                    .first()
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let snippet_line = Line::from(vec![Span::styled(
                    format!("    {}", snippet_str),
                    dim_style,
                )]);

                let empty_line = Line::from(vec![Span::styled("", header_style)]);

                left_list_items.push(ListItem::new(vec![header_line, snippet_line, empty_line]));
            }

            let left_list = List::new(left_list_items).block(Block::default());
            let mut left_state = ListState::default();
            left_state.select(Some(view_model.selected_index));
            f.render_stateful_widget(left_list, split_layout[0], &mut left_state);

            render_messages_list(f, split_layout[2], right_messages, theme, None, true, false);
        }
        ViewContent::TreeList(rows) => {
            render_tree_list(f, padded_content[1], rows, view_model.selected_index, theme);
        }
        ViewContent::HistoryList(messages) => {
            render_messages_list(
                f,
                padded_content[1],
                messages,
                theme,
                Some(view_model.selected_index),
                false,
                true,
            );
        }
        ViewContent::MessagesList(messages) => {
            render_messages_list(
                f,
                padded_content[1],
                messages,
                theme,
                Some(view_model.selected_index),
                false,
                false,
            );
        }
        ViewContent::List(list_items) => {
            let items: Vec<ListItem> = list_items
                .iter()
                .map(|i| ListItem::new(Line::from(vec![Span::raw(i.to_string())])))
                .collect();

            let list = List::new(items)
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("┃ ");
            f.render_stateful_widget(list, padded_content[1], list_state);
        }
    }

    let bindings_text = match &view_model.content {
        ViewContent::TreeList(_) => {
            " [↑/k ↓/j] Navigate  [e/l] Expand  [Enter] Read  [C] Screen Ref  [Y/y] Copy  [Esc/h] Back  [q] Quit "
        }
        ViewContent::HistoryList(_) => {
            " [↑/k ↓/j] Navigate  [Enter/l] Read  [e] Expand  [C] Screen Ref  [Y/y] Copy  [Esc/h] Back  [q] Quit "
        }
        ViewContent::MessagesList(_) => {
            " [↑/↓] Scroll  [C] Screen Ref  [Y/y] Copy  [Esc/h] Back  [q] Quit "
        }
        _ => {
            " [↑/k ↓/j] Navigate  [e] Expand  [Enter/l] Select  [C] Screen Ref  [Y/y] Copy  [Esc/h] Back  [q] Quit "
        }
    };

    let footer_spans = vec![
        Span::styled(
            format!("  {}  ", view_model.filter_text),
            Style::default().fg(theme.dim),
        ),
        Span::styled(
            view_model
                .status_text
                .as_ref()
                .map(|status| format!("  {status}  "))
                .unwrap_or_default(),
            Style::default().fg(theme.assistant_msg),
        ),
        Span::styled(bindings_text, Style::default().fg(theme.dim)),
    ];
    let footer = Paragraph::new(Line::from(footer_spans));
    f.render_widget(footer, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils;
    use ratatui::{backend::TestBackend, Terminal};
    use shared::ConversationPreview;

    #[test]
    fn test_render_workspaces_table() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        let view_model = ViewModel {
            title: "Workspaces".into(),
            breadcrumb: "Workspaces".into(),
            active_id: None,
            content: ViewContent::Table {
                headers: vec!["Workspace".into(), "Convs".into(), "Last Active".into()],
                rows: vec![
                    vec!["~/.config/konfigue".into(), "5 convs".into(), "2h".into()],
                    vec!["~/recall".into(), "1 conv".into(), "1d".into()],
                ],
            },
            selected_index: 0,
            filter_text: "Filter: All".into(),
            status_text: None,
        };

        let theme = Theme::default();
        let mut list_state = ListState::default();
        let mut table_state = TableState::default();
        table_state.select(Some(view_model.selected_index));

        terminal
            .draw(|f| {
                render_ui(f, &view_model, &mut list_state, &mut table_state, &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_str = test_utils::buffer_to_string(buffer);

        assert!(buffer_str.contains(".config/konfigue"));
        assert!(buffer_str.contains("5 convs"));
        assert!(buffer_str.contains("2h"));
        assert!(buffer_str.contains("recall"));
        assert!(buffer_str.contains("1 conv"));
        assert!(buffer_str.contains("1d"));
    }

    #[test]
    fn test_render_split_pane() {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let view_model = ViewModel {
            title: "Conversations".into(),
            breadcrumb: "Workspaces > test".into(),
            active_id: Some("conv-1".into()),
            content: ViewContent::Split {
                conversations: vec![
                    ConversationPreview {
                        id: "conv-1".into(),
                        title: "conv-1".into(),
                        provider_label: "Claude Code".into(),
                        relative_time: "1d ago".into(),
                        snippet: "hello".into(),
                        is_selected: true,
                    },
                    ConversationPreview {
                        id: "conv-2".into(),
                        title: "conv-2".into(),
                        provider_label: "Codex".into(),
                        relative_time: "1d ago".into(),
                        snippet: "hi there".into(),
                        is_selected: false,
                    },
                ],
                right_messages: vec![
                    MessagePreview {
                        source_index: 0,
                        kind: MessageKind::UserMessage,
                        participant_label: "You".into(),
                        content: "hello".into(),
                        depth: 0,
                        is_focused: true,
                        is_expanded: false,
                        relative_time: Some("1d ago".into()),
                    },
                    MessagePreview {
                        source_index: 1,
                        kind: MessageKind::AssistantMessage,
                        participant_label: "Codex".into(),
                        content: "hi there".into(),
                        depth: 0,
                        is_focused: false,
                        is_expanded: false,
                        relative_time: Some("1d ago".into()),
                    },
                ],
            },
            selected_index: 0,
            filter_text: "Filter: All".into(),
            status_text: None,
        };

        let theme = Theme::default();
        let mut list_state = ListState::default();
        let mut table_state = TableState::default();
        table_state.select(Some(view_model.selected_index));

        terminal
            .draw(|f| {
                render_ui(f, &view_model, &mut list_state, &mut table_state, &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_str = test_utils::buffer_to_string(buffer);

        assert!(buffer_str.contains("conv-1"));
        assert!(buffer_str.contains("hi there"));
    }

    #[test]
    fn transcript_render_windows_around_selected_message() {
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).unwrap();

        let view_model = ViewModel {
            title: "Messages".into(),
            breadcrumb: "Workspaces > test > conv".into(),
            active_id: Some("conv-1".into()),
            content: ViewContent::MessagesList(
                (0..6)
                    .map(|idx| MessagePreview {
                        source_index: idx,
                        kind: MessageKind::UserMessage,
                        participant_label: "You".into(),
                        content: format!("message-{}", idx),
                        depth: 0,
                        is_focused: false,
                        is_expanded: false,
                        relative_time: Some("now".into()),
                    })
                    .collect(),
            ),
            selected_index: 4,
            filter_text: "Filter: All".into(),
            status_text: None,
        };

        let theme = Theme::default();
        let mut list_state = ListState::default();
        let mut table_state = TableState::default();

        terminal
            .draw(|f| {
                render_ui(f, &view_model, &mut list_state, &mut table_state, &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_str = test_utils::buffer_to_string(buffer);

        assert!(buffer_str.contains("message-4"));
        assert!(!buffer_str.contains("message-0"));
    }

    #[test]
    fn tree_list_renders_aligned_columns_with_header() {
        let backend = TestBackend::new(90, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        let view_model = ViewModel {
            title: "Conversations".into(),
            breadcrumb: "Workspaces > bookmarking".into(),
            active_id: Some("parent".into()),
            content: ViewContent::TreeList(vec![
                TreeRowPreview {
                    id: "conv:parent".into(),
                    kind: TreeRowKind::Conversation,
                    label: "research-building-a-user-model".into(),
                    secondary: Some("Claude Code 2d".into()),
                    depth: 0,
                    is_selected: true,
                    is_expandable: true,
                    is_expanded: true,
                },
                TreeRowPreview {
                    id: "entry:anchor".into(),
                    kind: TreeRowKind::BranchAnchor,
                    label: "parent root".into(),
                    secondary: Some("Branch point • 2d".into()),
                    depth: 1,
                    is_selected: false,
                    is_expandable: false,
                    is_expanded: false,
                },
                TreeRowPreview {
                    id: "conv:child".into(),
                    kind: TreeRowKind::BranchConversation,
                    label: "sticky-note".into(),
                    secondary: Some("Branch • Claude Code 1d".into()),
                    depth: 2,
                    is_selected: false,
                    is_expandable: false,
                    is_expanded: false,
                },
            ]),
            selected_index: 0,
            filter_text: "Filter: Claude Code".into(),
            status_text: None,
        };

        let theme = Theme::default();
        let mut list_state = ListState::default();
        let mut table_state = TableState::default();

        terminal
            .draw(|f| {
                render_ui(f, &view_model, &mut list_state, &mut table_state, &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_str = test_utils::buffer_to_string(buffer);

        assert!(buffer_str.contains("Node"));
        assert!(buffer_str.contains("Agent"));
        assert!(buffer_str.contains("Last Active"));
        assert!(buffer_str.contains("research-building-a-user-model"));
        assert!(buffer_str.contains("Claude Code"));
        assert!(buffer_str.contains("Branch point"));
        assert!(buffer_str.contains("sticky-note"));
    }

    #[test]
    fn render_footer_shows_status_text() {
        let backend = TestBackend::new(100, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        let view_model = ViewModel {
            title: "Messages".into(),
            breadcrumb: "Workspaces > test > conv".into(),
            active_id: Some("conv-1".into()),
            content: ViewContent::MessagesList(vec![]),
            selected_index: 0,
            filter_text: "Filter: All".into(),
            status_text: Some("Saved screen ref: /tmp/ref.json".into()),
        };

        let theme = Theme::default();
        let mut list_state = ListState::default();
        let mut table_state = TableState::default();

        terminal
            .draw(|f| {
                render_ui(f, &view_model, &mut list_state, &mut table_state, &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_str = test_utils::buffer_to_string(buffer);

        assert!(buffer_str.contains("Saved screen ref: /tmp/ref.json"));
    }
}
