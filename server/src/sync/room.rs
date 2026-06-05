use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::{GetString, ReadTxn};
use yrs::Transact;

pub struct SyncRoom {
    pub file_id: String,
    pub doc: yrs::Doc,
    pub update_tx: broadcast::Sender<Vec<u8>>,
    pub clients: AtomicUsize,
}

impl SyncRoom {
    pub fn new(file_id: String) -> Self {
        let doc = yrs::Doc::new();
        let (tx, _) = broadcast::channel::<Vec<u8>>(256);
        Self {
            file_id,
            doc,
            update_tx: tx,
            clients: AtomicUsize::new(0),
        }
    }

    pub fn apply_update(&mut self, update: &[u8]) -> Result<(), yrs::encoding::read::Error> {
        let mut txn = self.doc.transact_mut();
        let u = yrs::Update::decode_v1(update)?;
        txn.apply_update(u);
        Ok(())
    }

    pub fn encode_state(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&yrs::StateVector::default())
    }
}

pub struct RoomRegistry {
    rooms: Arc<RwLock<HashMap<String, Arc<RwLock<SyncRoom>>>>>,
}

impl RoomRegistry {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn has_active_clients(&self, file_id: &str) -> bool {
        let room = self.rooms.read().await.get(file_id).cloned();
        let Some(room) = room else {
            return false;
        };

        let active_clients = room.read().await.clients.load(Ordering::Acquire);
        active_clients > 0
    }

    pub async fn get_or_create(
        &self,
        file_id: &str,
        pool: &sqlx::SqlitePool,
    ) -> Arc<RwLock<SyncRoom>> {
        if let Some(room) = self.rooms.read().await.get(file_id).cloned() {
            room.read().await.clients.fetch_add(1, Ordering::Relaxed);
            return room;
        }

        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get(file_id).cloned() {
            room.read().await.clients.fetch_add(1, Ordering::Relaxed);
            return room;
        }

        let mut room = SyncRoom::new(file_id.to_string());

        // Load existing CRDT state from DB
        let result: Result<Option<(Vec<u8>,)>, _> =
            sqlx::query_as("SELECT ydoc_state FROM crdt_docs WHERE file_id = ?")
                .bind(file_id)
                .fetch_optional(pool)
                .await;

        if let Ok(Some((state,))) = result {
            if !state.is_empty() {
                let _ = room.apply_update(&state);
            }
        }
        room.clients.store(1, Ordering::Relaxed);

        let room = Arc::new(RwLock::new(room));
        rooms.insert(file_id.to_string(), room.clone());
        room
    }

    pub async fn release(&self, file_id: &str, pool: &sqlx::SqlitePool) {
        let Some(room) = self.rooms.read().await.get(file_id).cloned() else {
            return;
        };

        let room_guard = room.read().await;
        let previous = room_guard.clients.fetch_sub(1, Ordering::AcqRel);
        if previous > 1 {
            return;
        }

        let state = room_guard.encode_state();
        let room_file_id = room_guard.file_id.clone();
        drop(room_guard);

        let mut rooms = self.rooms.write().await;
        if let Some(current) = rooms.get(file_id) {
            let current_clients = current.read().await.clients.load(Ordering::Acquire);
            if Arc::ptr_eq(current, &room) && current_clients == 0 {
                rooms.remove(file_id);
            }
        }
        drop(rooms);

        match persist_state(&room_file_id, &state, pool).await {
            Ok(()) => auto_snapshot_after_persist(&room_file_id, &state, pool).await,
            Err(err) => tracing::error!("Failed to persist CRDT state for {room_file_id}: {err:?}"),
        }
    }
}

async fn persist_state(
    file_id: &str,
    state: &[u8],
    pool: &sqlx::SqlitePool,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let size = decode_state_text_len(state).unwrap_or(0) as i64;
    sqlx::query("UPDATE crdt_docs SET ydoc_state = ?, updated_at = ? WHERE file_id = ?")
        .bind(state)
        .bind(now)
        .bind(file_id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE files SET size = ?, updated_at = ? WHERE id = ?")
        .bind(size)
        .bind(now)
        .bind(file_id)
        .execute(pool)
        .await?;

    Ok(())
}

fn decode_state_text_len(state: &[u8]) -> Option<usize> {
    if state.is_empty() {
        return Some(0);
    }
    let doc = yrs::Doc::new();
    let mut txn = doc.transact_mut();
    let update = yrs::Update::decode_v1(state).ok()?;
    txn.apply_update(update);
    drop(txn);
    let text = doc.get_or_insert_text("content");
    let txn = doc.transact();
    Some(text.get_string(&txn).len())
}

async fn auto_snapshot_after_persist(file_id: &str, state: &[u8], pool: &sqlx::SqlitePool) {
    if state.is_empty() {
        return;
    }

    let project: Option<(String,)> = sqlx::query_as("SELECT project_id FROM files WHERE id = ?")
        .bind(file_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

    let Some((project_id,)) = project else {
        return;
    };

    let latest: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT ydoc_state FROM snapshots WHERE file_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(file_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if latest
        .as_ref()
        .is_some_and(|(latest_state,)| latest_state.as_slice() == state)
    {
        return;
    }

    let now = chrono::Utc::now().timestamp();
    if let Err(err) = sqlx::query(
        "INSERT INTO snapshots (id, file_id, project_id, label, ydoc_state, created_by, source, created_at)
         VALUES (?, ?, ?, 'auto', ?, 'system', 'auto', ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(file_id)
    .bind(&project_id)
    .bind(state)
    .bind(now)
    .execute(pool)
    .await
    {
        tracing::error!("Failed to create automatic snapshot for {file_id}: {err:?}");
        return;
    }

    prune_snapshots(pool, file_id, 100).await;
}

async fn prune_snapshots(pool: &sqlx::SqlitePool, file_id: &str, limit: i64) {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM snapshots WHERE file_id = ?")
        .bind(file_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if count <= limit {
        return;
    }

    let _ = sqlx::query(
        "DELETE FROM snapshots WHERE id IN (
            SELECT id FROM snapshots WHERE file_id = ? ORDER BY created_at ASC LIMIT ?
        )",
    )
    .bind(file_id)
    .bind(count - limit)
    .execute(pool)
    .await;
}
