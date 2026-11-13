mod model;
mod pdf;
mod store;

use model::{Drop as Destination, Id, Note, Page, Project, Rect, rotate_rect};
use rune::{App, AppHandle, AppSpec, Cx, FileDialogOptions, Key, TextEdit, Tx};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::{Arc, mpsc},
};
use store::Viewport;

mod ui;

const EXTENSION: &str = "pdftree";
const HISTORY_LIMIT: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Hand,
    Highlight,
    Note,
}

#[derive(Clone, Copy)]
enum Import {
    Open,
    Add,
    Replace(Id),
}

#[derive(Clone, Copy)]
enum Next {
    OpenPdf,
    OpenProject,
    Quit,
}

enum Prompt {
    Unsaved(Next),
    Import {
        action: Import,
        bytes: Arc<[u8]>,
        sizes: Vec<[f32; 2]>,
    },
    Note {
        id: Option<Id>,
        position: [f32; 2],
    },
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PreviewKey {
    source: Id,
    index: u32,
    rotation: u8,
    edge: u32,
}

struct Preview {
    pixels: rune::Pixels,
    text: Vec<Rect>,
}

struct Cache {
    pending: BTreeSet<PreviewKey>,
    entries: BTreeMap<PreviewKey, Result<Preview, String>>,
    order: VecDeque<PreviewKey>,
}

enum Job {
    Import {
        path: PathBuf,
        action: Import,
        generation: u64,
    },
    Render {
        key: PreviewKey,
        bytes: Arc<[u8]>,
        generation: u64,
    },
}

struct State {
    project: Project,
    saved: Project,
    history: VecDeque<Project>,
    redo: Vec<Project>,
    path: Option<PathBuf>,
    view: Viewport,
    canvas: Rect,
    canvas_drag_delta: [f32; 2],
    tool: Tool,
    selected: BTreeSet<Id>,
    anchor: Option<Id>,
    drag: Option<Id>,
    drop: Option<Destination>,
    selection: Option<(Id, [f32; 2], Vec<Rect>)>,
    note_drag: Option<(Id, [f32; 2])>,
    prompt: Option<Prompt>,
    draft: TextEdit,
    status: String,
    generation: u64,
    importing: bool,
    choosing_file: bool,
    cache: RefCell<Cache>,
    jobs: mpsc::Sender<Job>,
    app: AppHandle<Self>,
}

impl State {
    fn new(app: AppHandle<Self>) -> Self {
        let (jobs, receiver) = mpsc::channel();
        let handle = app.clone();
        std::thread::spawn(move || worker(receiver, handle));
        Self {
            project: Project::default(),
            saved: Project::default(),
            history: VecDeque::new(),
            redo: vec![],
            path: None,
            view: Viewport::default(),
            canvas: [0.0, 90.0, 1100.0, 650.0],
            canvas_drag_delta: [0.0; 2],
            tool: Tool::Select,
            selected: BTreeSet::new(),
            anchor: None,
            drag: None,
            drop: None,
            selection: None,
            note_drag: None,
            prompt: None,
            draft: TextEdit::default(),
            status: String::new(),
            generation: 0,
            importing: false,
            choosing_file: false,
            cache: RefCell::new(Cache {
                pending: BTreeSet::new(),
                entries: BTreeMap::new(),
                order: VecDeque::new(),
            }),
            jobs,
            app,
        }
    }

    fn edit(&mut self, apply: impl FnOnce(&mut Project)) {
        let before = self.project.clone();
        apply(&mut self.project);
        if self.project != before {
            if self.history.len() == HISTORY_LIMIT {
                self.history.pop_front();
            }
            self.history.push_back(before);
            self.redo.clear();
        }
        self.selected
            .retain(|id| self.project.tree.pages.contains_key(id));
    }

    fn history(&mut self, redo: bool) {
        if redo {
            if let Some(project) = self.redo.pop() {
                self.history
                    .push_back(std::mem::replace(&mut self.project, project));
            }
        } else if let Some(project) = self.history.pop_back() {
            self.redo
                .push(std::mem::replace(&mut self.project, project));
        }
        self.selected
            .retain(|id| self.project.tree.pages.contains_key(id));
        self.invalidate_previews();
    }

