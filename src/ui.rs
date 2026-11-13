use super::*;
use rune::{
    Color, Cursor, Drag, El, LineCap, Mods, Phase, Shape, TextStyle, canvas, col, el, row, text,
    text_input,
};

fn color(hex: &str) -> Color {
    rune::color::hex(hex).unwrap()
}

struct Palette {
    canvas: Color,
    panel: Color,
    surface: Color,
    border: Color,
    text: Color,
    muted: Color,
    accent: Color,
    accent_bg: Color,
    note: Color,
    note_text: Color,
}

impl Palette {
    fn new() -> Self {
        Self {
            canvas: color("#14191f"),
            panel: color("#1c232c"),
            surface: color("#252f3a"),
            border: color("#384652"),
            text: color("#eaf1f5"),
            muted: color("#a4b4c3"),
            accent: color("#7bdfbc"),
            accent_bg: color("#23453e"),
            note: color("#f7e5b4"),
            note_text: color("#382c19"),
        }
    }
}

struct Card {
    id: Id,
    parent: Option<Id>,
    number: usize,
    rect: Rect,
    image: Rect,
}

fn cards(s: &State) -> Vec<Card> {
    let mut y = 28.0;
    let z = s.view.zoom;
    s.project
        .tree
        .order()
        .into_iter()
        .enumerate()
        .map(|(index, (id, parent, depth))| {
            let page = &s.project.tree.pages[&id];
            let [w, h] = if page.rotation % 2 == 0 {
                page.size
            } else {
                [page.size[1], page.size[0]]
            };
            let fit = (276.0 / w).min(328.0 / h);
            let [iw, ih] = [w * fit, h * fit];
            let x = 44.0 + depth as f32 * 78.0;
            let [left, top] = [s.view.pan[0] + x * z, s.view.pan[1] + y * z];
            let rect = [left, top, 296.0 * z, (ih + 49.0) * z];
            let image = [(296.0 - iw) / 2.0 * z, 39.0 * z, iw * z, ih * z];
            y += ih + 76.0;
            Card {
                id,
                parent,
                number: index + 1,
                rect,
                image,
            }
        })
        .collect()
}

fn contains([x, y, w, h]: Rect, [px, py]: [f32; 2]) -> bool {
    px >= x && py >= y && px <= x + w && py <= y + h
}

fn visible([x, y, w, h]: Rect, [_, _, vw, vh]: Rect) -> bool {
    x < vw && y < vh && x + w > 0.0 && y + h > 0.0
}

fn target(s: &State, drag: &Drag) -> Option<Destination> {
    let [wx, wy] = drag.window_point();
    let point = [wx - s.canvas[0], wy - s.canvas[1]];
    if !contains([0.0, 0.0, s.canvas[2], s.canvas[3]], point) {
        return None;
    }
    let dest = if contains(root_zone(s), point) {
        Some(Destination::Root)
    } else {
        cards(s).iter().find_map(|card| {
            let [x, y, w, h] = card.rect;
            let gap = 24.0 * s.view.zoom;
            if contains([x, y - gap, w, gap], point) {
                Some(Destination::Before(card.id))
            } else if contains([x, y, w, h], point) {
                Some(Destination::Child(card.id))
            } else {
                None
            }
        })
    }?;
    s.project.tree.can_move(&s.selected, dest).then_some(dest)
}

fn root_zone(s: &State) -> Rect {
    [(s.canvas[2] - 164.0).max(0.0), 16.0, 148.0, 32.0]
}

fn select(s: &mut State, id: Id, mods: Mods) {
    if mods.shift {
        let ordered: Vec<_> = cards(s).into_iter().map(|card| card.id).collect();
        if let (Some(a), Some(b)) = (
            ordered.iter().position(|&n| Some(n) == s.anchor),
            ordered.iter().position(|&n| n == id),
        ) {
            if !mods.ctrl {
                s.selected.clear();
            }
            s.selected
                .extend(ordered[a.min(b)..=a.max(b)].iter().copied());
            return;
        }
    }
    if mods.ctrl {
        if !s.selected.insert(id) {
            s.selected.remove(&id);
        }
    } else {
        s.selected = BTreeSet::from([id]);
    }
    s.anchor = Some(id);
}

