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

- Open or drop a PDF. Click cards to select them, Ctrl-click to toggle individual cards, or Shift-click for a range. Drag from a card header. Drop on another card to append children, in the gap above a card to insert siblings, or on **Move to root** to append at the top level.
- Scroll to pan vertically, Shift-scroll to pan horizontally, and Ctrl-scroll or pinch to zoom. Drag empty space to pan. Use **Hand** to pan over pages. Click the zoom percentage to reset the view.
- Add blank pages or choose pages from another PDF. Use **Rotate**, **Replace**, **Duplicate**, and **Remove** on selected cards. Removal promotes children into the removed page's position. Ctrl+Z undoes edits and Ctrl+Shift+Z redoes them.
- Choose **Highlight** and drag across embedded PDF text. Scans without a text layer can be arranged and annotated with canvas notes.
- Choose **Note** and click empty canvas space. Click an existing note's text to edit it, drag its header to move it, or use its close button to delete it.
- **Save** creates a portable `.pdftree` file. Open it with **Open project**. Source PDFs are embedded. Moving or deleting the original PDFs does not affect the project.

## Checks

```sh
cargo fmt --check
cargo test --locked
```

Tests cover tree moves and deletion, Poppler rendering and text alignment across rotations, SQLite round trips and save errors, and Rune input for drag, highlight, and dialog requests.
