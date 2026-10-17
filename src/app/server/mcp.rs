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
            Tool::ALL
                .iter()
                .filter(|tool| self.brain.all_tools || tool.is_core())
                .map(|tool| definition(*tool))
                .collect(),
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
