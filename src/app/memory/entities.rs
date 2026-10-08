use rusqlite::types::{Type, Value as Sql};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::db::{Db, Tree};
use super::journal::{self, Entry, RecordType};
use super::model::{
    AtMost, Attribution, Author, EntityId, Limit, MemoryError, NoteId, NoteKind, ProjectName,
    RunId, Scope, Status, Title, Token, invalid, now, seconds_from_now, string_enum,
    validated_string,
};
use super::notes::check_task;

const DEFAULT_STATUS: &str = "new";
const MAX_KEY_CHARS: usize = 256;
const MAX_ATTR_NAME_CHARS: usize = 48;
const MAX_ATTR_TEXT_CHARS: usize = 1024;
const MAX_ATTRS_PER_CALL: usize = 32;
const MAX_ATTRS_PER_ENTITY: usize = 64;
const MAX_IN_VALUES: usize = 32;
const DEFAULT_CLAIM_SECONDS: u32 = 15 * 60;
const MAX_CLAIM_SECONDS: u32 = 24 * 60 * 60;
const DEFAULT_QUERY_LIMIT: usize = 20;
const MAX_GROUPS: i64 = 100;
const MAX_LISTED: i64 = 100;
const MAX_EVENTS_LISTED: i64 = 20;
const MAX_GRAPH_NODES: usize = 500;
const MAX_GRAPH_EDGES: usize = 1000;

type Result<T> = std::result::Result<T, MemoryError>;

validated_string!(EntityKey, validate_key);

fn validate_key(value: String) -> std::result::Result<String, String> {
    let key = value.trim();
    if key.is_empty() || key.chars().count() > MAX_KEY_CHARS || key.chars().any(char::is_control) {
        return Err(format!(
            "key must be 1-{MAX_KEY_CHARS} characters on one line"
        ));
    }
    Ok(key.to_string())
}