    fn invalidate_previews(&mut self) {
        self.generation += 1;
        self.importing = false;
        let cache = self.cache.get_mut();
        cache.entries.clear();
        cache.pending.clear();
        cache.order.clear();
    }

    fn reset(&mut self, project: Project, path: Option<PathBuf>, view: Viewport) {
        self.saved = project.clone();
        self.project = project;
        self.path = path;
        self.view = view;
        self.tool = Tool::Select;
        self.selected.clear();
        self.anchor = None;
        self.history.clear();
        self.redo.clear();
        self.invalidate_previews();
    }

    fn request(&mut self, next: Next, tx: &mut Tx) {
        if self.project != self.saved {
            self.prompt = Some(Prompt::Unsaved(next));
        } else {
            self.proceed(next, tx);
        }
    }

    fn proceed(&mut self, next: Next, tx: &mut Tx) {
        match next {
            Next::Quit => tx.quit(),
            Next::OpenPdf => self.pick_pdf(Import::Open, tx),
            Next::OpenProject => {
                self.choosing_file = true;
                tx.open_file(
                    dialog("Open project", EXTENSION),
                    |s: &mut Self, answer, _| {
                        s.choosing_file = false;
                        match answer {
                            Ok(Some(path)) => match store::load(&path) {
                                Ok((project, view)) => {
                                    s.reset(project, Some(path), view);
                                    s.status = "Project opened.".into();
                                }
                                Err(error) => s.status = error.to_string(),
                            },
                            Ok(None) => {}
                            Err(error) => s.status = error.to_string(),
                        }
                    },
                );
            }
        }
    }

    fn save(&mut self, tx: &mut Tx, next: Option<Next>) {
        if let Some(path) = self.path.clone() {
            self.save_to(path, tx, next);
        } else {
            let mut options = dialog("Save project", EXTENSION);
            options.file_name = Some(format!("Untitled.{EXTENSION}"));
            self.choosing_file = true;
            tx.save_file(options, move |s: &mut Self, answer, tx| {
                s.choosing_file = false;
                match answer {
                    Ok(Some(mut path)) => {
                        if path.extension().is_none() {
                            path.set_extension(EXTENSION);
                        }
                        s.save_to(path, tx, next);
                    }
                    Ok(None) => {}
                    Err(error) => s.status = error.to_string(),
                }
            });
        }
    }

    fn save_to(&mut self, path: PathBuf, tx: &mut Tx, next: Option<Next>) {
        match store::save(&path, &self.project, self.view) {
            Ok(()) => {
                self.path = Some(path);
                self.saved = self.project.clone();
                self.status = "Project saved.".into();
                if let Some(next) = next {
                    self.proceed(next, tx);
                }
            }
            Err(error) => self.status = format!("Save failed: {error}"),
        }
    }

    fn pick_pdf(&mut self, action: Import, tx: &mut Tx) {
        self.choosing_file = true;
        tx.open_file(
            dialog("Choose a PDF", "pdf"),
            move |s: &mut Self, answer, _| {
                s.choosing_file = false;
                match answer {
                    Ok(Some(path)) => s.read_pdf(path, action),
                    Ok(None) => {}
                    Err(error) => s.status = error.to_string(),
                }
            },
        );
    }

    fn read_pdf(&mut self, path: PathBuf, action: Import) {
        self.status = "Reading PDF…".into();
        self.importing = self
            .jobs
            .send(Job::Import {
                path,
                action,
                generation: self.generation,
            })
            .is_ok();
        if !self.importing {
            self.status = "PDF worker is unavailable.".into();
        }
    }

