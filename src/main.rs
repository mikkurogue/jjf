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
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::{io::stdout, process::Command};

mod ansi;
mod jj;

use jj::{Bookmark, TreeItem};

#[derive(Parser)]
#[command(name = "jjf")]
#[command(about = "Fuzzy finder for jujutsu (jj) revisions")]
struct Cli {
    /// Number of ancestor revisions to show per bookmark
    #[arg(short, long, default_value = "5")]
    depth: usize,

    /// Print tree structure without launching TUI (for debugging)
    #[arg(long)]
    debug: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Search,
    List,
}

/// What action to take on selection
#[derive(Clone)]
enum Action {
    New(String),  // jj new <target>
    Edit(String), // jj edit <target>
}

struct App {
    bookmarks: Vec<Bookmark>,
    tree_items: Vec<TreeItem>,
    filtered_indices: Vec<usize>,
    list_state: ListState,
    input: String,
    focus: Focus,
    should_quit: bool,
    action: Option<Action>,
}

impl App {
    fn new(bookmarks: Vec<Bookmark>) -> Self {
        let tree_items = jj::flatten_tree(&bookmarks);
        let filtered_indices: Vec<usize> = (0..tree_items.len()).collect();

        let mut app = App {
            bookmarks,
            tree_items,
            filtered_indices,
            list_state: ListState::default(),
            input: String::new(),
            focus: Focus::List,
            should_quit: false,
            action: None,
        };

        if !app.filtered_indices.is_empty() {
            app.list_state.select(Some(0));
        }
        app
    }

    fn rebuild_tree(&mut self) {
        self.tree_items = jj::flatten_tree(&self.bookmarks);
        self.filter_entries();
    }

