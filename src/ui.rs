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
            canvas: color("#111112"),
            panel: color("#19191b"),
            surface: color("#232326"),
            border: color("#343438"),
            text: color("#ebebed"),
            muted: color("#a7a7ae"),
            accent: color("#c3d4ff"),
            accent_bg: color("#2b354a"),
            note: color("#dedede"),
            note_text: color("#202020"),
        }
    }
}

struct Card {
    id: Id,
    parent: Option<Id>,
    number: usize,
    group: Option<(Id, u8)>,
    row_bottom: f32,
    rect: Rect,
    image: Rect,
}

const CARD_WIDTH: f32 = 296.0;
const BRANCH_GAP: f32 = 48.0;
const GROUP_GAP: f32 = 108.0;
const ROW_GAP: f32 = 90.0;

// Each entry is the leftmost and rightmost card edge at one depth.
#[derive(Default)]
struct Contour {
    rows: VecDeque<[f32; 2]>,
    origin: f32,
    bounds: [f32; 2],
    group_right: Option<f32>,
}

impl Contour {
    fn append(&mut self, branch: Self, grouped: bool, gap: f32) -> f32 {
        let mut shift = self
            .rows
            .iter()
            .zip(&branch.rows)
            .map(|(left, right)| left[1] - self.origin + gap - (right[0] - branch.origin))
            .reduce(f32::max)
            .unwrap_or(0.0);
        let boundary = if grouped {
            Some(self.bounds[1])
        } else {
            self.group_right
        };
        if let Some(right) = boundary {
            shift = shift.max(right - self.origin + GROUP_GAP - (branch.bounds[0] - branch.origin));
        }
        let offset = shift + self.origin - branch.origin;
        self.bounds[0] = self.bounds[0].min(branch.bounds[0] + offset);
        self.bounds[1] = self.bounds[1].max(branch.bounds[1] + offset);
        if grouped {
            self.group_right = Some(branch.bounds[1] + offset);
        }
        for (depth, [left, right]) in branch.rows.into_iter().enumerate() {
            let edges = [left + offset, right + offset];
            if let Some(row) = self.rows.get_mut(depth) {
                row[0] = row[0].min(edges[0]);
                row[1] = row[1].max(edges[1]);
            } else {
                self.rows.push_back(edges);
            }
        }
        shift
    }
}

fn horizontal_positions(
    tree: &model::Tree,
    order: &[(Id, Option<Id>, usize)],
    groups: &BTreeMap<Id, Option<(Id, u8)>>,
) -> BTreeMap<Id, f32> {
    let mut contours = BTreeMap::<Id, Contour>::new();
    let mut offsets = BTreeMap::new();
    for &(id, _, _) in order.iter().rev() {
        let children = &tree.pages[&id].children;
        let mut contour = match children.as_slice() {
            [] => Contour::default(),
            [child] => {
                offsets.insert(*child, 0.0);
                contours.remove(child).unwrap()
            }
            _ => {
                let mut combined = contours.remove(&children[0]).unwrap();
                offsets.insert(children[0], 0.0);
                combined.group_right = tree.pages[&children[0]].group.map(|_| combined.bounds[1]);
                let mut last = 0.0;
                for child in &children[1..] {
                    last = combined.append(
                        contours.remove(child).unwrap(),
                        tree.pages[child].group.is_some(),
                        if groups[&id].is_some() {
                            GROUP_GAP
                        } else {
                            BRANCH_GAP
                        },
                    );
                    offsets.insert(*child, last);
                }
                let center = last / 2.0;
                for child in children {
                    *offsets.get_mut(child).unwrap() -= center;
                }
                combined.origin += center;
                combined
            }
        };
        contour
            .rows
            .push_front([contour.origin, contour.origin + CARD_WIDTH]);
        contour.bounds[0] = contour.bounds[0].min(contour.origin);
        contour.bounds[1] = contour.bounds[1].max(contour.origin + CARD_WIDTH);
        contours.insert(id, contour);
    }

    let mut forest = Contour::default();
    for root in &tree.roots {
        let branch = contours.remove(root).unwrap();
        let x = if forest.rows.is_empty() {
            forest = branch;
            forest.group_right = tree.pages[root].group.map(|_| forest.bounds[1]);
            0.0
        } else {
            forest.append(branch, tree.pages[root].group.is_some(), BRANCH_GAP)
        };
        offsets.insert(*root, x);
    }

    for &(id, parent, _) in order {
        let parent_x = parent.map_or(44.0, |parent| offsets[&parent]);
        *offsets.get_mut(&id).unwrap() += parent_x;
    }
    offsets
}

