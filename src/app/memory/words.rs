// Matching by words. FTS proposes candidates; each is scored by the share of
// the query it covers (rare words weigh more, an exact word more than a
// prefix). The score is comparable across stores.

use rusqlite::{Connection, params};

use super::model::MemoryError;

const MAX_CHUNKS: usize = 24;
const EXACT: f32 = 1.0;
const PREFIX: f32 = 0.8;
const STEM: f32 = 0.6;
const PARTIAL: f32 = 0.5;
const TITLE_BONUS: f32 = 0.1;
// Below this a word says nothing by itself ("of", "in").
const MIN_PREFIX_CHARS: usize = 3;

type Result<T> = std::result::Result<T, MemoryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Store {
    Notes,
    Entities,
}

impl Store {
    // (id, title, rest of the text) of the records matching an FTS expression.
    fn sql(self) -> &'static str {
        match self {
            Self::Notes => {
                "SELECT n.id FROM notes_fts JOIN notes n ON n.id = notes_fts.rowid
                 WHERE notes_fts MATCH ?1 AND (?2 IS NULL OR n.project = ?2)
                 ORDER BY bm25(notes_fts, 4.0, 1.0, 2.0), n.id DESC LIMIT ?3"
            }
            Self::Entities => {
                "SELECT e.id
                 FROM entities_fts JOIN entities e ON e.id = entities_fts.rowid
                 WHERE entities_fts MATCH ?1 AND (?2 IS NULL OR e.project = ?2)
                 ORDER BY bm25(entities_fts, 4.0, 2.0, 1.0, 1.0), e.id DESC LIMIT ?3"
            }
        }
    }
}

// Lowercased runs of letters and digits, the way the full-text index splits text.
pub fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase().replace('ё', "е"))
}

// " word word " so that a whole word, or the start of one, is a substring test.
fn normalised(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(' ');
    for token in tokens(text) {
        out.push_str(&token);
        out.push(' ');
    }
    out
}

// Cuts the ending off a long word, so that other forms of it match as a prefix.
fn stem(token: &str) -> Option<String> {
    if !token.chars().all(char::is_alphabetic) {
        return None;
    }
    let length = token.chars().count();
    if length < 5 {
        return None;
    }
    // Three quarters of the word: enough to tell `backup` from `backoff`.
    Some(token.chars().take((length * 3).div_ceil(4)).collect())
}

// One whitespace-separated piece of the query: a word, a path, an identifier.
#[derive(Debug)]
struct Chunk {
    raw: String,
    tokens: Vec<String>,
    stem: Option<String>,
    weight: f32,
}

impl Chunk {
    fn phrase(&self) -> String {
        self.tokens.join(" ")
    }

    fn is_short(&self) -> bool {
        self.tokens.len() == 1 && self.tokens[0].chars().count() < MIN_PREFIX_CHARS
    }

    // How well the text holds this chunk, between 0 and 1.
    fn found_in(&self, text: &str) -> f32 {
        let phrase = self.phrase();
        if text.contains(&format!(" {phrase} ")) {
            return EXACT;
        }
        if self.is_short() {
            return 0.0;
        }
        if text.contains(&format!(" {phrase}")) {
            return PREFIX;
        }
        if let Some(stem) = &self.stem
            && text.contains(&format!(" {stem}"))
        {
            return STEM;
        }
        if self.tokens.len() > 1 {
            let present = self
                .tokens
                .iter()
                .filter(|token| text.contains(&format!(" {token} ")))
                .count();
            return PARTIAL * present as f32 / self.tokens.len() as f32;
        }
        0.0
    }

    // Quoted, so FTS5 operators in user text stay plain text.
    fn expression(&self) -> String {
        let quoted = |text: &str| format!("\"{}\"", text.replace('"', "\"\""));
        if self.is_short() {
            return quoted(&self.raw);
        }
        match &self.stem {
            Some(stem) => format!("{}*", quoted(stem)),
            None => format!("{}*", quoted(&self.raw)),
        }
    }
}

