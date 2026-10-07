use anyhow::Result;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServiceExt};
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::app::memory::Brain;
use crate::app::tools::{self, Tool};

const MAX_LINE_BYTES: usize = 1024 * 1024;

pub async fn serve_stdio(brain: Arc<Brain>) -> Result<()> {
    let (stdin, stdout) = rmcp::transport::stdio();
    serve(brain, stdin, stdout).await
}

pub async fn serve<R, W>(brain: Arc<Brain>, reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let transport = (
        LineLimited {
            inner: reader,
            line_bytes: 0,
        },
        writer,
    );
    let session = McpServer { brain }.serve(transport).await?;
    session.waiting().await?;
    Ok(())
}

struct McpServer {
    brain: Arc<Brain>,
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(tools::INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(
            Tool::ALL.iter().map(|tool| definition(*tool)).collect(),
        ))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        Tool::parse(name).map(definition)
    }

    // A failed tool is still a successful call with isError set, so the agent
    // can read why. Only an unknown tool name is a protocol error.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let tool = Tool::parse(&request.name)
            .ok_or_else(|| ErrorData::invalid_params("unknown tool", None))?;
        let arguments = request.arguments.map_or(Value::Null, Value::Object);

        let result = match tools::call(&self.brain, tool, arguments).await {
            Ok(value) => CallToolResult::success(vec![ContentBlock::text(value.to_string())]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        };
        Ok(result.into())
    }
}

fn definition(tool: Tool) -> rmcp::model::Tool {
    let (description, schema) = tools::describe(tool);
    let annotations = ToolAnnotations::new()
        .read_only(!tool.changes_memory())
        .destructive(tool.deletes())
        // Nothing here reaches beyond the local memory.
        .open_world(false);
    rmcp::model::Tool::new(
        tool.as_str(),
        description,
        schema.as_object().cloned().unwrap_or_default(),
    )
    .with_annotations(annotations)
}

// The SDK transport has no line cap, so fail the stream past the limit.
struct LineLimited<R> {
    inner: R,
    line_bytes: usize,
}

