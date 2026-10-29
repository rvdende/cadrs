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
use rmcp::transport::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
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
    GetFrameStats(GetFrameStats),
    AddPartStudio(AddPartStudio),
    AddAssembly(AddPartStudio),
    AddSketch(AddSketch),
    Extrude(Extrude),
    Screenshot(Screenshot),
    AddFeature(AddFeature),
    EditFeature(EditFeature),
    EditSketch(EditSketch),
    GetFeature(FeatureName),
    DeleteFeature(FeatureName),
    RenameFeature(RenameFeature),
    SetPart(SetPart),
    SetFeatureVisible(SetFeatureVisible),
    SetRollback(SetRollback),
    ShowFeature(FeatureName),
    GetUiLayout(GetUiLayout),
    SetPreferences(SetPreferences),
    ListFaces(PartStudio),
    ListEdges(PartStudio),
    Undo,
    CreateVersion(CreateVersion),
    ListVersions,
    RestoreVersion(VersionName),
    ImportStep(ImportStep),
    ExportStep(ExportStep),
    RunScenario(RunScenario),
    CreateDrawing(CreateDrawing),
    AddAnnotation(AddAnnotation),
    RenameTab(RenameTab),
    DeleteTab(TabName),
    GetAssembly(AssemblyName),
    InsertInstance(InsertInstance),
    GetBoard(BoardName),
    GetBoardFit(BoardFit),
    DeriveBoard(DeriveBoard),
    CreateBoardFromSketch(CreateBoardFromSketch),
    MoveFootprint(MoveFootprint),
    AddFootprint(AddFootprint),
    RemoveFootprint(RemoveFootprint),
    LinkOutline(LinkOutline),
}

/// What the app answers.
#[derive(Debug, Clone)]
enum Answer {
    Json(serde_json::Value),
    /// A PNG, with a line about it.
    Image {
        png: Vec<u8>,
        caption: String,
    },
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
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        std::thread::Builder::new()
            .name("cadrs-mcp".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let listener = match tokio::net::TcpListener::from_std(listener) {
                        Ok(l) => l,
                        Err(e) => {
                            eprintln!("cadrs mcp: {e}");
                            return;
                        }
                    };
                    let service: StreamableHttpService<Cadrs, LocalSessionManager> =
                        StreamableHttpService::new(
                            move || Ok(Cadrs::new(tx.clone())),
                            Default::default(),
                            StreamableHttpServerConfig::default()
                                .with_sse_keep_alive(None)
                                .with_cancellation_token(ct.child_token()),
                        );
                    let router = axum::Router::new().nest_service("/mcp", service);
                    let _ = axum::serve(listener, router)
                        .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                        .await;
                });
            })?;
        Ok(Server {
            addr,
            requests: Mutex::new(rx),
            cancel,
        })
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
        Self {
            tx,
            tool_router: Self::tool_router(),
        }
    }

    async fn call(&self, call: Call) -> Result<CallToolResult, ErrorData> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        if self.tx.send(Request { call, reply }).is_err() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "cadrs is shutting down",
            )]));
        }
        Ok(match tokio::time::timeout(CALL_TIMEOUT, answer).await {
            Ok(Ok(Ok(Answer::Json(v)))) => CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap_or_default(),
            )]),
            Ok(Ok(Ok(Answer::Image { png, caption }))) => {
                use base64::Engine;
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                CallToolResult::success(vec![
                    ContentBlock::image(data, "image/png"),
                    ContentBlock::text(caption),
                ])
            }
            Ok(Ok(Err(e))) => CallToolResult::error(vec![ContentBlock::text(e)]),
            Ok(Err(_)) => CallToolResult::error(vec![ContentBlock::text("cadrs dropped the call")]),
            Err(_) => {
                CallToolResult::error(vec![ContentBlock::text("cadrs did not answer in time")])
            }
        })
    }
}

