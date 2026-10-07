use anyhow::anyhow;
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::model::{MemoryError, invalid};

const MIGRATIONS: [&str; 2] = [
    include_str!("migrations/001_notes.sql"),
    include_str!("migrations/002_state.sql"),
];
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

type Result<T> = std::result::Result<T, MemoryError>;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        Self::prepare(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn prepare(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Overwrite deleted rows instead of just unlinking them.
        conn.pragma_update(None, "secure_delete", "ON")?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn run<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex is poisoned"))?;
            work(&mut conn)
        })
        .await?
    }
}

// Version is read under the write lock, so two processes cannot both migrate.
fn migrate(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let applied = usize::try_from(version)
        .ok()
        .filter(|applied| *applied <= MIGRATIONS.len())
        .ok_or_else(|| {
            anyhow!(
                "database has schema version {version}, this build supports up to {}",
                MIGRATIONS.len()
            )
        })?;
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(applied) {
        tx.execute_batch(migration)?;
        tx.pragma_update(None, "user_version", index as i64 + 1)?;
    }
    tx.commit()?;
    Ok(())
}

// Well below the depth where SQLite fails on cascading deletes.
pub const MAX_TREE_DEPTH: i64 = 64;

#[derive(Clone, Copy)]
pub enum Tree {
    Notes,
    Entities,
}

impl Tree {
    fn table(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Entities => "entities",
        }
    }

    pub fn contains(self, conn: &Connection, root: i64, candidate: i64) -> Result<bool> {
        let table = self.table();
        Ok(conn.query_row(
            &format!(
                "WITH RECURSIVE sub (id) AS (
                     SELECT ?1
                     UNION
                     SELECT c.id FROM {table} c JOIN sub ON c.parent_id = sub.id
                 )
                 SELECT EXISTS (SELECT 1 FROM sub WHERE id = ?2)"
            ),
            [root, candidate],
            |row| row.get(0),
        )?)
    }

    pub fn check_depth(self, conn: &Connection, parent: i64, moved: Option<i64>) -> Result<()> {
        let levels_below = match moved {
            Some(id) => self.height(conn, id)?,
            None => 0,
        };
        if self.ancestors(conn, parent)? + 1 + levels_below >= MAX_TREE_DEPTH {
            return Err(invalid(format!(
                "the tree would be deeper than {MAX_TREE_DEPTH} levels"
            )));
        }
        Ok(())
    }

    fn ancestors(self, conn: &Connection, id: i64) -> Result<i64> {
        let table = self.table();
        Ok(conn.query_row(
            &format!(
                "WITH RECURSIVE up (id) AS (
                     SELECT parent_id FROM {table} WHERE id = ?1
                     UNION ALL
                     SELECT t.parent_id FROM {table} t JOIN up ON t.id = up.id
                 )
                 SELECT count(*) FROM up WHERE id IS NOT NULL"
            ),
            [id],
            |row| row.get(0),
        )?)
    }

    fn height(self, conn: &Connection, id: i64) -> Result<i64> {
        let table = self.table();
        Ok(conn.query_row(
            &format!(
                "WITH RECURSIVE down (id, level) AS (
                     SELECT ?1, 0
                     UNION ALL
                     SELECT c.id, down.level + 1 FROM {table} c JOIN down ON c.parent_id = down.id
                 )
                 SELECT max(level) FROM down"
            ),
            [id],
            |row| row.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_database_from_the_first_release_is_upgraded_in_place() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO notes (project, kind, status, title, body, tags, created_at, updated_at)
             VALUES ('p', 'fact', 'active', 'old note', '', '', 't', 't')",
            [],
        )
        .unwrap();

        migrate(&mut conn).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        let (title, author): (String, Option<String>) = conn
            .query_row("SELECT title, author FROM notes", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!((title.as_str(), author), ("old note", None));

        migrate(&mut conn).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
    }

    #[test]
    fn a_database_from_a_newer_build_is_refused() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .unwrap();
        assert!(migrate(&mut conn).is_err());
    }
}
