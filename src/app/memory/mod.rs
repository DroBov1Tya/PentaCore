pub mod concepts;
pub mod db;
pub mod entities;
pub mod model;
pub mod notes;

use anyhow::Context;
use std::path::Path;

use concepts::ConceptStore;
use db::Db;
use entities::EntityStore;
use model::{MemoryError, ProjectName, Scope};
use notes::NoteStore;

pub struct Brain {
    pub notes: NoteStore,
    pub entities: EntityStore,
    pub concepts: ConceptStore,
    pub default_project: ProjectName,
}

impl Brain {
    pub fn open(home: &Path, default_project: ProjectName) -> Result<Self, MemoryError> {
        create_private_dir(home)
            .with_context(|| format!("cannot create data directory {}", home.display()))?;
        let db = Db::open(&home.join("brain.sqlite"))?;
        Ok(Self {
            notes: NoteStore::new(db.clone()),
            entities: EntityStore::new(db),
            concepts: ConceptStore::new(home.join("concepts.lancedb"), home.join("models")),
            default_project,
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
