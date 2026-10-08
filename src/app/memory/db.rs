use anyhow::anyhow;
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::model::{MemoryError, invalid};

const MIGRATIONS: [&str; 3] = [
    include_str!("migrations/001_notes.sql"),
    include_str!("migrations/002_state.sql"),
    include_str!("migrations/003_history.sql"),
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

    fn prepare(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Overwrite deleted rows instead of just unlinking them.
        conn.pragma_update(None, "secure_delete", "ON")?;
        // A migration may rebuild a table, which SQLite allows only with
        // foreign keys off; migrate() checks the links before it commits.
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        migrate(&mut conn)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // Moves everything out of the write-ahead log and empties it. A deleted
    // row is overwritten in the database file, but its old pages would
    // otherwise stay readable in the log until the log is next recycled.
    pub async fn scrub_log(&self) -> Result<()> {
        self.run(|conn| {
            let busy: i64 =
                conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
            if busy != 0 {
                return Err(
                    anyhow!("another connection is reading; the log was not emptied").into(),
                );
            }
            Ok(())
        })
        .await
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
    let broken: i64 = tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
        row.get(0)
    })?;
    if broken > 0 {
        return Err(
            anyhow!("migration would leave {broken} broken links; nothing was changed").into(),
        );
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
