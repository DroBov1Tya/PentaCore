CREATE TABLE IF NOT EXISTS targets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    domain      TEXT UNIQUE NOT NULL,
    description TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS scopes (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id    INTEGER NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    objective    TEXT NOT NULL,
    in_scope     TEXT NOT NULL,
    out_of_scope TEXT,
    rules        TEXT,
    created_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS target_relations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    from_id     INTEGER NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    to_id       INTEGER NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    type        TEXT NOT NULL CHECK(type IN ('subdomain','cdn','shared_infra','pivot','related')),
    description TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(from_id, to_id, type)
);

CREATE TABLE IF NOT EXISTS credentials (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id   INTEGER NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    type        TEXT NOT NULL CHECK(type IN ('basic','cookie','token','apikey','other')),
    username    TEXT,
    secret      TEXT NOT NULL,
    description TEXT,
    notes       TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(target_id, type, username, secret)
);

CREATE TABLE IF NOT EXISTS proxies (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id   INTEGER REFERENCES targets(id) ON DELETE CASCADE,
    url         TEXT NOT NULL,
    type        TEXT NOT NULL CHECK(type IN ('http','socks5','burp')),
    active      INTEGER NOT NULL DEFAULT 1,
    description TEXT,
    notes       TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS endpoints (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id   INTEGER NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    method      TEXT NOT NULL,
    path        TEXT NOT NULL,
    status_code INTEGER,
    auth        INTEGER NOT NULL DEFAULT 0,
    params      TEXT NOT NULL DEFAULT '[]',
    description TEXT,
    notes       TEXT,
    tested      TEXT NOT NULL DEFAULT '[]',
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(target_id, method, path)
);

CREATE TABLE IF NOT EXISTS requests (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_id      INTEGER NOT NULL REFERENCES endpoints(id) ON DELETE CASCADE,
    raw_request      TEXT NOT NULL,
    raw_response     TEXT,
    status_code      INTEGER,
    response_time_ms INTEGER,
    description      TEXT,
    notes            TEXT,
    created_at       DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS findings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id   INTEGER NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    endpoint_id INTEGER REFERENCES endpoints(id) ON DELETE SET NULL,
    request_id  INTEGER REFERENCES requests(id) ON DELETE SET NULL,
    parent_id   INTEGER REFERENCES findings(id) ON DELETE SET NULL,
    type        TEXT NOT NULL,
    severity    TEXT NOT NULL CHECK(severity IN ('info','low','medium','high','critical')),
    status      TEXT NOT NULL DEFAULT 'potential'
                    CHECK(status IN ('potential','confirmed','false_positive')),
    raw_request TEXT,
    payload     TEXT,
    evidence    TEXT,
    description TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS coverage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_id INTEGER NOT NULL REFERENCES endpoints(id) ON DELETE CASCADE,
    vector      TEXT NOT NULL CHECK(vector IN (
                    'sqli','xss','ssrf','csrf','idor','bola',
                    'rce','lfi','xxe','ssti','auth','cors','other')),
    status      TEXT NOT NULL DEFAULT 'pending'
                    CHECK(status IN ('pending','in_progress','done','skipped')),
    description TEXT,
    notes       TEXT,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(endpoint_id, vector)
);

CREATE TABLE IF NOT EXISTS attack_chains (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id   INTEGER NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    description TEXT,
    severity    TEXT NOT NULL CHECK(severity IN ('info','low','medium','high','critical')),
    status      TEXT NOT NULL DEFAULT 'in_progress'
                    CHECK(status IN ('in_progress','completed','false_positive')),
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS chain_steps (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    chain_id   INTEGER NOT NULL REFERENCES attack_chains(id) ON DELETE CASCADE,
    finding_id INTEGER NOT NULL REFERENCES findings(id),
    step_order INTEGER NOT NULL,
    notes      TEXT,
    UNIQUE(chain_id, step_order)
);

CREATE INDEX IF NOT EXISTS idx_scopes_target         ON scopes(target_id);
CREATE INDEX IF NOT EXISTS idx_relations_from        ON target_relations(from_id);
CREATE INDEX IF NOT EXISTS idx_relations_to          ON target_relations(to_id);
CREATE INDEX IF NOT EXISTS idx_endpoints_target      ON endpoints(target_id, status_code);
CREATE INDEX IF NOT EXISTS idx_findings_severity     ON findings(target_id, severity);
CREATE INDEX IF NOT EXISTS idx_findings_status       ON findings(target_id, status);
CREATE INDEX IF NOT EXISTS idx_findings_parent       ON findings(parent_id);
CREATE INDEX IF NOT EXISTS idx_requests_endpoint     ON requests(endpoint_id, created_at);
CREATE INDEX IF NOT EXISTS idx_coverage_endpoint     ON coverage(endpoint_id, status);
CREATE INDEX IF NOT EXISTS idx_credentials_target    ON credentials(target_id);
CREATE INDEX IF NOT EXISTS idx_chain_steps_chain     ON chain_steps(chain_id, step_order);
CREATE INDEX IF NOT EXISTS idx_attack_chains_target  ON attack_chains(target_id, severity);

CREATE TABLE IF NOT EXISTS test_objects (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id       INTEGER NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    object_type     TEXT NOT NULL,
    object_id       TEXT NOT NULL,
    description     TEXT,
    rollback_method TEXT,
    rollback_url    TEXT,
    rollback_body   TEXT,
    status          TEXT NOT NULL DEFAULT 'active'
                        CHECK(status IN ('active','rolled_back','orphaned')),
    created_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS endpoint_examples (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_id     INTEGER NOT NULL REFERENCES endpoints(id) ON DELETE CASCADE,
    raw_request     TEXT NOT NULL,
    raw_response    TEXT,
    status_code     INTEGER,
    description     TEXT,
    created_at      DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(endpoint_id)
);

CREATE INDEX IF NOT EXISTS idx_test_objects_target ON test_objects(target_id, status);
CREATE INDEX IF NOT EXISTS idx_endpoint_examples   ON endpoint_examples(endpoint_id);