#[derive(Debug)]
pub struct Query {
    chunks: Vec<Chunk>,
}

impl Query {
    pub fn parse(text: &str) -> Self {
        let chunks = text
            .split_whitespace()
            .filter_map(|raw| {
                let tokens: Vec<String> = tokens(raw).collect();
                if tokens.is_empty() {
                    return None;
                }
                let stem = match tokens.as_slice() {
                    [only] => stem(only),
                    _ => None,
                };
                Some(Chunk {
                    raw: raw.to_string(),
                    tokens,
                    stem,
                    weight: 1.0,
                })
            })
            .take(MAX_CHUNKS)
            .collect();
        Self { chunks }
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    fn any_of(&self) -> Option<String> {
        if self.chunks.is_empty() {
            return None;
        }
        let parts: Vec<String> = self.chunks.iter().map(Chunk::expression).collect();
        Some(parts.join(" OR "))
    }

    // A word found in few records tells more than one found in most of them.
    fn weigh(&mut self, conn: &Connection) -> Result<()> {
        let total: f64 = conn.query_row("SELECT count(*) FROM notes", [], |row| row.get(0))?;
        let mut holding = conn
            .prepare_cached("SELECT coalesce((SELECT doc FROM notes_vocab WHERE term = ?1), 0)")?;
        for chunk in &mut self.chunks {
            let mut rarest = f64::MAX;
            for token in &chunk.tokens {
                let count: f64 = holding.query_row([token], |row| row.get(0))?;
                rarest = rarest.min(count);
            }
            chunk.weight = (1.0 + (total + 1.0) / (rarest + 1.0)).ln() as f32;
        }
        Ok(())
    }

    // Share of the query found in a record, 0 to 1, plus a title bonus.
    pub fn coverage(&self, title: &str, rest: &str) -> f32 {
        let (title, rest) = (normalised(title), normalised(rest));
        let total: f32 = self.chunks.iter().map(|chunk| chunk.weight).sum();
        if total <= 0.0 {
            return 0.0;
        }
        let (mut found, mut in_title) = (0.0, 0.0);
        for chunk in &self.chunks {
            let title_match = chunk.found_in(&title);
            found += chunk.weight * title_match.max(chunk.found_in(&rest));
            in_title += chunk.weight * title_match;
        }
        (found + TITLE_BONUS * in_title) / total
    }
}

// Ids of the records of one store that share words with the query, in the
// order the full-text index ranks them.
pub fn candidates(
    conn: &Connection,
    store: Store,
    query: &Query,
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<i64>> {
    let Some(expression) = query.any_of() else {
        return Ok(Vec::new());
    };
    Ok(conn
        .prepare_cached(store.sql())?
        .query_map(params![expression, project, limit as i64], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

// Parses the query and weighs its words against what the memory holds.
pub fn weighed(conn: &Connection, text: &str) -> Result<Query> {
    let mut query = Query::parse(text);
    query.weigh(conn)?;
    Ok(query)
}

// Mostly Latin script. The English-only model rates any two texts in
// another script as close, so it is not consulted for those.
pub fn mostly_latin(text: &str) -> bool {
    let (mut letters, mut latin) = (0u32, 0u32);
    for c in text.chars().filter(|c| c.is_alphabetic()).take(600) {
        letters += 1;
        latin += u32::from(c.is_ascii());
    }
    letters > 0 && latin * 10 >= letters * 6
}

// Share of words two texts have in common, from 0 to 1.
pub fn overlap(a: &str, b: &str) -> f32 {
    use std::collections::HashSet;
    let (a, b): (HashSet<String>, HashSet<String>) = (tokens(a).collect(), tokens(b).collect());
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(&b).count() as f32;
    shared / (a.len() + b.len()) as f32 * 2.0
}
