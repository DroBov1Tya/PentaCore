// One search over notes, entities and concepts. Order is by relevance: word
// coverage or closeness in meaning, either one enough. Dropped or superseded
// records are scaled down; freshness and task membership add a little.
// Repeats are folded under the best one; nothing is deleted.

use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

use super::index::Neighbor;
use super::journal::RecordType;
use super::model::{Limit, MemoryError, NoteId, Scope, SearchText, invalid};
use super::words::{self, Query, Store};

const DEFAULT_LIMIT: usize = 8;
const CANDIDATES_PER_STORE: usize = 40;
pub const SEMANTIC_CANDIDATES: usize = 40;
const PREVIEW_CHARS: usize = 300;
const SNIPPET_CHARS: usize = 200;
const SNIPPET_LEAD_CHARS: usize = 60;
const COMPARED_CHARS: usize = 600;

// Similarity of unrelated texts sits near the floor; of a paraphrase, near the ceiling.
const SIMILARITY_FLOOR: f32 = 0.25;
const SIMILARITY_CEILING: f32 = 0.75;
const AGREEMENT_BONUS: f32 = 0.25;
const MIN_RELEVANCE: f32 = 0.08;
// Share of the best score below which a record is not shown.
const WEAKEST_SHOWN: f32 = 0.3;

const NO_LONGER_VALID: f32 = 0.5;
const DISPUTED: f32 = 0.8;
const FRESHNESS_BONUS: f32 = 0.06;
const FRESHNESS_HALF_LIFE_DAYS: f32 = 45.0;
const TASK_BONUS: f32 = 0.2;

const SAME_TEXT: f32 = 0.8;
const SAME_TITLE_SIMILAR_TEXT: f32 = 0.5;
// A rewording is close in meaning and shares words; meaning alone would also
// merge different facts about one subject.
const SAME_MEANING: f32 = 0.75;
const SAME_MEANING_SHARED_WORDS: f32 = 0.35;

type Result<T> = std::result::Result<T, MemoryError>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallQuery {
    pub query: SearchText,
    pub project: Option<Scope>,
    pub limit: Option<Limit>,
    pub task: Option<NoteId>,
    pub only: Option<RecordType>,
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    Note,
    Entity,
    Concept,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Entity => "entity",
            Self::Concept => "concept",
        }
    }

    fn record_type(self) -> RecordType {
        match self {
            Self::Note => RecordType::Note,
            Self::Entity => RecordType::Entity,
            Self::Concept => RecordType::Concept,
        }
    }

    fn store(self) -> Store {
        match self {
            Self::Note => Store::Notes,
            Self::Entity => Store::Entities,
            Self::Concept => Store::Concepts,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Validity {
    dropped: bool,
    superseded: bool,
    disputed: bool,
}

struct Item {
    source: Source,
    id: String,
    title: String,
    text: String,
    updated_at: String,
    in_task: bool,
    // Whether the embedding model can read this record.
    latin: bool,
    validity: Validity,
    // Fields of the result as the caller sees them.
    shown: Map<String, Value>,
    words: f32,
    meaning: Option<f32>,
    score: f32,
    superseded_by: Vec<NoteId>,
    contradicted_by: Vec<NoteId>,
    supersedes: Vec<NoteId>,
    duplicates: Vec<Value>,
}

// The stronger evidence plus a share of the weaker one; 0 to about 1.25.
pub fn relevance(words: f32, similarity: Option<f32>) -> f32 {
    let meaning = similarity.map_or(0.0, |similarity| {
        ((similarity - SIMILARITY_FLOOR) / (SIMILARITY_CEILING - SIMILARITY_FLOOR)).clamp(0.0, 1.0)
    });
    words.max(meaning) + AGREEMENT_BONUS * words.min(meaning)
}

fn adjusted(relevance: f32, validity: Validity, age_days: f32, in_task: bool) -> f32 {
    let valid = if validity.dropped || validity.superseded {
        NO_LONGER_VALID
    } else if validity.disputed {
        DISPUTED
    } else {
        1.0
    };
    let fresh = 1.0 + FRESHNESS_BONUS * 0.5f32.powf(age_days.max(0.0) / FRESHNESS_HALF_LIFE_DAYS);
    let task = if in_task { 1.0 + TASK_BONUS } else { 1.0 };
    relevance * valid * fresh * task
}

fn age_days(updated_at: &str, now: chrono::DateTime<chrono::Utc>) -> f32 {
    chrono::DateTime::parse_from_rfc3339(updated_at).map_or(0.0, |then| {
        (now - then.with_timezone(&chrono::Utc)).num_seconds() as f32 / 86_400.0
    })
}

fn ids_json<'a>(ids: impl Iterator<Item = &'a String>) -> String {
    Value::from(ids.cloned().collect::<Vec<_>>()).to_string()
}

const SUBTREE: &str = "\
    WITH RECURSIVE sub (id) AS (
        SELECT ?2
        UNION
        SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
    )
    SELECT id FROM sub";

