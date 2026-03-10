use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{io::stdout, process::Command};

mod ansi;
mod jj;

use jj::LogEntry;

#[derive(Parser)]
#[command(name = "jjf")]
#[command(about = "Fuzzy finder for jujutsu (jj) revisions")]
struct Cli {
    /// Revset to show (default: ::@)
    #[arg(default_value = "::@")]
    revisions: String,

    /// Maximum number of revisions to show
    #[arg(short = 'n', long, default_value = "100")]
    limit: usize,
}

struct App {
    entries: Vec<LogEntry>,
    filtered_indices: Vec<usize>,
    list_state: ListState,
    input: String,
    preview_content: Vec<Line<'static>>,
    preview_scroll: u16,
    should_quit: bool,
    selected_change_id: Option<String>,
}

impl App {
    fn new(entries: Vec<LogEntry>) -> Self {
        let filtered_indices: Vec<usize> = (0..entries.len()).collect();
        let mut app = App {
            entries,
            filtered_indices,
            list_state: ListState::default(),
            input: String::new(),
            preview_content: Vec::new(),
            should_quit: false,
            selected_change_id: None,
        };
        if !app.filtered_indices.is_empty() {
            app.list_state.select(Some(0));
            app.update_preview();
        }
        app
    }

    fn filter_entries(&mut self) {
        if self.input.is_empty() {
            self.filtered_indices = (0..self.entries.len()).collect();
        } else {
            let haystacks: Vec<&str> = self
                .entries
                .iter()
                .map(|e| e.search_text.as_str())
                .collect();
            let matches = frizbee::match_list(&self.input, &haystacks, &frizbee::Config::default());
            self.filtered_indices = matches.into_iter().map(|m| m.index as usize).collect();
        }

        // Reset selection
        if self.filtered_indices.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
        self.update_preview();
    }

    fn update_preview(&mut self) {
        let Some(selected) = self.list_state.selected() else {
            self.preview_content = vec![Line::from("No revision selected")];
            return;
        };

        let Some(&entry_idx) = self.filtered_indices.get(selected) else {
            self.preview_content = vec![Line::from("No revision selected")];
            return;
        };

        let entry = &self.entries[entry_idx];
        let change_id = &entry.change_id;

        // Run jj diff for this revision
        let output = Command::new("jj")
            .args(["diff", "-r", change_id, "--color=always"])
            .output();

        match output {
            Ok(output) => {
                let content = String::from_utf8_lossy(&output.stdout);
                self.preview_content = ansi::parse_ansi_to_lines(&content);
            }
            Err(e) => {
                self.preview_content = vec![Line::from(format!("Error running jj diff: {}", e))];
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }

        let current = self.list_state.selected().unwrap_or(0);
        let new_index = if delta > 0 {
            (current + delta as usize).min(len - 1)
        } else {
            current.saturating_sub((-delta) as usize)
        };

        self.list_state.select(Some(new_index));
        self.update_preview();
    }

    fn confirm_selection(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if let Some(&entry_idx) = self.filtered_indices.get(selected) {
                self.selected_change_id = Some(self.entries[entry_idx].change_id.clone());
                self.should_quit = true;
            }
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Get jj log entries
    let entries =
        jj::get_log_entries(&cli.revisions, cli.limit).context("Failed to get jj log entries")?;

    if entries.is_empty() {
        eprintln!("No revisions found for revset: {}", cli.revisions);
        return Ok(());
    }

    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Create and run app
    let mut app = App::new(entries);
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result?;

    // Execute jj new if a selection was made
    if let Some(change_id) = app.selected_change_id {
        let status = Command::new("jj")
            .args(["new", &change_id])
            .status()
            .context("Failed to execute jj new")?;

        if !status.success() {
            anyhow::bail!("jj new failed with status: {}", status);
        }
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if app.should_quit {
            return Ok(());
        }

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Esc => {
                    app.should_quit = true;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.should_quit = true;
                }
                KeyCode::Enter => {
                    app.confirm_selection();
                }
                KeyCode::Up | KeyCode::Char('k')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.move_selection(-1);
                }
                KeyCode::Down | KeyCode::Char('j')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.move_selection(1);
                }
                KeyCode::PageUp => {
                    app.move_selection(-10);
                }
                KeyCode::PageDown => {
                    app.move_selection(10);
                }
                KeyCode::Char(c) => {
                    app.input.push(c);
                    app.filter_entries();
                }
                KeyCode::Backspace => {
                    app.input.pop();
                    app.filter_entries();
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Input
            Constraint::Min(0),    // Main content
        ])
        .split(f.area());

    // Input box
    let input_block = Block::default().borders(Borders::ALL).title(" Search ");
    let input = Paragraph::new(format!("> {}", app.input))
        .block(input_block)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(input, chunks[0]);

    // Split main area into list and preview
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40), // List
            Constraint::Percentage(60), // Preview
        ])
        .split(chunks[1]);

    // Revision list
    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .map(|&idx| {
            let entry = &app.entries[idx];
            ListItem::new(entry.display_lines.clone())
        })
        .collect();

    let list_title = format!(
        " Revisions ({}/{}) ",
        app.filtered_indices.len(),
        app.entries.len()
    );
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, main_chunks[0], &mut app.list_state.clone());

    // Preview pane
    let preview = Paragraph::new(app.preview_content.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Preview (jj diff) "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(preview, main_chunks[1]);
}
