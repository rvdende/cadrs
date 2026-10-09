//! The MCP server (Model Context Protocol): AI assistants (Claude Desktop, Claude Code, Cursor,
//! …) drive the running app through it, as the Preferences' **Enable MCP server** turns it on.
//!
//! - It serves Streamable HTTP on a loopback address (`http://127.0.0.1:<port>/mcp`) from a
//!   thread of its own with its own tokio runtime, so the app needs neither.
//! - Each tool call becomes a [`Request`] holding a typed [`Call`]. The app takes the requests
//!   off [`Server::try_next`] on its main thread, runs them through its command layer (so they
//!   are undoable like any edit) and answers with [`Request::reply`]: a JSON value, or an error
//!   the assistant reads; [`Request::reply_image`] answers with a PNG (a screenshot) the
//!   assistant sees.
//! - The tools' parameters are the structs in [`tools`]; their JSON schemas (from their doc
//!   comments) are what the assistant sees.

use std::io;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::StreamableHttpServerConfig;
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use tokio_util::sync::CancellationToken;

pub mod tools;

use tools::*;

/// The default port (`http://127.0.0.1:7680/mcp`).
pub const DEFAULT_PORT: u16 = 7680;

/// How long a call may wait for the app (a rebuild included) before the assistant is told so.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// What the assistant asked for.
#[derive(Debug, Clone)]
pub enum Call {
    CreateDocument(CreateDocument),
    OpenDocument(OpenDocument),
    GetDocument,
    AddPartStudio(AddPartStudio),
    AddSketch(AddSketch),
    Extrude(Extrude),
    Screenshot(Screenshot),
}

/// What the app answers.
#[derive(Debug, Clone)]
enum Answer {
    Json(serde_json::Value),
    /// A PNG, with a line about it.
    Image { png: Vec<u8>, caption: String },
}

/// A tool call waiting for the app.
pub struct Request {
    pub call: Call,
    reply: tokio::sync::oneshot::Sender<Result<Answer, String>>,
}

impl Request {
    /// Answers the call: what it did, or why it failed.
    pub fn reply(self, result: Result<serde_json::Value, String>) {
        let _ = self.reply.send(result.map(Answer::Json));
    }

    /// Answers with a PNG image (and a caption).
    pub fn reply_image(self, png: Vec<u8>, caption: String) {
        let _ = self.reply.send(Ok(Answer::Image { png, caption }));
    }
}

/// The running server. Dropping it stops it.
pub struct Server {
    addr: SocketAddr,
    requests: Mutex<mpsc::Receiver<Request>>,
    cancel: CancellationToken,
}

impl Server {
    /// Starts serving on `addr` (a loopback address). Fails if the port can't be bound.
    pub fn start(addr: SocketAddr) -> io::Result<Server> {
        let listener = std::net::TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let (tx, rx) = mpsc::channel();
        let cancel = CancellationToken::new();
        let ct = cancel.clone();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        std::thread::Builder::new().name("cadrs-mcp".into()).spawn(move || {
            runtime.block_on(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("cadrs mcp: {e}");
                        return;
                    }
                };
                let service: StreamableHttpService<Cadrs, LocalSessionManager> = StreamableHttpService::new(
                    move || Ok(Cadrs::new(tx.clone())),
                    Default::default(),
                    StreamableHttpServerConfig::default().with_sse_keep_alive(None).with_cancellation_token(ct.child_token()),
                );
                let router = axum::Router::new().nest_service("/mcp", service);
                let _ = axum::serve(listener, router).with_graceful_shutdown(async move { ct.cancelled_owned().await }).await;
            });
        })?;
        Ok(Server { addr, requests: Mutex::new(rx), cancel })
    }

    /// Where clients connect: `http://127.0.0.1:7680/mcp`.
    pub fn url(&self) -> String {
        url(self.addr)
    }

    /// The next call waiting for the app, if any.
    pub fn try_next(&self) -> Option<Request> {
        self.requests.lock().ok()?.try_recv().ok()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// The endpoint's URL for an address.
pub fn url(addr: SocketAddr) -> String {
    format!("http://{addr}/mcp")
}

/// The tools: each sends its call to the app and waits for the answer.
#[derive(Clone)]
struct Cadrs {
    tx: mpsc::Sender<Request>,
    tool_router: ToolRouter<Self>,
}

impl Cadrs {
    fn new(tx: mpsc::Sender<Request>) -> Self {
        Self { tx, tool_router: Self::tool_router() }
    }

    async fn call(&self, call: Call) -> Result<CallToolResult, ErrorData> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        if self.tx.send(Request { call, reply }).is_err() {
            return Ok(CallToolResult::error(vec![ContentBlock::text("cadrs is shutting down")]));
        }
        Ok(match tokio::time::timeout(CALL_TIMEOUT, answer).await {
            Ok(Ok(Ok(Answer::Json(v)))) => CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(&v).unwrap_or_default())]),
            Ok(Ok(Ok(Answer::Image { png, caption }))) => {
                use base64::Engine;
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                CallToolResult::success(vec![ContentBlock::image(data, "image/png"), ContentBlock::text(caption)])
            }
            Ok(Ok(Err(e))) => CallToolResult::error(vec![ContentBlock::text(e)]),
            Ok(Err(_)) => CallToolResult::error(vec![ContentBlock::text("cadrs dropped the call")]),
            Err(_) => CallToolResult::error(vec![ContentBlock::text("cadrs did not answer in time")]),
        })
    }
}

