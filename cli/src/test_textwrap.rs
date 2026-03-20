#[cfg(test)]
mod tests {
    use textwrap::wrap;

    #[test]
    fn test_wrap_preserves_text_and_width() {
        let text = "This is a very long text that should be wrapped to a specific width.";
        let wrapped = wrap(text, 20);
        let rejoined = wrapped
            .iter()
            .map(|line| line.as_ref())
            .collect::<Vec<_>>()
            .join(" ");

        assert!(!wrapped.is_empty());
        assert!(wrapped.iter().all(|line| line.chars().count() <= 20));
        assert_eq!(rejoined, text);
    }
}
