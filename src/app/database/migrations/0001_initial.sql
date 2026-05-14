CREATE TABLE IF NOT EXISTS targets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    domain     TEXT UNIQUE NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS credentials (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id  INTEGER NOT NULL REFERENCES targets(id),
    type       TEXT NOT NULL CHECK(type IN ('basic','cookie','token','apikey','other')),
    username   TEXT,
    secret     TEXT NOT NULL,
    notes      TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS proxies (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id  INTEGER REFERENCES targets(id),
    url        TEXT NOT NULL,
    type       TEXT NOT NULL CHECK(type IN ('http','socks5','burp')),
    active     BOOLEAN DEFAULT TRUE,
    notes      TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS endpoints (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id   INTEGER NOT NULL REFERENCES targets(id),
    method      TEXT NOT NULL,
    path        TEXT NOT NULL,
    status_code INTEGER,
    auth        BOOLEAN DEFAULT FALSE,
    params      TEXT DEFAULT '[]',
    notes       TEXT,
    tested      TEXT DEFAULT '[]',
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(target_id, method, path)
);

CREATE TABLE IF NOT EXISTS requests (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_id INTEGER NOT NULL REFERENCES endpoints(id),
    raw_request TEXT NOT NULL,
    raw_response TEXT,
    status_code INTEGER,
    response_time_ms INTEGER,
    notes       TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS findings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id   INTEGER NOT NULL REFERENCES targets(id),
    endpoint_id INTEGER REFERENCES endpoints(id),
    request_id  INTEGER REFERENCES requests(id),
    parent_id   INTEGER REFERENCES findings(id),
    type        TEXT NOT NULL,
    severity    TEXT NOT NULL CHECK(severity IN ('info','low','medium','high','critical')),
    status      TEXT NOT NULL DEFAULT 'potential'
                    CHECK(status IN ('potential','confirmed','false_positive')),
    raw_request TEXT,
    payload     TEXT,
    evidence    TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Прогресс тестирования по классам уязвимостей
CREATE TABLE IF NOT EXISTS coverage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_id INTEGER NOT NULL REFERENCES endpoints(id),
    vector      TEXT NOT NULL CHECK(vector IN (
                    'sqli','xss','ssrf','csrf','idor','bola',
                    'rce','lfi','xxe','ssti','auth','cors','other')),
    status      TEXT NOT NULL DEFAULT 'pending'
                    CHECK(status IN ('pending','in_progress','done','skipped')),
    notes       TEXT,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(endpoint_id, vector)
);

CREATE TABLE IF NOT EXISTS attack_chains (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id   INTEGER NOT NULL REFERENCES targets(id),
    title       TEXT NOT NULL,
    description TEXT,
    severity    TEXT NOT NULL CHECK(severity IN ('info','low','medium','high','critical')),
    status      TEXT NOT NULL DEFAULT 'in_progress'
                    CHECK(status IN ('in_progress','completed','false_positive')),
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS chain_steps (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    chain_id    INTEGER NOT NULL REFERENCES attack_chains(id),
    finding_id  INTEGER NOT NULL REFERENCES findings(id),
    step_order  INTEGER NOT NULL,
    notes       TEXT,
    UNIQUE(chain_id, step_order)
);


CREATE INDEX IF NOT EXISTS idx_endpoints_target    ON endpoints(target_id, status_code);
CREATE INDEX IF NOT EXISTS idx_findings_severity   ON findings(target_id, severity);
CREATE INDEX IF NOT EXISTS idx_findings_status     ON findings(target_id, status);
CREATE INDEX IF NOT EXISTS idx_findings_parent     ON findings(parent_id);
CREATE INDEX IF NOT EXISTS idx_requests_endpoint   ON requests(endpoint_id, created_at);
CREATE INDEX IF NOT EXISTS idx_coverage_endpoint   ON coverage(endpoint_id, status);
CREATE INDEX IF NOT EXISTS idx_credentials_target  ON credentials(target_id);
CREATE INDEX IF NOT EXISTS idx_chain_steps_chain   ON chain_steps(chain_id, step_order);
CREATE INDEX IF NOT EXISTS idx_attack_chains_target ON attack_chains(target_id, severity);
