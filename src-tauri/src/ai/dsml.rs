//! DSML (DeepSeek Markup Language) tool-call parser.
//!
//! Some DeepSeek models emit tool calls in DeepSeek's consumer-web rendering
//! format inside the `delta.content` stream instead of via `delta.tool_calls[]`:
//!
//! ```text
//! <｜｜DSML｜｜tool_calls>
//! <｜｜DSML｜｜invoke name="search_web">
//! <｜｜DSML｜｜parameter name="query" string="true">some query</｜｜DSML｜｜parameter>
//! </｜｜DSML｜｜invoke>
//! </｜｜DSML｜｜tool_calls>
//! ```
//!
//! (Older / v4-pro format used trailing `｜｜` before `>`, e.g.
//! `<｜｜DSML｜｜tool_calls｜｜>` — this parser accepts both via
//! `find_tool_calls_open` / `find_tool_calls_close`. v4-flash emits without
//! trailing pipes; the parser handles it.)
//!
//! This is not a wire format on DeepSeek's HTTP API per the docs — it's the
//! consumer chat-web format. But models trained on that rendering sometimes
//! leak it into streaming responses. Without this parser, the XML blocks
//! would be displayed verbatim to the user and the tool calls would never
//! execute.
//!
//! `DsmlParser` is a streaming state machine: callers feed it `delta.content`
//! chunks as they arrive (chunk boundaries may split tags mid-character),
//! and it returns plain-text fragments plus reconstructed tool calls.

use serde_json::{Map, Value};

/// Tag delimiters. `｜` is the full-width vertical bar used by DeepSeek.
///
/// Outer `tool_calls` tags come in two variants depending on model version:
///   - With trailing pipes (older / v4-pro): `<｜｜DSML｜｜tool_calls｜｜>`
///   - Without (v4-flash):                `<｜｜DSML｜｜tool_calls>`
/// The parser tries both via `find_tool_calls_open` / `find_tool_calls_close`.
const TOOL_CALLS_OPEN_WITH_PIPES: &str = "<｜｜DSML｜｜tool_calls｜｜>";
const TOOL_CALLS_OPEN_WITHOUT_PIPES: &str = "<｜｜DSML｜｜tool_calls>";
const TOOL_CALLS_CLOSE_WITH_PIPES: &str = "</｜｜DSML｜｜tool_calls｜｜>";
const TOOL_CALLS_CLOSE_WITHOUT_PIPES: &str = "</｜｜DSML｜｜tool_calls>";
const INVOKE_OPEN_PREFIX: &str = "<｜｜DSML｜｜invoke name=\"";
const INVOKE_CLOSE: &str = "</｜｜DSML｜｜invoke>";
const PARAMETER_OPEN_PREFIX: &str = "<｜｜DSML｜｜parameter name=\"";
const PARAMETER_CLOSE: &str = "</｜｜DSML｜｜parameter>";

/// Find the next tool_calls open tag, accepting both with-pipes and
/// without-pipes variants. Returns `(start, length)` of the matched tag.
fn find_tool_calls_open(buffer: &str) -> Option<(usize, usize)> {
    if let Some(pos) = buffer.find(TOOL_CALLS_OPEN_WITH_PIPES) {
        return Some((pos, TOOL_CALLS_OPEN_WITH_PIPES.len()));
    }
    if let Some(pos) = buffer.find(TOOL_CALLS_OPEN_WITHOUT_PIPES) {
        return Some((pos, TOOL_CALLS_OPEN_WITHOUT_PIPES.len()));
    }
    None
}

/// Same as `find_tool_calls_open` for the close tag.
fn find_tool_calls_close(buffer: &str) -> Option<(usize, usize)> {
    if let Some(pos) = buffer.find(TOOL_CALLS_CLOSE_WITH_PIPES) {
        return Some((pos, TOOL_CALLS_CLOSE_WITH_PIPES.len()));
    }
    if let Some(pos) = buffer.find(TOOL_CALLS_CLOSE_WITHOUT_PIPES) {
        return Some((pos, TOOL_CALLS_CLOSE_WITHOUT_PIPES.len()));
    }
    None
}

