use crate::theme::Theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState},
    Frame,
};
use shared::{MessagePreview, ViewContent, ViewModel};

pub fn render_messages_list(f: &mut Frame, area: Rect, messages: &[MessagePreview], theme: &Theme) {
    let mut list_items = Vec::new();
    let wrap_width = area.width.saturating_sub(4) as usize;
    let wrap_width = if wrap_width == 0 { 80 } else { wrap_width };

    let mut focused_item_index = 0;
    let mut current_item_idx = 0;

    for msg in messages {
        if msg.is_focused {
            focused_item_index = current_item_idx;
        }

        let is_user = msg.participant_label == "You";
        let (color, prefix) = if is_user {
            (theme.user_msg, msg.participant_label.as_str())
        } else {
            (theme.assistant_msg, msg.participant_label.as_str())
        };

        let focus_prefix = if msg.is_focused { "▎ " } else { "  " };
        let focus_style = if msg.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };

        let mut header_spans = vec![
            Span::styled(focus_prefix, focus_style),
            Span::styled(
                prefix,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ];

        if let Some(time) = &msg.relative_time {
            header_spans.push(Span::styled(
                format!(" {}", time),
                Style::default().fg(theme.dim),
            ));
        }

        list_items.push(ListItem::new(Line::from(header_spans)));
        list_items.push(ListItem::new(Line::from(vec![Span::styled(
            focus_prefix,
            focus_style,
        )])));
        current_item_idx += 2;

        let wrapped_lines = textwrap::wrap(&msg.content, wrap_width);
        let total_lines = wrapped_lines.len();

        if total_lines <= 12 || msg.is_expanded {
            for line in wrapped_lines {
                list_items.push(ListItem::new(Line::from(vec![
                    Span::styled(focus_prefix, focus_style),
                    Span::raw(line.to_string()),
                ])));
                current_item_idx += 1;
            }
        } else {
            for line in &wrapped_lines[0..6] {
                list_items.push(ListItem::new(Line::from(vec![
                    Span::styled(focus_prefix, focus_style),
                    Span::raw(line.to_string()),
                ])));
                current_item_idx += 1;
            }

            let hidden = total_lines - 11;
            let trunc_msg = format!("... ({} more lines)", hidden);
            list_items.push(ListItem::new(Line::from(vec![
                Span::styled(focus_prefix, focus_style),
                Span::styled(
                    trunc_msg,
                    Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
                ),
            ])));
            current_item_idx += 1;

            for line in &wrapped_lines[total_lines - 5..] {
                list_items.push(ListItem::new(Line::from(vec![
                    Span::styled(focus_prefix, focus_style),
                    Span::raw(line.to_string()),
                ])));
                current_item_idx += 1;
            }
        }

        list_items.push(ListItem::new(Line::from(vec![Span::styled(
            focus_prefix,
            focus_style,
        )])));
        current_item_idx += 1;
    }

    let list = List::new(list_items).block(Block::default());
    let mut preview_state = ListState::default();
    preview_state.select(Some(focused_item_index));
    f.render_stateful_widget(list, area, &mut preview_state);
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

            render_messages_list(f, split_layout[2], right_messages, theme);
        }
        ViewContent::MessagesList(messages) => {
            render_messages_list(f, padded_content[1], messages, theme);
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

    let footer_spans = vec![
        Span::styled(
            format!("  {}  ", view_model.filter_text),
            Style::default().fg(theme.dim),
        ),
        Span::styled(
            " [↑/k ↓/j] Navigate  [J/K] Messages  [e] Expand  [Enter/l] Select  [Esc/h] Back  [Y/y] Copy  [q] Quit ",
            Style::default().fg(theme.dim),
        ),
    ];
    let footer = Paragraph::new(Line::from(footer_spans));
    f.render_widget(footer, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils;
    use ratatui::backend::TestBackend;

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
                        participant_label: "You".into(),
                        content: "hello".into(),
                        is_focused: true,
                        is_expanded: false,
                        relative_time: Some("1d ago".into()),
                    },
                    MessagePreview {
                        participant_label: "Codex".into(),
                        content: "hi there".into(),
                        is_focused: false,
                        is_expanded: false,
                        relative_time: Some("1d ago".into()),
                    },
                ],
            },
            selected_index: 0,
            filter_text: "Filter: All".into(),
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
}
