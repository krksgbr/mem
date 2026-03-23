#[cfg(test)]
mod tests {
    use crate::{render::render_messages_list, test_utils::buffer_to_string, theme::Theme};
    use ratatui::{backend::TestBackend, Terminal};
    use shared::{MessageKind, MessagePreview};

    #[test]
    fn test_large_message_truncates_in_preview() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let messages = vec![MessagePreview {
            kind: MessageKind::AssistantMessage,
            participant_label: "Assistant".into(),
            content: "lorem ipsum dolor sit amet ".repeat(40),
            depth: 0,
            is_focused: true,
            is_expanded: false,
            relative_time: Some("1d ago".into()),
        }];

        let theme = Theme::default();

        terminal
            .draw(|f| {
                render_messages_list(f, f.size(), &messages, &theme, None, true);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = buffer_to_string(buffer);
        assert!(rendered.contains("Assistant"));
        assert!(rendered.contains("more lines"));
        assert!(rendered.contains("lorem ipsum"));
    }
}