    fn import_pages(
        &mut self,
        action: Import,
        bytes: Arc<[u8]>,
        sizes: &[[f32; 2]],
        pages: Vec<usize>,
    ) {
        if matches!(action, Import::Open) {
            let mut project = Project::default();
            let source = project.source(bytes);
            for &index in &pages {
                project
                    .tree
                    .add(Page::new(Some((source, index as u32)), sizes[index]));
            }
            self.reset(project, None, Viewport::default());
            self.saved = Project::default();
        } else {
            self.edit(|project| {
                let source = project.source(bytes);
                if let Import::Replace(id) = action {
                    if let Some(page) = project.tree.pages.get_mut(&id) {
                        let index = pages[0];
                        page.source = Some((source, index as u32));
                        page.size = sizes[index];
                        page.rotation = 0;
                        page.highlights.clear();
                    }
                } else {
                    for &index in &pages {
                        project
                            .tree
                            .add(Page::new(Some((source, index as u32)), sizes[index]));
                    }
                }
            });
        }
        self.status = if matches!(action, Import::Replace(_)) {
            "Page replaced.".into()
        } else {
            format!("Imported {} page(s).", pages.len())
        };
    }

    fn note_editor(&mut self, id: Option<Id>, position: [f32; 2]) {
        self.status.clear();
        self.draft = TextEdit::new(id.map_or("", |id| &self.project.tree.notes[&id].text));
        self.prompt = Some(Prompt::Note { id, position });
    }

    fn apply_prompt(&mut self) {
        match self.prompt.take() {
            Some(Prompt::Import {
                action,
                bytes,
                sizes,
            }) => {
                match page_range(
                    &self.draft.text,
                    sizes.len(),
                    matches!(action, Import::Replace(_)),
                ) {
                    Ok(pages) => self.import_pages(action, bytes, &sizes, pages),
                    Err(error) => {
                        self.status = error.into();
                        self.prompt = Some(Prompt::Import {
                            action,
                            bytes,
                            sizes,
                        });
                    }
                }
            }
            Some(Prompt::Note { id, position }) => {
                let text = self.draft.text.trim().to_owned();
                if !text.is_empty() {
                    self.edit(|project| {
                        let id = id.unwrap_or_else(|| project.tree.id());
                        project.tree.notes.insert(id, Note { position, text });
                    });
                    self.tool = Tool::Select;
                }
            }
            prompt => self.prompt = prompt,
        }
    }

    fn rotate(&mut self) {
        let selected = self.selected.clone();
        self.edit(|p| {
            for id in selected {
                let page = p.tree.pages.get_mut(&id).unwrap();
                page.rotation = (page.rotation + 1) % 4;
            }
        });
    }

    fn duplicate(&mut self) {
        let selected = self.selected.clone();
        self.edit(|p| p.tree.duplicate(&selected));
    }

    fn remove(&mut self) {
        let selected = self.selected.clone();
        self.edit(|p| p.tree.remove(&selected));
    }

    fn zoom(&mut self, factor: f32, point: [f32; 2]) {
        let next = (self.view.zoom * factor).clamp(0.25, 4.0);
        let ratio = next / self.view.zoom;
        for (pan, point) in self.view.pan.iter_mut().zip(point) {
            *pan = point + (*pan - point) * ratio;
        }
        self.view.zoom = next;
    }

    fn preview(&self, key: PreviewKey) -> Option<Result<(rune::Pixels, Vec<Rect>), String>> {
        let mut cache = self.cache.borrow_mut();
        if cache.entries.contains_key(&key) {
            cache.order.retain(|entry| *entry != key);
            cache.order.push_back(key);
            return cache.entries.get(&key).map(|entry| {
                entry
                    .as_ref()
                    .map(|p| (p.pixels.clone(), p.text.clone()))
                    .map_err(Clone::clone)
            });
        }
        if cache.pending.len() < 6
            && !cache.pending.contains(&key)
            && let Some(bytes) = self.project.sources.get(&key.source)
            && self
                .jobs
                .send(Job::Render {
                    key,
                    bytes: bytes.clone(),
                    generation: self.generation,
                })
                .is_ok()
        {
            cache.pending.insert(key);
        }
        cache.entries.iter().rev().find_map(|(other, preview)| {
            (other.source == key.source
                && other.index == key.index
                && other.rotation == key.rotation)
                .then(|| {
                    preview
                        .as_ref()
                        .ok()
                        .map(|p| Ok((p.pixels.clone(), p.text.clone())))
                })
                .flatten()
        })
    }
}

fn dialog(title: &str, extension: &str) -> FileDialogOptions {
    FileDialogOptions {
        title: title.into(),
        ..Default::default()
    }
    .filter(
        if extension == "pdf" {
            "PDF"
        } else {
            "PDF Tree project"
        },
        &[extension],
    )
}

fn page_range(input: &str, count: usize, single: bool) -> Result<Vec<usize>, &'static str> {
    let mut pages = BTreeSet::new();
    let parse = |s: &str| {
        s.trim()
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=count).contains(n))
    };
    for piece in input.split(',') {
        let range = if let Some((first, last)) = piece.split_once('-') {
            parse(first).zip(parse(last)).filter(|(a, b)| a <= b)
        } else {
            parse(piece).map(|n| (n, n))
        };
        let Some((first, last)) = range else {
            return Err("Enter valid page numbers or ranges, for example 1, 3-5.");
        };
        pages.extend(first - 1..last);
    }
    if single && pages.len() != 1 {
        return Err("Choose one replacement page.");
    }
    Ok(pages.into_iter().collect())
}

