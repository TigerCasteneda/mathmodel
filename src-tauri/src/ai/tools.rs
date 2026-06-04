use claude_code_rs::api::ToolDefinition;
use serde_json::json;

pub fn modeler_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "web_search",
            "Search the web through the host SearXNG instance.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 20 }
                },
                "required": ["query"]
            }),
        ),
        ToolDefinition::new(
            "fetch_url",
            "Fetch and extract a web page as markdown through Firecrawl.",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
        ),
        ToolDefinition::new(
            "save_reference",
            "Save a useful reference into the project's research library.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "url": { "type": "string" },
                    "summary": { "type": "string" },
                    "category": { "type": "string", "enum": ["literature", "dataset", "code", "formula", "competition"] },
                    "methodology": { "type": "string" },
                    "key_parameters": { "type": "string" },
                    "ai_relevance": { "type": "string" }
                },
                "required": ["title", "url", "summary", "category"]
            }),
        ),
        ToolDefinition::new(
            "read_file",
            "Read a file from the current local workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        ),
        ToolDefinition::new(
            "write_file",
            "Create a new file in the current local workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::modeler_tool_definitions;

    #[test]
    fn exposes_phase9_tool_names() {
        let names = modeler_tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"web_search".to_string()));
        assert!(names.contains(&"fetch_url".to_string()));
        assert!(names.contains(&"save_reference".to_string()));
        assert!(names.contains(&"read_file".to_string()));
    }
}