fn cards(s: &State) -> Vec<Card> {
    let tree = &s.project.tree;
    let order = tree.order();
    let mut groups = BTreeMap::new();
    for &(id, parent, _) in &order {
        let group = tree.pages[&id]
            .group
            .map(|color| (id, color))
            .or_else(|| parent.and_then(|parent| groups[&parent]));
        groups.insert(id, group);
    }
    let positions = horizontal_positions(tree, &order, &groups);

    let mut row_heights = Vec::<f32>::new();
    let image_sizes: Vec<_> = order
        .iter()
        .map(|&(id, _, depth)| {
            let page = &tree.pages[&id];
            let [w, h] = if page.rotation % 2 == 0 {
                page.size
            } else {
                [page.size[1], page.size[0]]
            };
            let fit = (276.0 / w).min(328.0 / h);
            let image = if s.graph_mode {
                [276.0, 82.0]
            } else if page.text.is_some() {
                [276.0, 120.0]
            } else {
                [w * fit, h * fit]
            };
            if depth == row_heights.len() {
                row_heights.push(0.0);
            }
            row_heights[depth] = row_heights[depth].max(image[1] + 49.0);
            image
        })
        .collect();
    let mut row_gaps = vec![ROW_GAP; row_heights.len()];
    for &(id, parent, depth) in &order {
        if let Some(parent) = parent
            && (groups[&id].is_none() || groups[&id] != groups[&parent])
        {
            let spread = (positions[&id] - positions[&parent]).abs();
            // Give wide fans room to descend while keeping straight chains compact.
            let gap = (spread * 0.40).clamp(ROW_GAP, 320.0);
            row_gaps[depth - 1] = row_gaps[depth - 1].max(gap);
        }
    }
    let row_tops: Vec<_> = row_heights
        .iter()
        .zip(row_gaps)
        .scan(28.0, |top, (height, gap)| {
            let current = *top;
            *top += height + gap;
            Some(current)
        })
        .collect();

    let z = s.view.zoom;
    let mut placed = BTreeMap::<Id, [f32; 2]>::new();
    order
        .into_iter()
        .zip(image_sizes)
        .enumerate()
        .map(|(index, ((id, parent, depth), [iw, ih]))| {
            let page = &tree.pages[&id];
            let position = s
                .reposition
                .filter(|drag| drag.node == id)
                .map(|drag| drag.position)
                .or(page.branch_position)
                .filter(|position| {
                    parent == Some(position.parent)
                        && (groups[&id].is_none() || groups[&id] != groups[&position.parent])
                });
            let default = [positions[&id], row_tops[depth]];
            let point = if let Some(parent) = parent {
                let [px, py] = placed[&parent];
                let [dx, dy] = position.map_or(
                    [
                        default[0] - positions[&parent],
                        default[1] - row_tops[depth - 1],
                    ],
                    |position| position.offset,
                );
                [px + dx, py + dy]
            } else {
                default
            };
            placed.insert(id, point);
            Card {
                id,
                parent,
                number: index + 1,
                group: groups[&id],
                row_bottom: s.view.pan[1] + (point[1] + row_heights[depth]) * z,
                rect: [
                    s.view.pan[0] + point[0] * z,
                    s.view.pan[1] + point[1] * z,
                    CARD_WIDTH * z,
                    (ih + 49.0) * z,
                ],
                image: [(CARD_WIDTH - iw) / 2.0 * z, 39.0 * z, iw * z, ih * z],
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
            if contains([x - gap, y, gap, h], point) {
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

pub(super) fn reveal_root_move(s: &mut State) {
    let layout = cards(s);
    let mut moved = layout
        .iter()
        .filter(|card| card.parent.is_none() && s.selected.contains(&card.id));
    let Some(first) = moved.next() else { return };
    let [x, y, width, height] = first.rect;
    let (right, bottom) = moved.fold((x + width, y + height), |(right, bottom), card| {
        let [x, y, w, h] = card.rect;
        (right.max(x + w), bottom.max(y + h))
    });
    let margin = 20.0;
    let [_, _, vw, vh] = s.canvas;
    let reveal = |start: f32, end: f32, extent: f32| {
        if start < margin || end - start > extent - 2.0 * margin {
            margin - start
        } else {
            (extent - margin - end).min(0.0)
        }
    };
    s.view.pan[0] += reveal(x, right, vw);
    s.view.pan[1] += reveal(y, bottom, vh);
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
                    if destination == Destination::Root {
                        reveal_root_move(s);
                        s.status = "Moved selection to the top level.".into();
                    }
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
            let color = s.highlight_color;
            s.edit(|p| {
                p.tree.pages.get_mut(&id).unwrap().highlights.extend(
                    rects
                        .into_iter()
                        .map(|rect| Highlight::Colored { rect, color }),
                );
            });
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
    let ink = if active { palette.accent } else { palette.text };
    control(title, palette, active, enabled, press)
        .px(7.0)
        .child(label(title, 11.0, 500.0, ink))
}

fn control(
    title: &str,
    palette: &Palette,
    active: bool,
    enabled: bool,
    press: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let mut control = row()
        .key(title.to_lowercase().replace(' ', "-"))
        .label(title)
        .help(title)
        .h(28.0)
        .center()
        .radius(5.0)
        .fill(if active {
            palette.accent_bg
        } else {
            palette.panel
        })
        .opacity(if enabled { 1.0 } else { 0.46 })
        .disabled(!enabled)
        .focusable(enabled)
        .on_press_tx(move |s, tx| {
            if enabled {
                press(s, tx);
            }
        });
    if enabled {
        control = control.cursor(Cursor::Pointer).hover(if active {
            palette.accent_bg
        } else {
            palette.surface
        });
    }
    control
}

const COMMAND: &str = if cfg!(target_os = "macos") {
    "Cmd"
} else {
    "Ctrl"
};

enum ToolbarIcon {
    Cursor,
    Hand,
    Pencil,
    Plus,
    Marker,
    Rotate,
    Group,
    BlankPage,
    AddPdf,
    Replace,
    Duplicate,
    Delete,
    Reposition,
    OpenPdf,
    Folder,
    Save,
    Undo,
    Redo,
}

impl ToolbarIcon {
    fn view(self, ink: Color) -> El {
        use rune::Path;

        let path = match self {
            Self::Cursor => Path::new()
                .move_to([4.0, 2.0])
                .line_to([4.0, 14.0])
                .line_to([7.5, 10.5])
                .line_to([9.5, 15.0])
                .line_to([11.5, 14.0])
                .line_to([9.5, 9.5])
                .line_to([14.0, 9.5])
                .close(),
            Self::Hand => Path::new()
                .move_to([6.0, 9.0])
                .line_to([6.0, 3.5])
                .quad_to([7.0, 1.5], [8.0, 3.5])
                .line_to([8.0, 7.0])
                .line_to([8.0, 2.5])
                .quad_to([9.0, 0.5], [10.0, 2.5])
                .line_to([10.0, 7.0])
                .line_to([10.0, 3.5])
                .quad_to([11.0, 1.5], [12.0, 3.5])
                .line_to([12.0, 7.0])
                .line_to([12.0, 5.0])
                .quad_to([13.0, 3.0], [14.0, 5.0])
                .line_to([14.0, 10.5])
                .quad_to([14.0, 15.0], [10.0, 15.0])
                .quad_to([7.0, 15.0], [5.5, 12.5])
                .line_to([3.0, 9.0])
                .quad_to([2.5, 7.0], [4.5, 7.5])
                .close(),
            Self::Pencil => Path::new()
                .move_to([3.0, 10.0])
                .line_to([11.5, 1.5])
                .line_to([14.5, 4.5])
                .line_to([6.0, 13.0])
                .line_to([2.0, 14.0])
                .close()
                .move_to([9.5, 3.5])
                .line_to([12.5, 6.5])
                .move_to([8.5, 14.5])
                .line_to([14.5, 14.5]),
            Self::Plus => Path::new()
                .move_to([3.0, 8.0])
                .line_to([13.0, 8.0])
                .move_to([8.0, 3.0])
                .line_to([8.0, 13.0]),
            Self::Marker => Path::new()
                .move_to([9.0, 2.0])
                .line_to([14.0, 7.0])
                .line_to([8.0, 13.0])
                .line_to([3.0, 8.0])
                .close()
                .move_to([3.0, 8.0])
                .line_to([2.0, 13.0])
                .line_to([5.0, 13.0])
                .line_to([6.5, 11.5])
                .move_to([2.0, 15.0])
                .line_to([10.0, 15.0]),
            Self::Rotate => Path::new()
                .move_to([13.5, 6.0])
                .quad_to([12.0, 2.0], [8.0, 2.0])
                .quad_to([2.0, 2.0], [2.0, 8.0])
                .quad_to([2.0, 14.0], [8.0, 14.0])
                .quad_to([12.0, 14.0], [14.0, 10.0])
                .move_to([10.0, 6.0])
                .line_to([14.0, 6.0])
                .line_to([14.0, 2.0]),
            Self::Group => Path::new()
                .move_to([1.5, 5.0])
                .line_to([1.5, 1.5])
                .line_to([5.0, 1.5])
                .move_to([11.0, 1.5])
                .line_to([14.5, 1.5])
                .line_to([14.5, 5.0])
                .move_to([14.5, 11.0])
                .line_to([14.5, 14.5])
                .line_to([11.0, 14.5])
                .move_to([5.0, 14.5])
                .line_to([1.5, 14.5])
                .line_to([1.5, 11.0])
                .move_to([4.0, 5.0])
                .line_to([7.0, 5.0])
                .line_to([7.0, 11.0])
                .line_to([4.0, 11.0])
                .close()
                .move_to([9.0, 5.0])
                .line_to([12.0, 5.0])
                .line_to([12.0, 11.0])
                .line_to([9.0, 11.0])
                .close(),
            Self::BlankPage => Path::new()
                .move_to([3.0, 1.5])
                .line_to([9.0, 1.5])
                .line_to([13.0, 5.5])
                .line_to([13.0, 14.5])
                .line_to([3.0, 14.5])
                .close()
                .move_to([9.0, 1.5])
                .line_to([9.0, 5.5])
                .line_to([13.0, 5.5]),
            Self::AddPdf => Path::new()
                .move_to([2.0, 1.5])
                .line_to([8.0, 1.5])
                .line_to([12.0, 5.5])
                .line_to([12.0, 8.0])
                .move_to([8.0, 1.5])
                .line_to([8.0, 5.5])
                .line_to([12.0, 5.5])
                .move_to([7.0, 14.5])
                .line_to([2.0, 14.5])
                .line_to([2.0, 1.5])
                .move_to([4.5, 8.0])
                .line_to([8.0, 8.0])
                .move_to([4.5, 10.5])
                .line_to([6.0, 10.5])
                .move_to([8.5, 12.0])
                .line_to([15.0, 12.0])
                .move_to([11.8, 8.8])
                .line_to([11.8, 15.2]),
            Self::Replace => Path::new()
                .move_to([2.0, 8.0])
                .line_to([2.0, 2.0])
                .line_to([8.0, 2.0])
                .line_to([11.0, 5.0])
                .line_to([11.0, 7.0])
                .move_to([8.0, 2.0])
                .line_to([8.0, 5.0])
                .line_to([11.0, 5.0])
                .move_to([1.5, 10.0])
                .line_to([14.5, 10.0])
                .line_to([12.0, 7.5])
                .move_to([14.5, 13.5])
                .line_to([1.5, 13.5])
                .line_to([4.0, 11.0]),
            Self::Duplicate => Path::new()
                .move_to([5.5, 5.5])
                .line_to([14.0, 5.5])
                .line_to([14.0, 14.0])
                .line_to([5.5, 14.0])
                .close()
                .move_to([2.0, 10.5])
                .line_to([2.0, 2.0])
                .line_to([10.5, 2.0])
                .line_to([10.5, 3.5]),
            Self::Delete => Path::new()
                .move_to([2.0, 4.0])
                .line_to([14.0, 4.0])
                .move_to([5.5, 4.0])
                .line_to([5.5, 2.0])
                .line_to([10.5, 2.0])
                .line_to([10.5, 4.0])
                .move_to([3.5, 4.0])
                .line_to([4.5, 14.0])
                .line_to([11.5, 14.0])
                .line_to([12.5, 4.0])
                .move_to([6.5, 6.5])
                .line_to([7.0, 11.5])
                .move_to([9.5, 6.5])
                .line_to([9.0, 11.5]),
            Self::Reposition => Path::new()
                .move_to([8.0, 1.0])
                .line_to([8.0, 15.0])
                .move_to([1.0, 8.0])
                .line_to([15.0, 8.0])
                .move_to([5.5, 3.5])
                .line_to([8.0, 1.0])
                .line_to([10.5, 3.5])
                .move_to([5.5, 12.5])
                .line_to([8.0, 15.0])
                .line_to([10.5, 12.5])
                .move_to([3.5, 5.5])
                .line_to([1.0, 8.0])
                .line_to([3.5, 10.5])
                .move_to([12.5, 5.5])
                .line_to([15.0, 8.0])
                .line_to([12.5, 10.5]),
            Self::OpenPdf => Path::new()
                .move_to([3.0, 1.5])
                .line_to([9.0, 1.5])
                .line_to([13.0, 5.5])
                .line_to([13.0, 14.5])
                .line_to([3.0, 14.5])
                .close()
                .move_to([9.0, 1.5])
                .line_to([9.0, 5.5])
                .line_to([13.0, 5.5])
                .move_to([5.5, 8.0])
                .line_to([10.5, 8.0])
                .move_to([5.5, 11.0])
                .line_to([10.5, 11.0]),
            Self::Folder => Path::new()
                .move_to([1.5, 13.5])
                .line_to([1.5, 3.0])
                .line_to([6.5, 3.0])
                .line_to([8.0, 5.0])
                .line_to([13.0, 5.0])
                .line_to([13.0, 7.0])
                .move_to([1.5, 13.5])
                .line_to([4.0, 7.5])
                .line_to([15.0, 7.5])
                .line_to([13.0, 13.5])
                .close(),
            Self::Save => Path::new()
                .move_to([2.0, 2.0])
                .line_to([12.0, 2.0])
                .line_to([14.0, 4.0])
                .line_to([14.0, 14.0])
                .line_to([2.0, 14.0])
                .close()
                .move_to([5.0, 2.0])
                .line_to([5.0, 6.0])
                .line_to([11.0, 6.0])
                .line_to([11.0, 2.0])
                .move_to([5.0, 14.0])
                .line_to([5.0, 9.0])
                .line_to([11.0, 9.0])
                .line_to([11.0, 14.0]),
            Self::Undo => Path::new()
                .move_to([5.5, 3.0])
                .line_to([2.0, 6.5])
                .line_to([5.5, 10.0])
                .move_to([2.0, 6.5])
                .line_to([9.5, 6.5])
                .quad_to([14.0, 6.5], [14.0, 11.0])
                .line_to([14.0, 13.0]),
            Self::Redo => Path::new()
                .move_to([10.5, 3.0])
                .line_to([14.0, 6.5])
                .line_to([10.5, 10.0])
                .move_to([14.0, 6.5])
                .line_to([6.5, 6.5])
                .quad_to([2.0, 6.5], [2.0, 11.0])
                .line_to([2.0, 13.0]),
        };
        canvas(Shape::new().stroke_path(&path, 1.3, LineCap::Round, ink)).size(16.0, 16.0)
    }
}

fn icon_button(
    title: &str,
    icon: ToolbarIcon,
    palette: &Palette,
    active: bool,
    enabled: bool,
    press: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let ink = if active { palette.accent } else { palette.text };
    control(title, palette, active, enabled, press)
        .w(30.0)
        .child(icon.view(ink))
}

fn reposition_drag(s: &mut State, id: Id, initial: BranchPosition, d: &Drag) {
    match d.phase {
        Phase::Down => {
            s.reposition = Some(RepositionDrag {
                node: id,
                initial,
                position: initial,
            })
        }
        Phase::Move | Phase::Up => {
            let Some(drag) = s.reposition.filter(|drag| drag.node == id) else {
                return;
            };
            let initial = drag.initial;
            let offset = [
                initial.offset[0] + d.dx / s.view.zoom,
                initial.offset[1] + d.dy / s.view.zoom,
            ];
            let position = BranchPosition { offset, ..initial };
            if d.phase == Phase::Up {
                s.reposition = None;
                if !d.cancelled()
                    && offset != initial.offset
                    && offset.iter().all(|v| v.is_finite())
                {
                    s.edit(|project| {
                        project.tree.pages.get_mut(&id).unwrap().branch_position = Some(position);
                    });
                }
            } else {
                s.reposition = Some(RepositionDrag { position, ..drag });
            }
        }
    }
}

fn card_view(s: &State, card: &Card, parent: Option<&Card>, p: &Palette, scale: f32) -> El {
    let id = card.id;
    let page = &s.project.tree.pages[&id];
    let z = s.view.zoom;
    let [x, y, w, h] = card.rect;
    let [ix, iy, iw, ih] = card.image;
    let selected = s.selected.contains(&id);
    let targeted = s.drop == Some(Destination::Child(id));
    let graph_text = s.graph_mode.then(|| match page.source {
        Some(source) => s
            .slide_title(source)
            .unwrap_or_else(|| format!("Page {}", source.1 + 1)),
        None => page.text.clone().unwrap_or_else(|| "Blank page".into()),
    });
    let mut image = el()
        .key(format!("page-image-{id}"))
        .absolute(ix, iy)
        .size(iw, ih)
        .fill(Color::WHITE);
    let mut geometry = Vec::new();
    if !s.graph_mode
        && let Some((source, index)) = page.source
    {
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
    for ((rect, ink), opacity) in page
        .highlights
        .iter()
        .filter(|_| !s.graph_mode)
        .map(|r| (r.parts(), 0.42))
        .chain(preview.iter().map(|&r| ((r, s.highlight_color), 0.65)))
    {
        let [x, y, w, h] = rotate_rect(rect, page.rotation);
        image = image.child(
            el().absolute(x * iw, y * ih)
                .size(w * iw, h * ih)
                .fill(color(ink.style().1).alpha(opacity)),
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
    let radius = if s.graph_mode { 24.0 * z } else { 7.0 };
    let text_align = if s.graph_mode {
        rune::Align::Center
    } else {
        rune::Align::Start
    };
    if let Some(content) = graph_text.as_ref().or(page.text.as_ref()) {
        image = el()
            .key(format!("text-node-body-{id}"))
            .absolute(ix, iy)
            .size(iw, ih)
            .padding(10.0 * z)
            .fill(if s.graph_mode { p.panel } else { p.surface })
            .clip_rounded(3.0)
            .child(
                label(
                    content,
                    ((if s.graph_mode { 17.0 } else { 14.0 }) * z).clamp(8.0, 36.0),
                    if s.graph_mode { 700.0 } else { 450.0 },
                    p.text,
                )
                .align(text_align)
                .w_full()
                .wrap()
                .max_lines(if s.graph_mode { 3 } else { 5 }),
            )
            .on_press_tx(move |s: &mut State, tx| select(s, id, tx.mods()));
        if s.graph_mode {
            image = image.center();
        }
        if page.text.is_some() {
            image = image.on_double_press(move |s: &mut State| s.text_node_editor(Some(id)));
        }
    }
    let caption = match page.source {
        Some((_, index)) => format!("{:02}    Page {}", card.number, index + 1),
        None if page.text.is_some() => format!("{:02}    Text node", card.number),
        None => format!("{:02}    Blank page", card.number),
    };
    let group_color = card
        .group
        .map(|(_, index)| color(GROUP_COLORS[index as usize].1));
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
    if s.graph_mode {
        header = header.center();
    }
    if s.tool == Tool::Note {
        header = header.on_press(move |s: &mut State| s.sticky_editor(id));
        image = image.on_press(move |s: &mut State| s.sticky_editor(id));
    }
    if s.tool == Tool::Select {
        header = header
            .cursor(Cursor::Grab)
            .on_drag(move |s: &mut State, d, tx| page_drag(s, id, d, tx.mods()));
    }
    if s.tool == Tool::Reposition
        && let Some(parent) = parent
        && (card.group.is_none() || card.group != parent.group)
    {
        let initial = BranchPosition {
            parent: parent.id,
            offset: [(x - parent.rect[0]) / z, (y - parent.rect[1]) / z],
        };
        header = header
            .cursor(Cursor::Grab)
            .on_drag(move |s: &mut State, d, _| reposition_drag(s, id, initial, d));
        header = header.on_double_press(move |s: &mut State| {
            s.edit(|project| project.tree.pages.get_mut(&id).unwrap().branch_position = None);
        });
    }
    let outline = if targeted {
        p.accent
    } else if let Some(ink) = group_color {
        ink.alpha(if selected { 1.0 } else { 0.74 })
    } else if selected {
        p.accent.alpha(0.72)
    } else {
        Color::TRANSPARENT
    };
    el().key(format!("page-{id}"))
        .absolute(x, y)
        .size(w, h)
        .radius(radius)
        .clip_rounded(radius)
        .fill(p.panel)
        .on_secondary_press(move |s: &mut State, _, _| {
            if s.importing || s.choosing_file || s.prompt.is_some() {
                return;
            }
            let mut subtree = s.project.tree.descendants(id);
            subtree.insert(id);
            s.tool = Tool::Select;
            s.anchor = Some(id);
            s.status = format!(
                "Selected {} node(s). Drag a selected header to move the subtree.",
                subtree.len()
            );
            s.selected = subtree;
        })
        .child(header)
        .child(image)
        .child(
            el().absolute(0.0, 0.0)
                .size(w, h)
                .radius(radius)
                .border(1.0, outline),
        )
}

const GROUP_COLORS: [(&str, &str); 12] = [
    ("Red", "#ee7777"),
    ("Orange", "#efa65c"),
    ("Gold", "#e5cc60"),
    ("Lime", "#b7d46a"),
    ("Green", "#73ce89"),
    ("Mint", "#65d9bb"),
    ("Cyan", "#68cadf"),
    ("Blue", "#7cafff"),
    ("Indigo", "#a1a1ff"),
    ("Violet", "#c39bf7"),
    ("Pink", "#f299cf"),
    ("Rose", "#f092a7"),
];

fn note_view(s: &State, id: Id, note: &Note, position: [f32; 2], p: &Palette) -> El {
    let z = s.view.zoom;
    let [left, top] = position;
    let header = row().grow(1.0).h(26.0 * z).center_y().child(label(
        "Note",
        (12.0 * z).clamp(8.0, 24.0),
        700.0,
        p.note_text,
    ));
    let remove = label("×", (17.0 * z).clamp(10.0, 30.0), 650.0, p.note_text);
    let actions = row().w_full().child(header).child(
        el().label("Delete note")
            .help("Delete note")
            .size(24.0 * z, 26.0 * z)
            .center()
            .child(remove)
            .on_press(move |s: &mut State| {
                s.edit(|p| {
                    p.tree.notes.remove(&id);
                });
            }),
    );
    let radius = if s.graph_mode { 35.0 * z } else { 7.0 };
    let mut body = label(&note.text, (14.0 * z).clamp(8.0, 32.0), 500.0, p.note_text)
        .w_full()
        .wrap()
        .on_press(move |s: &mut State| {
            let node = s.project.tree.notes[&id].node;
            s.note_editor(Some(id), node);
        });
    if s.graph_mode {
        body = body.align(rune::Align::Center);
    }
    col()
        .key(format!("note-{id}"))
        .absolute(left, top)
        .w(230.0 * z)
        .padding(if s.graph_mode { 20.0 * z } else { 10.0 * z })
        .gap(4.0 * z)
        .clip_rounded(radius)
        .radius(radius)
        .fill(p.note)
        .child(actions)
        .child(body)
}

fn soft_edge(parent: &Card, child: &Card, zoom: f32) -> rune::Path {
    let [px, py, pw, ph] = parent.rect;
    let [x, ey, w, _] = child.rect;
    let [sx, sy] = [px + pw / 2.0, py + ph];
    let ex = x + w / 2.0;
    let end = [ex, ey];
    let clearance = 32.0 * zoom;
    let gap = ey - sy;
    let mut path = rune::Path::new().move_to([sx, sy]);
    if gap > 0.0 {
        // Keep both tangents vertical and spread the turn across the available gap.
        return path.cubic_to([sx, sy + gap * 0.65], [ex, ey - gap * 0.65], end);
    }

    // Route around the parent when the child is beside or above it.
    let outside = if ex >= sx {
        (px + pw).max(x + w) + clearance
    } else {
        px.min(x) - clearance
    };
    let bottom = py + ph + clearance;
    let top = ey.min(py) - clearance;
    path = path
        .cubic_to(
            [sx, bottom + clearance],
            [outside, bottom + clearance],
            [outside, bottom],
        )
        .line_to([outside, top + clearance]);
    path.cubic_to(
        [outside, top - clearance],
        [ex, top - clearance],
        [ex, top + clearance * 0.5],
    )
    .line_to(end)
}

fn homepage(s: &State, p: &Palette) -> El {
    let ready = !s.importing && !s.choosing_file && s.prompt.is_none();
    let start = col()
        .items_center()
        .gap(10.0)
        .child(label("Start new project", 29.0, 690.0, p.text))
        .child(label(
            "Open or drop a PDF to lay out its pages.",
            14.0,
            450.0,
            p.muted,
        ))
        .child(
            button("Open a PDF", p, true, ready, |s, tx| {
                s.request(Next::OpenPdf, tx)
            })
            .help_keys([COMMAND, "Shift", "O"]),
        );
    let mut home = col()
        .key("homepage")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .gap(20.0)
        .padding(16.0)
        .child(start);
    if !s.recent_projects.is_empty() {
        let mut projects = col()
            .w(440.0)
            .min_h(0.0)
            .shrink()
            .gap(4.0)
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            });
        for (index, path) in s.recent_projects.iter().enumerate() {
            let name = path
                .file_stem()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            let location = path
                .parent()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default();
            let help = path.display().to_string();
            let name = name.into_owned();
            let path = path.clone();
            projects = projects.child(
                control(&name, p, false, ready, move |s, tx| {
                    s.request(Next::OpenRecent(path.clone()), tx)
                })
                .key(format!("recent-project-{index}"))
                .help(help)
                .w_full()
                .px(10.0)
                .gap(12.0)
                .fill(p.panel)
                .child(label(name, 13.0, 550.0, p.text).truncate())
                .child(label(location, 11.0, 450.0, p.muted).grow(1.0).truncate()),
            );
        }
        home = home.child(
            col()
                .min_h(0.0)
                .shrink()
                .gap(6.0)
                .child(label("Recent projects", 13.0, 600.0, p.muted))
                .child(projects),
        );
    }
    home
}

fn workspace(s: &State, p: &Palette, scale: f32) -> El {
    let cards = cards(s);
    let mut lines = Shape::new();
    let by_id: BTreeMap<_, _> = cards.iter().map(|c| (c.id, c)).collect();
    for card in &cards {
        let Some(parent) = card.parent.map(|id| by_id[&id]) else {
            continue;
        };
        let [px, py, pw, ph] = parent.rect;
        let [x, y, w, h] = card.rect;
        let sx = px + pw / 2.0;
        let ex = x + w / 2.0;
        let margin = 64.0 * s.view.zoom;
        if (py + ph).max(y + h) + margin < 0.0
            || py.min(y) - margin > s.canvas[3]
            || (px + pw).max(x + w) + margin < 0.0
            || px.min(x) - margin > s.canvas[2]
        {
            continue;
        }
        let ink = p.muted.alpha(0.55);
        if parent.group.is_some() && parent.group == card.group {
            let junction = (parent.row_bottom + y) / 2.0;
            lines = lines.stroke(
                vec![[sx, py + ph], [sx, junction], [ex, junction], [ex, y]],
                1.5,
                ink,
            );
        } else {
            let path = soft_edge(parent, card, s.view.zoom);
            lines = lines.stroke_path(&path, 1.5, LineCap::Round, ink);
        }
    }
    let mut background = canvas(lines)
        .key("canvas-background")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full();
    if matches!(s.tool, Tool::Hand | Tool::Select | Tool::Reposition) {
        background = background
            .cursor(if s.tool == Tool::Note {
                Cursor::Crosshair
            } else {
                Cursor::Grab
            })
            .on_drag(|s: &mut State, d, _| match d.phase {
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
    if cards.is_empty() && s.path.is_none() {
        board = board.child(homepage(s, p));
    }
    for card in cards.iter().filter(|c| {
        visible(c.rect, s.canvas)
            || s.drag == Some(c.id)
            || s.reposition.is_some_and(|drag| drag.node == c.id)
    }) {
        board = board.child(card_view(
            s,
            card,
            card.parent.map(|id| by_id[&id]),
            p,
            scale,
        ));
        if s.drop == Some(Destination::Before(card.id)) {
            let [x, y, _, h] = card.rect;
            board = board.child(
                canvas(Shape::new().line([2.0, 0.0], [2.0, h], 3.0, LineCap::Round, p.accent))
                    .absolute(x - 10.0, y)
                    .size(4.0, h),
            );
        }
    }
    for (&id, note) in &s.project.tree.notes {
        let position = if let Some(node) = note.node {
            let Some(card) = by_id.get(&node) else {
                continue;
            };
            let rect = card.rect;
            if !visible(rect, s.canvas) {
                continue;
            }
            [rect[0], rect[1]]
        } else {
            [
                note.position[0] * s.view.zoom + s.view.pan[0],
                note.position[1] * s.view.zoom + s.view.pan[1],
            ]
        };
        board = board.child(note_view(s, id, note, position, p));
    }
    if s.drag.is_some() {
        let [x, y, w, h] = root_zone(s);
        let active = s.drop == Some(Destination::Root);
        board = board.child(
            el().key("move-to-root")
                .absolute(x, y)
                .size(w, h)
                .center()
                .radius(6.0)
                .fill(p.accent_bg)
                .border(if active { 1.0 } else { 0.5 }, p.accent)
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
        Prompt::HighlightColor => ("Highlight color", String::new(), ""),
        Prompt::Group(_) => ("Group color", String::new(), ""),
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
        Prompt::TextNode(Some(_)) => ("Edit text node", String::new(), "Update text"),
        Prompt::TextNode(None) => ("Add text node", String::new(), "Add node"),
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
        .border(0.5, p.border)
        .modal(false, |s: &mut State| s.prompt = None)
        .child(label(title, 22.0, 700.0, p.text));
    if !detail.is_empty() {
        panel = panel.child(label(detail, 13.0, 450.0, p.muted).wrap());
    }
    let mut actions = row()
        .gap(8.0)
        .child(button("Cancel", p, false, true, |s, _| s.prompt = None).help_keys(["Esc"]));
    if matches!(modal, Prompt::HighlightColor) {
        for colors in [
            [
                HighlightColor::Yellow,
                HighlightColor::Green,
                HighlightColor::Blue,
            ],
            [
                HighlightColor::Pink,
                HighlightColor::Orange,
                HighlightColor::Gray,
            ],
        ] {
            let mut choices = row().gap(8.0);
            for ink in colors {
                let (name, hex) = ink.style();
                choices = choices.child(
                    button(name, p, s.highlight_color == ink, true, move |s, _| {
                        s.highlight_color = ink;
                        s.prompt = None;
                    })
                    .border(1.0, color(hex)),
                );
            }
            panel = panel.child(choices);
        }
    } else if let Prompt::Group(id) = modal {
        let id = *id;
        for colors in GROUP_COLORS.chunks(3) {
            let mut choices = row().gap(8.0);
            for &(name, hex) in colors {
                let index = GROUP_COLORS
                    .iter()
                    .position(|entry| entry.0 == name)
                    .unwrap() as u8;
                let active = s.project.tree.pages[&id].group == Some(index);
                choices = choices.child(
                    button(name, p, active, true, move |s, _| {
                        s.set_group(id, Some(index))
                    })
                    .border(1.0, color(hex)),
                );
            }
            panel = panel.child(choices);
        }
        actions = actions.child(button(
            "Remove group",
            p,
            false,
            s.project.tree.pages[&id].group.is_some(),
            move |s, _| s.set_group(id, None),
        ));
    } else if let Prompt::Unsaved(next) = modal {
        let discard = next.clone();
        let save = next.clone();
        actions = actions
            .child(button("Discard changes", p, false, true, move |s, tx| {
                s.prompt = None;
                s.proceed(discard.clone(), tx);
            }))
            .child(button(confirm, p, true, true, move |s, tx| {
                s.prompt = None;
                s.save(tx, Some(save.clone()));
            }));
    } else {
        let note = matches!(modal, Prompt::Note { .. } | Prompt::TextNode(_));
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
                .border(0.5, p.border)
                .fill(p.canvas)
                .child(input),
        );
        if !s.status.is_empty() {
            panel = panel.child(label(&s.status, 12.0, 500.0, p.accent).wrap());
        }
        let submit = button(confirm, p, true, true, |s, _| s.apply_prompt());
        actions = actions.child(if note {
            submit
        } else {
            submit.help_keys(["Enter"])
        });
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
    let selected_pdf = s
        .selected
        .iter()
        .any(|id| s.project.tree.pages[id].text.is_none());
    let dirty = s.saved != s.project;
    let title = s
        .path
        .as_ref()
        .and_then(|path| path.file_stem())
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "Untitled".into());
    let mut primary = row()
        .h(48.0)
        .w_full()
        .px(14.0)
        .gap(7.0)
        .center_y()
        .fill(p.panel)
        .child(label("PDF Tree", 15.0, 700.0, p.text))
        .child(
            icon_button(
                "Open PDF",
                ToolbarIcon::OpenPdf,
                &p,
                false,
                ready,
                |s, tx| s.request(Next::OpenPdf, tx),
            )
            .help_keys([COMMAND, "Shift", "O"]),
        )
        .child(
            icon_button(
                "Open project",
                ToolbarIcon::Folder,
                &p,
                false,
                ready,
                |s, tx| s.request(Next::OpenProject, tx),
            )
            .help_keys([COMMAND, "O"]),
        )
        .child(
            icon_button("Save", ToolbarIcon::Save, &p, false, ready, |s, tx| {
                s.save(tx, None)
            })
            .help_keys([COMMAND, "S"]),
        )
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
        .child(
            icon_button(
                "Undo",
                ToolbarIcon::Undo,
                &p,
                false,
                ready && !s.history.is_empty(),
                |s, _| s.history(false),
            )
            .help_keys([COMMAND, "Z"]),
        )
        .child(
            icon_button(
                "Redo",
                ToolbarIcon::Redo,
                &p,
                false,
                ready && !s.redo.is_empty(),
                |s, _| s.history(true),
            )
            .help_keys([COMMAND, "Shift", "Z"]),
        );
    primary = primary.child(
        row()
            .key("graph-mode")
            .help("Toggle Graph mode")
            .help_keys([COMMAND, "G"])
            .label(if s.graph_mode {
                "Graph mode on"
            } else {
                "Graph mode off"
            })
            .gap(6.0)
            .h(30.0)
            .px(5.0)
            .center_y()
            .disabled(!ready)
            .focusable(ready)
            .on_press(|s: &mut State| s.toggle_graph())
            .child(label("Graph", 12.0, 550.0, p.text))
            .child(
                el().size(32.0, 18.0)
                    .radius(9.0)
                    .fill(if s.graph_mode { p.accent } else { p.border })
                    .child(
                        el().absolute(if s.graph_mode { 16.0 } else { 2.0 }, 2.0)
                            .size(14.0, 14.0)
                            .radius(7.0)
                            .fill(if s.graph_mode { p.panel } else { p.text }),
                    ),
            ),
    );
    if s.tool == Tool::Highlight {
        let (name, hex) = s.highlight_color.style();
        primary = primary.child(
            button(&format!("Color · {name}"), &p, false, ready, |s, _| {
                s.highlight_color_picker()
            })
            .help("Highlight color")
            .help_keys(["C"])
            .border(1.0, color(hex)),
        );
    }
    let mut actions = row()
        .h(40.0)
        .w_full()
        .px(14.0)
        .gap(4.0)
        .center_y()
        .fill(p.panel)
        .border_sides([1.0, 0.0, 1.0, 0.0], p.border);
    for (name, icon, tool, shortcut) in [
        ("Select", ToolbarIcon::Cursor, Tool::Select, "S"),
        ("Hand", ToolbarIcon::Hand, Tool::Hand, "P"),
        ("Note", ToolbarIcon::Pencil, Tool::Note, "T"),
    ] {
        actions = actions.child(
            icon_button(name, icon, &p, s.tool == tool, ready, move |s, _| {
                s.set_tool(tool);
            })
            .help_keys([shortcut]),
        );
    }
    actions = actions
        .child(
            icon_button("Text node", ToolbarIcon::Plus, &p, false, ready, |s, _| {
                s.text_node_editor(None)
            })
            .help_keys(["N"]),
        )
        .child(
            icon_button(
                "Highlight",
                ToolbarIcon::Marker,
                &p,
                s.tool == Tool::Highlight,
                ready && !s.graph_mode,
                |s, _| s.set_tool(Tool::Highlight),
            )
            .help_keys(["H"]),
        )
        .child(
            icon_button(
                "Rotate",
                ToolbarIcon::Rotate,
                &p,
                false,
                ready && selected_pdf,
                |s, _| s.rotate(),
            )
            .help_keys(["R"]),
        )
        .child(
            icon_button(
                "Group",
                ToolbarIcon::Group,
                &p,
                false,
                ready && s.group_root().is_some(),
                |s, _| s.group_editor(),
            )
            .help_keys([COMMAND, "U"]),
        )
        .child(
            icon_button(
                "Add Blank Page",
                ToolbarIcon::BlankPage,
                &p,
                false,
                ready,
                |s, _| s.add_blank_page(),
            )
            .help_keys([COMMAND, "B"]),
        )
        .child(
            icon_button(
                "Add PDF Page",
                ToolbarIcon::AddPdf,
                &p,
                false,
                ready,
                |s, tx| s.pick_pdf(Import::Add, tx),
            )
            .help_keys([COMMAND, "I"]),
        )
        .child(
            icon_button(
                "Replace",
                ToolbarIcon::Replace,
                &p,
                false,
                ready && selected_pdf && s.selected.len() == 1,
                |s, tx| s.replace_page(tx),
            )
            .help_keys([COMMAND, "R"]),
        )
        .child(
            icon_button(
                "Duplicate",
                ToolbarIcon::Duplicate,
                &p,
                false,
                ready && selected,
                |s, _| s.duplicate(),
            )
            .help_keys([COMMAND, "D"]),
        )
        .child(
            icon_button(
                "Delete",
                ToolbarIcon::Delete,
                &p,
                false,
                ready && selected,
                |s, _| s.remove(),
            )
            .help_keys(["Delete"]),
        )
        .child(
            icon_button(
                "Reposition",
                ToolbarIcon::Reposition,
                &p,
                s.tool == Tool::Reposition,
                ready,
                |s, _| s.toggle_reposition(),
            )
            .help_keys(["V"]),
        );
    let help = match s.tool {
        Tool::Select => "Drag headers to arrange pages · Right-click to select a subtree",
        Tool::Hand => "Drag the canvas to pan · Ctrl-scroll to zoom",
        Tool::Reposition => {
            "Drag a soft branch header to move it · Double-click its header to reset"
        }
        Tool::Highlight => "Drag across PDF text to highlight it",
        Tool::Note => "Click a node to add or edit its sticky note",
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
        .child(
            button("−", &p, false, ready, |s, _| s.zoom_center(0.8))
                .help("Zoom out")
                .help_keys([COMMAND, "−"]),
        )
        .child(
            button(
                &format!("{:.0}%", s.view.zoom * 100.0),
                &p,
                false,
                ready,
                |s, _| s.view = Viewport::default(),
            )
            .help("Reset view")
            .help_keys([COMMAND, "0"]),
        )
        .child(
            button("+", &p, false, ready, |s, _| s.zoom_center(1.25))
                .help("Zoom in")
                .help_keys([COMMAND, "+"]),
        );
    let mut window = col()
        .key("window")
        .w_full()
        .h_full()
        .fill(p.panel)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siblings_share_rows_and_wide_branches_reserve_space() {
        let app = App::new(AppSpec::new("PDF Tree", (1100.0, 760.0), empty_state, view));
        let mut state = State::new(app.handle());
        let tree = &mut state.project.tree;
        let page = || Page::new(None, [200.0, 100.0]);
        let root = tree.add(page());
        let left = tree.append_child(root, page());
        let right = tree.append_child(root, Page::new(None, [100.0, 200.0]));
        let a = tree.append_child(left, page());
        let b = tree.append_child(left, page());
        let c = tree.append_child(right, page());
        let leaf = tree.append_child(root, page());
        tree.move_pages(&BTreeSet::from([leaf]), Destination::Before(left));
        let layout: BTreeMap<_, _> = cards(&state)
            .into_iter()
            .map(|card| (card.id, card.rect))
            .collect();
        assert_eq!(layout[&right][1], layout[&leaf][1]);
        assert!(layout[&right][1] - (layout[&root][1] + layout[&root][3]) > ROW_GAP);
        assert_eq!(layout[&left][0] - layout[&leaf][0], CARD_WIDTH + BRANCH_GAP);
        assert_eq!(layout[&left][1], layout[&right][1]);
        assert_eq!(layout[&a][1], layout[&b][1]);
        assert_eq!(layout[&b][1], layout[&c][1]);
        assert!(layout[&c][1] > layout[&right][1] + layout[&right][3]);
        assert!(layout[&b][0] + CARD_WIDTH + BRANCH_GAP <= layout[&c][0]);
        assert_eq!(layout[&right][0], layout[&c][0]);
        assert_eq!(layout[&left][0], (layout[&a][0] + layout[&b][0]) / 2.0);
        state.project.tree.pages.get_mut(&left).unwrap().group = Some(0);
        let grouped = cards(&state)
            .into_iter()
            .map(|card| (card.id, card.rect))
            .collect::<BTreeMap<_, _>>();
        assert!(grouped[&leaf][0] + CARD_WIDTH + BRANCH_GAP <= grouped[&a][0]);
        assert!(grouped[&b][0] + CARD_WIDTH + BRANCH_GAP <= grouped[&right][0]);
    }
}
