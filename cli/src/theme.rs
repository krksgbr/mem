use ratatui::style::Color;

pub struct Theme {
    pub border: Color,
    pub dim: Color,
    pub user_msg: Color,
    pub assistant_msg: Color,
    pub selected_bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            border: Color::DarkGray,
            dim: Color::DarkGray,
            user_msg: Color::Cyan,
            assistant_msg: Color::Green,
            selected_bg: Color::Rgb(28, 28, 28),
        }
    }
}
