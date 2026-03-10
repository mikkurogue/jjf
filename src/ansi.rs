use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Parse ANSI-colored text into ratatui Lines
pub fn parse_ansi_to_lines(input: &str) -> Vec<Line<'static>> {
    input.lines().map(|line| parse_ansi_line(line)).collect()
}

/// Parse a single line with ANSI codes into a ratatui Line
fn parse_ansi_line(input: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current_style = Style::default();
    let mut current_text = String::new();

    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Start of ANSI escape sequence
            if chars.peek() == Some(&'[') {
                // Flush current text
                if !current_text.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current_text),
                        current_style,
                    ));
                }

                chars.next(); // consume '['

                // Parse the escape sequence
                let mut sequence = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphabetic() {
                        chars.next();
                        break;
                    }
                    sequence.push(chars.next().unwrap());
                }

                // Apply the escape sequence to style
                current_style = apply_sgr_sequence(&sequence, current_style);
            }
        } else {
            current_text.push(c);
        }
    }

    // Flush remaining text
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    Line::from(spans)
}

/// Apply SGR (Select Graphic Rendition) sequence to style
fn apply_sgr_sequence(sequence: &str, mut style: Style) -> Style {
    if sequence.is_empty() || sequence == "0" {
        return Style::default();
    }

    let codes: Vec<u8> = sequence.split(';').filter_map(|s| s.parse().ok()).collect();

    let mut iter = codes.iter().peekable();

    while let Some(&code) = iter.next() {
        match code {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),

            // Standard foreground colors
            30 => style = style.fg(Color::Black),
            31 => style = style.fg(Color::Red),
            32 => style = style.fg(Color::Green),
            33 => style = style.fg(Color::Yellow),
            34 => style = style.fg(Color::Blue),
            35 => style = style.fg(Color::Magenta),
            36 => style = style.fg(Color::Cyan),
            37 => style = style.fg(Color::White),

            // 256-color foreground
            38 => {
                if iter.next() == Some(&5) {
                    if let Some(&color) = iter.next() {
                        style = style.fg(Color::Indexed(color));
                    }
                }
            }

            39 => style = style.fg(Color::Reset),

            // Standard background colors
            40 => style = style.bg(Color::Black),
            41 => style = style.bg(Color::Red),
            42 => style = style.bg(Color::Green),
            43 => style = style.bg(Color::Yellow),
            44 => style = style.bg(Color::Blue),
            45 => style = style.bg(Color::Magenta),
            46 => style = style.bg(Color::Cyan),
            47 => style = style.bg(Color::White),

            // 256-color background
            48 => {
                if iter.next() == Some(&5) {
                    if let Some(&color) = iter.next() {
                        style = style.bg(Color::Indexed(color));
                    }
                }
            }

            49 => style = style.bg(Color::Reset),

            // Bright foreground colors
            90 => style = style.fg(Color::DarkGray),
            91 => style = style.fg(Color::LightRed),
            92 => style = style.fg(Color::LightGreen),
            93 => style = style.fg(Color::LightYellow),
            94 => style = style.fg(Color::LightBlue),
            95 => style = style.fg(Color::LightMagenta),
            96 => style = style.fg(Color::LightCyan),
            97 => style = style.fg(Color::White),

            // Bright background colors
            100 => style = style.bg(Color::DarkGray),
            101 => style = style.bg(Color::LightRed),
            102 => style = style.bg(Color::LightGreen),
            103 => style = style.bg(Color::LightYellow),
            104 => style = style.bg(Color::LightBlue),
            105 => style = style.bg(Color::LightMagenta),
            106 => style = style.bg(Color::LightCyan),
            107 => style = style.bg(Color::White),

            _ => {}
        }
    }

    style
}
