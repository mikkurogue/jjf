use anyhow::{Context, Result};
use ratatui::text::Line;
use std::process::Command;

use crate::ansi;

/// Represents a single jj log entry
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LogEntry {
    /// The change ID (short form) used for jj new
    pub change_id: String,
    /// The commit ID (short form)
    pub commit_id: String,
    /// Bookmarks associated with this revision
    pub bookmarks: Vec<String>,
    /// First line of the description
    pub description: String,
    /// Author name
    pub author: String,
    /// Timestamp
    pub timestamp: String,
    /// The full display line with ANSI colors preserved (for rendering)
    pub display_lines: Vec<Line<'static>>,
    /// Plain text for searching (no ANSI codes)
    pub search_text: String,
}

/// Get log entries from jj
pub fn get_log_entries(revset: &str, limit: usize) -> Result<Vec<LogEntry>> {
    // We'll use a custom template that outputs structured data we can parse
    // Format: CHANGE_ID|COMMIT_ID|BOOKMARKS|AUTHOR|TIMESTAMP|DESCRIPTION
    // But we also need the colored output for display, so we do two queries:
    // 1. Structured data for parsing
    // 2. Colored output for display

    let template = r#"separate("|",
        change_id.shortest(8),
        commit_id.shortest(8),
        bookmarks.map(|b| b.name()).join(","),
        author.name(),
        committer.timestamp().format("%Y-%m-%d %H:%M"),
        description.first_line()
    ) ++ "\n""#;

    // Get structured data
    let output = Command::new("jj")
        .args([
            "log",
            "-r",
            revset,
            "-n",
            &limit.to_string(),
            "--no-graph",
            "-T",
            template,
            "--color=never",
        ])
        .output()
        .context("Failed to run jj log")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("jj log failed: {}", stderr);
    }

    let structured_output = String::from_utf8_lossy(&output.stdout);

    // Get colored output for display - use jj's native format
    // which properly colors the short change_id prefix
    let display_template = r#"separate(" ",
        change_id.shortest(8),
        commit_id.shortest(8),
        author.name(),
        committer.timestamp().ago(),
        if(bookmarks, bookmarks.map(|b| b.name()).join(" ")),
        description.first_line()
    ) ++ "\n""#;

    let colored_output = Command::new("jj")
        .args([
            "log",
            "-r",
            revset,
            "-n",
            &limit.to_string(),
            "--no-graph",
            "-T",
            display_template,
            "--color=always",
        ])
        .output()
        .context("Failed to run jj log (colored)")?;

    let colored_str = String::from_utf8_lossy(&colored_output.stdout);
    let colored_lines: Vec<&str> = colored_str.lines().collect();

    // Parse structured data and combine with colored output
    let mut entries = Vec::new();

    for (i, line) in structured_output.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(6, '|').collect();
        if parts.len() < 6 {
            continue;
        }

        let change_id = parts[0].to_string();
        let commit_id = parts[1].to_string();
        let bookmarks: Vec<String> = parts[2]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let author = parts[3].to_string();
        let timestamp = parts[4].to_string();
        let description = parts[5].to_string();

        // Build search text (all searchable fields)
        let search_text = format!(
            "{} {} {} {} {} {}",
            change_id,
            commit_id,
            bookmarks.join(" "),
            author,
            timestamp,
            description
        );

        // Get colored display line
        let display_lines = if i < colored_lines.len() {
            ansi::parse_ansi_to_lines(colored_lines[i])
        } else {
            vec![Line::from(format!(
                "{} {} {}",
                change_id, commit_id, description
            ))]
        };

        entries.push(LogEntry {
            change_id,
            commit_id,
            bookmarks,
            description,
            author,
            timestamp,
            display_lines,
            search_text,
        });
    }

    Ok(entries)
}