/// Reconstructed chunk mirroring `research_agent::ToolCallChunk` /
/// `chat::ToolCallChunk`. Fields are optional to match those types' shapes.
#[derive(Debug, Clone)]
pub struct ToolCallChunk {
    pub index: i64,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

/// What `DsmlParser::feed` can emit.
#[derive(Debug, Clone)]
pub enum DsmlEvent {
    /// Plain text fragment with DSML blocks stripped. Safe to surface to the user.
    Text(String),
    /// A fully-reconstructed tool call derived from a DSML block.
    SyntheticToolCall(ToolCallChunk),
}

pub struct DsmlParser {
    buffer: String,
}

impl DsmlParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Feed a `delta.content` chunk. Returns zero or more events to emit.
    ///
    /// **Hold-back semantics:** if no complete `<｜｜DSML｜｜tool_calls｜｜>` is
    /// found in the buffer, do NOT emit eagerly. We treat everything from the
    /// last `<` onward as a potential DSML tag fragment and hold it until the
    /// next chunk arrives or the stream ends. This is critical because the
    /// model can stream DSML tags split across chunks (e.g. one chunk emits
    /// `<`, the next emits `｜｜DSML｜｜tool_calls｜｜>`).
    ///
    /// Call [`flush`](Self::flush) when the stream ends (`data: [DONE]`) to
    /// release any remaining text the model emitted without a closing DSML tag.
    pub fn feed(&mut self, content: &str) -> Vec<DsmlEvent> {
        self.buffer.push_str(content);
        let mut events = Vec::new();
        loop {
            // Find the next complete DSML tool_calls open tag, accepting both
            // with-pipes and without-pipes variants.
            let Some((start, open_len)) = find_tool_calls_open(&self.buffer) else {
                // No complete open tag yet. Find the last '<' in the buffer —
                // anything from there onward might be the start of a DSML
                // tag, so hold it. Emit only what's safely before it.
                let safe_end = self.buffer.rfind('<').unwrap_or(self.buffer.len());
                if safe_end > 0 {
                    events.push(DsmlEvent::Text(self.buffer[..safe_end].to_string()));
                    self.buffer.drain(..safe_end);
                }
                // Buffer now contains either: nothing, a leading '<', or
                // characters including a '<' we couldn't resolve. Hold all
                // of it until the next chunk.
                return events;
            };
            // Found the opening tag. Emit text before it (none if at start).
            if start > 0 {
                events.push(DsmlEvent::Text(self.buffer[..start].to_string()));
                self.buffer.drain(..start);
                // `start` is now 0; continue the loop to process the block.
            }
            // Find the matching close tag (also dual-format tolerant).
            let Some((close_rel, close_len)) =
                find_tool_calls_close(&self.buffer[open_len..])
            else {
                // Open complete, close not yet — keep buffer for next feed.
                return events;
            };
            let end = open_len + close_rel + close_len;
            let inner = self.buffer[open_len..open_len + close_rel].to_string();
            for chunk in parse_dsml_invoke_blocks(&inner) {
                events.push(DsmlEvent::SyntheticToolCall(chunk));
            }
            self.buffer.drain(..end);
        }
    }

    /// Flush any leftover buffer as plain text. Call this when the upstream
    /// stream ends (`data: [DONE]`) so a truncated response doesn't leave
    /// buffered text orphaned.
    pub fn flush(&mut self) -> Vec<DsmlEvent> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        vec![DsmlEvent::Text(std::mem::take(&mut self.buffer))]
    }
}

impl Default for DsmlParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse the inner contents of a `<｜｜DSML｜｜tool_calls>...</｜｜DSML｜｜tool_calls>`
/// block into a list of synthetic `ToolCallChunk`s. Each `<｜｜DSML｜｜invoke>`
/// becomes one chunk; parameters are collected into a JSON object string.
fn parse_dsml_invoke_blocks(inner: &str) -> Vec<ToolCallChunk> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = inner[cursor..].find(INVOKE_OPEN_PREFIX) {
        let invoke_start = cursor + rel + INVOKE_OPEN_PREFIX.len();
        let Some(name_end_rel) = inner[invoke_start..].find('"') else {
            break;
        };
        let name = inner[invoke_start..invoke_start + name_end_rel].to_string();
        let after_name = invoke_start + name_end_rel + 1;
        let body_start_rel = inner[after_name..]
            .find('>')
            .map(|i| i + 1)
            .unwrap_or(0);
        let body_start = after_name + body_start_rel;
        let Some(close_rel) = inner[body_start..].find(INVOKE_CLOSE) else {
            break;
        };
        let body = &inner[body_start..body_start + close_rel];
        let arguments = collect_dsml_parameters_to_json(body);
        out.push(ToolCallChunk {
            index: out.len() as i64,
            id: Some(uuid::Uuid::new_v4().to_string()),
            name: Some(name),
            arguments: Some(arguments),
        });
        cursor = body_start + close_rel + INVOKE_CLOSE.len();
    }
    out
}

