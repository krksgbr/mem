#[cfg(test)]
mod tests {
    use crate::{render::render_ui, test_utils::buffer_to_string, theme::Theme};
    use ratatui::{
        backend::TestBackend,
        widgets::{ListState, TableState},
        Terminal,
    };
    use shared::{ConversationPreview, MessagePreview, ViewContent, ViewModel};
    use std::fs;

    fn load_fixture_messages() -> Vec<Vec<String>> {
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/crispy_messages.json"
        );
        let content = fs::read_to_string(fixture_path).expect("fixture should exist");
        serde_json::from_str(&content).expect("fixture should parse")
    }

    #[test]
    fn test_render_crispy_fixture() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        let msgs = load_fixture_messages();
        let right_messages: Vec<MessagePreview> = msgs
            .iter()
            .enumerate()
            .map(|(idx, msg)| MessagePreview {
                participant_label: msg[0].clone(),
                content: msg[1].clone(),
                is_focused: idx == 0,
                is_expanded: false,
                relative_time: Some("1d ago".into()),
            })
            .collect();

        let view_model = ViewModel {
            title: "Conversations".into(),
            breadcrumb: "Workspaces > reference".into(),
            active_id: Some("conv-1".into()),
            content: ViewContent::Split {
                conversations: vec![ConversationPreview {
                    id: "conv-1".into(),
                    title: "reference".into(),
                    provider_label: "Claude Code".into(),
                    relative_time: "1d ago".into(),
                    snippet: "hello".into(),
                    is_selected: true,
                }],
                right_messages,
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
        let rendered = buffer_to_string(buffer);
        assert!(rendered.contains("Claude Code"));
        assert!(rendered.contains("We should minimize the code we own."));
        assert!(rendered.contains("Question to you:"));
    }
}
