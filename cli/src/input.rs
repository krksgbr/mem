use anyhow::Result;
use crossterm::event::KeyCode;
use shared::{Event, ViewContent, ViewModel};

pub enum InputOutcome {
    Continue,
    Event(Event),
    CopyActiveId,
    Quit,
}

pub fn handle_key_code(view_model: &ViewModel, key_code: KeyCode) -> Result<InputOutcome> {
    let on_messages_screen = matches!(view_model.content, ViewContent::MessagesList(_));

    match key_code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Ok(InputOutcome::Quit),
        KeyCode::Up => Ok(InputOutcome::Event(if on_messages_screen {
            Event::MessageUp
        } else {
            Event::Up
        })),
        KeyCode::Down => Ok(InputOutcome::Event(if on_messages_screen {
            Event::MessageDown
        } else {
            Event::Down
        })),
        KeyCode::Char('k') => Ok(InputOutcome::Event(Event::Up)),
        KeyCode::Char('j') => Ok(InputOutcome::Event(Event::Down)),
        KeyCode::Enter => Ok(InputOutcome::Event(Event::Select)),
        KeyCode::Right | KeyCode::Char('l') => {
            if selected_row_is_expandable(view_model) {
                Ok(InputOutcome::Event(Event::ToggleMessage))
            } else {
                Ok(InputOutcome::Event(Event::Select))
            }
        }
        KeyCode::Esc | KeyCode::Backspace => Ok(InputOutcome::Event(Event::Back)),
        KeyCode::Left | KeyCode::Char('h') => {
            if selected_row_is_expanded(view_model) {
                Ok(InputOutcome::Event(Event::ToggleMessage))
            } else {
                Ok(InputOutcome::Event(Event::Back))
            }
        }
        KeyCode::Char('f') | KeyCode::Char('F') => Ok(InputOutcome::Event(Event::CycleFilter)),
        KeyCode::Char('e') | KeyCode::Char('E') => Ok(InputOutcome::Event(Event::ToggleMessage)),
        KeyCode::Char('~') => Ok(InputOutcome::Event(Event::ToggleLayout)),
        KeyCode::Char('Y') | KeyCode::Char('y') => Ok(InputOutcome::CopyActiveId),
        _ => Ok(InputOutcome::Continue),
    }
}

fn selected_row_is_expandable(view_model: &ViewModel) -> bool {
    match &view_model.content {
        ViewContent::TreeList(rows) => rows
            .get(view_model.selected_index)
            .map(|row| row.is_expandable)
            .unwrap_or(false),
        ViewContent::Table { rows, .. } => rows
            .get(view_model.selected_index)
            .and_then(|row| row.first())
            .map(|cell| cell.starts_with('▸') || cell.starts_with('▾'))
            .unwrap_or(false),
        _ => false,
    }
}