impl<R: AsyncRead + Unpin> AsyncRead for LineLimited<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let already_filled = buf.filled().len();
        ready!(Pin::new(&mut self.inner).poll_read(cx, buf))?;
        let fresh = &buf.filled()[already_filled..];

        let is_newline = |byte: &u8| *byte == b'\n';
        let (ends_current_line_at, starts_next_line_with) = match (
            fresh.iter().position(is_newline),
            fresh.iter().rposition(is_newline),
        ) {
            (Some(first), Some(last)) => (first, fresh.len() - last - 1),
            _ => (fresh.len(), self.line_bytes + fresh.len()),
        };
        if self.line_bytes + ends_current_line_at > MAX_LINE_BYTES {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("MCP message is larger than {MAX_LINE_BYTES} bytes"),
            )));
        }
        self.line_bytes = starts_next_line_with;
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tools::tests::brain;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;
    const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

    struct Client {
        to_server: DuplexStream,
        from_server: BufReader<DuplexStream>,
        server: tokio::task::JoinHandle<Result<()>>,
        _dir: tempfile::TempDir,
    }

    impl Client {
        fn connect() -> Self {
            let (brain, dir) = brain();
            let (to_server, server_input) = tokio::io::duplex(4 * MAX_LINE_BYTES);
            let (server_output, from_server) = tokio::io::duplex(4 * MAX_LINE_BYTES);
            let server = tokio::spawn(serve(Arc::new(brain), server_input, server_output));
            Self {
                to_server,
                from_server: BufReader::new(from_server),
                server,
                _dir: dir,
            }
        }

        async fn initialized() -> Self {
            let mut client = Self::connect();
            client.send(INITIALIZE).await;
            client.receive().await.expect("initialize result");
            client.send(INITIALIZED).await;
            client
        }

        async fn send(&mut self, line: &str) {
            self.to_server.write_all(line.as_bytes()).await.unwrap();
            self.to_server.write_all(b"\n").await.unwrap();
        }

        async fn receive(&mut self) -> Option<Value> {
            let mut line = String::new();
            let read = tokio::time::timeout(
                Duration::from_millis(500),
                self.from_server.read_line(&mut line),
            );
            match read.await {
                Ok(Ok(bytes)) if bytes > 0 => Some(serde_json::from_str(&line).unwrap()),
                _ => None,
            }
        }

        async fn ask(&mut self, requests: &[Value]) -> HashMap<i64, Value> {
            for request in requests {
                self.send(&request.to_string()).await;
            }
            let mut responses = HashMap::new();
            while responses.len() < requests.len() {
                let response = self.receive().await.expect("a response for every request");
                responses.insert(response["id"].as_i64().unwrap(), response);
            }
            responses
        }
    }

    fn call(id: i64, name: &str, arguments: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}})
    }

    #[tokio::test]
    async fn initialize_describes_the_server() {
        let mut client = Client::connect();
        client.send(INITIALIZE).await;
        let result = client.receive().await.unwrap()["result"].clone();
        assert_eq!(result["protocolVersion"], "2025-06-18");
        assert_eq!(result["serverInfo"]["name"], "pentacore");
        assert!(result["capabilities"]["tools"].is_object());
        assert!(
            result["instructions"]
                .as_str()
                .unwrap()
                .contains("working memory")
        );
    }

    #[tokio::test]
    async fn an_unknown_protocol_version_is_answered_with_one_the_server_speaks() {
        let mut client = Client::connect();
        client
            .send(&INITIALIZE.replace("2025-06-18", "1999-01-01"))
            .await;
        let result = client.receive().await.unwrap()["result"].clone();
        let offered = result["protocolVersion"].as_str().unwrap();
        assert!(offered != "1999-01-01" && offered.len() == 10, "{offered}");
    }

    #[tokio::test]
    async fn tools_are_listed_with_schemas_and_hints() {
        let mut client = Client::initialized().await;
        let responses = client
            .ask(&[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})])
            .await;
        let listed = responses[&1]["result"]["tools"].as_array().unwrap().clone();
        assert_eq!(listed.len(), Tool::ALL.len());

        let by_name = |name: &str| {
            listed
                .iter()
                .find(|tool| tool["name"] == name)
                .unwrap()
                .clone()
        };
        assert_eq!(by_name("find")["annotations"]["readOnlyHint"], true);
        assert_eq!(by_name("note")["annotations"]["readOnlyHint"], false);
        assert_eq!(by_name("note")["annotations"]["destructiveHint"], false);
        assert_eq!(
            by_name("forget_note")["annotations"]["destructiveHint"],
            true
        );
        assert_eq!(
            by_name("note")["inputSchema"]["required"],
            json!(["kind", "title"])
        );
    }

    #[tokio::test]
    async fn tool_calls_return_content_and_flag_failures() {
        let mut client = Client::initialized().await;
        let created = client
            .ask(&[call(1, "note", json!({"kind": "goal", "title": "Ship"}))])
            .await;
        let result = &created[&1]["result"];
        assert_eq!(result["isError"], false);
        let note: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(note["title"], "Ship");

        let responses = client
            .ask(&[
                call(2, "get_note", json!({"id": 999})),
                call(3, "note", json!({"kind": "goal"})),
                json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "resume"}}),
            ])
            .await;
        assert_eq!(responses[&2]["result"]["isError"], true);
        assert_eq!(
            responses[&2]["result"]["content"][0]["text"],
            "note 999 not found"
        );
        assert_eq!(responses[&3]["result"]["isError"], true);
        assert_eq!(responses[&4]["result"]["isError"], false);
    }

    #[tokio::test]
    async fn protocol_errors_use_json_rpc_codes() {
        let mut client = Client::initialized().await;
        let responses = client
            .ask(&[
                call(1, "rm_rf", json!({})),
                json!({"jsonrpc": "2.0", "id": 2, "method": "no/such/method"}),
                json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
            ])
            .await;
        assert_eq!(responses[&1]["error"]["code"], -32602);
        assert_eq!(responses[&2]["error"]["code"], -32601);
        assert_eq!(responses[&3]["result"], json!({}));
    }

    #[tokio::test]
    async fn a_notification_is_never_answered_and_never_runs_a_tool() {
        let mut client = Client::initialized().await;
        client
            .send(r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"note","arguments":{"kind":"goal","title":"sneaky"}}}"#)
            .await;
        let found = client
            .ask(&[call(1, "find", json!({"query": "sneaky"}))])
            .await;
        let text = found[&1]["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(text).unwrap()["hits"],
            json!([])
        );
        assert!(client.receive().await.is_none());
    }

    #[tokio::test]
    async fn a_message_at_the_size_limit_is_served() {
        let mut client = Client::initialized().await;
        let ping = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;
        let padded = format!("{ping}{}", " ".repeat(MAX_LINE_BYTES - ping.len()));
        assert_eq!(padded.len(), MAX_LINE_BYTES);
        client.send(&padded).await;
        assert_eq!(client.receive().await.unwrap()["id"], 7);
    }

    #[tokio::test]
    async fn an_oversized_message_ends_the_session() {
        let mut client = Client::initialized().await;
        client.send(&"x".repeat(MAX_LINE_BYTES + 1)).await;
        client
            .send(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#)
            .await;
        assert!(client.receive().await.is_none());
        let ended = tokio::time::timeout(Duration::from_secs(5), client.server).await;
        assert!(
            ended.is_ok(),
            "the server must stop reading after an oversized message"
        );
    }
}
