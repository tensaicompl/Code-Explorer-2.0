CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- keys: schema_version, repo_id, repo_url, commit_sha, profile, engine_version,
--       pdx_version, language_matrix_version, precise_sources (json array)

CREATE TABLE files (
  file_id TEXT PRIMARY KEY,            -- node_id of the File node
  path TEXT NOT NULL UNIQUE,
  language TEXT NOT NULL,              -- Appendix A id, or 'unknown'
  status TEXT NOT NULL CHECK (status IN ('parsed','partial','failed','skipped','binary','redacted')),
  status_reason TEXT,
  blob_sha TEXT,                       -- git blob sha (hex); NULL for a file never read
  size_bytes INTEGER NOT NULL,
  line_count INTEGER                   -- NULL for a file never read
);

CREATE TABLE nodes (
  node_id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  qualified_name TEXT NOT NULL,
  file_id TEXT REFERENCES files(file_id),
  parent_id TEXT,                      -- CONTAINS parent
  start_line INTEGER, end_line INTEGER,
  layer_role TEXT,                     -- 4.2.3 LayerRole value or NULL
  signature TEXT,
  doc TEXT,
  props TEXT NOT NULL DEFAULT '{}'     -- JSON: language-specific extras, visibility, is_entry_point, is_test
);
CREATE INDEX nodes_parent ON nodes(parent_id);
CREATE INDEX nodes_file ON nodes(file_id);
CREATE INDEX nodes_kind_name ON nodes(kind, name);

CREATE TABLE sites (                    -- evidence sites, identity per 4.2.1
  site_id TEXT PRIMARY KEY, file_id TEXT NOT NULL REFERENCES files(file_id),
  enclosing_node_id TEXT, site_kind TEXT NOT NULL,   -- call|reference|import|type_ref|field_rw|route|contract
  ast_fingerprint TEXT NOT NULL,
  start_byte INTEGER NOT NULL, end_byte INTEGER NOT NULL,
  start_line INTEGER NOT NULL, start_col INTEGER NOT NULL, end_line INTEGER NOT NULL, end_col INTEGER NOT NULL,
  callee_text TEXT, receiver_text TEXT
);
CREATE INDEX sites_file ON sites(file_id);
CREATE INDEX sites_node ON sites(enclosing_node_id);

CREATE TABLE edges (
  edge_id TEXT PRIMARY KEY,
  src TEXT NOT NULL, dst TEXT NOT NULL,
  kind TEXT NOT NULL,
  band TEXT NOT NULL,                  -- 4.2.2 (11 bands)
  observed INTEGER NOT NULL DEFAULT 0, -- orthogonal runtime flag (link layer only; always 0 in repo segments)
  engine_score REAL, engine_strategy TEXT, engine_candidates INTEGER,
  site_id TEXT REFERENCES sites(site_id),
  weight INTEGER NOT NULL DEFAULT 1,
  props TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX edges_src ON edges(src, kind);
CREATE INDEX edges_dst ON edges(dst, kind);
CREATE INDEX edges_site ON edges(site_id);

CREATE TABLE evidence (                -- provider verdicts for precise/contradicted/manual facts only
  evidence_id TEXT PRIMARY KEY,        -- sha of (fact_id, provider, provider_version, verdict)
  fact_kind TEXT NOT NULL CHECK (fact_kind IN ('edge','node','candidate')),
  fact_id TEXT NOT NULL,
  provider TEXT NOT NULL, provider_version TEXT NOT NULL,
  authority TEXT NOT NULL CHECK (authority IN ('compiler','engine_typed','structural','manual')),
  verdict TEXT NOT NULL CHECK (verdict IN ('supports','contradicts','conflict')),
  target_node_id TEXT,                 -- the provider's target when it differs from the fact's dst
  file_id TEXT, start_byte INTEGER, end_byte INTEGER,
  metadata TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX evidence_fact ON evidence(fact_kind, fact_id);

CREATE TABLE semantic_occurrences (    -- compiler-provider occurrences (precise profile only); backs pdx_references
  occ_id TEXT PRIMARY KEY,             -- sha of (provider, symbol, file_id, start_byte, end_byte, role)
  provider TEXT NOT NULL, provider_symbol TEXT NOT NULL,
  target_node_id TEXT,                 -- mapped PDX node (NULL when unmapped)
  file_id TEXT NOT NULL REFERENCES files(file_id),
  start_byte INTEGER NOT NULL, end_byte INTEGER NOT NULL, start_line INTEGER NOT NULL, start_col INTEGER NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('definition','reference','read','write','import','type_reference','implementation','override')),
  site_id TEXT REFERENCES sites(site_id),   -- the structural site it intersects, when exactly one does
  enclosing_node_id TEXT
);
CREATE INDEX semocc_target ON semantic_occurrences(target_node_id, role);
CREATE INDEX semocc_file ON semantic_occurrences(file_id);

CREATE TABLE candidates (              -- non-drawn call sites, one row per site
  site_id TEXT PRIMARY KEY REFERENCES sites(site_id),
  src TEXT NOT NULL,
  callee_name TEXT NOT NULL,
  band TEXT NOT NULL CHECK (band IN ('candidate','external','blocked','unresolved','contradicted')),
  candidate_ids TEXT NOT NULL DEFAULT '[]',   -- JSON array of node_id
  engine_score REAL, engine_strategy TEXT, engine_candidates INTEGER,
  reason TEXT NOT NULL
);
CREATE INDEX candidates_src ON candidates(src);

CREATE TABLE contracts (               -- 4.7
  contract_id TEXT PRIMARY KEY,        -- node_id with repo_id='estate' over namespace_key (4.2.1)
  kind TEXT NOT NULL,                  -- ApiContract|RpcMethod|Channel|Table|Column|Artifact|ArtifactVersion
  key TEXT NOT NULL,                   -- normalised matching key (4.7.1)
  namespace_key TEXT NOT NULL,         -- identity + key (4.7.1); unresolved identity is prefixed 'unresolved:<repo_id>:'
  identity_strength TEXT NOT NULL CHECK (identity_strength IN ('exact','declared','unresolved')),
  owner_node_id TEXT,                  -- node in this repo that defines/produces it (NULL for consumed-only)
  direction TEXT NOT NULL CHECK (direction IN ('provides','consumes','both')),
  props TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX contracts_key ON contracts(kind, namespace_key);

CREATE TABLE metrics (                 -- 4.12
  node_id TEXT NOT NULL, metric TEXT NOT NULL, value REAL NOT NULL,
  PRIMARY KEY (node_id, metric)
);

CREATE TABLE coverage (                -- per language summary
  language TEXT PRIMARY KEY,
  files INTEGER, parsed INTEGER, partial INTEGER, failed INTEGER, skipped INTEGER,
  symbols INTEGER, call_sites INTEGER,
  by_band TEXT NOT NULL,              -- JSON object band -> count (all 11 bands)
  observed_links INTEGER NOT NULL DEFAULT 0
);

CREATE VIRTUAL TABLE fts USING fts5(name, qualified_name, doc, content='nodes', content_rowid='rowid', tokenize='unicode61');
