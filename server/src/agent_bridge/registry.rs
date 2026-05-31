use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

pub type AgentOutboundTx = mpsc::UnboundedSender<Value>;

pub struct ProjectAgentBridge {
    agent: RwLock<Option<(String, AgentOutboundTx)>>,
    frontend_tx: broadcast::Sender<Value>,
}

impl ProjectAgentBridge {
    fn new() -> Self {
        let (frontend_tx, _) = broadcast::channel(512);
        Self {
            agent: RwLock::new(None),
            frontend_tx,
        }
    }

    pub async fn set_agent(&self, connection_id: String, tx: AgentOutboundTx) {
        *self.agent.write().await = Some((connection_id, tx));
        self.broadcast_to_frontends(serde_json::json!({
            "type": "agent_status",
            "status": "connected"
        }));
    }

    pub async fn clear_agent(&self, connection_id: &str) {
        let mut agent = self.agent.write().await;
        if agent
            .as_ref()
            .is_some_and(|(current_id, _)| current_id == connection_id)
        {
            *agent = None;
            self.broadcast_to_frontends(serde_json::json!({
                "type": "agent_status",
                "status": "disconnected"
            }));
        }
    }

    pub async fn has_agent(&self) -> bool {
        self.agent.read().await.is_some()
    }

    pub async fn send_to_agent(&self, message: Value) -> Result<(), ()> {
        let agent = self.agent.read().await;
        let Some((_, tx)) = agent.as_ref() else {
            return Err(());
        };
        tx.send(message).map_err(|_| ())
    }

    pub fn subscribe_frontend(&self) -> broadcast::Receiver<Value> {
        self.frontend_tx.subscribe()
    }

    pub fn broadcast_to_frontends(&self, message: Value) {
        let _ = self.frontend_tx.send(message);
    }
}

pub struct AgentRegistry {
    projects: RwLock<HashMap<String, Arc<ProjectAgentBridge>>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            projects: RwLock::new(HashMap::new()),
        }
    }

    pub async fn get_or_create(&self, project_id: &str) -> Arc<ProjectAgentBridge> {
        if let Some(bridge) = self.projects.read().await.get(project_id).cloned() {
            return bridge;
        }

        let mut projects = self.projects.write().await;
        projects
            .entry(project_id.to_string())
            .or_insert_with(|| Arc::new(ProjectAgentBridge::new()))
            .clone()
    }
}