fn worker(receiver: mpsc::Receiver<Job>, handle: AppHandle<State>) {
    let mut document: Option<pdf::Document> = None;
    while let Ok(job) = receiver.recv() {
        match job {
            Job::Import {
                path,
                action,
                generation,
            } => {
                let result = std::fs::read(path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        let bytes = Arc::<[u8]>::from(bytes);
                        let pdf = pdf::Document::open(bytes.clone())?;
                        Ok((bytes, pdf.sizes()?))
                    });
                handle.update(move |s, _| {
                    if s.generation != generation {
                        return;
                    }
                    s.importing = false;
                    match result {
                        Ok((bytes, sizes)) if matches!(action, Import::Open) => {
                            let pages = (0..sizes.len()).collect();
                            s.import_pages(action, bytes, &sizes, pages);
                        }
                        Ok((bytes, sizes)) => {
                            s.status.clear();
                            s.draft = TextEdit::new(
                                if matches!(action, Import::Replace(_)) || sizes.len() == 1 {
                                    "1".to_owned()
                                } else {
                                    format!("1-{}", sizes.len())
                                },
                            );
                            s.prompt = Some(Prompt::Import {
                                action,
                                bytes,
                                sizes,
                            });
                        }
                        Err(error) => s.status = error,
                    }
                });
            }
            Job::Render {
                key,
                bytes,
                generation,
            } => {
                let result = (|| {
                    if !document.as_ref().is_some_and(|doc| doc.matches(&bytes)) {
                        document = Some(pdf::Document::open(bytes)?);
                    }
                    document
                        .as_ref()
                        .unwrap()
                        .render(key.index, key.rotation, key.edge)
                })();
                handle.update(move |s, _| {
                    if s.generation != generation {
                        return;
                    }
                    let cache = s.cache.get_mut();
                    cache.pending.remove(&key);
                    let entry = result.map(|preview| {
                        let pixels = rune::Pixels::new();
                        pixels.set(preview.width, preview.height, preview.rgba);
                        Preview {
                            pixels,
                            text: preview.text,
                        }
                    });
                    cache.entries.insert(key, entry);
                    cache.order.push_back(key);
                    while cache.order.len() > 24 {
                        if let Some(old) = cache.order.pop_front() {
                            cache.entries.remove(&old);
                        }
                    }
                });
            }
        }
    }
}

fn key(s: &mut State, event: &rune::KeyEvent, tx: &mut Tx) -> bool {
    if s.prompt.is_some() {
        if event.key == Key::Escape {
            s.prompt = None;
            return true;
        }
        return false;
    }
    if s.importing || s.choosing_file {
        return false;
    }
    match event.key {
        Key::Command('s') => s.save(tx, None),
        Key::Command('o') => s.request(Next::OpenProject, tx),
        Key::Command('q' | 'w') => s.request(Next::Quit, tx),
        Key::Command('z') => s.history(event.mods.shift),
        Key::Command('y') => s.history(true),
        Key::Command('d') => s.duplicate(),
        Key::Delete => s.remove(),
        Key::Escape => {
            s.drag = None;
            s.drop = None;
            s.selection = None;
            s.note_drag = None;
        }
        _ => return false,
    }
    true
}

fn empty_state() -> State {
    unreachable!("the app initializes its worker with the window handle")
}

