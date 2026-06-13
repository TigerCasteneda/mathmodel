#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub category: String,
    pub tools_used: Vec<String>,
    pub system_prompt_fragment: String,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct SkillRegistry {
    skills: Arc<RwLock<HashMap<String, SkillDefinition>>>,
    active_skill: Arc<RwLock<Option<SkillDefinition>>>,
    skills_dir: PathBuf,
}

impl SkillRegistry {
    pub fn new(data_dir: PathBuf) -> Self {
        let skills_dir = data_dir.join("skills");
        let registry = Self {
            skills: Arc::new(RwLock::new(HashMap::new())),
            active_skill: Arc::new(RwLock::new(None)),
            skills_dir,
        };
        // Register built-in skills synchronously
        for skill in builtin_skills() {
            registry.skills.try_write().unwrap().insert(skill.name.clone(), skill);
        }
        registry
    }

    pub async fn reload(&self) {
        // Reload user skills from disk
        self.load_user_skills().await;
    }

    pub async fn get(&self, name: &str) -> Option<SkillDefinition> {
        let skills = self.skills.read().await;
        skills.get(name).cloned()
    }

    pub async fn list_all(&self) -> Vec<SkillDefinition> {
        let skills = self.skills.read().await;
        skills.values().cloned().collect()
    }

    pub async fn search(&self, query: &str) -> Vec<SkillDefinition> {
        let q = query.to_lowercase();
        let skills = self.skills.read().await;
        skills
            .values()
            .filter(|s| {
                s.name.to_lowercase().contains(&q)
                    || s.description.to_lowercase().contains(&q)
                    || s.category.to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    pub async fn set_active_skill(&self, name: &str) -> Option<SkillDefinition> {
        let skill = self.get(name).await?;
        self.active_skill.write().await.replace(skill.clone());
        Some(skill)
    }

    pub async fn active_skill_fragment(&self) -> Option<String> {
        self.active_skill.read().await.as_ref().map(|s| s.system_prompt_fragment.clone())
    }

    pub async fn clear_active_skill(&self) {
        self.active_skill.write().await.take();
    }

    async fn load_user_skills(&self) {
        if !self.skills_dir.exists() {
            return;
        }
        let Ok(_entries) = tokio::fs::read_dir(&self.skills_dir).await else {
            return;
        };
        // For now load from json files
        // Future: parse SKILL.md format
        let mut skills = self.skills.write().await;
        // Keep builtins (name starts with "builtin:")
        skills.retain(|k, _| k.starts_with("builtin:"));
    }
}

fn builtin_skills() -> Vec<SkillDefinition> {
    vec![
        SkillDefinition {
            name: "builtin:code-review".into(),
            description: "Review code for correctness, security, and maintainability issues.".into(),
            category: "CodeReview".into(),
            tools_used: vec!["file_read".into(), "search_files".into(), "list_files".into()],
            system_prompt_fragment: "You are a code reviewer. Examine the code thoroughly for bugs, security issues, performance problems, and style violations. Provide specific, actionable feedback with file and line references.".into(),
            metadata: HashMap::new(),
        },
        SkillDefinition {
            name: "builtin:math-verify".into(),
            description: "Verify mathematical derivations, check equation correctness, and validate model assumptions.".into(),
            category: "Math".into(),
            tools_used: vec!["file_read".into(), "search_files".into(), "web_search".into()],
            system_prompt_fragment: "You are a mathematical verification assistant. Check derivations step by step, verify dimensional consistency, validate assumptions, and flag potential errors. Use LaTeX for mathematical notation.".into(),
            metadata: HashMap::new(),
        },
        SkillDefinition {
            name: "builtin:model-fit".into(),
            description: "Fit mathematical model parameters to data, assess goodness of fit, and suggest improvements.".into(),
            category: "Math".into(),
            tools_used: vec!["file_read".into(), "file_write".into(), "execute_command".into()],
            system_prompt_fragment: "You are a model fitting specialist. Analyze data, choose appropriate fitting methods, estimate parameters, compute confidence intervals, and validate against holdout data. Implement solutions in Python or Julia.".into(),
            metadata: HashMap::new(),
        },
        SkillDefinition {
            name: "builtin:refactor".into(),
            description: "Refactor code for clarity, performance, and maintainability while preserving behavior.".into(),
            category: "CodeReview".into(),
            tools_used: vec!["file_read".into(), "file_write".into(), "file_edit".into(), "search_files".into()],
            system_prompt_fragment: "You are a refactoring specialist. Improve code structure without changing behavior. Extract functions, reduce duplication, improve naming, simplify logic. Never change public APIs or test expectations.".into(),
            metadata: HashMap::new(),
        },
        SkillDefinition {
            name: "builtin:latex-compile".into(),
            description: "Compile LaTeX documents and troubleshoot compilation errors.".into(),
            category: "Utility".into(),
            tools_used: vec!["file_read".into(), "file_write".into(), "execute_command".into()],
            system_prompt_fragment: "You are a LaTeX compilation assistant. Check for common LaTeX errors (missing packages, mismatched braces, undefined references), suggest fixes, and run compilation when possible.".into(),
            metadata: HashMap::new(),
        },
    ]
}