fn first_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn blank(source: Source, id: String, title: String, text: String, updated_at: String) -> Item {
    Item {
        source,
        id,
        latin: words::mostly_latin(&format!("{title} {text}")),
        title,
        text,
        updated_at,
        in_task: false,
        validity: Validity::default(),
        shown: Map::new(),
        words: 0.0,
        meaning: None,
        score: 0.0,
        superseded_by: Vec::new(),
        contradicted_by: Vec::new(),
        supersedes: Vec::new(),
        duplicates: Vec::new(),
    }
}

fn load_notes(
    conn: &Connection,
    ids: &str,
    task: Option<NoteId>,
    query: &Query,
) -> Result<Vec<Item>> {
    let mut items: Vec<Item> = conn
        .prepare(&format!(
            "SELECT n.id, n.project, n.kind, n.status, n.title, n.parent_id, n.updated_at,
                    substr(n.body, 1, 8000), n.tags,
                    ?2 IS NOT NULL AND n.id IN ({SUBTREE})
             FROM notes n WHERE n.id IN (SELECT CAST(value AS INTEGER) FROM json_each(?1))"
        ))?
        .query_map(params![ids, task], |row| {
            let id: i64 = row.get(0)?;
            let (kind, status, title): (String, String, String) =
                (row.get(2)?, row.get(3)?, row.get(4)?);
            let (body, tags): (String, String) = (row.get(7)?, row.get(8)?);
            let updated_at: String = row.get(6)?;
            let mut item = blank(
                Source::Note,
                id.to_string(),
                title.clone(),
                format!("{body} {tags}"),
                updated_at.clone(),
            );
            item.in_task = row.get(9)?;
            item.validity.dropped = status == "dropped";
            item.shown = json!({
                "id": id,
                "project": row.get::<_, String>(1)?,
                "kind": kind,
                "status": status,
                "title": title,
                "parent": row.get::<_, Option<i64>>(5)?,
                "updated_at": updated_at,
                "snippet": snippet(&body, query),
            })
            .as_object()
            .cloned()
            .unwrap_or_default();
            Ok(item)
        })?
        .collect::<rusqlite::Result<_>>()?;

    // A newer note that replaces or disputes this one.
    let mut challenges = conn.prepare(
        "SELECT e.dst, e.src, e.kind FROM edges e
         JOIN notes s ON s.id = e.src
         JOIN notes d ON d.id = e.dst
         WHERE e.dst IN (SELECT CAST(value AS INTEGER) FROM json_each(?1))
           AND s.status <> 'dropped'
           AND (e.kind = 'supersedes' OR (e.kind = 'contradicts' AND s.created_at >= d.created_at))
         ORDER BY e.src",
    )?;
    let challenges: Vec<(i64, i64, String)> = challenges
        .query_map([ids], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (target, challenger, kind) in challenges {
        let Some(item) = items.iter_mut().find(|item| item.id == target.to_string()) else {
            continue;
        };
        if kind == "supersedes" {
            item.validity.superseded = true;
            item.superseded_by.push(challenger);
        } else {
            item.validity.disputed = true;
            item.contradicted_by.push(challenger);
        }
    }
    Ok(items)
}

fn load_concepts(conn: &Connection, ids: &str, include_archived: bool) -> Result<Vec<Item>> {
    Ok(conn
        .prepare(
            "SELECT c.id, c.project, c.title, substr(c.content, 1, 8000), c.tags, c.updated_at,
                    c.archived
             FROM concepts c
             WHERE c.id IN (SELECT value FROM json_each(?1)) AND (?2 OR c.archived = 0)",
        )?
        .query_map(params![ids, include_archived], |row| {
            let (id, title, content, tags): (String, String, String, String) =
                (row.get(0)?, row.get(2)?, row.get(3)?, row.get(4)?);
            let updated_at: String = row.get(5)?;
            let archived: bool = row.get(6)?;
            let mut item = blank(
                Source::Concept,
                id.clone(),
                title.clone(),
                format!("{content} {tags}"),
                updated_at.clone(),
            );
            item.validity.dropped = archived;
            let mut shown = json!({
                "id": id,
                "project": row.get::<_, String>(1)?,
                "title": title,
                "preview": first_chars(&content, PREVIEW_CHARS),
                "updated_at": updated_at,
            });
            if archived {
                shown["archived"] = json!(true);
            }
            item.shown = shown.as_object().cloned().unwrap_or_default();
            Ok(item)
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn load_entities(conn: &Connection, ids: &str, task: Option<NoteId>) -> Result<Vec<Item>> {
    Ok(conn
        .prepare(&format!(
            "SELECT e.id, e.project, e.type, e.key, e.status, e.attrs, e.updated_at,
                    ?2 IS NOT NULL AND EXISTS (
                        SELECT 1 FROM notes t WHERE t.entity_id = e.id AND t.id IN ({SUBTREE})),
                    f.attrs
             FROM entities e JOIN entities_fts f ON f.rowid = e.id
             WHERE e.id IN (SELECT CAST(value AS INTEGER) FROM json_each(?1))"
        ))?
        .query_map(params![ids, task], |row| {
            let id: i64 = row.get(0)?;
            let (kind, key, status, attrs): (String, String, String, String) =
                (row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?);
            let updated_at: String = row.get(6)?;
            let attr_words: String = row.get(8)?;
            let title = format!("{kind} {key}");
            let mut item = blank(
                Source::Entity,
                id.to_string(),
                title.clone(),
                format!("{status} {attr_words}"),
                updated_at.clone(),
            );
            item.in_task = row.get(7)?;
            item.shown = json!({
                "id": id,
                "project": row.get::<_, String>(1)?,
                "type": kind,
                "key": key,
                "status": status,
                "title": title,
                "preview": first_chars(&attrs, PREVIEW_CHARS),
                "updated_at": updated_at,
            })
            .as_object()
            .cloned()
            .unwrap_or_default();
            Ok(item)
        })?
        .collect::<rusqlite::Result<_>>()?)
}

// The part of the body around the first query word found in it, that word in brackets.
fn snippet(body: &str, query: &Query) -> String {
    let mut run_start = None;
    let mut hit = None;
    for (offset, c) in body.char_indices().chain([(body.len(), ' ')]) {
        match (c.is_alphanumeric(), run_start) {
            (true, None) => run_start = Some(offset),
            (false, Some(start)) => {
                run_start = None;
                if query.coverage("", &body[start..offset]) > 0.0 {
                    hit = Some((start, offset));
                    break;
                }
            }
            _ => {}
        }
    }
    let Some((start, end)) = hit else {
        return first_chars(body, SNIPPET_CHARS);
    };
    let lead_start = body[..start]
        .char_indices()
        .rev()
        .nth(SNIPPET_LEAD_CHARS - 1)
        .map_or(0, |(offset, _)| offset);
    let tail = first_chars(&body[end..], SNIPPET_CHARS - SNIPPET_LEAD_CHARS);
    let lead = if lead_start > 0 { " ... " } else { "" };
    format!(
        "{lead}{}[{}]{tail}",
        &body[lead_start..start],
        &body[start..end]
    )
}

pub type Vectors = HashMap<(RecordType, String), Vec<f32>>;

impl Item {
    fn record(&self) -> Option<(RecordType, String)> {
        let record = match self.source {
            Source::Note => RecordType::Note,
            Source::Concept => RecordType::Concept,
            Source::Entity => return None,
        };
        Some((record, self.id.clone()))
    }
}

fn same_meaning(a: &Item, b: &Item, vectors: &Vectors) -> bool {
    if !(a.latin && b.latin) {
        return false;
    }
    let vector = |item: &Item| item.record().and_then(|record| vectors.get(&record));
    let (Some(a), Some(b)) = (vector(a), vector(b)) else {
        return false;
    };
    // The model's vectors have unit length, so this is the cosine.
    a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() >= SAME_MEANING
}

fn same_thing(a: &Item, b: &Item, vectors: &Vectors) -> bool {
    let text = |item: &Item| format!("{} {}", item.title, first_chars(&item.text, COMPARED_CHARS));
    let overlap = words::overlap(&text(a), &text(b));
    let same_title = words::overlap(&a.title, &b.title) >= 1.0;
    overlap >= SAME_TEXT
        || (same_title && overlap >= SAME_TITLE_SIMILAR_TEXT)
        || (overlap >= SAME_MEANING_SHARED_WORDS && same_meaning(a, b, vectors))
}

// Keeps the best of each group of repeats; folds a replaced note into its successor.
fn without_repeats(ranked: Vec<Item>, vectors: &Vectors) -> Vec<Item> {
    let mut kept: Vec<Item> = Vec::new();
    for item in ranked {
        if item.source == Source::Note
            && let Some(newer) = kept.iter_mut().find(|kept| {
                kept.source == Source::Note
                    && kept
                        .id
                        .parse()
                        .is_ok_and(|id: NoteId| item.superseded_by.contains(&id))
            })
        {
            newer.supersedes.extend(item.id.parse::<NoteId>());
            continue;
        }
        if let Some(original) = kept
            .iter_mut()
            .find(|kept| same_thing(kept, &item, vectors))
        {
            original
                .duplicates
                .push(json!({ "source": item.source.as_str(), "id": item.shown["id"] }));
            continue;
        }
        kept.push(item);
    }
    kept
}

fn round(value: f32) -> f64 {
    (f64::from(value) * 1000.0).round() / 1000.0
}

fn shown(item: Item) -> Value {
    let mut result = item.shown;
    result.insert("source".into(), json!(item.source.as_str()));
    result.insert("score".into(), json!(round(item.score)));
    let mut matched = json!({ "words": round(item.words) });
    if let Some(similarity) = item.meaning {
        matched["meaning"] = json!(round(similarity));
    }
    result.insert("matched".into(), matched);
    for (name, ids) in [
        ("superseded_by", item.superseded_by),
        ("contradicted_by", item.contradicted_by),
        ("supersedes", item.supersedes),
    ] {
        if !ids.is_empty() {
            result.insert(name.into(), json!(ids));
        }
    }
    if !item.duplicates.is_empty() {
        result.insert("duplicates".into(), Value::from(item.duplicates));
    }
    Value::Object(result)
}

// What `find` would have called the best note match; kept for older clients.
fn notes_matched(items: &[Item]) -> &'static str {
    let best = items
        .iter()
        .filter(|item| item.source == Source::Note)
        .map(|item| item.words)
        .fold(0.0, f32::max);
    if best >= 1.0 {
        "all"
    } else if best >= 0.8 {
        "prefix"
    } else {
        "any"
    }
}

// Ranks records against the query. `neighbors` is None without the model.
pub fn rank(
    conn: &Connection,
    args: &RecallQuery,
    scope: &Scope,
    neighbors: Option<Vec<Neighbor>>,
) -> Result<Ranked> {
    let limit = Limit::or(args.limit, DEFAULT_LIMIT);
    let project = scope.project();
    if let Some(task) = args.task {
        super::notes::load(conn, task)?;
    }
    let query = words::weighed(conn, args.query.as_str())?;
    if args.only == Some(RecordType::Checkpoint) {
        return Err(invalid("`only` must be note, entity or concept"));
    }
    if query.is_empty() {
        return Err(invalid("query has no letters or digits to search for"));
    }
    let semantic = neighbors.is_some();
    let query_is_latin = words::mostly_latin(args.query.as_str());

    let mut similarity: HashMap<(Source, String), f32> = HashMap::new();
    for neighbor in neighbors.unwrap_or_default() {
        let source = match neighbor.record {
            RecordType::Note => Source::Note,
            RecordType::Concept => Source::Concept,
            _ => continue,
        };
        similarity.insert((source, neighbor.id), neighbor.similarity);
    }

    let mut items = Vec::new();
    for source in [Source::Note, Source::Entity, Source::Concept] {
        if args.only.is_some_and(|only| only != source.record_type()) {
            continue;
        }
        let mut ids =
            words::candidates(conn, source.store(), &query, project, CANDIDATES_PER_STORE)?;
        ids.extend(
            similarity
                .keys()
                .filter(|(of, id)| *of == source && !ids.contains(id))
                .map(|(_, id)| id.clone())
                .collect::<Vec<_>>(),
        );
        let ids = ids_json(ids.iter());
        items.extend(match source {
            Source::Note => load_notes(conn, &ids, args.task, &query)?,
            Source::Entity => load_entities(conn, &ids, args.task)?,
            Source::Concept => load_concepts(conn, &ids, args.include_archived)?,
        });
    }

    let now = chrono::Utc::now();
    for item in &mut items {
        item.words = query.coverage(&item.title, &item.text);
        item.meaning = similarity
            .get(&(item.source, item.id.clone()))
            .copied()
            .filter(|_| query_is_latin && item.latin);
        let relevance = relevance(item.words, item.meaning);
        item.score = if relevance < MIN_RELEVANCE {
            0.0
        } else {
            adjusted(
                relevance,
                item.validity,
                age_days(&item.updated_at, now),
                item.in_task,
            )
        };
    }
    // A record far behind the best one is noise next to it.
    let best = items.iter().map(|item| item.score).fold(0.0, f32::max);
    items.retain(|item| item.score > 0.0 && item.score >= best * WEAKEST_SHOWN);
    items.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    // Enough to fill the page after repeats are folded away.
    items.truncate(limit * 4);
    Ok(Ranked {
        matched: notes_matched(&items),
        items,
        limit,
        semantic,
    })
}

// The candidates in order, before repeats are folded.
pub struct Ranked {
    items: Vec<Item>,
    limit: usize,
    matched: &'static str,
    semantic: bool,
}

impl Ranked {
    pub fn is_semantic(&self) -> bool {
        self.semantic
    }

    // Records whose vectors help to tell a rewording from a different fact.
    pub fn records(&self) -> Vec<(RecordType, String)> {
        self.items.iter().filter_map(Item::record).collect()
    }

    pub fn finish(self, vectors: &Vectors) -> Value {
        let mut results = without_repeats(self.items, vectors);
        results.truncate(self.limit);
        json!({
            "results": results.into_iter().map(shown).collect::<Vec<_>>(),
            "notes_matched": self.matched,
            "concepts_available": self.semantic,
            "semantic": self.semantic,
        })
    }
}

#[cfg(test)]
#[path = "tests/recall.rs"]
mod tests;
