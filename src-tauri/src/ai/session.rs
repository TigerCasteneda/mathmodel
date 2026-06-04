use claude_code_rs::ChatMessage;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct ChatSessionStore {
    conversations: Mutex<HashMap<String, Vec<ChatMessage>>>,
}

impl ChatSessionStore {
    pub fn history(&self, conversation_id: &str) -> Result<Vec<ChatMessage>, String> {
        Ok(self
            .conversations
            .lock()
            .map_err(|e| e.to_string())?
            .get(conversation_id)
            .cloned()
            .unwrap_or_default())
    }

    pub fn push_user(&self, conversation_id: &str, content: String) -> Result<(), String> {
        self.push(conversation_id, ChatMessage::user(content))
    }

    pub fn push_assistant(&self, conversation_id: &str, content: String) -> Result<(), String> {
        self.push(conversation_id, ChatMessage::assistant(content))
    }

    fn push(&self, conversation_id: &str, message: ChatMessage) -> Result<(), String> {
        self.conversations
            .lock()
            .map_err(|e| e.to_string())?
            .entry(conversation_id.to_string())
            .or_default()
            .push(message);
        Ok(())
    }
}
