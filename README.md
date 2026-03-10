# jjf

A fuzzy finder TUI for [jujutsu (jj)](https://github.com/martinvonz/jj) revisions.

## Features

- Tree view with bookmarks as collapsible groups
- Fuzzy search filtering
- Vim keybindings
- Preserves jj's native ANSI coloring for change/commit IDs

## Installation

```bash
cargo install --path .
```

## Usage

```bash
jjf              # Launch TUI
jjf -d 10        # Show 10 ancestor revisions per bookmark (default: 5)
jjf --debug      # Print tree without TUI
```

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `g` / `G` | Go to first / last |
| `l` / `h` | Expand / collapse bookmark |
| `Enter` / `Space` | Toggle bookmark |
| `n` | `jj new` on selected revision |
| `e` | `jj edit` on selected revision |
| `/` | Focus search |
| `Tab` | Toggle focus between search and list |
| `Esc` / `Ctrl+C` | Quit |
