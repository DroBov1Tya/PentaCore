use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_BODY_BYTES: usize = 64 * 1024;
pub const MAX_TAG_CHARS: usize = 32;
pub const MAX_PROJECT_CHARS: usize = 64;
pub const MAX_QUERY_CHARS: usize = 512;
pub const MAX_LIMIT: u64 = 100;
pub const MAX_GRAPH_DEPTH: u8 = 32;
pub const MAX_TOKEN_CHARS: usize = 48;
pub const MAX_AUTHOR_CHARS: usize = 64;

pub type NoteId = i64;
pub type EntityId = i64;

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("internal error")]
    Internal(#[from] anyhow::Error),
}

macro_rules! internal_error_from {
    ($($source:ty),+ $(,)?) => {$(
        impl From<$source> for MemoryError {
            fn from(error: $source) -> Self {
                Self::Internal(error.into())
            }
        }
    )+};
}

internal_error_from!(
    rusqlite::Error,
    lancedb::Error,
    arrow_schema::ArrowError,
    tokio::task::JoinError,
);

pub fn invalid(message: impl Into<String>) -> MemoryError {
    MemoryError::Invalid(message.into())
}

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            #[allow(dead_code)]
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
pub(crate) use string_enum;

macro_rules! sql_text {
    ($($name:ident),+) => {$(
        impl rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                let text = value.as_str()?;
                Self::parse(text).ok_or_else(|| {
                    rusqlite::types::FromSqlError::Other(
                        format!("unknown {}: {text}", stringify!($name)).into(),
                    )
                })
            }
        }
    )+};
}

string_enum!(
    NoteKind {
        Goal => "goal",
        Step => "step",
        Attempt => "attempt",
        Fact => "fact",
        Decision => "decision",
        Question => "question",
    }
);

string_enum!(Status {
    Open => "open",
    Active => "active",
    Done => "done",
    Failed => "failed",
    Dropped => "dropped",
});

string_enum!(
    EdgeKind {
        DependsOn => "depends_on",
        Supports => "supports",
        Contradicts => "contradicts",
        Answers => "answers",
        Supersedes => "supersedes",
        RelatesTo => "relates_to",
    }
);

sql_text!(NoteKind, Status, EdgeKind);

impl NoteKind {
    pub fn allowed_statuses(self) -> &'static [Status] {
        use Status::*;
        match self {
            Self::Goal | Self::Step => &[Open, Active, Done, Failed, Dropped],
            Self::Attempt => &[Active, Done, Failed],
            Self::Question => &[Open, Done, Dropped],
            Self::Fact | Self::Decision => &[Active, Dropped],
        }
    }

    // An attempt has no default: its outcome is the point.
    pub fn default_status(self) -> Option<Status> {
        match self {
            Self::Goal | Self::Step | Self::Question => Some(Status::Open),
            Self::Fact | Self::Decision => Some(Status::Active),
            Self::Attempt => None,
        }
    }

    pub fn check_status(self, status: Status) -> Result<(), MemoryError> {
        if self.allowed_statuses().contains(&status) {
            return Ok(());
        }
        Err(invalid(format!(
            "status '{status}' is not valid for a {self}; allowed: {}",
            names(self.allowed_statuses())
        )))
    }
}

pub fn names(statuses: &[Status]) -> String {
    let names: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    names.join(", ")
}

macro_rules! validated_string {
    ($(#[$meta:meta])* $name:ident, $validate:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(value: String) -> std::result::Result<Self, String> {
                $validate(value).map(Self)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }
    };
}

pub(crate) use validated_string;

validated_string!(Title, validate_title);
validated_string!(
    #[derive(Default)]
    Body,
    validate_body
);
validated_string!(Tag, validate_tag);
validated_string!(ProjectName, validate_project);
validated_string!(SearchText, validate_search_text);
validated_string!(Token, validate_token);
validated_string!(Author, validate_author);

fn validate_title(value: String) -> Result<String, String> {
    let title = value.trim();
    if title.is_empty() {
        return Err("title must not be empty".into());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!("title is longer than {MAX_TITLE_CHARS} characters"));
    }
    if title
        .chars()
        .any(|c| c.is_control() || is_layout_control(c))
    {
        return Err("title must be a single line without control characters".into());
    }
    Ok(title.to_string())
}

