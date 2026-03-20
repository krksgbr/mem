use ratatui::style::Color;

pub struct Theme {
    pub border: Color,
    pub dim: Color,
    pub user_msg: Color,
    pub assistant_msg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            border: Color::DarkGray,
            dim: Color::DarkGray,
            user_msg: Color::Cyan,
            assistant_msg: Color::Green,
        }
    }
}