fn main() {
    let app = App::new(AppSpec::new(
        "PDF Tree",
        (1100.0, 760.0),
        empty_state,
        ui::view,
    ))
    .window_options(rune::WindowOptions {
        min_size: Some((860.0, 440.0)),
        ..Default::default()
    })
    .keep_running()
    .modifier_keys()
    .on_close(|s, tx| {
        if s.project == s.saved {
            tx.quit();
        } else {
            s.prompt = Some(Prompt::Unsaved(Next::Quit));
            s.app.open();
        }
    });
    let handle = app.handle();
    app.init(move || State::new(handle)).run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rotation: u16, content: &str) -> Arc<[u8]> {
        let mut data = String::from("%PDF-1.4\n");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ];
        let mut offsets = vec![];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(data.len());
            data.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
        }
        let xref = data.len();
        data.push_str("xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            data.push_str(&format!("{offset:010} 00000 n \n"));
        }
        data.push_str(&format!(
            "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        ));
        Arc::from(data.into_bytes())
    }

    #[test]
    fn poppler_renders_and_maps_text_through_both_rotations() {
        let text = "BT /F1 12 Tf 20 70 Td (Hello PDF Tree) Tj ET";
        for rotation in [0, 90, 180, 270] {
            let doc = pdf::Document::open(fixture(rotation, text)).unwrap();
            let size = doc.sizes().unwrap()[0];
            assert_eq!(
                size,
                if rotation % 180 == 0 {
                    [200.0, 100.0]
                } else {
                    [100.0, 200.0]
                }
            );
            for turn in 0..4 {
                let preview = doc.render(0, turn, 400).unwrap();
                let rects: Vec<_> = preview.text.iter().map(|&r| rotate_rect(r, turn)).collect();
                assert!(rects.iter().filter(|r| r[2] > 0.0).count() > 8);
                let mut ink = 0;
                let mut aligned = 0;
                for (i, pixel) in preview.rgba.chunks_exact(4).enumerate() {
                    if pixel[0] > 100 {
                        continue;
                    }
                    ink += 1;
                    let x = (i as u32 % preview.width) as f32 / preview.width as f32;
                    let y = (i as u32 / preview.width) as f32 / preview.height as f32;
                    if rects.iter().any(|&[l, t, w, h]| {
                        x >= l - 0.01 && x <= l + w + 0.01 && y >= t - 0.01 && y <= t + h + 0.01
                    }) {
                        aligned += 1;
                    }
                }
                assert!(ink > 30);
                assert!(
                    aligned * 100 >= ink * 95,
                    "source rotation {rotation}, added turn {turn}: {aligned}/{ink}"
                );
            }
        }
        let doc = pdf::Document::open(fixture(0, "0 0 20 20 re f")).unwrap();
        assert!(doc.render(0, 0, 200).unwrap().text.is_empty());
        assert!(pdf::Document::open(Arc::from(&b"malformed"[..])).is_err());
    }

    #[test]
    fn sqlite_round_trip_embeds_source_and_rejects_unrelated_files() {
        let dir = std::env::temp_dir().join(format!("pdf-tree-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("project.pdftree");
        let mut project = Project::default();
        let bytes = fixture(0, "");
        let source = project.source(bytes.clone());
        assert_eq!(source, project.source(bytes));
        let root = project
            .tree
            .add(Page::new(Some((source, 0)), [200.0, 100.0]));
        project.tree.pages.get_mut(&root).unwrap().highlights = vec![[0.1, 0.2, 0.3, 0.4]];
        project.tree.duplicate(&BTreeSet::from([root]));
        let note = project.tree.id();
        project.tree.notes.insert(
            note,
            Note {
                text: "Study".into(),
                position: [510.0, 45.0],
            },
        );
        let view = Viewport {
            pan: [5.0, -20.0],
            zoom: 1.25,
        };
        store::save(&path, &project, view).unwrap();
        let (reopened, restored) = store::load(&path).unwrap();
        assert!(reopened == project);
        assert_eq!(restored.pan, view.pan);
        assert_eq!(restored.zoom, view.zoom);
        assert!(pdf::Document::open(reopened.sources[&source].clone()).is_ok());
        let unrelated = dir.join("unrelated.db");
        rusqlite::Connection::open(&unrelated)
            .unwrap()
            .execute_batch("CREATE TABLE keep (value TEXT); PRAGMA user_version=1;")
            .unwrap();
        assert!(store::save(&unrelated, &project, view).is_err());
        assert!(store::save(&dir.join("missing/project.pdftree"), &project, view).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn runtime() -> rune::Runtime<State> {
        let app = App::new(AppSpec::new(
            "PDF Tree",
            (1100.0, 760.0),
            empty_state,
            ui::view,
        ));
        rune::Runtime::new(
            State::new(app.handle()),
            ui::view,
            (1100.0, 760.0),
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
        )
    }

    #[test]
    fn rune_canvas_drags_pages_and_requests_native_dialogs() {
        let mut rt = runtime();
        let [a, b] =
            std::array::from_fn(|_| rt.state.project.tree.add(Page::new(None, [200.0, 100.0])));
        rt.frame(0.0);
        let start = rt.find(&format!("page-header-{b}")).unwrap();
        let end = rt.find(&format!("page-header-{a}")).unwrap();
        rt.pointer_down(start.0, start.1);
        rt.frame(0.0);
        rt.pointer_move(end.0, end.1);
        rt.frame(0.0);
        assert_eq!(rt.state.drop, Some(Destination::Child(a)));
        rt.pointer_up(end.0, end.1);
        rt.frame(0.0);
        assert_eq!(rt.state.project.tree.pages[&a].children, [b]);
        assert_eq!(rt.state.project.tree.roots, [a]);
        rt.state.history(false);
        assert_eq!(rt.state.project.tree.roots, [a, b]);
        rt.state.history(true);
        assert_eq!(rt.state.project.tree.roots, [a]);
        rt.update(|s, tx| s.pick_pdf(Import::Add, tx));
        rt.frame(0.0);
        assert!(matches!(
            rt.file_dialog(),
            Some((rune::FileDialogKind::Open, _))
        ));
        rt.resolve_file_dialog(None).unwrap();
    }

    #[test]
    fn rune_highlight_gesture_saves_page_coordinates() {
        let mut rt = runtime();
        let bytes = fixture(0, "BT /F1 12 Tf 20 70 Td (Hello PDF Tree) Tj ET");
        let preview = pdf::Document::open(bytes.clone())
            .unwrap()
            .render(0, 0, 320)
            .unwrap();
        let source = rt.state.project.source(bytes);
        let id = rt
            .state
            .project
            .tree
            .add(Page::new(Some((source, 0)), [200.0, 100.0]));
        let key = PreviewKey {
            source,
            index: 0,
            rotation: 0,
            edge: 320,
        };
        let pixels = rune::Pixels::new();
        pixels.set(preview.width, preview.height, preview.rgba);
        let first = preview.text[0];
        let last = preview.text[5];
        rt.state.cache.get_mut().entries.insert(
            key,
            Ok(Preview {
                pixels,
                text: preview.text,
            }),
        );
        rt.state.tool = Tool::Highlight;
        rt.frame(0.0);
        let center = rt.find(&format!("page-image-{id}")).unwrap();
        let point = |[x, y, w, h]: Rect| {
            (
                center.0 + (x + w / 2.0 - 0.5) * 276.0,
                center.1 + (y + h / 2.0 - 0.5) * 138.0,
            )
        };
        let start = point(first);
        let end = point(last);
        rt.pointer_down(start.0, start.1);
        rt.frame(0.0);
        rt.pointer_move(end.0, end.1);
        rt.frame(0.0);
        rt.pointer_up(end.0, end.1);
        rt.frame(0.0);
        let highlights = &rt.state.project.tree.pages[&id].highlights;
        assert_eq!(highlights.first(), Some(&first));
        assert_eq!(highlights.last(), Some(&last));
    }

    #[test]
    fn validates_import_page_ranges() {
        assert_eq!(page_range("3, 1-2, 2", 4, false).unwrap(), [0, 1, 2]);
        assert!(page_range("0", 4, false).is_err());
        assert!(page_range("3-2", 4, false).is_err());
        assert!(page_range("1-2", 4, true).is_err());
    }
}