// Invisible characters that make text display as something else.
fn is_layout_control(c: char) -> bool {
    matches!(c, '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn validate_body(value: String) -> Result<String, String> {
    if value.len() > MAX_BODY_BYTES {
        return Err(format!("text is larger than {MAX_BODY_BYTES} bytes"));
    }
    if value.contains('\0') {
        return Err("text must not contain NUL".into());
    }
    Ok(value)
}

fn validate_tag(value: String) -> Result<String, String> {
    let length = value.chars().count();
    let well_formed = value
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if length == 0 || length > MAX_TAG_CHARS || !well_formed {
        return Err(format!(
            "tag '{value}' must be 1-{MAX_TAG_CHARS} letters, digits, '.', '_' or '-'"
        ));
    }
    Ok(value)
}

fn validate_project(value: String) -> Result<String, String> {
    let well_formed = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if value.is_empty() || value.len() > MAX_PROJECT_CHARS || !well_formed {
        return Err(format!(
            "project must be 1-{MAX_PROJECT_CHARS} ASCII letters, digits, '.', '_' or '-'"
        ));
    }
    Ok(value)
}

fn validate_token(value: String) -> Result<String, String> {
    let well_formed = value
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    let starts_plain = value.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit());
    if value.len() > MAX_TOKEN_CHARS || !well_formed || !starts_plain {
        return Err(format!(
            "'{value}' must be 1-{MAX_TOKEN_CHARS} lowercase letters, digits, '.', '_' or '-', starting with a letter or digit"
        ));
    }
    Ok(value)
}

fn validate_author(value: String) -> Result<String, String> {
    let well_formed = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '@'));
    if value.is_empty() || value.len() > MAX_AUTHOR_CHARS || !well_formed {
        return Err(format!(
            "author must be 1-{MAX_AUTHOR_CHARS} ASCII letters, digits, '.', '_', '-', '/' or '@'"
        ));
    }
    Ok(value)
}

// Fixed width UTC, so text order is time order.
pub fn now() -> String {
    timestamp(chrono::Utc::now())
}

pub fn seconds_from_now(seconds: i64) -> String {
    timestamp(chrono::Utc::now() + chrono::Duration::seconds(seconds))
}

fn timestamp(time: chrono::DateTime<chrono::Utc>) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn validate_search_text(value: String) -> Result<String, String> {
    let text = value.trim();
    if text.is_empty() {
        return Err("query must not be empty".into());
    }
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(format!("query is longer than {MAX_QUERY_CHARS} characters"));
    }
    if text.chars().any(char::is_control) {
        return Err("query must not contain control characters".into());
    }
    Ok(text.to_string())
}

impl ProjectName {
    pub fn sanitized(raw: &str) -> Option<Self> {
        let cleaned: String = raw
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '-'
                }
            })
            .take(MAX_PROJECT_CHARS)
            .collect();
        Self::try_from(cleaned).ok()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "Vec<T>")]
pub struct AtMost<T, const MAX: usize>(Vec<T>);

impl<T, const MAX: usize> TryFrom<Vec<T>> for AtMost<T, MAX> {
    type Error = String;

    fn try_from(items: Vec<T>) -> Result<Self, String> {
        if items.len() > MAX {
            return Err(format!(
                "at most {MAX} items are allowed, got {}",
                items.len()
            ));
        }
        Ok(Self(items))
    }
}

impl<T, const MAX: usize> Default for AtMost<T, MAX> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T, const MAX: usize> AtMost<T, MAX> {
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

pub type Tags = AtMost<Tag, 16>;
pub type Links = AtMost<Link, 16>;

pub fn tag_strings(tags: &Tags) -> Vec<&str> {
    tags.as_slice().iter().map(Tag::as_str).collect()
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "u64")]
pub struct Limit(usize);

impl Limit {
    pub fn or(limit: Option<Self>, default: usize) -> usize {
        limit.map_or(default, |l| l.0)
    }
}

