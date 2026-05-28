use anyhow::Result;
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::store::MemoryStore;

/// Fixed namespace UUID for deterministic seed IDs.
/// Using this namespace guarantees the same source file always gets the same memory ID,
/// so seeding is fully idempotent even after SQLite is cleared.
/// Bytes spell out "pentacore_seeds\0".
const SEED_NS: Uuid = Uuid::from_bytes([
    b'p', b'e', b'n', b't', b'a', b'c', b'o', b'r', b'e', b'_', b's', b'e', b'e', b'd', b's', 0u8,
]);

fn seed_id(source_key: &str) -> String {
    Uuid::new_v5(&SEED_NS, source_key.as_bytes()).to_string()
}

struct KnowledgeEntry {
    category: String,
    title: String,
    tags: Vec<String>,
    body: String,
}

pub async fn seed(store: &mut MemoryStore, db: &Pool<Sqlite>, binary_dir: &Path) -> Result<()> {
    let knowledge_dir = binary_dir.join("knowledge");
    if !knowledge_dir.exists() {
        tracing::debug!(
            "knowledge/ not found at {:?}, skipping base knowledge seed",
            knowledge_dir
        );
        return Ok(());
    }

    let files = collect_md_files(&knowledge_dir);
    if files.is_empty() {
        return Ok(());
    }

    // source_key -> (content_hash, memory_id)
    let existing: HashMap<String, (String, String)> =
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT source_key, content_hash, memory_id FROM knowledge_seeds",
        )
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|(k, h, m)| (k, (h, m)))
        .collect();

    let mut seeded = 0usize;
    for path in files {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read {:?}: {}", path, e);
                continue;
            }
        };

        let source_key = path
            .strip_prefix(&knowledge_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let hash = content_hash(&content);

        if let Some((existing_hash, _)) = existing.get(source_key.as_str()) {
            if existing_hash == &hash {
                continue; // unchanged
            }
        }

        let entry = match parse(&content) {
            Some(e) => e,
            None => {
                tracing::warn!("Skipping {:?}: missing or malformed frontmatter", path);
                continue;
            }
        };

        // Deterministic ID derived from source_key: stable across SQLite resets.
        // Pre-delete makes seeding idempotent: no duplicates even if SQLite was wiped
        // while LanceDB still had the old entry from a previous run.
        let memory_id = seed_id(&source_key);
        if let Err(e) = store.forget(&memory_id).await {
            tracing::debug!("Pre-delete no-op for {}: {}", source_key, e);
        }

        store
            .memorize_with_id(
                &memory_id,
                "global",
                &entry.category,
                &entry.title,
                &entry.body,
                &entry.tags,
            )
            .await?;

        sqlx::query(
            "INSERT INTO knowledge_seeds (source_key, content_hash, memory_id)
             VALUES (?, ?, ?)
             ON CONFLICT(source_key) DO UPDATE
             SET content_hash = excluded.content_hash,
                 memory_id    = excluded.memory_id,
                 seeded_at    = CURRENT_TIMESTAMP",
        )
        .bind(&source_key)
        .bind(&hash)
        .bind(&memory_id)
        .execute(db)
        .await?;

        seeded += 1;
        tracing::debug!("Seeded: {}", source_key);
    }

    if seeded > 0 {
        tracing::info!(
            "📚 Seeded {} base knowledge entries from knowledge/",
            seeded
        );
    }

    Ok(())
}

fn collect_md_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_recursive(dir, &mut files);
    files.sort();
    files
}

fn collect_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_recursive(&path, out);
        } else if path.extension().map_or(false, |e| e == "md") {
            out.push(path);
        }
    }
}

fn content_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn parse(content: &str) -> Option<KnowledgeEntry> {
    let content = content.trim();
    let body_start = if content.starts_with("---") {
        content[3..].find("---").map(|i| i + 6)?
    } else {
        0
    };

    let frontmatter = if body_start > 0 {
        &content[3..body_start - 3]
    } else {
        ""
    };

    let body = content[body_start..].trim();

    let category =
        frontmatter_value(frontmatter, "category").unwrap_or_else(|| "technique".to_string());

    let title = frontmatter_value(frontmatter, "title").or_else(|| {
        body.lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l.trim_start_matches("# ").trim().to_string())
    })?;

    let tags = frontmatter_list(frontmatter, "tags");

    Some(KnowledgeEntry {
        category,
        title,
        tags,
        body: body.to_string(),
    })
}

fn frontmatter_value(fm: &str, key: &str) -> Option<String> {
    for line in fm.lines() {
        if let Some(rest) = line.trim().strip_prefix(&format!("{key}:")) {
            let v = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn frontmatter_list(fm: &str, key: &str) -> Vec<String> {
    for line in fm.lines() {
        if let Some(rest) = line.trim().strip_prefix(&format!("{key}:")) {
            let inner = rest.trim().trim_start_matches('[').trim_end_matches(']');
            return inner
                .split(',')
                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}