/// Build a JSON object string from `<｜｜DSML｜｜parameter>` entries.
/// All values are treated as strings (DeepSeek's consumer format always
/// uses `string="true"` for tool arguments, even when the underlying type is
/// numeric — downstream tool code parses the string as needed).
fn collect_dsml_parameters_to_json(body: &str) -> String {
    let mut map = Map::new();
    let mut cursor = 0;
    while let Some(rel) = body[cursor..].find(PARAMETER_OPEN_PREFIX) {
        let name_start = cursor + rel + PARAMETER_OPEN_PREFIX.len();
        let Some(name_end_rel) = body[name_start..].find('"') else {
            break;
        };
        let name = body[name_start..name_start + name_end_rel].to_string();
        let after_name = name_start + name_end_rel + 1;
        let val_start_rel = body[after_name..]
            .find('>')
            .map(|i| i + 1)
            .unwrap_or(0);
        let val_start = after_name + val_start_rel;
        let Some(close_rel) = body[val_start..].find(PARAMETER_CLOSE) else {
            break;
        };
        let value = body[val_start..val_start + close_rel].to_string();
        map.insert(name, Value::String(value));
        cursor = val_start + close_rel + PARAMETER_CLOSE.len();
    }
    Value::Object(map).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_invoke_block() {
        let mut p = DsmlParser::new();
        let events = p.feed(
            r#"Let me search for that. <｜｜DSML｜｜tool_calls｜｜><｜｜DSML｜｜invoke name="search_web"><｜｜DSML｜｜parameter name="query" string="true">gnn traffic forecasting</｜｜DSML｜｜parameter></｜｜DSML｜｜invoke></｜｜DSML｜｜tool_calls｜｜> Done."#,
        );
        assert_eq!(events.len(), 3, "text + tool call + trailing text");
        match &events[0] {
            DsmlEvent::Text(t) => assert_eq!(t, "Let me search for that. "),
            _ => panic!("expected text first"),
        }
        match &events[1] {
            DsmlEvent::SyntheticToolCall(tc) => {
                assert_eq!(tc.name.as_deref(), Some("search_web"));
                assert!(tc
                    .arguments
                    .as_deref()
                    .unwrap_or("")
                    .contains("gnn traffic forecasting"));
                assert!(tc.id.is_some(), "id should be auto-generated");
            }
            _ => panic!("expected synthetic tool call"),
        }
        match &events[2] {
            DsmlEvent::Text(t) => assert_eq!(t, " Done."),
            _ => panic!("expected trailing text"),
        }
    }

    #[test]
    fn parses_multiple_invoke_blocks_in_one_call() {
        let mut p = DsmlParser::new();
        let events = p.feed(
            r#"<｜｜DSML｜｜tool_calls｜｜><｜｜DSML｜｜invoke name="a"><｜｜DSML｜｜parameter name="x" string="true">1</｜｜DSML｜｜parameter></｜｜DSML｜｜invoke><｜｜DSML｜｜invoke name="b"><｜｜DSML｜｜parameter name="y" string="true">2</｜｜DSML｜｜parameter></｜｜DSML｜｜invoke></｜｜DSML｜｜tool_calls｜｜>"#,
        );
        let tool_calls: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                DsmlEvent::SyntheticToolCall(tc) => Some(tc.name.clone().unwrap_or_default()),
                _ => None,
            })
            .collect();
        assert_eq!(tool_calls, vec!["a", "b"]);
    }

    #[test]
    fn handles_block_split_across_chunks() {
        let mut p = DsmlParser::new();
        let events1 = p.feed(
            r#"<｜｜DSML｜｜tool_calls｜｜><｜｜DSML｜｜invoke name="fetch_url"><｜｜DSML｜｜parameter name="url" string="true">"#,
        );
        assert!(
            events1.is_empty(),
            "no events while block is incomplete (buffer holds it)"
        );
        let events2 = p.feed(
            r#"https://example.com</｜｜DSML｜｜parameter></｜｜DSML｜｜invoke></｜｜DSML｜｜tool_calls｜｜>"#,
        );
        let tc = events2
            .iter()
            .find_map(|e| match e {
                DsmlEvent::SyntheticToolCall(t) => Some(t),
                _ => None,
            })
            .expect("synthetic tool call");
        assert_eq!(tc.name.as_deref(), Some("fetch_url"));
        assert!(tc.arguments.as_deref().unwrap_or("").contains("https://example.com"));
    }

    #[test]
    fn no_dsml_passes_through_verbatim() {
        let mut p = DsmlParser::new();
        let events = p.feed("Just plain text with no special markup.");
        assert_eq!(events.len(), 1);
        match &events[0] {
            DsmlEvent::Text(t) => assert_eq!(t, "Just plain text with no special markup."),
            _ => panic!("expected single text event"),
        }
    }

    #[test]
    fn parameters_build_json_object() {
        let args = collect_dsml_parameters_to_json(
            r#"<｜｜DSML｜｜parameter name="query" string="true">deepseek</｜｜DSML｜｜parameter><｜｜DSML｜｜parameter name="limit" string="true">5</｜｜DSML｜｜parameter>"#,
        );
        let parsed: Value = serde_json::from_str(&args).unwrap();
        assert_eq!(parsed["query"], "deepseek");
        assert_eq!(parsed["limit"], "5");
    }

    #[test]
    fn flush_releases_orphan_buffer() {
        let mut p = DsmlParser::new();
        // Feed an unterminated DSML block (no close tag) and then flush.
        let _ = p.feed(r#"<｜｜DSML｜｜tool_calls｜｜><｜｜DSML｜｜invoke name="x">"#);
        let flushed = p.flush();
        assert_eq!(flushed.len(), 1);
        match &flushed[0] {
            DsmlEvent::Text(t) => assert!(t.contains("DSML")),
            _ => panic!("flush should emit text"),
        }
    }

    /// Regression test for the live DeepSeek streaming pattern observed in
    /// production: the model emits `<` in one chunk and `｜｜DSML｜｜tool_calls｜｜>...`
    /// in the next. Without hold-back, the standalone `<` would leak to the
    /// UI as visible text.
    #[test]
    fn holds_partial_dsml_until_full_tag_arrives() {
        let mut p = DsmlParser::new();
        // Chunk 1: just the opening angle bracket.
        let events1 = p.feed("<");
        assert!(events1.is_empty(), "lone '<' must be held, not emitted");
        // Chunk 2: the rest of the opening tag plus a complete invoke block.
        let events2 = p.feed(
            r#"｜｜DSML｜｜tool_calls｜｜><｜｜DSML｜｜invoke name="search_web"><｜｜DSML｜｜parameter name="query" string="true">deepseek api</｜｜DSML｜｜parameter></｜｜DSML｜｜invoke></｜｜DSML｜｜tool_calls｜｜>"#,
        );
        let tool_calls: Vec<String> = events2
            .iter()
            .filter_map(|e| match e {
                DsmlEvent::SyntheticToolCall(tc) => tc.name.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(tool_calls, vec!["search_web".to_string()]);
    }

    /// Three-way split across chunks: `<` | `｜｜DSML｜｜` | `tool_calls｜｜>...`
    /// Each fragment alone is not enough to identify a DSML block.
    #[test]
    fn holds_three_way_split_across_chunks() {
        let mut p = DsmlParser::new();
        assert!(p.feed("<").is_empty());
        assert!(p.feed("｜｜DSML｜｜").is_empty());
        let events = p.feed(
            r#"tool_calls｜｜><｜｜DSML｜｜invoke name="a"><｜｜DSML｜｜parameter name="x" string="true">1</｜｜DSML｜｜parameter></｜｜DSML｜｜invoke></｜｜DSML｜｜tool_calls｜｜>"#,
        );
        let names: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                DsmlEvent::SyntheticToolCall(tc) => tc.name.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["a".to_string()]);
    }

    /// Regression test for the live v4-flash pattern: outer `tool_calls`
    /// tags have NO trailing `｜｜` before `>`. Earlier parser constants
    /// required trailing pipes, which silently failed to match this format.
    #[test]
    fn parses_v4_flash_format_no_trailing_pipes() {
        let mut p = DsmlParser::new();
        let events = p.feed(
            r#"<｜｜DSML｜｜tool_calls>
<｜｜DSML｜｜invoke name="fetch_url">
<｜｜DSML｜｜parameter name="url" string="true">https://dl.acm.org/doi/fullHtml/10.1145/3308558.3313442</｜｜DSML｜｜parameter>
</｜｜DSML｜｜invoke>
</｜｜DSML｜｜tool_calls>"#,
        );
        let tool_calls: Vec<ToolCallChunk> = events
            .iter()
            .filter_map(|e| match e {
                DsmlEvent::SyntheticToolCall(tc) => Some(tc.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name.as_deref(), Some("fetch_url"));
        let args = tool_calls[0].arguments.as_deref().unwrap_or("");
        assert!(args.contains("https://dl.acm.org/doi/fullHtml/10.1145/3308558.3313442"));
    }

    /// Mixed format in the same response: outer tags use trailing pipes,
    /// inner tags use no trailing pipes. Both should parse.
    #[test]
    fn parses_mixed_pipes_with_and_without() {
        let mut p = DsmlParser::new();
        // With-pipes outer + without-pipes inner (v4-pro style inner + v4-flash outer)
        let events = p.feed(
            r#"<｜｜DSML｜｜tool_calls｜｜>
<｜｜DSML｜｜invoke name="x">
<｜｜DSML｜｜parameter name="k" string="true">v</｜｜DSML｜｜parameter>
</｜｜DSML｜｜invoke>
</｜｜DSML｜｜tool_calls｜｜>"#,
        );
        let names: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                DsmlEvent::SyntheticToolCall(tc) => tc.name.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["x".to_string()]);
    }
}