impl TryFrom<u64> for Limit {
    type Error = String;

    fn try_from(value: u64) -> Result<Self, String> {
        if (1..=MAX_LIMIT).contains(&value) {
            Ok(Self(value as usize))
        } else {
            Err(format!("limit must be between 1 and {MAX_LIMIT}"))
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "String")]
pub enum Scope {
    All,
    Project(ProjectName),
}

impl TryFrom<String> for Scope {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        if value == "*" {
            Ok(Self::All)
        } else {
            ProjectName::try_from(value).map(Self::Project)
        }
    }
}

impl Scope {
    pub fn project(&self) -> Option<&str> {
        match self {
            Self::All => None,
            Self::Project(name) => Some(name.as_str()),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub kind: EdgeKind,
    pub to: NoteId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewNote {
    pub kind: NoteKind,
    pub title: Title,
    #[serde(default)]
    pub body: Body,
    pub status: Option<Status>,
    #[serde(default)]
    pub tags: Tags,
    pub parent: Option<NoteId>,
    pub project: Option<ProjectName>,
    #[serde(default)]
    pub links: Links,
    pub entity: Option<EntityId>,
    pub author: Option<Author>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotePatch {
    pub id: NoteId,
    pub title: Option<Title>,
    pub body: Option<Body>,
    pub status: Option<Status>,
    pub tags: Option<Tags>,
    pub parent: Option<NoteId>,
    pub append: Option<Body>,
    pub entity: Option<EntityId>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindQuery {
    pub query: SearchText,
    pub project: Option<Scope>,
    pub kind: Option<NoteKind>,
    pub status: Option<Status>,
    pub under: Option<NoteId>,
    pub entity: Option<EntityId>,
    pub limit: Option<Limit>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Note {
    pub id: NoteId,
    pub project: String,
    pub kind: NoteKind,
    pub status: Status,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub parent: Option<NoteId>,
    pub entity: Option<EntityId>,
    pub author: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct CreatedNote {
    #[serde(flatten)]
    pub note: Note,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub similar: Vec<NoteBrief>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteBrief {
    pub id: NoteId,
    pub project: String,
    pub kind: NoteKind,
    pub status: Status,
    pub title: String,
    pub parent: Option<NoteId>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Edge {
    pub src: NoteId,
    pub dst: NoteId,
    pub kind: EdgeKind,
}

#[derive(Debug, Serialize)]
pub struct NoteDetail {
    pub note: Note,
    pub children: Vec<NoteBrief>,
    pub links: Vec<LinkedNote>,
}

#[derive(Debug, Serialize)]
pub struct LinkedNote {
    #[serde(flatten)]
    pub edge: Edge,
    pub peer: NoteBrief,
}

#[derive(Debug, Serialize)]
pub struct Hit {
    #[serde(flatten)]
    pub note: NoteBrief,
    pub snippet: String,
}

#[derive(Debug, Serialize)]
pub struct FindResult {
    pub matched: &'static str,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Serialize)]
pub struct GraphNode {
    pub id: NoteId,
    pub parent: Option<NoteId>,
    pub depth: i64,
    pub kind: NoteKind,
    pub status: Status,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub goals: Vec<NoteBrief>,
    pub active_steps: Vec<NoteBrief>,
    pub open_questions: Vec<NoteBrief>,
    pub recent: Vec<NoteBrief>,
    pub total_notes: i64,
    pub checkpoint: Option<Checkpoint>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCheckpoint {
    pub summary: Body,
    pub project: Option<ProjectName>,
    pub author: Option<Author>,
}

#[derive(Debug, Serialize)]
pub struct Checkpoint {
    pub id: i64,
    pub project: String,
    pub summary: String,
    pub author: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    #[test]
    fn title_is_trimmed_and_bounded() {
        assert_eq!(parse::<Title>(json!("  hi  ")).unwrap().as_str(), "hi");
        assert!(parse::<Title>(json!("   ")).is_err());
        assert!(parse::<Title>(json!("two\nlines")).is_err());
        assert!(parse::<Title>(json!("safe\u{202E}txt.exe")).is_err());
        assert!(parse::<Title>(json!("line\u{2028}break")).is_err());
        assert!(parse::<Title>(json!("x".repeat(MAX_TITLE_CHARS))).is_ok());
        assert!(parse::<Title>(json!("x".repeat(MAX_TITLE_CHARS + 1))).is_err());
    }

    #[test]
    fn body_is_bounded() {
        assert!(parse::<Body>(json!("x".repeat(MAX_BODY_BYTES))).is_ok());
        assert!(parse::<Body>(json!("x".repeat(MAX_BODY_BYTES + 1))).is_err());
        assert!(parse::<Body>(json!("a\0b")).is_err());
    }

    #[test]
    fn tags_and_projects_use_an_allowlist() {
        assert!(parse::<Tag>(json!("сборка-2")).is_ok());
        assert!(parse::<Tag>(json!("two words")).is_err());
        assert!(parse::<Tag>(json!("")).is_err());
        assert!(parse::<ProjectName>(json!("agent_core-1.0")).is_ok());
        assert!(parse::<ProjectName>(json!("a' OR '1'='1")).is_err());
        assert!(parse::<ProjectName>(json!("проект")).is_err());
        assert!(parse::<Tags>(json!(vec!["t"; 16])).is_ok());
        assert!(parse::<Tags>(json!(vec!["t"; 17])).is_err());
    }

    #[test]
    fn tokens_and_authors_use_an_allowlist() {
        for good in ["endpoint", "api.v2", "not-checked", "2fa_flow"] {
            assert!(parse::<Token>(json!(good)).is_ok(), "{good}");
        }
        for bad in [
            "",
            "Endpoint",
            "has space",
            "-leading",
            "a'b",
            &"x".repeat(MAX_TOKEN_CHARS + 1),
        ] {
            assert!(parse::<Token>(json!(bad)).is_err(), "{bad}");
        }
        assert!(parse::<Author>(json!("claude/reviewer@host-1")).is_ok());
        assert!(parse::<Author>(json!("two words")).is_err());
        assert!(parse::<Author>(json!("")).is_err());
    }

    #[test]
    fn timestamps_order_as_text() {
        assert!(now() < seconds_from_now(1));
        assert!(seconds_from_now(-1) < now());
    }

    #[test]
    fn limit_rejects_zero_and_oversize() {
        assert!(parse::<Limit>(json!(0)).is_err());
        assert!(parse::<Limit>(json!(1)).is_ok());
        assert!(parse::<Limit>(json!(MAX_LIMIT)).is_ok());
        assert!(parse::<Limit>(json!(MAX_LIMIT + 1)).is_err());
        assert!(parse::<Limit>(json!(-1)).is_err());
    }

    #[test]
    fn scope_star_means_all_projects() {
        assert!(matches!(parse::<Scope>(json!("*")).unwrap(), Scope::All));
        assert!(matches!(
            parse::<Scope>(json!("web")).unwrap(),
            Scope::Project(_)
        ));
        assert!(parse::<Scope>(json!("a b")).is_err());
    }

    #[test]
    fn unknown_fields_and_kinds_are_rejected() {
        assert!(parse::<NewNote>(json!({"kind": "fact", "title": "t", "admin": true})).is_err());
        assert!(parse::<NewNote>(json!({"kind": "manager", "title": "t"})).is_err());
        assert!(parse::<NewNote>(json!({"kind": "fact", "title": "t"})).is_ok());
    }

    #[test]
    fn every_kind_accepts_its_default_status() {
        for kind in NoteKind::ALL {
            if let Some(status) = kind.default_status() {
                assert!(kind.check_status(status).is_ok());
            }
        }
        assert!(NoteKind::Fact.check_status(Status::Failed).is_err());
        assert!(NoteKind::Attempt.check_status(Status::Open).is_err());
    }

    #[test]
    fn sanitized_project_replaces_foreign_characters() {
        assert_eq!(
            ProjectName::sanitized("my project!").unwrap().as_str(),
            "my-project-"
        );
        assert!(ProjectName::sanitized("").is_none());
    }
}
