use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub type Id = u64;
pub type Rect = [f32; 4];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HighlightColor {
    Gray,
    #[default]
    Yellow,
    Green,
    Blue,
    Pink,
    Orange,
}

impl HighlightColor {
    pub fn style(self) -> (&'static str, &'static str) {
        match self {
            Self::Gray => ("Gray", "#777777"),
            Self::Yellow => ("Yellow", "#ffcf26"),
            Self::Green => ("Green", "#48c96d"),
            Self::Blue => ("Blue", "#43a7ff"),
            Self::Pink => ("Pink", "#f773bc"),
            Self::Orange => ("Orange", "#ff913f"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Highlight {
    Legacy(Rect),
    Colored { rect: Rect, color: HighlightColor },
}

impl Highlight {
    pub fn parts(self) -> (Rect, HighlightColor) {
        match self {
            Self::Legacy(rect) => (rect, HighlightColor::Gray),
            Self::Colored { rect, color } => (rect, color),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BranchPosition {
    pub parent: Id,
    pub offset: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub source: Option<(Id, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_position: Option<BranchPosition>,
    pub size: [f32; 2],
    pub rotation: u8,
    pub highlights: Vec<Highlight>,
    pub children: Vec<Id>,
}

impl Page {
    pub fn new(source: Option<(Id, u32)>, size: [f32; 2]) -> Self {
        Self {
            source,
            text: None,
            group: None,
            branch_position: None,
            size,
            rotation: 0,
            highlights: vec![],
            children: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<Id>,
    pub position: [f32; 2],
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    next: Id,
    pub roots: Vec<Id>,
    pub pages: BTreeMap<Id, Page>,
    pub notes: BTreeMap<Id, Note>,
}

#[derive(Clone, Default, PartialEq)]
pub struct Project {
    pub tree: Tree,
    pub sources: BTreeMap<Id, Arc<[u8]>>,
}

impl Project {
    pub fn source(&mut self, bytes: Arc<[u8]>) -> Id {
        if let Some((&id, _)) = self
            .sources
            .iter()
            .find(|(_, saved)| saved.as_ref() == bytes.as_ref())
        {
            return id;
        }
        let id = self.tree.id();
        self.sources.insert(id, bytes);
        id
    }

    pub fn validate(&self) -> bool {
        let tree = &self.tree;
        let mut seen = BTreeSet::new();
        let mut stack = tree.roots.clone();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) || id == 0 || id > tree.next || tree.notes.contains_key(&id) {
                return false;
            }
            let Some(page) = tree.pages.get(&id) else {
                return false;
            };
            if page.rotation > 3
                || page.group.is_some_and(|color| color >= 12)
                || page
                    .branch_position
                    .is_some_and(|position| !position.offset.iter().all(|v| v.is_finite()))
                || !page.size.iter().all(|v| v.is_finite() && *v > 0.0)
                || page
                    .highlights
                    .iter()
                    .flat_map(|highlight| highlight.parts().0)
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
                || page
                    .source
                    .is_some_and(|(s, _)| !self.sources.contains_key(&s))
            {
                return false;
            }
            stack.extend(&page.children);
        }
        tree.next < i64::MAX as u64
            && seen.len() == tree.pages.len()
            && tree.notes.iter().all(|(&id, note)| {
                id > 0
                    && id <= tree.next
                    && note.node.is_none_or(|node| seen.contains(&node))
                    && note.position.iter().all(|v| v.is_finite())
            })
            && self.sources.keys().all(|&id| {
                id > 0 && id <= tree.next && !seen.contains(&id) && !tree.notes.contains_key(&id)
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drop {
    Root,
    Child(Id),
    Before(Id),
}

impl Tree {
    pub fn id(&mut self) -> Id {
        self.next += 1;
        self.next
    }

    pub fn order(&self) -> Vec<(Id, Option<Id>, usize)> {
        let mut stack: Vec<_> = self.roots.iter().rev().map(|&id| (id, None, 0)).collect();
        let mut rows = Vec::with_capacity(self.pages.len());
        while let Some((id, parent, depth)) = stack.pop() {
            rows.push((id, parent, depth));
            stack.extend(
                self.pages[&id]
                    .children
                    .iter()
                    .rev()
                    .map(|&child| (child, Some(id), depth + 1)),
            );
        }
        rows
    }

    fn siblings(&mut self, parent: Option<Id>) -> &mut Vec<Id> {
        match parent {
            Some(id) => &mut self.pages.get_mut(&id).unwrap().children,
            None => &mut self.roots,
        }
    }

    pub fn add(&mut self, page: Page) -> Id {
        let id = self.id();
        self.pages.insert(id, page);
        self.roots.push(id);
        id
    }

    pub fn append_child(&mut self, parent: Id, page: Page) -> Id {
        let id = self.id();
        self.pages.insert(id, page);
        self.siblings(Some(parent)).push(id);
        id
    }

    pub fn descendants(&self, id: Id) -> BTreeSet<Id> {
        let mut selected = BTreeSet::new();
        let mut pending = self.pages[&id].children.clone();
        while let Some(child) = pending.pop() {
            selected.insert(child);
            pending.extend(&self.pages[&child].children);
        }
        selected
    }

    pub fn duplicate(&mut self, selected: &BTreeSet<Id>) {
        for (id, parent, _) in self.order() {
            if !selected.contains(&id) {
                continue;
            }
            let mut copy = self.pages[&id].clone();
            copy.children.clear();
            let new = self.id();
            self.pages.insert(new, copy);
            let siblings = self.siblings(parent);
            let index = siblings.iter().position(|&sibling| sibling == id).unwrap();
            siblings.insert(index + 1, new);
        }
    }

    pub fn remove(&mut self, selected: &BTreeSet<Id>) {
        for (id, parent, _) in self.order().into_iter().rev() {
            if !selected.contains(&id) {
                continue;
            }
            let page = self.pages.remove(&id).unwrap();
            for child in &page.children {
                self.pages.get_mut(child).unwrap().branch_position = None;
            }
            self.notes.retain(|_, note| note.node != Some(id));
            let siblings = self.siblings(parent);
            let index = siblings.iter().position(|&sibling| sibling == id).unwrap();
            siblings.splice(index..=index, page.children);
        }
    }

    fn move_plan(&self, selected: &BTreeSet<Id>, drop: Drop) -> Option<MovePlan> {
        let order = self.order();
        let parent = match drop {
            Drop::Root => None,
            Drop::Child(id) => Some(id),
            Drop::Before(id) => order.iter().find(|row| row.0 == id)?.1,
        };
        let mut moving = BTreeSet::new();
        let mut roots = Vec::new();
        for (id, from, _) in order {
            if from.is_some_and(|id| moving.contains(&id)) {
                moving.insert(id);
            } else if selected.contains(&id) {
                moving.insert(id);
                roots.push((id, from));
            }
        }
        let target = match drop {
            Drop::Root => None,
            Drop::Child(id) | Drop::Before(id) => Some(id),
        };
        if roots.is_empty()
            || target.is_some_and(|id| !self.pages.contains_key(&id) || moving.contains(&id))
        {
            return None;
        }
        Some(MovePlan { roots, parent })
    }

    pub fn can_move(&self, selected: &BTreeSet<Id>, drop: Drop) -> bool {
        self.move_plan(selected, drop).is_some()
    }

    pub fn move_pages(&mut self, selected: &BTreeSet<Id>, drop: Drop) {
        let Some(MovePlan { roots, parent }) = self.move_plan(selected, drop) else {
            return;
        };
        for &(id, from) in &roots {
            let siblings = self.siblings(from);
            let index = siblings.iter().position(|&sibling| sibling == id).unwrap();
            siblings.remove(index);
            if from != parent {
                self.pages.get_mut(&id).unwrap().branch_position = None;
            }
        }
        let siblings = self.siblings(parent);
        let index = match drop {
            Drop::Before(id) => siblings.iter().position(|&sibling| sibling == id).unwrap(),
            _ => siblings.len(),
        };
        siblings.splice(index..index, roots.into_iter().map(|(id, _)| id));
    }
}

struct MovePlan {
    roots: Vec<(Id, Option<Id>)>,
    parent: Option<Id>,
}

pub fn rotate_rect([x, y, w, h]: Rect, rotation: u8) -> Rect {
    match rotation % 4 {
        1 => [1.0 - y - h, x, h, w],
        2 => [1.0 - x - w, 1.0 - y - h, w, h],
        3 => [y, 1.0 - x - w, h, w],
        _ => [x, y, w, h],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_selected_roots_in_order_and_rejects_cycles() {
        let mut tree = Tree::default();
        let [a, b, c, d] = std::array::from_fn(|_| tree.add(Page::new(None, [200.0, 100.0])));
        tree.move_pages(&BTreeSet::from([b]), Drop::Child(a));
        assert!(!tree.can_move(&BTreeSet::from([a]), Drop::Child(b)));
        tree.move_pages(&BTreeSet::from([c, b, a]), Drop::Child(d));
        assert_eq!(tree.pages[&d].children, [a, c]);
        assert_eq!(tree.pages[&a].children, [b]);
        tree.move_pages(&BTreeSet::from([c]), Drop::Before(a));
        assert_eq!(tree.pages[&d].children, [c, a]);
        tree.remove(&BTreeSet::from([a, d]));
        assert_eq!(tree.roots, [c, b]);
    }
}