fn selected_row_is_expanded(view_model: &ViewModel) -> bool {
    match &view_model.content {
        ViewContent::TreeList(rows) => rows
            .get(view_model.selected_index)
            .map(|row| row.is_expanded)
            .unwrap_or(false),
        ViewContent::Table { rows, .. } => rows
            .get(view_model.selected_index)
            .and_then(|row| row.first())
            .map(|cell| cell.starts_with('▾'))
            .unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages_view() -> ViewModel {
        ViewModel {
            title: "Messages".into(),
            breadcrumb: "Workspaces > test > convo".into(),
            active_id: Some("conv-1".into()),
            content: ViewContent::MessagesList(vec![]),
            selected_index: 0,
            filter_text: "Filter: All".into(),
        }
    }

    fn history_view() -> ViewModel {
        ViewModel {
            title: "History".into(),
            breadcrumb: "Workspaces > test > convo > History".into(),
            active_id: Some("conv-1".into()),
            content: ViewContent::TreeList(vec![]),
            selected_index: 0,
            filter_text: "Filter: All".into(),
        }
    }

    fn tree_view(expandable: bool, expanded: bool) -> ViewModel {
        ViewModel {
            title: "Conversations".into(),
            breadcrumb: "Workspaces > test".into(),
            active_id: Some("conv-1".into()),
            content: ViewContent::TreeList(vec![shared::TreeRowPreview {
                id: "conv:1".into(),
                label: "dump-screen".into(),
                secondary: None,
                depth: 0,
                is_selected: true,
                is_expandable: expandable,
                is_expanded: expanded,
            }]),
            selected_index: 0,
            filter_text: "Filter: All".into(),
        }
    }

    fn table_view(selected_index: usize, first_cell: &str) -> ViewModel {
        ViewModel {
            title: "Conversations".into(),
            breadcrumb: "Workspaces > test".into(),
            active_id: Some("conv-1".into()),
            content: ViewContent::Table {
                headers: vec![
                    "Conversation".into(),
                    "Provider".into(),
                    "First Active".into(),
                    "Last Active".into(),
                ],
                rows: vec![vec![
                    first_cell.into(),
                    "Codex".into(),
                    "1h".into(),
                    "now".into(),
                ]],
            },
            selected_index,
            filter_text: "Filter: Codex".into(),
        }
    }

    #[test]
    fn selected_row_is_expandable_for_parent_rows() {
        assert!(selected_row_is_expandable(&table_view(0, "▸ dump-screen")));
        assert!(selected_row_is_expandable(&table_view(0, "▾ dump-screen")));
        assert!(selected_row_is_expandable(&tree_view(true, false)));
    }

    #[test]
    fn selected_row_is_not_expandable_for_plain_rows() {
        assert!(!selected_row_is_expandable(&table_view(
            0,
            "  plain conversation"
        )));
        assert!(!selected_row_is_expandable(&table_view(
            0,
            "  ├─ main session"
        )));
    }

    #[test]
    fn selected_row_is_expanded_for_open_parent_rows() {
        assert!(selected_row_is_expanded(&table_view(0, "▾ dump-screen")));
        assert!(!selected_row_is_expanded(&table_view(0, "▸ dump-screen")));
        assert!(selected_row_is_expanded(&tree_view(true, true)));
        assert!(!selected_row_is_expanded(&table_view(
            0,
            "  ├─ main session"
        )));
    }

    #[test]
    fn transcript_scroll_uses_only_arrow_keys() {
        let view = messages_view();

        assert!(matches!(
            handle_key_code(&view, KeyCode::Up).unwrap(),
            InputOutcome::Event(Event::MessageUp)
        ));
        assert!(matches!(
            handle_key_code(&view, KeyCode::Down).unwrap(),
            InputOutcome::Event(Event::MessageDown)
        ));
        assert!(matches!(
            handle_key_code(&view, KeyCode::Char('j')).unwrap(),
            InputOutcome::Event(Event::Down)
        ));
        assert!(matches!(
            handle_key_code(&view, KeyCode::Char('k')).unwrap(),
            InputOutcome::Event(Event::Up)
        ));
        assert!(matches!(
            handle_key_code(&view, KeyCode::Char('J')).unwrap(),
            InputOutcome::Continue
        ));
        assert!(matches!(
            handle_key_code(&view, KeyCode::Char('K')).unwrap(),
            InputOutcome::Continue
        ));
    }

    #[test]
    fn history_view_uses_list_navigation_keys() {
        let view = history_view();

        assert!(matches!(
            handle_key_code(&view, KeyCode::Up).unwrap(),
            InputOutcome::Event(Event::Up)
        ));
        assert!(matches!(
            handle_key_code(&view, KeyCode::Down).unwrap(),
            InputOutcome::Event(Event::Down)
        ));
        assert!(matches!(
            handle_key_code(&view, KeyCode::Char('j')).unwrap(),
            InputOutcome::Event(Event::Down)
        ));
        assert!(matches!(
            handle_key_code(&view, KeyCode::Char('k')).unwrap(),
            InputOutcome::Event(Event::Up)
        ));
    }
}
