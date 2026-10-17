pub mod db;
pub mod entities;
pub mod index;
pub mod journal;
pub mod model;
pub mod notes;
pub mod practices;
pub mod recall;
pub mod tasks;
pub mod words;

use anyhow::Context;
use std::path::Path;

use db::Db;
use entities::EntityStore;
use index::Semantic;
use journal::Journal;
use model::{MemoryError, ProjectName, Scope};
use notes::NoteStore;

pub struct Brain {
    pub db: Db,
    pub notes: NoteStore,
    pub entities: EntityStore,
    pub journal: Journal,
    pub semantic: Semantic,
    pub default_project: ProjectName,
    // Whether the agent is shown every tool or only the everyday ones.
    pub all_tools: bool,
}

impl Brain {
    pub fn open(
        home: &Path,
        key: &str,
        default_project: ProjectName,
        all_tools: bool,
    ) -> Result<Self, MemoryError> {
        create_private_dir(home)
            .with_context(|| format!("cannot create data directory {}", home.display()))?;
        let db = Db::open(home, key)?;
        Ok(Self {
            notes: NoteStore::new(db.clone()),
            entities: EntityStore::new(db.clone()),
            journal: Journal::new(db.clone()),
            semantic: Semantic::new(db.clone(), Some(home.join("models"))),
            db,
            default_project,
            all_tools,
        })
    }

    pub fn default_scope(&self) -> Scope {
        Scope::Project(self.default_project.clone())
    }
}

// Memory can hold anything the agent has seen, so only the owner may read it.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
