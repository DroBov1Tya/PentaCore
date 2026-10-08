use serde::{Deserialize, Serialize};

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_BODY_BYTES: usize = 64 * 1024;
pub const MAX_TAG_CHARS: usize = 32;
pub const MAX_PROJECT_CHARS: usize = 64;
pub const MAX_QUERY_CHARS: usize = 512;
pub const MAX_LIMIT: u64 = 100;
pub const MAX_GRAPH_DEPTH: u8 = 32;
pub const MAX_TOKEN_CHARS: usize = 48;
pub const MAX_AUTHOR_CHARS: usize = 64;
pub const MAX_BASIS_CHARS: usize = 500;

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

internal_error_from!(rusqlite::Error, tokio::task::JoinError,);

pub fn invalid(message: impl Into<String>) -> MemoryError {
    MemoryError::Invalid(message.into())
}

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            #[allow(dead_code)]
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

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        Lesson => "lesson",
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

pub(crate) use sql_text;

sql_text!(NoteKind, Status, EdgeKind);

impl NoteKind {
    pub fn allowed_statuses(self) -> &'static [Status] {
        use Status::*;
        match self {
            Self::Goal | Self::Step => &[Open, Active, Done, Failed, Dropped],
            Self::Attempt => &[Active, Done, Failed],
            Self::Question => &[Open, Done, Dropped],
            Self::Fact | Self::Decision | Self::Lesson => &[Active, Dropped],
        }
    }

    // An attempt has no default: its outcome is the point.
    pub fn default_status(self) -> Option<Status> {
        match self {
            Self::Goal | Self::Step | Self::Question => Some(Status::Open),
            Self::Fact | Self::Decision | Self::Lesson => Some(Status::Active),
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
validated_string!(
    // Label of the session or run that made a change. Same alphabet as an author.
    RunId,
    validate_run
);

// How sure the author is that a record holds, from 0 to 1.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "f64")]
pub struct Confidence(f64);

impl Confidence {
    pub fn value(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Confidence {
    type Error = String;

    fn try_from(value: f64) -> Result<Self, String> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err("confidence must be between 0 and 1".into())
        }
    }
}

validated_string!(
    // One or two sentences on what a confidence rests on.
    Basis,
    validate_basis
);

fn validate_basis(value: String) -> Result<String, String> {
    let basis = value.trim();
    if basis.is_empty() || basis.chars().count() > MAX_BASIS_CHARS {
        return Err(format!("basis must be 1-{MAX_BASIS_CHARS} characters"));
    }
    if basis.chars().any(char::is_control) {
        return Err("basis must be a single line without control characters".into());
    }
    Ok(basis.to_string())
}

// Who made a change: labels for the journal, never a credential.
#[derive(Debug, Clone, Default)]
pub struct Attribution {
    pub author: Option<String>,
    pub run: Option<String>,
    pub task: Option<NoteId>,
}

impl Attribution {
    pub fn new(author: Option<Author>, run: Option<RunId>, task: Option<NoteId>) -> Self {
        Self {
            author: author.map(String::from),
            run: run.map(String::from),
            task,
        }
    }
}

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

fn validate_run(value: String) -> Result<String, String> {
    validate_author(value).map_err(|problem| problem.replacen("author", "run", 1))
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
    pub run: Option<RunId>,
    pub task: Option<NoteId>,
    pub confidence: Option<Confidence>,
    pub basis: Option<Basis>,
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
    pub author: Option<Author>,
    pub run: Option<RunId>,
    pub expected_revision: Option<i64>,
    pub confidence: Option<Confidence>,
    pub basis: Option<Basis>,
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
    pub revision: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    // When someone last checked the note against reality, and who.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_by: Option<String>,
    // True for the closing summary of the task this note sits under.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub summary: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
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
    pub tasks: Vec<TaskBrief>,
}

// An unfinished goal, with what is needed to decide whether to continue it.
#[derive(Debug, Serialize)]
pub struct TaskBrief {
    pub id: NoteId,
    pub project: String,
    pub status: Status,
    pub title: String,
    pub checkpoint_at: Option<String>,
    pub changes_since_checkpoint: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCheckpoint {
    pub summary: Body,
    pub project: Option<ProjectName>,
    pub author: Option<Author>,
    pub run: Option<RunId>,
    pub task: Option<NoteId>,
}

#[derive(Debug, Serialize)]
pub struct Checkpoint {
    pub id: i64,
    pub project: String,
    pub summary: String,
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<NoteId>,
    pub created_at: String,
    #[serde(skip)]
    pub journal_id: i64,
}
