use crate::model::{Id, Project, Tree};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MIN_ZOOM: f32 = 0.08;
pub const MAX_ZOOM: f32 = 4.0;

const APPLICATION_ID: i64 = 0x50545245;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    pub pan: [f32; 2],
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: [0.0, 0.0],
            zoom: 1.0,
        }
    }
}

pub fn save(path: &Path, project: &Project, view: Viewport) -> Result<()> {
    let mut db = Connection::open(path)?;
    let tx = db.transaction()?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let app_id: i64 = tx.pragma_query_value(None, "application_id", |row| row.get(0))?;
    match version {
        0 if app_id == 0 => {
            let tables: i64 =
                tx.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
            if tables != 0 {
                return Err("This file is not a PDF Tree project.".into());
            }
            tx.execute_batch("CREATE TABLE project (id INTEGER PRIMARY KEY CHECK(id=1), tree TEXT NOT NULL, viewport TEXT NOT NULL);
                CREATE TABLE sources (id INTEGER PRIMARY KEY, pdf BLOB NOT NULL);
                PRAGMA user_version=1;")?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        }
        1 if app_id == APPLICATION_ID => {}
        _ => return Err("Unsupported PDF Tree project version.".into()),
    }
    tx.execute(
        "INSERT OR REPLACE INTO project VALUES (1, ?, ?)",
        params![
            serde_json::to_string(&project.tree)?,
            serde_json::to_string(&view)?
        ],
    )?;
    tx.execute("DELETE FROM sources", [])?;
    for (id, pdf) in &project.sources {
        if project
            .tree
            .pages
            .values()
            .any(|page| page.source.is_some_and(|(source, _)| source == *id))
        {
            tx.execute(
                "INSERT INTO sources VALUES (?, ?)",
                params![id, pdf.as_ref()],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn load(path: &Path) -> Result<(Project, Viewport)> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let app_id: i64 = db.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if version != 1 || app_id != APPLICATION_ID {
        return Err("Unsupported PDF Tree project version.".into());
    }
    let (tree, view): (String, String) =
        db.query_row("SELECT tree, viewport FROM project WHERE id=1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    let mut stmt = db.prepare("SELECT id, pdf FROM sources")?;
    let sources = stmt
        .query_map([], |row| {
            Ok((row.get::<_, Id>(0)?, Arc::from(row.get::<_, Vec<u8>>(1)?)))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let project = Project {
        tree: serde_json::from_str::<Tree>(&tree)?,
        sources,
    };
    let view: Viewport = serde_json::from_str(&view)?;
    if !project.validate()
        || !(MIN_ZOOM..=MAX_ZOOM).contains(&view.zoom)
        || view.pan.iter().any(|v| !v.is_finite())
    {
        return Err("The project contains invalid data.".into());
    }
    Ok((project, view))
}

pub const RECENT_LIMIT: usize = 6;

fn recent_path() -> Option<PathBuf> {
    let config = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::home_dir().map(|home| home.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::home_dir().map(|home| home.join(".config")))
    }?;
    Some(config.join("pdf-tree").join("recent-projects.json"))
}

pub fn recent_projects() -> Vec<PathBuf> {
    recent_path()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Vec<PathBuf>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|path| path.is_file())
        .take(RECENT_LIMIT)
        .collect()
}

pub fn save_recent_projects(paths: &[PathBuf]) -> Result<()> {
    let Some(path) = recent_path() else {
        return Ok(());
    };
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!("recent-projects.{}.tmp", std::process::id()));
    let write = || -> Result<()> {
        fs::File::create(&temp)?.write_all(&serde_json::to_vec(paths)?)?;
        fs::rename(&temp, &path)?;
        Ok(())
    };
    let result = write();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