    fn filter_entries(&mut self) {
        if self.input.is_empty() {
            self.filtered_indices = (0..self.tree_items.len()).collect();
        } else {
            let haystacks: Vec<&str> = self.tree_items.iter().map(|e| e.search_text()).collect();
            let matches = frizbee::match_list(&self.input, &haystacks, &frizbee::Config::default());
            self.filtered_indices = matches.into_iter().map(|m| m.index as usize).collect();
        }

        // Reset selection
        if self.filtered_indices.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
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
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Search => Focus::List,
            Focus::List => Focus::Search,
        };
    }

    fn get_selected_item(&self) -> Option<&TreeItem> {
        let selected = self.list_state.selected()?;
        let &idx = self.filtered_indices.get(selected)?;
        self.tree_items.get(idx)
    }

    fn toggle_expand(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if let Some(&idx) = self.filtered_indices.get(selected) {
                if let Some(item) = self.tree_items.get(idx) {
                    if let TreeItem::BookmarkHeader { bookmark_idx, .. } = item {
                        self.bookmarks[*bookmark_idx].expanded =
                            !self.bookmarks[*bookmark_idx].expanded;
                        self.rebuild_tree();
                    }
                }
            }
        }
    }

    fn expand_selected(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if let Some(&idx) = self.filtered_indices.get(selected) {
                if let Some(item) = self.tree_items.get(idx) {
                    if let TreeItem::BookmarkHeader { bookmark_idx, .. } = item {
                        if !self.bookmarks[*bookmark_idx].expanded {
                            self.bookmarks[*bookmark_idx].expanded = true;
                            self.rebuild_tree();
                        }
                    }
                }
            }
        }
    }

    fn collapse_selected(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if let Some(&idx) = self.filtered_indices.get(selected) {
                if let Some(item) = self.tree_items.get(idx) {
                    match item {
                        TreeItem::BookmarkHeader { bookmark_idx, .. } => {
                            if self.bookmarks[*bookmark_idx].expanded {
                                self.bookmarks[*bookmark_idx].expanded = false;
                                self.rebuild_tree();
                            }
                        }
                        TreeItem::Revision { bookmark_idx, .. } => {
                            // Collapse parent bookmark and move selection to it
                            let bi = *bookmark_idx;
                            self.bookmarks[bi].expanded = false;
                            self.rebuild_tree();
                            // Find the bookmark header in the new list
                            for (i, item) in self.tree_items.iter().enumerate() {
                                if let TreeItem::BookmarkHeader {
                                    bookmark_idx: idx, ..
                                } = item
                                {
                                    if *idx == bi {
                                        self.list_state.select(Some(i));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn do_new(&mut self) {
        if let Some(item) = self.get_selected_item().cloned() {
            let target = match &item {
                TreeItem::BookmarkHeader { name, .. } => name.clone(),
                TreeItem::Revision { revision, .. } => revision.change_id.clone(),
            };
            self.action = Some(Action::New(target));
            self.should_quit = true;
        }
    }

    fn do_edit(&mut self) {
        if let Some(item) = self.get_selected_item().cloned() {
            // Can only edit revisions, not bookmarks
            if let TreeItem::Revision { revision, .. } = &item {
                self.action = Some(Action::Edit(revision.change_id.clone()));
                self.should_quit = true;
            }
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Get bookmarks with their revisions
    let bookmarks = jj::get_bookmarks(cli.depth).context("Failed to get bookmarks")?;

    if bookmarks.is_empty() {
        eprintln!("No bookmarks found in this repository");
        return Ok(());
    }

    // Debug mode: print tree without TUI
    if cli.debug {
        for bookmark in &bookmarks {
            println!("▶ {}", bookmark.name);
            for rev in &bookmark.revisions {
                let wc = if rev.is_working_copy { "@ " } else { "  " };
                println!(
                    "   {}{} {} {}",
                    wc, rev.change_id, rev.commit_id, rev.description
                );
            }
        }
        return Ok(());
    }

    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Create and run app
    let mut app = App::new(bookmarks);
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result?;

    // Execute action if one was selected
    if let Some(action) = app.action {
        match action {
            Action::New(target) => {
                let status = Command::new("jj")
                    .args(["new", &target])
                    .status()
                    .context("Failed to execute jj new")?;

                if !status.success() {
                    anyhow::bail!("jj new failed with status: {}", status);
                }
                println!("Created new revision on {}", target);
            }
            Action::Edit(target) => {
                let status = Command::new("jj")
                    .args(["edit", &target])
                    .status()
                    .context("Failed to execute jj edit")?;

                if !status.success() {
                    anyhow::bail!("jj edit failed with status: {}", status);
                }
                println!("Now editing {}", target);
            }
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

            // Global keybindings
            match key.code {
                KeyCode::Esc => {
                    app.should_quit = true;
                    continue;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.should_quit = true;
                    continue;
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    app.toggle_focus();
                    continue;
                }
                _ => {}
            }

            // Focus-specific keybindings
            match app.focus {
                Focus::Search => match key.code {
                    KeyCode::Char(c) => {
                        app.input.push(c);
                        app.filter_entries();
                    }
                    KeyCode::Backspace => {
                        app.input.pop();
                        app.filter_entries();
                    }
                    KeyCode::Down | KeyCode::Enter => {
                        app.focus = Focus::List;
                    }
                    _ => {}
                },
                Focus::List => match key.code {
                    // Vim bindings
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.move_selection(1);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.move_selection(-1);
                    }
                    KeyCode::Char('g') => {
                        app.list_state.select(Some(0));
                    }
                    KeyCode::Char('G') => {
                        let len = app.filtered_indices.len();
                        if len > 0 {
                            app.list_state.select(Some(len - 1));
                        }
                    }
                    // Expand/collapse
                    KeyCode::Char('l') | KeyCode::Right => {
                        app.expand_selected();
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        app.collapse_selected();
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        // Toggle expand on bookmark only
                        if let Some(item) = app.get_selected_item() {
                            if item.is_bookmark() {
                                app.toggle_expand();
                            }
                        }
                    }
                    // Actions
                    KeyCode::Char('n') => {
                        app.do_new();
                    }
                    KeyCode::Char('e') => {
                        app.do_edit();
                    }
                    // Switch back to search
                    KeyCode::Char('/') => {
                        app.focus = Focus::Search;
                    }
                    KeyCode::PageUp => {
                        app.move_selection(-10);
                    }
                    KeyCode::PageDown => {
                        app.move_selection(10);
                    }
                    _ => {}
                },
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Input
            Constraint::Min(0),    // List
            Constraint::Length(1), // Help line
        ])
        .split(f.area());

    // Input box
    let search_style = if app.focus == Focus::Search {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(search_style)
        .title(" Search ");
    let cursor_char = if app.focus == Focus::Search { "_" } else { "" };
    let input = Paragraph::new(format!("> {}{}", app.input, cursor_char))
        .block(input_block)
        .style(Style::default().fg(Color::White));
    f.render_widget(input, chunks[0]);

    // Tree list
    let list_style = if app.focus == Focus::List {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let selected_idx = app.list_state.selected();
    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(i, &idx)| {
            let item = &app.tree_items[idx];
            let is_selected = selected_idx == Some(i);
            ListItem::new(item.to_display_line(is_selected))
        })
        .collect();

    let list_title = format!(" Bookmarks ({}) ", app.bookmarks.len(),);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(list_style)
                .title(list_title),
        )
        .highlight_symbol("› ");

    f.render_stateful_widget(list, chunks[1], &mut app.list_state.clone());

    // Help line - context-sensitive
    let help_text = match app.focus {
        Focus::Search => " Tab: list | Type to filter | Enter/↓: go to list",
        Focus::List => {
            " j/k: move | l/h: expand/collapse | n: new | e: edit | Enter: toggle/select | /: search"
        }
    };
    let help = Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[2]);
}
