CREATE TABLE IF NOT EXISTS arena_chat_messages (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    content TEXT NOT NULL,
    content_type TEXT NOT NULL DEFAULT 'text',
    reply_to_id TEXT REFERENCES arena_chat_messages(id),
    file_id TEXT REFERENCES files(id),
    content_attributes TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'sent',
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_arena_chat_project_created
    ON arena_chat_messages(project_id, created_at);

CREATE INDEX IF NOT EXISTS idx_arena_chat_project_created_desc
    ON arena_chat_messages(project_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_arena_chat_reply_to
    ON arena_chat_messages(reply_to_id);