fn is_attr_name(name: &str) -> bool {
    let mut chars = name.chars();
    let starts_well = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    starts_well
        && name.len() <= MAX_ATTR_NAME_CHARS
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

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

    fn try_from(value: f64) -> std::result::Result<Self, String> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err("confidence must be between 0 and 1".into())
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
pub struct ClaimId(Uuid);

impl TryFrom<String> for ClaimId {
    type Error = String;

    fn try_from(value: String) -> std::result::Result<Self, String> {
        Uuid::parse_str(&value)
            .map(Self)
            .map_err(|_| "claim_id must be a UUID".to_string())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(try_from = "Map<String, Value>")]
pub struct AttrsPatch(Map<String, Value>);

impl TryFrom<Map<String, Value>> for AttrsPatch {
    type Error = String;

    fn try_from(attrs: Map<String, Value>) -> std::result::Result<Self, String> {
        if attrs.len() > MAX_ATTRS_PER_CALL {
            return Err(format!("at most {MAX_ATTRS_PER_CALL} attributes per call"));
        }
        for (name, value) in &attrs {
            if !is_attr_name(name) {
                return Err(format!(
                    "attribute name '{name}' must be 1-{MAX_ATTR_NAME_CHARS} ASCII letters, digits or '_', not starting with a digit"
                ));
            }
            if !value.is_null() {
                scalar(value).map_err(|problem| format!("attribute '{name}': {problem}"))?;
            }
        }
        Ok(Self(attrs))
    }
}

fn scalar(value: &Value) -> std::result::Result<Sql, String> {
    match value {
        Value::Bool(flag) => Ok(Sql::Integer(i64::from(*flag))),
        Value::String(text) if text.chars().count() <= MAX_ATTR_TEXT_CHARS => {
            Ok(Sql::Text(text.clone()))
        }
        Value::String(_) => Err(format!(
            "text is longer than {MAX_ATTR_TEXT_CHARS} characters"
        )),
        Value::Number(number) => match (number.as_i64(), number.as_f64()) {
            (Some(integer), _) => Ok(Sql::Integer(integer)),
            (None, Some(real)) => Ok(Sql::Real(real)),
            (None, None) => Err("number is out of range".into()),
        },
        Value::Null | Value::Array(_) | Value::Object(_) => {
            Err("value must be a string, a number or a boolean".into())
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityUpsert {
    #[serde(rename = "type")]
    pub kind: Token,
    pub key: EntityKey,
    pub project: Option<ProjectName>,
    pub status: Option<Token>,
    pub confidence: Option<Confidence>,
    pub parent: Option<EntityId>,
    #[serde(default)]
    pub attrs: AttrsPatch,
    pub author: Option<Author>,
    pub run: Option<RunId>,
    pub task: Option<NoteId>,
    pub claim_id: Option<ClaimId>,
}

impl EntityUpsert {
    fn who(&self) -> Attribution {
        Attribution::new(self.author.clone(), self.run.clone(), self.task)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRequest {
    pub id: EntityId,
    pub ttl_seconds: Option<u32>,
    pub author: Option<Author>,
    pub run: Option<RunId>,
    pub claim_id: Option<ClaimId>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRequest {
    pub id: EntityId,
    pub claim_id: ClaimId,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckMark {
    pub id: EntityId,
    pub name: Token,
    pub result: Token,
    pub detail: Option<Title>,
    pub author: Option<Author>,
    pub run: Option<RunId>,
    pub task: Option<NoteId>,
    pub claim_id: Option<ClaimId>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityLink {
    pub src: EntityId,
    pub dst: EntityId,
    pub kind: Token,
    #[serde(default)]
    pub remove: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgetEntity {
    pub id: EntityId,
    #[serde(default)]
    pub recursive: bool,
    pub claim_id: Option<ClaimId>,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

string_enum!(Column {
    Id => "id",
    Type => "type",
    Key => "key",
    Status => "status",
    Confidence => "confidence",
    Parent => "parent",
    Author => "author",
    CreatedAt => "created_at",
    UpdatedAt => "updated_at",
});

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub enum Field {
    Column(Column),
    Attr(String),
    Check(Token),
    Claimed,
}

impl TryFrom<String> for Field {
    type Error = String;

    fn try_from(name: String) -> std::result::Result<Self, String> {
        if name == "claimed" {
            return Ok(Self::Claimed);
        }
        if let Some(column) = Column::parse(&name) {
            return Ok(Self::Column(column));
        }
        if let Some(attr) = name
            .strip_prefix("attrs.")
            .filter(|attr| is_attr_name(attr))
        {
            return Ok(Self::Attr(attr.to_string()));
        }
        if let Some(check) = name.strip_prefix("check.") {
            return Token::try_from(check.to_string()).map(Self::Check);
        }
        Err(format!(
            "unknown field '{name}'; use a column ({}), attrs.<name>, check.<name> or claimed",
            Column::ALL
                .iter()
                .map(|column| column.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

#[derive(Default)]
struct Fragment {
    sql: String,
    values: Vec<Sql>,
}

impl Fragment {
    fn push(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    fn bind(&mut self, sql: &str, value: Sql) {
        self.sql.push_str(sql);
        self.values.push(value);
    }

    fn append(&mut self, other: Fragment) {
        self.sql.push_str(&other.sql);
        self.values.extend(other.values);
    }
}

impl Field {
    // All SQL text here is constant. Names from the caller are bound, never spliced.
    fn expression(&self, now: &str) -> Fragment {
        let mut fragment = Fragment::default();
        match self {
            Self::Column(column) => fragment.push(match column {
                Column::Id => "e.id",
                Column::Type => "e.type",
                Column::Key => "e.key",
                Column::Status => "e.status",
                Column::Confidence => "e.confidence",
                Column::Parent => "e.parent_id",
                Column::Author => "e.author",
                Column::CreatedAt => "e.created_at",
                Column::UpdatedAt => "e.updated_at",
            }),
            Self::Attr(name) => {
                fragment.bind("json_extract(e.attrs, ?)", Sql::Text(format!("$.{name}")));
            }
            Self::Check(name) => fragment.bind(
                "(SELECT c.result FROM entity_checks c WHERE c.entity_id = e.id AND c.name = ?)",
                Sql::Text(name.as_str().to_string()),
            ),
            Self::Claimed => fragment.bind(
                "(e.claim_id IS NOT NULL AND e.claim_expires_at > ?)",
                Sql::Text(now.to_string()),
            ),
        }
        fragment
    }
}

string_enum!(Op {
    Eq => "eq",
    Ne => "ne",
    Gt => "gt",
    Gte => "gte",
    Lt => "lt",
    Lte => "lte",
    In => "in",
    Contains => "contains",
    IsNull => "is_null",
    NotNull => "not_null",
});

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCondition {
    field: Field,
    op: Op,
    #[serde(default)]
    value: Value,
}

#[derive(Debug)]
enum Test {
    Compare(&'static str, Sql),
    In(Vec<Sql>),
    Contains(String),
    IsNull,
    NotNull,
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "RawCondition")]
pub struct Condition {
    field: Field,
    test: Test,
}

impl TryFrom<RawCondition> for Condition {
    type Error = String;

    fn try_from(raw: RawCondition) -> std::result::Result<Self, String> {
        let compare = |operator| scalar(&raw.value).map(|value| Test::Compare(operator, value));
        let test = match raw.op {
            Op::Eq => compare("=")?,
            // IS NOT also matches rows where the field is absent.
            Op::Ne => compare("IS NOT")?,
            Op::Gt => compare(">")?,
            Op::Gte => compare(">=")?,
            Op::Lt => compare("<")?,
            Op::Lte => compare("<=")?,
            Op::In => match &raw.value {
                Value::Array(items) if (1..=MAX_IN_VALUES).contains(&items.len()) => Test::In(
                    items
                        .iter()
                        .map(scalar)
                        .collect::<std::result::Result<_, _>>()?,
                ),
                _ => return Err(format!("'in' needs an array of 1-{MAX_IN_VALUES} values")),
            },
            Op::Contains => match scalar(&raw.value)? {
                Sql::Text(text) if !text.is_empty() => Test::Contains(text),
                _ => return Err("'contains' needs a non-empty string".into()),
            },
            Op::IsNull | Op::NotNull if !raw.value.is_null() => {
                return Err(format!("'{}' takes no value", raw.op));
            }
            Op::IsNull => Test::IsNull,
            Op::NotNull => Test::NotNull,
        };
        Ok(Self {
            field: raw.field,
            test,
        })
    }
}

impl Condition {
    fn sql(&self, now: &str) -> Fragment {
        let mut fragment = self.field.expression(now);
        match &self.test {
            Test::Compare(operator, value) => {
                fragment.bind(&format!(" {operator} ?"), value.clone());
            }
            Test::In(values) => {
                let placeholders = vec!["?"; values.len()].join(", ");
                fragment.push(&format!(" IN ({placeholders})"));
                fragment.values.extend(values.iter().cloned());
            }
            Test::Contains(text) => {
                let mut wrapped = Fragment::default();
                wrapped.push("instr(");
                wrapped.append(fragment);
                wrapped.bind(", ?) > 0", Sql::Text(text.clone()));
                fragment = wrapped;
            }
            Test::IsNull => fragment.push(" IS NULL"),
            Test::NotNull => fragment.push(" IS NOT NULL"),
        }
        fragment
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityQuery {
    pub project: Option<Scope>,
    #[serde(rename = "type")]
    pub kind: Option<Token>,
    #[serde(default, rename = "where")]
    pub conditions: AtMost<Condition, 16>,
    pub order_by: Option<Field>,
    #[serde(default)]
    pub descending: bool,
    pub group_by: Option<Field>,
    pub limit: Option<Limit>,
}

#[derive(Debug, Serialize)]
pub struct Entity {
    pub id: EntityId,
    pub project: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub key: String,
    pub parent: Option<EntityId>,
    pub status: String,
    pub confidence: Option<f64>,
    pub attrs: Value,
    pub checks: Value,
    pub author: Option<String>,
    pub claimed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_until: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct Upserted {
    pub outcome: &'static str,
    #[serde(flatten)]
    pub entity: Entity,
}

#[derive(Debug, Serialize)]
pub struct ClaimGrant {
    pub id: EntityId,
    pub claim_id: String,
    pub claimed_until: String,
}

#[derive(Debug, Serialize)]
pub struct EntityBrief {
    pub id: EntityId,
    #[serde(rename = "type")]
    pub kind: String,
    pub key: String,
    pub status: String,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct EntityEdge {
    pub src: EntityId,
    pub dst: EntityId,
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub result: String,
    pub detail: Option<String>,
    pub author: Option<String>,
    pub checked_at: String,
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub event: String,
    pub detail: Value,
    pub author: Option<String>,
    pub at: String,
}

#[derive(Debug, Serialize)]
pub struct NoteRef {
    pub id: NoteId,
    pub kind: NoteKind,
    pub status: Status,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct EntityDetail {
    pub entity: Entity,
    pub checks: Vec<Check>,
    pub children: Vec<EntityBrief>,
    pub links: Vec<EntityEdge>,
    pub notes: Vec<NoteRef>,
    pub events: Vec<Event>,
}

#[derive(Debug, Serialize)]
pub struct Group {
    pub value: Value,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<Entity>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<Group>>,
}

#[derive(Debug, Serialize)]
pub struct EntityNode {
    pub id: EntityId,
    pub parent: Option<EntityId>,
    pub depth: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub key: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct EntityGraph {
    pub nodes: Vec<EntityNode>,
    pub edges: Vec<EntityEdge>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct TypeCount {
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub count: i64,
}

#[derive(Clone)]
pub struct EntityStore {
    db: Db,
}

impl EntityStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn upsert(
        &self,
        input: EntityUpsert,
        default_project: ProjectName,
    ) -> Result<Upserted> {
        self.db
            .run(move |conn| upsert(conn, input, default_project))
            .await
    }

    pub async fn get(&self, id: EntityId) -> Result<EntityDetail> {
        self.db.run(move |conn| detail(conn, id)).await
    }

    pub async fn query(&self, query: EntityQuery, default_scope: Scope) -> Result<QueryResult> {
        self.db
            .run(move |conn| run_query(conn, &query, &default_scope))
            .await
    }

    pub async fn claim(&self, request: ClaimRequest) -> Result<ClaimGrant> {
        self.db.run(move |conn| claim(conn, request)).await
    }

    pub async fn release(&self, request: ReleaseRequest) -> Result<bool> {
        self.db.run(move |conn| release(conn, request)).await
    }

    pub async fn mark_check(&self, mark: CheckMark) -> Result<Entity> {
        self.db.run(move |conn| mark_check(conn, mark)).await
    }

    pub async fn link(&self, link: EntityLink) -> Result<bool> {
        self.db.run(move |conn| set_link(conn, link)).await
    }

    pub async fn graph(&self, root: EntityId, max_depth: u8) -> Result<EntityGraph> {
        self.db.run(move |conn| graph(conn, root, max_depth)).await
    }

    pub async fn summary(&self, scope: Scope) -> Result<Vec<TypeCount>> {
        self.db.run(move |conn| summary(conn, &scope)).await
    }

    pub async fn forget(&self, request: ForgetEntity) -> Result<i64> {
        self.db.run(move |conn| forget(conn, request)).await
    }
}

const ENTITY_SELECT: &str = "\
    SELECT e.id, e.project, e.type, e.key, e.parent_id, e.status, e.confidence, e.attrs,
           (SELECT json_group_object(c.name, c.result) FROM entity_checks c WHERE c.entity_id = e.id),
           e.author, e.claim_id, e.claimed_by, e.claim_expires_at, e.created_at, e.updated_at
    FROM entities e";

fn json_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Value> {
    let text: String = row.get(index)?;
    serde_json::from_str(&text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
    })
}

// An expired claim reads as no claim. claim_id never leaves the store.
fn entity_from_row(row: &Row<'_>, now: &str) -> rusqlite::Result<Entity> {
    let claim_id: Option<String> = row.get(10)?;
    let claimed_until: Option<String> = row.get(12)?;
    let claimed = claim_id.is_some() && claimed_until.as_deref().is_some_and(|until| until > now);
    Ok(Entity {
        id: row.get(0)?,
        project: row.get(1)?,
        kind: row.get(2)?,
        key: row.get(3)?,
        parent: row.get(4)?,
        status: row.get(5)?,
        confidence: row.get(6)?,
        attrs: json_column(row, 7)?,
        checks: json_column(row, 8)?,
        author: row.get(9)?,
        claimed,
        claimed_by: if claimed { row.get(11)? } else { None },
        claimed_until: claimed_until.filter(|_| claimed),
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn not_found(id: EntityId) -> MemoryError {
    MemoryError::NotFound(format!("entity {id}"))
}

fn load(conn: &Connection, id: EntityId) -> Result<Entity> {
    let now = now();
    conn.query_row(&format!("{ENTITY_SELECT} WHERE e.id = ?1"), [id], |row| {
        entity_from_row(row, &now)
    })
    .optional()?
    .ok_or_else(|| not_found(id))
}

fn project_of(conn: &Connection, id: EntityId) -> Result<String> {
    conn.query_row("SELECT project FROM entities WHERE id = ?1", [id], |row| {
        row.get(0)
    })
    .optional()?
    .ok_or_else(|| not_found(id))
}

fn author_text(author: &Option<Author>) -> Option<&str> {
    author.as_ref().map(Author::as_str)
}

// The event keeps the values and goes when the entity is deleted. The journal
// entry names the fields only and stays.
fn log_event(
    conn: &Connection,
    entity: EntityId,
    op: journal::Op,
    detail: Value,
    who: &Attribution,
) -> Result<()> {
    let project = project_of(conn, entity)?;
    if let Some(task) = who.task {
        check_task(conn, task, &project)?;
    }
    conn.execute(
        "INSERT INTO entity_events (entity_id, event, detail, author, at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![entity, op, detail.to_string(), who.author, now()],
    )?;
    let event = conn.last_insert_rowid();
    journal::record(
        conn,
        Entry::new(&project, RecordType::Entity, entity, op, who)
            .fields(changed_fields(op, &detail))
            .event(event),
    )?;
    Ok(())
}

fn changed_fields(op: journal::Op, detail: &Value) -> Vec<String> {
    let Some(detail) = detail.as_object() else {
        return Vec::new();
    };
    match op {
        journal::Op::Updated => detail
            .iter()
            .flat_map(
                |(field, change)| match (field.as_str(), change.as_object()) {
                    ("attrs", Some(attrs)) => {
                        attrs.keys().map(|name| format!("attrs.{name}")).collect()
                    }
                    _ => vec![field.clone()],
                },
            )
            .collect(),
        journal::Op::Checked => detail
            .get("name")
            .and_then(Value::as_str)
            .map(|name| format!("check.{name}"))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

struct Claim {
    id: String,
    by: Option<String>,
    until: String,
}

fn active_claim(conn: &Connection, entity: EntityId) -> Result<Option<Claim>> {
    Ok(conn
        .query_row(
            "SELECT claim_id, claimed_by, claim_expires_at FROM entities
             WHERE id = ?1 AND claim_id IS NOT NULL AND claim_expires_at > ?2",
            params![entity, now()],
            |row| {
                Ok(Claim {
                    id: row.get(0)?,
                    by: row.get(1)?,
                    until: row.get(2)?,
                })
            },
        )
        .optional()?)
}

fn claim_conflict(entity: EntityId, claim: &Claim) -> MemoryError {
    let holder = claim
        .by
        .as_deref()
        .map(|by| format!(" by {by}"))
        .unwrap_or_default();
    MemoryError::Conflict(format!(
        "entity {entity} is claimed{holder} until {}; changing it needs that claim's claim_id",
        claim.until
    ))
}

fn require_claim(conn: &Connection, entity: EntityId, presented: Option<ClaimId>) -> Result<()> {
    match active_claim(conn, entity)? {
        Some(claim) if !holds(presented, &claim) => Err(claim_conflict(entity, &claim)),
        _ => Ok(()),
    }
}

fn holds(presented: Option<ClaimId>, claim: &Claim) -> bool {
    presented.is_some_and(|presented| presented.0.to_string() == claim.id)
}

fn upsert(
    conn: &mut Connection,
    input: EntityUpsert,
    default_project: ProjectName,
) -> Result<Upserted> {
    let project: String = input.project.clone().unwrap_or(default_project).into();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing: Option<EntityId> = tx
        .query_row(
            "SELECT id FROM entities WHERE project = ?1 AND type = ?2 AND key = ?3",
            params![project, input.kind.as_str(), input.key.as_str()],
            |row| row.get(0),
        )
        .optional()?;

    let (id, outcome) = match existing {
        None => (insert(&tx, &project, &input)?, "created"),
        Some(id) if apply_changes(&tx, id, &project, &input)? => (id, "updated"),
        Some(id) => (id, "unchanged"),
    };
    let entity = load(&tx, id)?;
    tx.commit()?;
    Ok(Upserted { outcome, entity })
}

fn check_parent_project(conn: &Connection, parent: EntityId, project: &str) -> Result<()> {
    let parent_project = project_of(conn, parent)?;
    if parent_project != project {
        return Err(invalid(format!(
            "parent {parent} belongs to project '{parent_project}', the entity to '{project}'"
        )));
    }
    Ok(())
}

fn insert(conn: &Connection, project: &str, input: &EntityUpsert) -> Result<EntityId> {
    if let Some(parent) = input.parent {
        check_parent_project(conn, parent, project)?;
        Tree::Entities.check_depth(conn, parent, None)?;
    }
    let attrs: Map<String, Value> = input
        .attrs
        .0
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let now = now();
    conn.execute(
        "INSERT INTO entities
             (project, type, key, parent_id, status, confidence, attrs, author, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            project,
            input.kind.as_str(),
            input.key.as_str(),
            input.parent,
            input.status.as_ref().map_or(DEFAULT_STATUS, Token::as_str),
            input.confidence.map(|confidence| confidence.0),
            Value::Object(attrs).to_string(),
            author_text(&input.author),
            now,
        ],
    )?;
    let id = conn.last_insert_rowid();
    log_event(conn, id, journal::Op::Created, json!({}), &input.who())?;
    Ok(id)
}

// Returns false when nothing differs, so identical writes leave no trace.
fn apply_changes(
    conn: &Connection,
    id: EntityId,
    project: &str,
    input: &EntityUpsert,
) -> Result<bool> {
    let current = load(conn, id)?;
    let Value::Object(mut attrs) = current.attrs else {
        return Err(anyhow::anyhow!("entity {id} has attrs that are not an object").into());
    };
    let mut changes = Map::new();

    let status = input
        .status
        .as_ref()
        .map_or(current.status.as_str(), Token::as_str);
    if status != current.status {
        changes.insert("status".into(), json!([current.status, status]));
    }
    let confidence = input
        .confidence
        .map(|confidence| confidence.0)
        .or(current.confidence);
    if confidence != current.confidence {
        changes.insert("confidence".into(), json!([current.confidence, confidence]));
    }
    let parent = input.parent.or(current.parent);
    if parent != current.parent {
        changes.insert("parent".into(), json!([current.parent, parent]));
    }
    let mut changed_attrs = Map::new();
    for (name, value) in &input.attrs.0 {
        let changed = if value.is_null() {
            attrs.remove(name).is_some()
        } else {
            attrs.insert(name.clone(), value.clone()).as_ref() != Some(value)
        };
        if changed {
            changed_attrs.insert(name.clone(), value.clone());
        }
    }
    if !changed_attrs.is_empty() {
        changes.insert("attrs".into(), Value::Object(changed_attrs));
    }
    if changes.is_empty() {
        return Ok(false);
    }

    require_claim(conn, id, input.claim_id)?;
    if attrs.len() > MAX_ATTRS_PER_ENTITY {
        return Err(invalid(format!(
            "an entity holds at most {MAX_ATTRS_PER_ENTITY} attributes"
        )));
    }
    if let Some(new_parent) = input.parent.filter(|_| parent != current.parent) {
        check_parent_project(conn, new_parent, project)?;
        if Tree::Entities.contains(conn, id, new_parent)? {
            return Err(MemoryError::Conflict(format!(
                "entity {new_parent} is entity {id} or one of its descendants; this move would close a cycle"
            )));
        }
        Tree::Entities.check_depth(conn, new_parent, Some(id))?;
    }

    conn.execute(
        "UPDATE entities
         SET status = ?2, confidence = ?3, parent_id = ?4, attrs = ?5, updated_at = ?6
         WHERE id = ?1",
        params![
            id,
            status,
            confidence,
            parent,
            Value::Object(attrs).to_string(),
            now()
        ],
    )?;
    log_event(
        conn,
        id,
        journal::Op::Updated,
        Value::Object(changes),
        &input.who(),
    )?;
    Ok(true)
}

// Immediate transaction: of two simultaneous claims, one wins.
fn claim(conn: &mut Connection, request: ClaimRequest) -> Result<ClaimGrant> {
    let ttl = request.ttl_seconds.unwrap_or(DEFAULT_CLAIM_SECONDS);
    if !(1..=MAX_CLAIM_SECONDS).contains(&ttl) {
        return Err(invalid(format!(
            "ttl_seconds must be between 1 and {MAX_CLAIM_SECONDS}"
        )));
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    project_of(&tx, request.id)?;
    let claim_id = match active_claim(&tx, request.id)? {
        Some(current) if holds(request.claim_id, &current) => current.id,
        Some(current) => return Err(claim_conflict(request.id, &current)),
        None => Uuid::new_v4().to_string(),
    };
    let claimed_until = seconds_from_now(i64::from(ttl));
    tx.execute(
        "UPDATE entities SET claim_id = ?2, claimed_by = ?3, claim_expires_at = ?4 WHERE id = ?1",
        params![
            request.id,
            claim_id,
            author_text(&request.author),
            claimed_until
        ],
    )?;
    log_event(
        &tx,
        request.id,
        journal::Op::Claimed,
        json!({ "until": claimed_until }),
        &Attribution::new(request.author.clone(), request.run.clone(), None),
    )?;
    tx.commit()?;
    Ok(ClaimGrant {
        id: request.id,
        claim_id,
        claimed_until,
    })
}

fn release(conn: &mut Connection, request: ReleaseRequest) -> Result<bool> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    project_of(&tx, request.id)?;
    let Some(current) = active_claim(&tx, request.id)? else {
        return Ok(false);
    };
    if !holds(Some(request.claim_id), &current) {
        return Err(claim_conflict(request.id, &current));
    }
    tx.execute(
        "UPDATE entities SET claim_id = NULL, claimed_by = NULL, claim_expires_at = NULL WHERE id = ?1",
        [request.id],
    )?;
    log_event(
        &tx,
        request.id,
        journal::Op::Released,
        json!({}),
        &Attribution::new(request.author, request.run, None),
    )?;
    tx.commit()?;
    Ok(true)
}

fn mark_check(conn: &mut Connection, mark: CheckMark) -> Result<Entity> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    project_of(&tx, mark.id)?;
    require_claim(&tx, mark.id, mark.claim_id)?;
    let now = now();
    tx.execute(
        "INSERT INTO entity_checks (entity_id, name, result, detail, author, checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (entity_id, name) DO UPDATE SET
             result = excluded.result, detail = excluded.detail,
             author = excluded.author, checked_at = excluded.checked_at",
        params![
            mark.id,
            mark.name.as_str(),
            mark.result.as_str(),
            mark.detail.as_ref().map(Title::as_str),
            author_text(&mark.author),
            now,
        ],
    )?;
    tx.execute(
        "UPDATE entities SET updated_at = ?2 WHERE id = ?1",
        params![mark.id, now],
    )?;
    let detail = json!({ "name": mark.name.as_str(), "result": mark.result.as_str() });
    log_event(
        &tx,
        mark.id,
        journal::Op::Checked,
        detail,
        &Attribution::new(mark.author.clone(), mark.run.clone(), mark.task),
    )?;
    let entity = load(&tx, mark.id)?;
    tx.commit()?;
    Ok(entity)
}

fn set_link(conn: &mut Connection, link: EntityLink) -> Result<bool> {
    if link.src == link.dst {
        return Err(invalid("an entity cannot link to itself"));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    project_of(&tx, link.src)?;
    project_of(&tx, link.dst)?;
    let (changed, op) = if link.remove {
        let removed = tx.execute(
            "DELETE FROM entity_edges WHERE src = ?1 AND dst = ?2 AND kind = ?3",
            params![link.src, link.dst, link.kind.as_str()],
        )?;
        (removed, journal::Op::Unlinked)
    } else {
        let added = tx.execute(
            "INSERT OR IGNORE INTO entity_edges (src, dst, kind, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![link.src, link.dst, link.kind.as_str(), now()],
        )?;
        (added, journal::Op::Linked)
    };
    if changed > 0 {
        let project = project_of(&tx, link.src)?;
        journal::record(
            &tx,
            Entry::new(
                &project,
                RecordType::Entity,
                link.src,
                op,
                &Attribution::default(),
            )
            .fields([format!("{}:{}", link.kind.as_str(), link.dst)]),
        )?;
    }
    tx.commit()?;
    Ok(!link.remove)
}

fn detail(conn: &Connection, id: EntityId) -> Result<EntityDetail> {
    let entity = load(conn, id)?;

    let checks = conn
        .prepare(
            "SELECT name, result, detail, author, checked_at FROM entity_checks
             WHERE entity_id = ?1 ORDER BY name LIMIT ?2",
        )?
        .query_map(params![id, MAX_LISTED], |row| {
            Ok(Check {
                name: row.get(0)?,
                result: row.get(1)?,
                detail: row.get(2)?,
                author: row.get(3)?,
                checked_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let children = conn
        .prepare(
            "SELECT id, type, key, status FROM entities WHERE parent_id = ?1 ORDER BY id LIMIT ?2",
        )?
        .query_map(params![id, MAX_LISTED], |row| {
            Ok(EntityBrief {
                id: row.get(0)?,
                kind: row.get(1)?,
                key: row.get(2)?,
                status: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let links = conn
        .prepare(
            "SELECT src, dst, kind FROM entity_edges WHERE src = ?1 OR dst = ?1
             ORDER BY src, dst, kind LIMIT ?2",
        )?
        .query_map(params![id, MAX_LISTED], edge_from_row)?
        .collect::<rusqlite::Result<_>>()?;

    let notes = conn
        .prepare(
            "SELECT id, kind, status, title FROM notes WHERE entity_id = ?1
             ORDER BY updated_at DESC, id DESC LIMIT ?2",
        )?
        .query_map(params![id, MAX_LISTED], |row| {
            Ok(NoteRef {
                id: row.get(0)?,
                kind: row.get(1)?,
                status: row.get(2)?,
                title: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let events = conn
        .prepare(
            "SELECT event, detail, author, at FROM entity_events WHERE entity_id = ?1
             ORDER BY id DESC LIMIT ?2",
        )?
        .query_map(params![id, MAX_EVENTS_LISTED], |row| {
            Ok(Event {
                event: row.get(0)?,
                detail: json_column(row, 1)?,
                author: row.get(2)?,
                at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    Ok(EntityDetail {
        entity,
        checks,
        children,
        links,
        notes,
        events,
    })
}

fn edge_from_row(row: &Row<'_>) -> rusqlite::Result<EntityEdge> {
    Ok(EntityEdge {
        src: row.get(0)?,
        dst: row.get(1)?,
        kind: row.get(2)?,
    })
}

fn filter(query: &EntityQuery, default_scope: &Scope, now: &str) -> Fragment {
    let mut fragment = Fragment::default();
    fragment.push(" WHERE TRUE");
    if let Some(project) = query.project.as_ref().unwrap_or(default_scope).project() {
        fragment.bind(" AND e.project = ?", Sql::Text(project.to_string()));
    }
    if let Some(kind) = &query.kind {
        fragment.bind(" AND e.type = ?", Sql::Text(kind.as_str().to_string()));
    }
    for condition in query.conditions.as_slice() {
        fragment.push(" AND (");
        fragment.append(condition.sql(now));
        fragment.push(")");
    }
    fragment
}

fn run_query(conn: &Connection, query: &EntityQuery, default_scope: &Scope) -> Result<QueryResult> {
    let now = now();

    let mut count = Fragment::default();
    count.push("SELECT count(*) FROM entities e");
    count.append(filter(query, default_scope, &now));
    let total = conn.query_row(&count.sql, params_from_iter(count.values), |row| row.get(0))?;

    if let Some(field) = &query.group_by {
        let mut grouped = Fragment::default();
        grouped.push("SELECT ");
        grouped.append(field.expression(&now));
        grouped.push(", count(*) FROM entities e");
        grouped.append(filter(query, default_scope, &now));
        grouped.bind(
            " GROUP BY 1 ORDER BY 2 DESC, 1 LIMIT ?",
            Sql::Integer(MAX_GROUPS),
        );
        let groups = conn
            .prepare(&grouped.sql)?
            .query_map(params_from_iter(grouped.values), |row| {
                Ok(Group {
                    value: json_scalar(row.get(0)?),
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        return Ok(QueryResult {
            total,
            entities: None,
            groups: Some(groups),
        });
    }

    let mut listing = Fragment::default();
    listing.push(ENTITY_SELECT);
    listing.append(filter(query, default_scope, &now));
    listing.push(" ORDER BY ");
    match &query.order_by {
        Some(field) => listing.append(field.expression(&now)),
        None => listing.push("e.updated_at"),
    }
    // Default order: most recently touched first.
    let descending = query.descending || query.order_by.is_none();
    listing.push(if descending {
        " DESC, e.id DESC"
    } else {
        " ASC, e.id ASC"
    });
    listing.bind(
        " LIMIT ?",
        Sql::Integer(Limit::or(query.limit, DEFAULT_QUERY_LIMIT) as i64),
    );
    let entities = conn
        .prepare(&listing.sql)?
        .query_map(params_from_iter(listing.values), |row| {
            entity_from_row(row, &now)
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(QueryResult {
        total,
        entities: Some(entities),
        groups: None,
    })
}

fn json_scalar(value: Sql) -> Value {
    match value {
        Sql::Null | Sql::Blob(_) => Value::Null,
        Sql::Integer(integer) => json!(integer),
        Sql::Real(real) => json!(real),
        Sql::Text(text) => json!(text),
    }
}

fn graph(conn: &Connection, root: EntityId, max_depth: u8) -> Result<EntityGraph> {
    project_of(conn, root)?;

    // Zero padded path sorts rows in depth first order.
    let mut nodes: Vec<EntityNode> = conn
        .prepare(
            "WITH RECURSIVE tree (id, depth, path) AS (
                 SELECT id, 0, printf('%019d', id) FROM entities WHERE id = ?1
                 UNION ALL
                 SELECT c.id, tree.depth + 1, tree.path || printf('/%019d', c.id)
                 FROM entities c JOIN tree ON c.parent_id = tree.id
                 WHERE tree.depth < ?2
             )
             SELECT e.id, e.parent_id, tree.depth, e.type, e.key, e.status
             FROM tree JOIN entities e ON e.id = tree.id
             ORDER BY tree.path
             LIMIT ?3",
        )?
        .query_map(
            params![root, max_depth, MAX_GRAPH_NODES as i64 + 1],
            |row| {
                Ok(EntityNode {
                    id: row.get(0)?,
                    parent: row.get(1)?,
                    depth: row.get(2)?,
                    kind: row.get(3)?,
                    key: row.get(4)?,
                    status: row.get(5)?,
                })
            },
        )?
        .collect::<rusqlite::Result<_>>()?;

    let mut edges: Vec<EntityEdge> = conn
        .prepare(
            "WITH RECURSIVE sub (id) AS (
                 SELECT ?1
                 UNION
                 SELECT c.id FROM entities c JOIN sub ON c.parent_id = sub.id
             )
             SELECT src, dst, kind FROM entity_edges
             WHERE src IN (SELECT id FROM sub) OR dst IN (SELECT id FROM sub)
             ORDER BY src, dst, kind
             LIMIT ?2",
        )?
        .query_map(params![root, MAX_GRAPH_EDGES as i64 + 1], edge_from_row)?
        .collect::<rusqlite::Result<_>>()?;

    let truncated = nodes.len() > MAX_GRAPH_NODES || edges.len() > MAX_GRAPH_EDGES;
    nodes.truncate(MAX_GRAPH_NODES);
    edges.truncate(MAX_GRAPH_EDGES);
    Ok(EntityGraph {
        nodes,
        edges,
        truncated,
    })
}

fn summary(conn: &Connection, scope: &Scope) -> Result<Vec<TypeCount>> {
    Ok(conn
        .prepare(
            "SELECT type, status, count(*) FROM entities
             WHERE (?1 IS NULL OR project = ?1)
             GROUP BY type, status ORDER BY type, status LIMIT ?2",
        )?
        .query_map(params![scope.project(), MAX_GROUPS], |row| {
            Ok(TypeCount {
                kind: row.get(0)?,
                status: row.get(1)?,
                count: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn forget(conn: &mut Connection, request: ForgetEntity) -> Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    project_of(&tx, request.id)?;
    require_claim(&tx, request.id, request.claim_id)?;
    let doomed: Vec<(EntityId, String)> = tx
        .prepare(
            "WITH RECURSIVE sub (id) AS (
                 SELECT ?1
                 UNION
                 SELECT c.id FROM entities c JOIN sub ON c.parent_id = sub.id
             )
             SELECT e.id, e.project FROM sub JOIN entities e ON e.id = sub.id",
        )?
        .query_map([request.id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let subtree_size = doomed.len() as i64;
    if subtree_size > 1 && !request.recursive {
        return Err(MemoryError::Conflict(format!(
            "entity {} has {} entities beneath it; pass recursive=true to delete them too",
            request.id,
            subtree_size - 1
        )));
    }
    let who = Attribution::new(request.author, request.run, None);
    for (entity, project) in &doomed {
        journal::record(
            &tx,
            Entry::new(
                project,
                RecordType::Entity,
                entity,
                journal::Op::Purged,
                &who,
            ),
        )?;
    }
    // Children, edges, checks and events go through ON DELETE CASCADE.
    tx.execute("DELETE FROM entities WHERE id = ?1", [request.id])?;
    tx.commit()?;
    Ok(subtree_size)
}

#[cfg(test)]
#[path = "tests/entities.rs"]
mod tests;
