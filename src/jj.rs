use anyhow::{Context, Result};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::process::Command;

use crate::ansi;

/// A revision entry
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Revision {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub is_working_copy: bool,
    pub display_line: Line<'static>,
    pub search_text: String,
}

/// A bookmark with its revisions
#[derive(Debug, Clone)]
pub struct Bookmark {
    pub name: String,
    pub revisions: Vec<Revision>,
    pub expanded: bool,
}

/// Tree item for the flat list display
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TreeItem {
    BookmarkHeader {
        name: String,
        bookmark_idx: usize,
        expanded: bool,
    },
    Revision {
        revision: Revision,
        bookmark_idx: usize,
        revision_idx: usize,
    },
}

impl TreeItem {
    pub fn search_text(&self) -> &str {
        match self {
            TreeItem::BookmarkHeader { name, .. } => name,
            TreeItem::Revision { revision, .. } => &revision.search_text,
        }
    }

    pub fn is_bookmark(&self) -> bool {
        matches!(self, TreeItem::BookmarkHeader { .. })
    }

    pub fn change_id(&self) -> Option<&str> {
        match self {
            TreeItem::BookmarkHeader { .. } => None,
            TreeItem::Revision { revision, .. } => Some(&revision.change_id),
        }
    }

    pub fn bookmark_name(&self) -> &str {
        match self {
            TreeItem::BookmarkHeader { name, .. } => name,
            TreeItem::Revision { .. } => "",
        }
    }

    /// Create a display line for this tree item
    pub fn to_display_line(&self) -> Line<'static> {
        match self {
            TreeItem::BookmarkHeader { name, expanded, .. } => {
                let arrow = if *expanded { "▼" } else { "▶" };
                Line::from(vec![
                    Span::styled(format!("{} ", arrow), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        name.clone(),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                ])
            }
            TreeItem::Revision { revision, .. } => {
                let mut spans = vec![Span::raw("   ")]; // Indent

                if revision.is_working_copy {
                    spans.push(Span::styled(
                        "@ ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ));
                }

                // Add the colored display line spans
                for span in revision.display_line.spans.iter() {
                    spans.push(span.clone());
                }

                Line::from(spans)
            }
        }
    }
}

/// Get the current working copy change_id
fn get_working_copy_id() -> Result<String> {
    let output = Command::new("jj")
        .args([
            "log",
            "-r",
            "@",
            "-T",
            "change_id.shortest(8)",
            "--no-graph",
            "--color=never",
        ])
        .output()
        .context("Failed to get working copy")?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get all bookmarks and their revision chains
pub fn get_bookmarks(depth: usize) -> Result<Vec<Bookmark>> {
    let working_copy_id = get_working_copy_id()?;

    // Get all bookmarks first
    let output = Command::new("jj")
        .args(["bookmark", "list", "-T", r#"name ++ "\n""#])
        .output()
        .context("Failed to list bookmarks")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("jj bookmark list failed: {}", stderr);
    }

    let bookmark_names: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let mut bookmarks = Vec::new();

    for name in bookmark_names {
        // Get revisions for this bookmark (the bookmark itself + ancestors)
        let revset = format!("ancestors({}, {})", name, depth);

        // Structured data
        let template = r#"change_id.shortest(8) ++ "|" ++ commit_id.shortest(8) ++ "|" ++ description.first_line() ++ "\n""#;

        let output = Command::new("jj")
            .args([
                "log",
                "-r",
                &revset,
                "--no-graph",
                "-T",
                template,
                "--color=never",
            ])
            .output()
            .context("Failed to get bookmark revisions")?;

        // Colored output - just change_id and commit_id with jj's native coloring
        let colored_template = r#"change_id.shortest(8) ++ " " ++ commit_id.shortest(8) ++ " " ++ description.first_line() ++ "\n""#;

        let colored_output = Command::new("jj")
            .args([
                "log",
                "-r",
                &revset,
                "--no-graph",
                "-T",
                colored_template,
                "--color=always",
            ])
            .output()
            .context("Failed to get colored output")?;

        let colored_str = String::from_utf8_lossy(&colored_output.stdout);
        let colored_lines: Vec<&str> = colored_str.lines().collect();

        let mut revisions = Vec::new();

        for (i, line) in String::from_utf8_lossy(&output.stdout).lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() < 3 {
                continue;
            }

            let change_id = parts[0].to_string();
            let commit_id = parts[1].to_string();
            let description = parts[2].to_string();

            let is_working_copy = change_id == working_copy_id;

            let search_text = format!("{} {} {}", change_id, commit_id, description);

            let display_line = if i < colored_lines.len() {
                ansi::parse_ansi_to_lines(colored_lines[i])
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| Line::from(""))
            } else {
                Line::from(format!("{} {} {}", change_id, commit_id, description))
            };

            revisions.push(Revision {
                change_id,
                commit_id,
                description,
                is_working_copy,
                display_line,
                search_text,
            });
        }

        bookmarks.push(Bookmark {
            name,
            revisions,
            expanded: false,
        });
    }

    Ok(bookmarks)
}

/// Flatten bookmarks into a displayable list based on expansion state
pub fn flatten_tree(bookmarks: &[Bookmark]) -> Vec<TreeItem> {
    let mut items = Vec::new();

    for (bookmark_idx, bookmark) in bookmarks.iter().enumerate() {
        items.push(TreeItem::BookmarkHeader {
            name: bookmark.name.clone(),
            bookmark_idx,
            expanded: bookmark.expanded,
        });

        if bookmark.expanded {
            for (revision_idx, revision) in bookmark.revisions.iter().enumerate() {
                items.push(TreeItem::Revision {
                    revision: revision.clone(),
                    bookmark_idx,
                    revision_idx,
                });
            }
        }
    }

    items
}
