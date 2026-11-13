# PDF Tree

PDF Tree is a Linux desktop app for arranging PDF pages into an ordered tree. A project embeds the original PDFs and keeps the tree, page rotations, highlights, notes, and viewport together.

## Run

Keep this repository beside the Rune checkout, at `~/repos/pdf-tree` and `~/repos/rune`. The app uses Rune commit `59f0326b` through a local Cargo path. It needs Rust 1.99.1, a C compiler, pkg-config, Poppler GLib and Cairo development libraries, and the desktop's XDG FileChooser portal.

On Arch Linux the development packages are `base-devel poppler-glib cairo`. On Debian-based systems they are `build-essential pkg-config libpoppler-glib-dev libcairo2-dev`. Use the portal backend for your desktop environment.

```sh
cd ~/repos/pdf-tree
rustup toolchain install 1.99.1 --component rustfmt --component clippy
cargo run --locked
```

## Use

- Open or drop a PDF to create a chain starting at its first page. Siblings share a row, and branches keep their spacing at levels where both have cards. Right-click any node to select it and all its descendants, then drag a selected header to move the tail. Click cards to select them, Ctrl-click to toggle individual cards, or Shift-click for a range. Drag from a card header. Drop on another card to append children, in the gap immediately left of a card to insert siblings, or on **Move to root** to append at the top level and bring the moved pages into view.
- Scroll to pan vertically, Shift-scroll to pan horizontally, and Ctrl-scroll or pinch to zoom. Drag empty space to pan. Use the **Hand** icon to pan over pages. Click the zoom percentage to reset the view.
- Add blank pages or choose pages from another PDF. Use **Rotate**, **Replace**, **Duplicate**, and **Delete** on selected cards. Removal promotes children into the removed page's position. Ctrl+Z undoes edits and Ctrl+Shift+Z redoes them.
- Choose **Highlight**, pick a **Color**, and drag across embedded PDF text. Each highlight saves its own color. Scans without a text layer can be arranged and annotated with canvas notes.
- Turn on **Graph** to show slides as text cards while keeping their connections. Each card uses the first non-empty line of PDF text as its title, with a page number for slides without selectable text.
- Click **+** to add an editable card to the tree. Double-click its text to edit it, or drag its header to move it.
- Right-click a subtree and choose **Group** to assign one of twelve colors. Group boundaries reserve their full width. Repeat inside a group to create a subgroup.
- Choose the pencil icon and click a node to attach a sticky note to its top-left corner. Click an existing note's text to edit it or use its close button to delete it.
- Choose **Reposition** and drag a branch header to place it relative to its parent. Soft connections follow the branch, and **Save** remembers the placement. Double-click the header in Reposition mode to return to automatic layout. Hard connections keep their structured layout.
- Hover over a toolbar icon to see its name and shortcut. Press **Ctrl+G** to toggle Graph mode, **M** to open a new note directly, or **T** to choose a node with the Note tool. On macOS, use **Cmd** for shortcuts shown with **Ctrl**.
- **Save** creates a portable `.pdftree` file. Open it with **Open project**. Source PDFs are embedded. Moving or deleting the original PDFs does not affect the project.

## Checks

```sh
cargo fmt --check
cargo test --locked
```

Tests cover tree moves and deletion, Poppler rendering and text alignment across rotations, SQLite round trips and save errors, and Rune input for drag, highlight, and dialog requests.