#[tool_router]
impl Cadrs {
    #[tool(
        description = "Create a new cadrs document and open it in the app. It starts with a Part Studio \
        (\"Part Studio 1\", the active tab) and an Assembly. Any open document is saved first."
    )]
    async fn create_document(
        &self,
        Parameters(p): Parameters<CreateDocument>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::CreateDocument(p)).await
    }

    #[tool(
        description = "Open a stored document by its name or id (any open document is saved first)."
    )]
    async fn open_document(
        &self,
        Parameters(p): Parameters<OpenDocument>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::OpenDocument(p)).await
    }

    #[tool(
        description = "Describe the open document: its tabs, and for the active Part Studio its features \
        (with any rebuild errors) and parts (volume in mm³, bounding box in mm)."
    )]
    async fn get_document(&self) -> Result<CallToolResult, ErrorData> {
        self.call(Call::GetDocument).await
    }

    #[tool(
        description = "The app's frame rate over the last two seconds: fps, the number of frames, and \
        frame times in ms (mean, median, 95th percentile, max); and where the last second's frames went \
        (profile): CPU ms a frame of the main loop's schedules, the slowest schedules and systems (with \
        their most in one frame and runs a frame), and the GPU ms of the slowest render passes. The CPU \
        times are only there when cadrs started with them on (profile.cpu.timed; they slow it down): \
        the Time systems preference, or CADRS_FRAME_PROFILE=1. The panel at the bottom left of the 3D \
        view shows the same (F12 steps through it, the fps line, nothing)."
    )]
    async fn get_frame_stats(
        &self,
        Parameters(p): Parameters<GetFrameStats>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::GetFrameStats(p)).await
    }

    #[tool(
        description = "Create a drawing of a Part Studio and open it as a tab: its front, top, side and \
        isometric views (four_views) on a sheet, at the largest scale that fits or at the given scale. Returns \
        the drawing's name and its views (names, ids, anchors and frames) for add_annotation."
    )]
    async fn create_drawing(
        &self,
        Parameters(p): Parameters<CreateDrawing>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::CreateDrawing(p)).await
    }

    #[tool(
        description = "Add an annotation to a drawing view: a dimension (Diameter, Radius, Distance or Angle, \
        between edges or points of the part), in the view's 2D coordinates (mm). Returns the annotation's id."
    )]
    async fn add_annotation(
        &self,
        Parameters(p): Parameters<AddAnnotation>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::AddAnnotation(p)).await
    }

    #[tool(
        description = "Rename a tab: a Part Studio (the title block of its drawings shows the name) or a drawing."
    )]
    async fn rename_tab(
        &self,
        Parameters(p): Parameters<RenameTab>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::RenameTab(p)).await
    }

    #[tool(
        description = "Delete a tab. A Part Studio or Assembly that an assembly still has instances of \
        is refused (delete those instances first)."
    )]
    async fn delete_tab(
        &self,
        Parameters(p): Parameters<TabName>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::DeleteTab(p)).await
    }

    #[tool(
        description = "Describe an Assembly tab: its instances (name, id, source tab and part, position \
        in mm, fixed, hidden, suppressed) and its mate features."
    )]
    async fn get_assembly(
        &self,
        Parameters(p): Parameters<AssemblyName>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::GetAssembly(p)).await
    }

    #[tool(
        description = "Insert into an assembly a whole Part Studio (one rigid instance of all its parts) \
        or one of its parts, at a position (mm; omitted: the origin, so the parts sit where they are in their \
        Part Studio), optionally fixed. One undo step. Returns the assembly as get_assembly does."
    )]
    async fn insert_instance(
        &self,
        Parameters(p): Parameters<InsertInstance>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::InsertInstance(p)).await
    }

    #[tool(description = "Add a Part Studio tab to the open document and make it the active tab.")]
    async fn add_part_studio(
        &self,
        Parameters(p): Parameters<AddPartStudio>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::AddPartStudio(p)).await
    }

    #[tool(
        description = "Add an empty Assembly tab to the open document (name omitted: the next default, \
        \"Assembly 2\"). insert_instance puts Part Studios or parts into it. Returns the assembly as \
        get_assembly does."
    )]
    async fn add_assembly(
        &self,
        Parameters(p): Parameters<AddPartStudio>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::AddAssembly(p)).await
    }

    #[tool(
        description = "Add a sketch on a default plane (Top, Front or Right) of a Part Studio, with rectangles, \
        circles, polygons, lines and arcs in the plane's 2D coordinates (mm). Top: x = world X, y = world Y \
        (normal +Z). Front: x = world X, y = world Z (normal -Y). Right: x = world Y, y = world Z (normal +X). \
        Outlines are closed loops of lines and arcs (rounded corners, fillets); circles inside a loop are \
        holes. Returns the sketch's name and its closed regions, each with a point inside it to pass to extrude."
    )]
    async fn add_sketch(
        &self,
        Parameters(p): Parameters<AddSketch>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::AddSketch(p)).await
    }

    #[tool(
        description = "Extrude regions of a sketch into a solid: a new part, or added to, removed from or \
        intersected with the parts it meets. Returns the feature's name, any rebuild error, and the Part \
        Studio's parts after it."
    )]
    async fn extrude(
        &self,
        Parameters(p): Parameters<Extrude>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::Extrude(p)).await
    }

    #[tool(
        description = "Add any feature to a Part Studio from JSON: its type and the fields that differ from \
        the type's defaults (lengths in mm, angles in degrees). Returns its name, any rebuild error and the parts."
    )]
    async fn add_feature(
        &self,
        Parameters(p): Parameters<AddFeature>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::AddFeature(p)).await
    }

    #[tool(
        description = "Change a feature: the given JSON fields are merged over its current ones."
    )]
    async fn edit_feature(
        &self,
        Parameters(p): Parameters<EditFeature>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::EditFeature(p)).await
    }

    #[tool(
        description = "Make any edit the sketch engine makes, on a sketch: geometry, trims and fillets, \
        mirrors and offsets, constraints, dimensions and text (the op's reference is in its doc). Returns the \
        ids of what the edit created and removed, the sketch's regions, and whether it is fully constrained or \
        has conflicting constraints or dimensions. Read a sketch's ids and state with get_feature."
    )]
    async fn edit_sketch(
        &self,
        Parameters(p): Parameters<EditSketch>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::EditSketch(p)).await
    }

    #[tool(
        description = "A feature's full JSON (its type and every field), to learn the format or edit it."
    )]
    async fn get_feature(
        &self,
        Parameters(p): Parameters<FeatureName>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::GetFeature(p)).await
    }

    #[tool(description = "Rename a feature of a Part Studio (one undo step).")]
    async fn rename_feature(
        &self,
        Parameters(p): Parameters<RenameFeature>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::RenameFeature(p)).await
    }

    #[tool(description = "Show or hide a sketch or a Plane feature (its eye in the feature list).")]
    async fn set_feature_visible(
        &self,
        Parameters(p): Parameters<SetFeatureVisible>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::SetFeatureVisible(p)).await
    }

    #[tool(
        description = "Move a Part Studio's rollback bar (one undo step): only the features above it are built, so the Part Studio shows what it was at that point. Give `index` (how many features stay above it) or `after` (a feature's name); neither: roll to the end. Returns where the bar is, the last feature built and the next one."
    )]
    async fn set_rollback(
        &self,
        Parameters(p): Parameters<SetRollback>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::SetRollback(p)).await
    }

    #[tool(
        description = "Scroll the Part Studio's feature list to a feature's row (opening its folder), as double-clicking its part in the view does. Returns once asked; the list scrolls over the next frames."
    )]
    async fn show_feature(
        &self,
        Parameters(p): Parameters<FeatureName>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::ShowFeature(p)).await
    }

    #[tool(
        description = "List the app's named UI nodes (panels, lists, buttons, the viewport area) with their on-screen rectangle in logical pixels (x, y of the top-left corner, w, h), whether they are shown, and their parent's name. Filter by part of the name. Use it to find what is laid out wrong: a panel taller than the window, a list that does not scroll."
    )]
    async fn get_ui_layout(
        &self,
        Parameters(p): Parameters<GetUiLayout>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::GetUiLayout(p)).await
    }

    #[tool(
        description = "Change this machine's preferences, as the Preferences dialog's OK does (they are saved): vsync (true: frames wait for the display's refresh; false: drawn as fast as they are made, so get_frame_stats shows what a frame costs). Give only what changes; it answers with all the preferences in force."
    )]
    async fn set_preferences(
        &self,
        Parameters(p): Parameters<SetPreferences>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::SetPreferences(p)).await
    }

    #[tool(
        description = "Rename a part and/or set its colour ([r, g, b], with an optional alpha for \
        see-through parts such as glass). Returns the part's name and id."
    )]
    async fn set_part(
        &self,
        Parameters(p): Parameters<SetPart>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::SetPart(p)).await
    }

    #[tool(description = "Delete a feature.")]
    async fn delete_feature(
        &self,
        Parameters(p): Parameters<FeatureName>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::DeleteFeature(p)).await
    }

    #[tool(
        description = "Every face of the Part Studio's parts: its reference (\"ref\", for features and \
        sketches), part, surface type, area, centre, plane normal, axis and radius."
    )]
    async fn list_faces(
        &self,
        Parameters(p): Parameters<PartStudio>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::ListFaces(p)).await
    }

    #[tool(
        description = "Every edge of the Part Studio's parts: its reference (\"ref\", for fillets, chamfers \
        and directions), part, ends, length, and circle (centre, normal, radius) if it is one."
    )]
    async fn list_edges(
        &self,
        Parameters(p): Parameters<PartStudio>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::ListEdges(p)).await
    }

    #[tool(description = "Undo the open document's last change.")]
    async fn undo(&self) -> Result<CallToolResult, ErrorData> {
        self.call(Call::Undo).await
    }

    #[tool(
        description = "Make a named version of the open document as it is now, to go back to later with restore_version."
    )]
    async fn create_version(
        &self,
        Parameters(p): Parameters<CreateVersion>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::CreateVersion(p)).await
    }

    #[tool(
        description = "List the open document's versions (name, description, history entry, time)."
    )]
    async fn list_versions(&self) -> Result<CallToolResult, ErrorData> {
        self.call(Call::ListVersions).await
    }

    #[tool(
        description = "Put the whole open document back as it was at a version (one undoable step; the history and later versions are kept). Answers like get_document once the Part Studio has rebuilt."
    )]
    async fn restore_version(
        &self,
        Parameters(p): Parameters<VersionName>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::RestoreVersion(p)).await
    }

    #[tool(
        description = "Import a STEP file as a new document (its parts in a Part Studio) and open it."
    )]
    async fn import_step(
        &self,
        Parameters(p): Parameters<ImportStep>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::ImportStep(p)).await
    }

    #[tool(description = "Write the Part Studio's parts to a STEP file.")]
    async fn export_step(
        &self,
        Parameters(p): Parameters<ExportStep>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::ExportStep(p)).await
    }

    #[tool(
        description = "Run a script of UI steps in the app, as its scenario files do: Click(ui(\"name\")) on a \
        named element, Click(xyz(x, y, z)) on a 3D point, ClickAt(x, y), DoubleClick, RightClick, Drag(from, to), \
        MoveTo(x, y), Hover, Key(\"Ctrl+Z\"), Type(\"text\"), Wait(frames), WaitFor(\"name\"), \
        ExpectText(\"name\", \"text\"), Custom(\"command\") and Screenshot(\"label\"). Each step waits for the app \
        to settle. Returns the screenshots' paths (the last one as an image), or the step that failed."
    )]
    async fn run_scenario(
        &self,
        Parameters(p): Parameters<RunScenario>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::RunScenario(p)).await
    }

    #[tool(
        description = "A PCB Studio board: its outline's box, thickness, nets, every footprint (reference, \
        value, place in mm, angle, side, on the board or not, its pads with their place, net and drill), \
        counts of tracks, vias and zones, and the Part Studios it is derived into with their warnings \
        (components running into parts)."
    )]
    async fn get_board(
        &self,
        Parameters(p): Parameters<BoardName>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::GetBoard(p)).await
    }

    #[tool(
        description = "Fitting a board derived into a Part Studio: for each part, the pins and holes \
        standing square to the board (where their axes meet it, in the layout's mm, and radius), and the \
        collisions of its components with the parts."
    )]
    async fn get_board_fit(
        &self,
        Parameters(p): Parameters<BoardFit>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::GetBoardFit(p)).await
    }

    #[tool(
        description = "Derive a PCB Studio board into a Part Studio: the board's solid and its \
        components' 3D models as parts, at a mate connector or on a sketch's plane. It follows the layout."
    )]
    async fn derive_board(
        &self,
        Parameters(p): Parameters<DeriveBoard>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::DeriveBoard(p)).await
    }

    #[tool(
        description = "Create a PCB board from a Part Studio sketch: its outline is the sketch's largest \
        region (holes included) and follows the sketch; the board is derived onto the sketch's plane."
    )]
    async fn create_board_from_sketch(
        &self,
        Parameters(p): Parameters<CreateBoardFromSketch>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::CreateBoardFromSketch(p)).await
    }

    #[tool(
        description = "Move a footprint on a board (one undo step): to x, y (mm), angle, side; or with pad \
        and onto_part, so the pad is centred on a pin or hole of that part (in a Part Studio the board is \
        derived into). Its tracks' ends follow. Returns the board."
    )]
    async fn move_footprint(
        &self,
        Parameters(p): Parameters<MoveFootprint>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::MoveFootprint(p)).await
    }

    #[tool(
        description = "Add a footprint to a board: a copy of one on it, with a new reference, value, place \
        and pad nets."
    )]
    async fn add_footprint(
        &self,
        Parameters(p): Parameters<AddFootprint>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::AddFootprint(p)).await
    }

    #[tool(description = "Remove a footprint from a board, and its symbol from the schematic.")]
    async fn remove_footprint(
        &self,
        Parameters(p): Parameters<RemoveFootprint>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::RemoveFootprint(p)).await
    }

    #[tool(
        description = "Make a board's outline follow a Part Studio sketch (its largest region), moving the \
        layout's parts with the outline's centre; or, without a sketch, break the link."
    )]
    async fn link_outline(
        &self,
        Parameters(p): Parameters<LinkOutline>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(Call::LinkOutline(p)).await
    }

    #[tool(
        description = "Take a screenshot of the whole cadrs window (toolbar, feature list, 3D view, dialogs) \
        as it is now, scaled to fit max_size pixels. Use it to check what a change looks like."
    )]
    async fn screenshot(
        &self,
        Parameters(p): Parameters<Screenshot>,
    ) -> Result<CallToolResult, ErrorData> {
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
             them. PCB Studio boards: get_board, derive_board into a Part Studio, get_board_fit for the pins \
             and holes around, move_footprint (onto_part lines a pad up with a pin).",
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
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
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
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap()
            .reply(Err("no document is open".into()));
        assert_eq!(rt.block_on(call).unwrap().unwrap().is_error, Some(true));
        // A screenshot comes back as an image block and its caption.
        let (tx, rx) = mpsc::channel();
        let cadrs = Cadrs::new(tx);
        let call = rt.spawn(async move {
            cadrs
                .call(Call::Screenshot(Screenshot {
                    max_size: None,
                    view: None,
                    camera: None,
                }))
                .await
        });
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap()
            .reply_image(vec![0x89, b'P', b'N', b'G'], "1600 × 1000".into());
        let result = rt.block_on(call).unwrap().unwrap();
        assert_eq!(result.content.len(), 2);
        assert!(
            result.content[0]
                .as_image()
                .is_some_and(|i| i.mime_type == "image/png")
        );
    }
}