fn page_drag(s: &mut State, id: Id, d: &Drag, mods: Mods) {
    match d.phase {
        Phase::Down => {
            if mods.ctrl || mods.shift || !s.selected.contains(&id) {
                select(s, id, mods);
            }
            s.drag = s.selected.contains(&id).then_some(id);
        }
        Phase::Move if s.drag == Some(id) && d.dx.hypot(d.dy) > 4.0 => {
            s.drop = target(s, d);
        }
        Phase::Up if s.drag.take() == Some(id) => {
            s.drop = None;
            if d.cancelled() {
                return;
            }
            if d.dx.hypot(d.dy) > 4.0 {
                if let Some(destination) = target(s, d) {
                    let selected = s.selected.clone();
                    s.edit(|p| p.tree.move_pages(&selected, destination));
                }
            } else if !mods.ctrl && !mods.shift {
                select(s, id, mods);
            }
        }
        _ => {}
    }
}

fn highlight(s: &mut State, id: Id, d: &Drag, text: &[Rect], rotation: u8) {
    let point = [(d.x / d.w).clamp(0.0, 1.0), (d.y / d.h).clamp(0.0, 1.0)];
    if d.phase == Phase::Down {
        s.selection = Some((id, point, vec![]));
        if text.is_empty() {
            s.status = "No selectable PDF text is available on this page.".into();
        }
        return;
    }
    let Some((page, start, preview)) = s.selection.as_mut() else {
        return;
    };
    if *page != id {
        return;
    }
    let nearest = |point: [f32; 2]| {
        text.iter()
            .enumerate()
            .filter(|(_, rect)| rect[2] > 0.0 && rect[3] > 0.0)
            .map(|(i, &rect)| {
                let [x, y, w, h] = rotate_rect(rect, rotation);
                let dx = (x - point[0]).max(point[0] - x - w).max(0.0) * d.w;
                let dy = (y - point[1]).max(point[1] - y - h).max(0.0) * d.h;
                (i, dx.hypot(dy))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    };
    *preview = match (nearest(*start), nearest(point)) {
        (Some((a, distance)), Some((b, _))) if distance < 28.0 && d.dx.hypot(d.dy) > 2.0 => text
            [a.min(b)..=a.max(b)]
            .iter()
            .copied()
            .filter(|r| r[2] > 0.0 && r[3] > 0.0)
            .collect(),
        _ => vec![],
    };
    if d.phase == Phase::Up {
        let (_, _, rects) = s.selection.take().unwrap();
        if !d.cancelled() && !rects.is_empty() {
            s.edit(|p| p.tree.pages.get_mut(&id).unwrap().highlights.extend(rects));
            s.status = "Highlight added.".into();
        }
    }
}

fn label(value: impl Into<String>, size: f32, weight: f32, ink: Color) -> El {
    text(value).font(TextStyle::new(size, weight)).color(ink)
}

fn button(
    title: &str,
    palette: &Palette,
    active: bool,
    enabled: bool,
    press: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    row()
        .key(title.to_lowercase().replace(' ', "-"))
        .label(title)
        .h(30.0)
        .px(10.0)
        .center()
        .radius(6.0)
        .border(
            1.0,
            if active {
                palette.accent
            } else {
                palette.border
            },
        )
        .fill(if active {
            palette.accent_bg
        } else {
            palette.surface
        })
        .opacity(if enabled { 1.0 } else { 0.48 })
        .disabled(!enabled)
        .focusable(enabled)
        .on_press_tx(move |s, tx| {
            if enabled {
                press(s, tx);
            }
        })
        .child(label(
            title,
            12.0,
            550.0,
            if active { palette.accent } else { palette.text },
        ))
}

fn card_view(s: &State, card: &Card, p: &Palette, scale: f32) -> El {
    let id = card.id;
    let page = &s.project.tree.pages[&id];
    let z = s.view.zoom;
    let [x, y, w, h] = card.rect;
    let [ix, iy, iw, ih] = card.image;
    let selected = s.selected.contains(&id);
    let targeted = s.drop == Some(Destination::Child(id));
    let mut image = el()
        .key(format!("page-image-{id}"))
        .absolute(ix, iy)
        .size(iw, ih)
        .fill(Color::WHITE);
    let mut geometry = Vec::new();
    if let Some((source, index)) = page.source {
        let desired = iw.max(ih) * scale;
        let edge = [320, 640, 960, 1440]
            .into_iter()
            .find(|&edge| edge as f32 >= desired)
            .unwrap_or(1440);
        match s.preview(PreviewKey {
            source,
            index,
            rotation: page.rotation,
            edge,
        }) {
            Some(Ok((pixels, text))) => {
                image = image.pixels(&pixels);
                geometry = text;
            }
            Some(Err(error)) => {
                image = image
                    .center()
                    .child(label(error, 11.0 * z, 450.0, Color::BLACK).wrap());
            }
            None => {
                image = image
                    .center()
                    .child(label("Loading page…", 11.0 * z, 450.0, Color::BLACK));
            }
        }
    }
    let preview = s
        .selection
        .as_ref()
        .filter(|(page, _, _)| *page == id)
        .map(|(_, _, r)| r.as_slice())
        .unwrap_or_default();
    for (&rect, opacity) in page
        .highlights
        .iter()
        .map(|r| (r, 0.42))
        .chain(preview.iter().map(|r| (r, 0.65)))
    {
        let [x, y, w, h] = rotate_rect(rect, page.rotation);
        image = image.child(
            el().absolute(x * iw, y * ih)
                .size(w * iw, h * ih)
                .fill(color("#ffc92c").alpha(opacity)),
        );
    }
    let rotation = page.rotation;
    image = match s.tool {
        Tool::Highlight => image
            .cursor(Cursor::Crosshair)
            .on_drag(move |s: &mut State, d, _| highlight(s, id, d, &geometry, rotation)),
        Tool::Select => image.on_press_tx(move |s: &mut State, tx| select(s, id, tx.mods())),
        _ => image,
    };
    let caption = match page.source {
        Some((_, index)) => format!("{:02}    Page {}", card.number, index + 1),
        None => format!("{:02}    Blank page", card.number),
    };
    let mut header = row()
        .key(format!("page-header-{id}"))
        .absolute(0.0, 0.0)
        .size(w, 36.0 * z)
        .px(10.0 * z)
        .center_y()
        .fill(if targeted || selected {
            p.accent_bg
        } else {
            p.surface
        })
        .child(
            label(
                caption,
                (12.0 * z).clamp(8.0, 25.0),
                650.0,
                if selected || targeted {
                    p.accent
                } else {
                    p.text
                },
            )
            .truncate(),
        );
    if s.tool == Tool::Select {
        header = header
            .cursor(Cursor::Grab)
            .on_drag(move |s: &mut State, d, tx| page_drag(s, id, d, tx.mods()));
    }
    el().key(format!("page-{id}"))
        .absolute(x, y)
        .size(w, h)
        .radius(7.0)
        .clip_rounded(7.0)
        .fill(p.panel)
        .border(
            if selected || targeted { 2.0 } else { 1.0 },
            if selected || targeted {
                p.accent
            } else {
                p.border
            },
        )
        .child(header)
        .child(image)
}

fn note_view(s: &State, id: Id, note: &Note, p: &Palette) -> El {
    let z = s.view.zoom;
    let [x, y] = s
        .note_drag
        .filter(|(moving, _)| *moving == id)
        .map(|(_, pos)| pos)
        .unwrap_or(note.position);
    let [left, top] = [x * z + s.view.pan[0], y * z + s.view.pan[1]];
    let title = label("Note", (12.0 * z).clamp(8.0, 24.0), 700.0, p.note_text);
    let mut header = row()
        .key(format!("note-drag-{id}"))
        .grow(1.0)
        .h(26.0 * z)
        .center_y()
        .child(title);
    if s.tool != Tool::Hand {
        header = header
            .cursor(Cursor::Grab)
            .on_drag(move |s: &mut State, d, _| {
                let base = s.project.tree.notes[&id].position;
                let pos = [base[0] + d.dx / s.view.zoom, base[1] + d.dy / s.view.zoom];
                match d.phase {
                    Phase::Down | Phase::Move => {
                        if d.phase == Phase::Down || s.note_drag.is_some() {
                            s.note_drag = Some((id, pos));
                        }
                    }
                    Phase::Up if s.note_drag.take().is_some() && !d.cancelled() => {
                        s.edit(|p| p.tree.notes.get_mut(&id).unwrap().position = pos);
                    }
                    _ => {}
                }
            });
    }
    let remove = label("×", (17.0 * z).clamp(10.0, 30.0), 650.0, p.note_text);
    let actions = row().w_full().child(header).child(
        el().label("Delete note")
            .size(24.0 * z, 26.0 * z)
            .center()
            .child(remove)
            .on_press(move |s: &mut State| {
                s.edit(|p| {
                    p.tree.notes.remove(&id);
                });
            }),
    );
    col()
        .key(format!("note-{id}"))
        .absolute(left, top)
        .w(230.0 * z)
        .padding(10.0 * z)
        .gap(4.0 * z)
        .clip_rounded(7.0)
        .radius(7.0)
        .fill(p.note)
        .child(actions)
        .child(
            label(&note.text, (14.0 * z).clamp(8.0, 32.0), 500.0, p.note_text)
                .w_full()
                .wrap()
                .on_press(move |s: &mut State| {
                    let position = s.project.tree.notes[&id].position;
                    s.note_editor(Some(id), position);
                }),
        )
}

fn workspace(s: &State, p: &Palette, scale: f32) -> El {
    let cards = cards(s);
    let mut lines = Shape::new();
    let headers: BTreeMap<_, _> = cards.iter().map(|c| (c.id, c.rect)).collect();
    for card in &cards {
        if let Some(parent) = card.parent {
            let [px, py, _, _] = headers[&parent];
            let [x, y, _, _] = card.rect;
            let inset = s.view.zoom * 16.0;
            let points = vec![
                [px - inset, py + 18.0 * s.view.zoom],
                [px - inset, y + 18.0 * s.view.zoom],
                [x, y + 18.0 * s.view.zoom],
            ];
            if points[2][1] >= 0.0 && points[0][1] < s.canvas[3] {
                lines = lines.stroke(points, 1.5, p.border);
            }
        }
    }
    let mut background = canvas(lines)
        .key("canvas-background")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full();
    if matches!(s.tool, Tool::Hand | Tool::Select | Tool::Note) {
        background = background
            .cursor(if s.tool == Tool::Note {
                Cursor::Crosshair
            } else {
                Cursor::Grab
            })
            .on_drag(|s: &mut State, d, _| {
                if s.tool == Tool::Note {
                    if d.phase == Phase::Up && !d.cancelled() && d.dx.hypot(d.dy) < 4.0 {
                        let pos = [
                            (d.x - s.view.pan[0]) / s.view.zoom,
                            (d.y - s.view.pan[1]) / s.view.zoom,
                        ];
                        s.note_editor(None, pos);
                    }
                } else {
                    match d.phase {
                        Phase::Down => d.data = 0.0,
                        Phase::Move => {
                            s.view.pan[0] += d.dx - s.canvas_drag_delta[0];
                            s.view.pan[1] += d.dy - s.canvas_drag_delta[1];
                            s.canvas_drag_delta = [d.dx, d.dy];
                            d.data = 1.0;
                        }
                        Phase::Up => {
                            if d.data == 0.0 && s.tool == Tool::Select && !d.cancelled() {
                                s.selected.clear();
                                s.anchor = None;
                            }
                            s.canvas_drag_delta = [0.0; 2];
                        }
                    }
                }
            });
    }
    if s.choosing_file || s.importing {
        background = background.disabled(true);
    }
    let mut board = el()
        .key("canvas")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .clip_rounded(0.1)
        .fill(p.canvas)
        .on_layout(|s: &mut State, rect| s.canvas = rect)
        .on_wheel(|s: &mut State, event, _| {
            if event.mods.ctrl {
                s.zoom((event.dy * 0.005).exp(), [event.x, event.y]);
            } else if event.mods.shift {
                s.view.pan[0] += event.dy + event.dx;
            } else {
                s.view.pan[0] += event.dx;
                s.view.pan[1] += event.dy;
            }
            true
        })
        .on_magnify(|s: &mut State, event, _| {
            s.zoom(1.0 + event.delta, [event.x, event.y]);
            true
        })
        .child(background);
    if cards.is_empty() {
        board = board.child(
            col()
                .absolute(42.0, 48.0)
                .gap(12.0)
                .child(label("Start with a PDF", 29.0, 690.0, p.text))
                .child(label(
                    "Open or drop a PDF to lay out its pages.",
                    14.0,
                    450.0,
                    p.muted,
                ))
                .child(button("Open a PDF", p, true, !s.importing, |s, tx| {
                    s.request(Next::OpenPdf, tx)
                })),
        );
    }
    for card in cards
        .iter()
        .filter(|c| visible(c.rect, s.canvas) || s.drag == Some(c.id))
    {
        board = board.child(card_view(s, card, p, scale));
        if s.drop == Some(Destination::Before(card.id)) {
            let [x, y, w, _] = card.rect;
            board = board.child(
                canvas(Shape::new().line([0.0, 2.0], [w, 2.0], 3.0, LineCap::Round, p.accent))
                    .absolute(x, y - 8.0)
                    .size(w, 4.0),
            );
        }
    }
    for (&id, note) in &s.project.tree.notes {
        board = board.child(note_view(s, id, note, p));
    }
    if s.drag.is_some() {
        let [x, y, w, h] = root_zone(s);
        let active = s.drop == Some(Destination::Root);
        board = board.child(
            el().absolute(x, y)
                .size(w, h)
                .center()
                .radius(6.0)
                .fill(p.accent_bg)
                .border(if active { 2.0 } else { 1.0 }, p.accent)
                .child(label("Move to root", 12.0, 650.0, p.accent)),
        );
    }
    if s.choosing_file || s.importing {
        board = board.child(
            el().key("busy-layer")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .on_press(|_: &mut State| {}),
        );
    }
    board
}

fn prompt(s: &State, p: &Palette, modal: &Prompt) -> El {
    let (title, detail, confirm) = match modal {
        Prompt::Unsaved(_) => (
            "Save changes?",
            "The project has unsaved changes.".to_owned(),
            "Save and continue",
        ),
        Prompt::Import {
            action: Import::Replace(id),
            sizes,
            ..
        } => {
            let detail = if s
                .project
                .tree
                .pages
                .get(id)
                .is_some_and(|page| !page.highlights.is_empty())
            {
                format!(
                    "Choose one of {} pages. Replacing clears the existing highlights.",
                    sizes.len()
                )
            } else {
                format!("Choose one of {} pages.", sizes.len())
            };
            ("Replace page", detail, "Replace")
        }
        Prompt::Import { sizes, .. } => (
            "Add PDF pages",
            format!(
                "Choose from {} pages. Enter a range such as 1-3.",
                sizes.len()
            ),
            "Add pages",
        ),
        Prompt::Note { id: Some(_), .. } => ("Edit canvas note", String::new(), "Update note"),
        Prompt::Note { .. } => ("Add canvas note", String::new(), "Add note"),
    };
    let mut panel = col()
        .key("prompt")
        .w(430.0)
        .padding(22.0)
        .gap(13.0)
        .radius(12.0)
        .fill(p.panel)
        .border(1.0, p.border)
        .modal(false, |s: &mut State| s.prompt = None)
        .child(label(title, 22.0, 700.0, p.text));
    if !detail.is_empty() {
        panel = panel.child(label(detail, 13.0, 450.0, p.muted).wrap());
    }
    let mut actions = row()
        .gap(8.0)
        .child(button("Cancel", p, false, true, |s, _| s.prompt = None));
    if let Prompt::Unsaved(next) = modal {
        let next = *next;
        actions = actions
            .child(button("Discard changes", p, false, true, move |s, tx| {
                s.prompt = None;
                s.proceed(next, tx);
            }))
            .child(button(confirm, p, true, true, move |s, tx| {
                s.prompt = None;
                s.save(tx, Some(next));
            }));
    } else {
        let note = matches!(modal, Prompt::Note { .. });
        let mut input = text_input(&s.draft, TextStyle::new(14.0, 450.0))
            .key("prompt-input")
            .w_full()
            .color(p.text)
            .on_edit(|s: &mut State| &mut s.draft);
        if note {
            input = input.on_submit(|s: &mut State, _| s.draft.insert("\n"));
        } else {
            input = input
                .single_line()
                .on_submit(|s: &mut State, _| s.apply_prompt());
        }
        panel = panel.child(
            el().w_full()
                .min_h(if note { 82.0 } else { 35.0 })
                .padding(8.0)
                .radius(5.0)
                .border(1.0, p.border)
                .fill(p.canvas)
                .child(input),
        );
        if !s.status.is_empty() {
            panel = panel.child(label(&s.status, 12.0, 500.0, p.accent).wrap());
        }
        actions = actions.child(button(confirm, p, true, true, |s, _| s.apply_prompt()));
    }
    el().key("scrim")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(Color::BLACK.alpha(0.65))
        .child(panel.child(actions))
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let p = Palette::new();
    let ready = !s.importing && !s.choosing_file && s.prompt.is_none();
    let selected = !s.selected.is_empty();
    let dirty = s.saved != s.project;
    let title = s
        .path
        .as_ref()
        .and_then(|path| path.file_stem())
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "Untitled".into());
    let primary = row()
        .h(48.0)
        .w_full()
        .px(14.0)
        .gap(7.0)
        .center_y()
        .fill(p.panel)
        .child(label("PDF Tree", 17.0, 750.0, p.accent).w(78.0))
        .child(button("Open PDF", &p, false, ready, |s, tx| {
            s.request(Next::OpenPdf, tx)
        }))
        .child(button("Open project", &p, false, ready, |s, tx| {
            s.request(Next::OpenProject, tx)
        }))
        .child(button("Save", &p, false, ready, |s, tx| s.save(tx, None)))
        .child(
            label(
                format!("{title}{}", if dirty { " •" } else { "" }),
                13.0,
                550.0,
                p.text,
            )
            .grow(1.0)
            .truncate(),
        )
        .child(button(
            "Undo",
            &p,
            false,
            ready && !s.history.is_empty(),
            |s, _| s.history(false),
        ))
        .child(button(
            "Redo",
            &p,
            false,
            ready && !s.redo.is_empty(),
            |s, _| s.history(true),
        ));
    let mut actions = row()
        .h(44.0)
        .w_full()
        .px(14.0)
        .gap(7.0)
        .center_y()
        .fill(p.panel);
    for (name, tool) in [
        ("Select", Tool::Select),
        ("Hand", Tool::Hand),
        ("Highlight", Tool::Highlight),
        ("Note", Tool::Note),
    ] {
        actions = actions.child(button(name, &p, s.tool == tool, ready, move |s, _| {
            s.tool = tool;
            s.status.clear();
        }));
    }
    actions = actions
        .child(el().w(1.0).h(22.0).fill(p.border))
        .child(button("Add PDF pages", &p, false, ready, |s, tx| {
            s.pick_pdf(Import::Add, tx)
        }))
        .child(button("Blank page", &p, false, ready, |s, _| {
            s.edit(|p| {
                p.tree.add(Page::new(None, [595.0, 842.0]));
            })
        }))
        .child(button("Rotate", &p, false, ready && selected, |s, _| {
            s.rotate()
        }))
        .child(button(
            "Replace",
            &p,
            false,
            ready && s.selected.len() == 1,
            |s, tx| {
                if let Some(&id) = s.selected.first() {
                    s.pick_pdf(Import::Replace(id), tx);
                }
            },
        ))
        .child(button("Duplicate", &p, false, ready && selected, |s, _| {
            s.duplicate()
        }))
        .child(button("Remove", &p, false, ready && selected, |s, _| {
            s.remove()
        }));
    let help = match s.tool {
        Tool::Select => {
            "Drag headers to arrange pages · Ctrl-click or Shift-click to select multiple"
        }
        Tool::Hand => "Drag the canvas to pan · Ctrl-scroll to zoom",
        Tool::Highlight => "Drag across PDF text to highlight it",
        Tool::Note => "Click the canvas to add a note",
    };
    let status = if s.status.is_empty() { help } else { &s.status };
    let footer = row()
        .h(38.0)
        .w_full()
        .px(14.0)
        .gap(7.0)
        .center_y()
        .fill(p.panel)
        .child(label(status, 12.0, 450.0, p.muted).grow(1.0).truncate())
        .child(label(
            format!(
                "{} pages · {} selected",
                s.project.tree.pages.len(),
                s.selected.len()
            ),
            12.0,
            500.0,
            p.muted,
        ))
        .child(button("−", &p, false, true, |s, _| {
            s.zoom(0.8, [s.canvas[2] * 0.5, s.canvas[3] * 0.5])
        }))
        .child(button(
            &format!("{:.0}%", s.view.zoom * 100.0),
            &p,
            false,
            true,
            |s, _| s.view = Viewport::default(),
        ))
        .child(button("+", &p, false, true, |s, _| {
            s.zoom(1.25, [s.canvas[2] * 0.5, s.canvas[3] * 0.5])
        }));
    let mut window = col()
        .key("window")
        .w_full()
        .h_full()
        .fill(p.panel)
        .on_key(key)
        .child(primary)
        .child(actions)
        .child(workspace(s, &p, cx.env().scale))
        .child(footer)
        .on_drop_files(|s: &mut State, files, _| {
            if s.importing || s.choosing_file || s.prompt.is_some() {
                return;
            }
            if let Some(path) = files.into_iter().find(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
            }) {
                let action = if s.project == Project::default() {
                    Import::Open
                } else {
                    Import::Add
                };
                s.read_pdf(path.to_path_buf(), action);
            }
        });
    if let Some(modal) = &s.prompt {
        window = window.child(prompt(s, &p, modal));
    }
    window
}
