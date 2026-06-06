use super::runtime::{is_tool_concurrency_safe, ModelerAiRuntime};
use claude_code_rs::api::ToolCall;
use futures::future::join_all;
use std::future::Future;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionRequest {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolExecutionBatch {
    Concurrent(Vec<ToolExecutionRequest>),
    Serial(ToolExecutionRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionResult {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub output: String,
}

pub fn build_execution_batches(requests: &[ToolExecutionRequest]) -> Vec<ToolExecutionBatch> {
    let mut batches = Vec::new();
    let mut concurrent = Vec::new();

    for request in requests {
        if is_tool_concurrency_safe(&request.name) {
            concurrent.push(request.clone());
            continue;
        }

        if !concurrent.is_empty() {
            batches.push(ToolExecutionBatch::Concurrent(std::mem::take(
                &mut concurrent,
            )));
        }
        batches.push(ToolExecutionBatch::Serial(request.clone()));
    }

    if !concurrent.is_empty() {
        batches.push(ToolExecutionBatch::Concurrent(concurrent));
    }

    batches
}

pub fn build_execution_requests(tool_calls: &[ToolCall]) -> Vec<ToolExecutionRequest> {
    tool_calls
        .iter()
        .enumerate()
        .map(|(index, tool_call)| ToolExecutionRequest {
            index,
            id: tool_call.id.clone(),
            name: tool_call.function.name.clone(),
            arguments: serde_json::from_str(&tool_call.function.arguments)
                .unwrap_or_else(|_| serde_json::json!({})),
        })
        .collect()
}

fn result_from_request(request: ToolExecutionRequest, output: String) -> ToolExecutionResult {
    ToolExecutionResult {
        index: request.index,
        id: request.id,
        name: request.name,
        arguments: request.arguments,
        output,
    }
}

pub async fn execute_requests_with<F, Fut>(
    requests: Vec<ToolExecutionRequest>,
    runner: F,
) -> Vec<ToolExecutionResult>
where
    F: Fn(ToolExecutionRequest) -> Fut,
    Fut: Future<Output = String>,
{
    let mut results = Vec::new();
    for batch in build_execution_batches(&requests) {
        match batch {
            ToolExecutionBatch::Concurrent(items) => {
                let futures = items.into_iter().map(|request| {
                    let future = runner(request.clone());
                    async move {
                        let output = future.await;
                        result_from_request(request, output)
                    }
                });
                results.extend(join_all(futures).await);
            }
            ToolExecutionBatch::Serial(request) => {
                let output = runner(request.clone()).await;
                results.push(result_from_request(request, output));
            }
        }
    }
    results.sort_by_key(|result| result.index);
    results
}

pub async fn execute_tool_calls(
    runtime: &ModelerAiRuntime,
    tool_calls: &[ToolCall],
) -> Vec<ToolExecutionResult> {
    let requests = build_execution_requests(tool_calls);
    execute_requests_with(requests, |request| async move {
        runtime
            .execute_tool(&request.name, request.arguments.clone())
            .await
            .unwrap_or_else(|| format!("Unknown tool: {}", request.name))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{build_execution_batches, ToolExecutionBatch, ToolExecutionRequest};
    use claude_code_rs::api::{ToolCall, ToolCallFunction};
    use serde_json::json;

    fn request(index: usize, name: &str) -> ToolExecutionRequest {
        ToolExecutionRequest {
            index,
            id: format!("call_{index}"),
            name: name.to_string(),
            arguments: json!({ "index": index }),
        }
    }

    #[test]
    fn batches_consecutive_safe_tools_and_keeps_unsafe_serial() {
        let requests = vec![
            request(0, "file_read"),
            request(1, "search_files"),
            request(2, "file_write"),
            request(3, "fetch_url"),
            request(4, "list_files"),
        ];

        let batches = build_execution_batches(&requests);

        assert_eq!(batches.len(), 3);
        assert!(matches!(&batches[0], ToolExecutionBatch::Concurrent(items) if items.len() == 2));
        assert!(
            matches!(&batches[1], ToolExecutionBatch::Serial(item) if item.name == "file_write")
        );
        assert!(matches!(&batches[2], ToolExecutionBatch::Concurrent(items) if items.len() == 2));
    }

    #[test]
    fn unknown_tools_fail_closed_as_serial() {
        let requests = vec![request(0, "definitely_missing_tool")];
        let batches = build_execution_batches(&requests);

        assert!(
            matches!(&batches[0], ToolExecutionBatch::Serial(item) if item.name == "definitely_missing_tool")
        );
    }

    fn tool_call(id: &str, name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            r#type: "function".to_string(),
            function: ToolCallFunction {
                name: name.to_string(),
                arguments: args.to_string(),
            },
        }
    }

    #[test]
    fn builds_requests_in_tool_call_order_and_parses_arguments() {
        let calls = vec![
            tool_call("call_b", "file_read", r#"{ "path": "b.txt" }"#),
            tool_call("call_a", "file_read", r#"{ "path": "a.txt" }"#),
        ];

        let requests = super::build_execution_requests(&calls);

        assert_eq!(requests[0].index, 0);
        assert_eq!(requests[0].id, "call_b");
        assert_eq!(requests[0].arguments["path"], "b.txt");
        assert_eq!(requests[1].index, 1);
        assert_eq!(requests[1].id, "call_a");
    }

    #[tokio::test]
    async fn executes_safe_batch_concurrently_but_returns_original_order() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};

        let requests = vec![request(0, "file_read"), request(1, "search_files")];
        let started = Arc::new(Mutex::new(Vec::new()));
        let started_for_runner = started.clone();
        let begin = Instant::now();

        let results = super::execute_requests_with(requests, move |request| {
            let started_for_runner = started_for_runner.clone();
            async move {
                started_for_runner.lock().unwrap().push(request.index);
                if request.index == 0 {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                } else {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                format!("done_{}", request.index)
            }
        })
        .await;

        assert!(begin.elapsed() < Duration::from_millis(140));
        assert_eq!(started.lock().unwrap().as_slice(), &[0, 1]);
        assert_eq!(
            results
                .iter()
                .map(|result| result.output.as_str())
                .collect::<Vec<_>>(),
            vec!["done_0", "done_1"]
        );
    }
}