#[tool_router]
impl Cadrs {
    #[tool(description = "Create a new cadrs document and open it in the app. It starts with a Part Studio \
        (\"Part Studio 1\", the active tab) and an Assembly. Any open document is saved first.")]
    async fn create_document(&self, Parameters(p): Parameters<CreateDocument>) -> Result<CallToolResult, ErrorData> {
        self.call(Call::CreateDocument(p)).await
    }

    #[tool(description = "Open a stored document by its name or id (any open document is saved first).")]
    async fn open_document(&self, Parameters(p): Parameters<OpenDocument>) -> Result<CallToolResult, ErrorData> {
        self.call(Call::OpenDocument(p)).await
    }

    #[tool(description = "Describe the open document: its tabs, and for the active Part Studio its features \
        (with any rebuild errors) and parts (volume in mm³, bounding box in mm).")]
    async fn get_document(&self) -> Result<CallToolResult, ErrorData> {
        self.call(Call::GetDocument).await
    }

    #[tool(description = "Add a Part Studio tab to the open document and make it the active tab.")]
    async fn add_part_studio(&self, Parameters(p): Parameters<AddPartStudio>) -> Result<CallToolResult, ErrorData> {
        self.call(Call::AddPartStudio(p)).await
    }

    #[tool(description = "Add a sketch on a default plane (Top, Front or Right) of a Part Studio, with rectangles, \
        circles, polygons, lines and arcs in the plane's 2D coordinates (mm). Top: x = world X, y = world Y \
        (normal +Z). Front: x = world X, y = world Z (normal -Y). Right: x = world Y, y = world Z (normal +X). \
        Returns the sketch's name and its closed regions, each with a point inside it to pass to extrude.")]
    async fn add_sketch(&self, Parameters(p): Parameters<AddSketch>) -> Result<CallToolResult, ErrorData> {
        self.call(Call::AddSketch(p)).await
    }

    #[tool(description = "Extrude regions of a sketch into a solid: a new part, or added to, removed from or \
        intersected with the parts it meets. Returns the feature's name, any rebuild error, and the Part \
        Studio's parts after it.")]
    async fn extrude(&self, Parameters(p): Parameters<Extrude>) -> Result<CallToolResult, ErrorData> {
        self.call(Call::Extrude(p)).await
    }

    #[tool(description = "Take a screenshot of the whole cadrs window (toolbar, feature list, 3D view, dialogs) \
        as it is now, scaled to fit max_size pixels. Use it to check what a change looks like.")]
    async fn screenshot(&self, Parameters(p): Parameters<Screenshot>) -> Result<CallToolResult, ErrorData> {
        self.call(Call::Screenshot(p)).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Cadrs {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "cadrs is a parametric CAD app (Onshape-style; units mm). A document has tabs (Part Studios, \
             Assemblies); a Part Studio has a feature list (sketches, extrudes, …) that rebuilds into parts. \
             Every change is an undoable step in the open document. Typical start: create_document, \
             add_sketch on Top, extrude its region, then get_document to check the parts and screenshot to see \
             them.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_binds_a_loopback_port() {
        let server = Server::start("127.0.0.1:0".parse().unwrap()).unwrap();
        assert!(server.url().starts_with("http://127.0.0.1:") && server.url().ends_with("/mcp"));
        assert!(server.try_next().is_none());
    }

    #[test]
    fn calls_reach_the_app_and_answers_reach_the_tool() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let (tx, rx) = mpsc::channel();
        let cadrs = Cadrs::new(tx);
        let call = rt.spawn(async move { cadrs.call(Call::GetDocument).await });
        let req = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(req.call, Call::GetDocument));
        req.reply(Ok(serde_json::json!({ "name": "Doc" })));
        let result = rt.block_on(call).unwrap().unwrap();
        assert_ne!(result.is_error, Some(true));
        // An error answer is a tool error the assistant reads, not a protocol error.
        let (tx, rx) = mpsc::channel();
        let cadrs = Cadrs::new(tx);
        let call = rt.spawn(async move { cadrs.call(Call::GetDocument).await });
        rx.recv_timeout(Duration::from_secs(5)).unwrap().reply(Err("no document is open".into()));
        assert_eq!(rt.block_on(call).unwrap().unwrap().is_error, Some(true));
        // A screenshot comes back as an image block and its caption.
        let (tx, rx) = mpsc::channel();
        let cadrs = Cadrs::new(tx);
        let call = rt.spawn(async move { cadrs.call(Call::Screenshot(Screenshot { max_size: None })).await });
        rx.recv_timeout(Duration::from_secs(5)).unwrap().reply_image(vec![0x89, b'P', b'N', b'G'], "1600 × 1000".into());
        let result = rt.block_on(call).unwrap().unwrap();
        assert_eq!(result.content.len(), 2);
        assert!(result.content[0].as_image().is_some_and(|i| i.mime_type == "image/png"));
    }
}
