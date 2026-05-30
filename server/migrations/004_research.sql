CREATE TABLE IF NOT EXISTS research_items (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL REFERENCES users(id),
    source TEXT NOT NULL,
    url TEXT NOT NULL,
    title TEXT,
    summary TEXT,
    notes TEXT,
    raw_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS research_context_pages (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES research_items(id) ON DELETE CASCADE,
    url TEXT,
    title TEXT,
    content TEXT,
    ordinal INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_research_items_project_created ON research_items(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_research_items_source_created ON research_items(source, created_at);
CREATE INDEX IF NOT EXISTS idx_research_context_pages_item_ordinal ON research_context_pages(item_id, ordinal);
