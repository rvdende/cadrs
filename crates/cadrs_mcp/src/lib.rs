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

pub mod fuzz;
pub mod tools;

use tools::*;

/// The default port (`http://127.0.0.1:7680/mcp`).
pub const DEFAULT_PORT: u16 = 7680;

/// How long a call may wait for the app (a rebuild included) before the assistant is told so.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// The command table: each entry is one command, its description, name and parameters (a struct
/// in [`tools`], whose doc comments are its JSON schema). From it come [`Call`] (a variant a
/// command), the MCP tools (a tool a command), [`Call::parse`] (a command by name, as scripts
/// and `cadrs --run` give it) and [`commands`], so a command added here is all of them.
macro_rules! commands {
    ($($description:tt $name:ident => $variant:ident $(($params:ident))?;)*) => {
        /// What the assistant or a script asked for: a command and its parameters.
        #[derive(Debug, Clone)]
        pub enum Call {
            $(#[doc = $description] $variant $(($params))?,)*
        }

        impl Call {
            /// The command's name (its MCP tool's).
            pub fn name(&self) -> &'static str {
                match self {
                    $(Call::$variant { .. } => stringify!($name),)*
                }
            }

            /// A command by name, with its parameters as JSON: an object of the tool's arguments,
            /// `null` or `{}` for none.
            pub fn parse(name: &str, params: serde_json::Value) -> Result<Call, String> {
                match name {
                    $(stringify!($name) => commands!(@parse params $variant $($params)?),)*
                    _ => Err(format!("unknown command {name:?}")),
                }
            }
        }

        #[tool_router]
        impl Cadrs {
            $(
                #[tool(description = $description)]
                async fn $name(
                    &self
                    $(, Parameters(p): Parameters<$params>)?
                ) -> Result<CallToolResult, ErrorData> {
                    self.call(Call::$variant $(({ let p: $params = p; p }))?).await
                }
            )*
        }
    };
    (@parse $json:ident $variant:ident $params:ident) => {
        serde_json::from_value::<$params>(match $json {
            serde_json::Value::Null => serde_json::json!({}),
            v => v,
        })
        .map(Call::$variant)
        .map_err(|e| format!("bad parameters: {e}"))
    };
    (@parse $json:ident $variant:ident) => {
        match $json {
            serde_json::Value::Null => Ok(Call::$variant),
            serde_json::Value::Object(o) if o.is_empty() => Ok(Call::$variant),
            _ => Err("it takes no parameters".into()),
        }
    };
}

/// A command as a client sees it: its name, description and parameters' JSON schema.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Every command, by name (what MCP's `tools/list` lists, and `cadrs --commands`).
pub fn commands() -> Vec<CommandInfo> {
    Cadrs::tool_router()
        .list_all()
        .into_iter()
        .map(|t| CommandInfo {
            name: t.name.to_string(),
            description: t.description.map(|d| d.to_string()).unwrap_or_default(),
            parameters: serde_json::Value::Object((*t.input_schema).clone()),
        })
        .collect()
}

/// What the app answers.
#[derive(Debug, Clone)]
pub enum Answer {
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

    /// A call from inside the app (a script's command, `cadrs --run`), and where its answer
    /// arrives: the app runs it as it runs an assistant's.
    pub fn local(call: Call) -> (Request, Answered) {
        let (reply, answer) = tokio::sync::oneshot::channel();
        (Request { call, reply }, Answered(answer))
    }
}

/// Where the answer to a [`Request::local`] call arrives.
pub struct Answered(tokio::sync::oneshot::Receiver<Result<Answer, String>>);

impl Answered {
    /// The answer once the app has given it (an error if it dropped the call).
    pub fn try_take(&mut self) -> Option<Result<Answer, String>> {
        use tokio::sync::oneshot::error::TryRecvError;
        match self.0.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Closed) => Some(Err("cadrs dropped the call".into())),
        }
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

commands! {
    "Create a new cadrs document and open it in the app. It starts with a Part Studio \
        (\"Part Studio 1\", the active tab) and an Assembly. Any open document is saved first."
    create_document => CreateDocument(CreateDocument);

    "Open a stored document by its name or id (any open document is saved first)."
    open_document => OpenDocument(OpenDocument);

    "Describe the open document: its tabs, and for the active Part Studio its features \
        (with any rebuild errors) and parts (volume in mm³, bounding box in mm)."
    get_document => GetDocument;

    "The app's frame rate over the last two seconds: fps, the number of frames, and \
        frame times in ms (mean, median, 95th percentile, max); and where the last second's frames went \
        (profile): CPU ms a frame of the main loop's schedules, the slowest schedules and systems (with \
        their most in one frame and runs a frame), and the GPU ms of the slowest render passes. The CPU \
        times are only there when cadrs started with them on (profile.cpu.timed; they slow it down): \
        the Time systems preference, or CADRS_FRAME_PROFILE=1. The panel at the bottom left of the 3D \
        view shows the same (F12 steps through it, the fps line, nothing)."
    get_frame_stats => GetFrameStats(GetFrameStats);

    "Create a drawing of a Part Studio and open it as a tab: its front, top, side and \
        isometric views (four_views) on a sheet, at the largest scale that fits or at the given scale. Returns \
        the drawing's name and its views (names, ids, anchors and frames) for add_annotation."
    create_drawing => CreateDrawing(CreateDrawing);

    "Add an annotation to a drawing view: a dimension (Diameter, Radius, Distance or Angle, \
        between edges or points of the part), in the view's 2D coordinates (mm). Returns the annotation's id."
    add_annotation => AddAnnotation(AddAnnotation);

    "Rename a tab: a Part Studio (the title block of its drawings shows the name) or a drawing."
    rename_tab => RenameTab(RenameTab);

    "Delete a tab. A Part Studio or Assembly that an assembly still has instances of \
        is refused (delete those instances first)."
    delete_tab => DeleteTab(TabName);

    "Describe an Assembly tab: its instances (name, id, source tab and part, position \
        in mm, fixed, hidden, suppressed) and its mate features."
    get_assembly => GetAssembly(AssemblyName);

    "Insert into an assembly a whole Part Studio (one rigid instance of all its parts) \
        or one of its parts, at a position (mm; omitted: the origin, so the parts sit where they are in their \
        Part Studio), optionally fixed. One undo step. Returns the assembly as get_assembly does."
    insert_instance => InsertInstance(InsertInstance);

    "Add a Part Studio tab to the open document and make it the active tab."
    add_part_studio => AddPartStudio(AddPartStudio);

    "Add an empty Assembly tab to the open document (name omitted: the next default, \
        \"Assembly 2\"). insert_instance puts Part Studios or parts into it. Returns the assembly as \
        get_assembly does."
    add_assembly => AddAssembly(AddPartStudio);

    "Add a sketch on a default plane (Top, Front or Right) of a Part Studio, with rectangles, \
        circles, polygons, lines and arcs in the plane's 2D coordinates (mm). Top: x = world X, y = world Y \
        (normal +Z). Front: x = world X, y = world Z (normal -Y). Right: x = world Y, y = world Z (normal +X). \
        Outlines are closed loops of lines and arcs (rounded corners, fillets); circles inside a loop are \
        holes. Returns the sketch's name and its closed regions, each with a point inside it to pass to extrude."
    add_sketch => AddSketch(AddSketch);

    "Extrude regions of a sketch into a solid: a new part, or added to, removed from or \
        intersected with the parts it meets. Returns the feature's name, any rebuild error, and the Part \
        Studio's parts after it."
    extrude => Extrude(Extrude);

    "Add any feature to a Part Studio from JSON: its type and the fields that differ from \
        the type's defaults (lengths in mm, angles in degrees). Returns its name, any rebuild error and the parts."
    add_feature => AddFeature(AddFeature);

    "Change a feature: the given JSON fields are merged over its current ones."
    edit_feature => EditFeature(EditFeature);

    "Make any edit the sketch engine makes, on a sketch: geometry, trims and fillets, \
        mirrors and offsets, constraints, dimensions and text (the op's reference is in its doc). Returns the \
        ids of what the edit created and removed, the sketch's regions, and whether it is fully constrained or \
        has conflicting constraints or dimensions. Read a sketch's ids and state with get_feature."
    edit_sketch => EditSketch(EditSketch);

    "A feature's full JSON (its type and every field), to learn the format or edit it."
    get_feature => GetFeature(FeatureName);

    "Rename a feature of a Part Studio (one undo step)."
    rename_feature => RenameFeature(RenameFeature);

    "Show or hide a sketch or a Plane feature (its eye in the feature list)."
    set_feature_visible => SetFeatureVisible(SetFeatureVisible);

    "Move a Part Studio's rollback bar (one undo step): only the features above it are built, so the Part Studio shows what it was at that point. Give `index` (how many features stay above it) or `after` (a feature's name); neither: roll to the end. Returns where the bar is, the last feature built and the next one."
    set_rollback => SetRollback(SetRollback);

    "Scroll the Part Studio's feature list to a feature's row (opening its folder), as double-clicking its part in the view does. Returns once asked; the list scrolls over the next frames."
    show_feature => ShowFeature(FeatureName);

    "List the app's named UI nodes (panels, lists, buttons, the viewport area) with their on-screen rectangle in logical pixels (x, y of the top-left corner, w, h), whether they are shown, and their parent's name. Filter by part of the name. Use it to find what is laid out wrong: a panel taller than the window, a list that does not scroll."
    get_ui_layout => GetUiLayout(GetUiLayout);

    "Change this machine's preferences, as the Preferences dialog's OK does (they are saved): vsync (true: frames wait for the display's refresh; false: drawn as fast as they are made, so get_frame_stats shows what a frame costs). Give only what changes; it answers with all the preferences in force."
    set_preferences => SetPreferences(SetPreferences);

    "Rename a part and/or set its colour ([r, g, b], with an optional alpha for \
        see-through parts such as glass). Returns the part's name and id."
    set_part => SetPart(SetPart);

    "Delete a feature."
    delete_feature => DeleteFeature(FeatureName);

    "Every face of the Part Studio's parts: its reference (\"ref\", for features and \
        sketches), part, surface type, area, centre, plane normal, axis and radius."
    list_faces => ListFaces(PartStudio);

    "Every edge of the Part Studio's parts: its reference (\"ref\", for fillets, chamfers \
        and directions), part, ends, length, and circle (centre, normal, radius) if it is one."
    list_edges => ListEdges(PartStudio);

    "Undo the open document's last change."
    undo => Undo;

    "Make a named version of the open document as it is now, to go back to later with restore_version."
    create_version => CreateVersion(CreateVersion);

    "List the open document's versions (name, description, history entry, time)."
    list_versions => ListVersions;

    "Put the whole open document back as it was at a version (one undoable step; the history and later versions are kept). Answers like get_document once the Part Studio has rebuilt."
    restore_version => RestoreVersion(VersionName);

    "Import a STEP file as a new document (its parts in a Part Studio) and open it."
    import_step => ImportStep(ImportStep);

    "Write the Part Studio's parts to a STEP file."
    export_step => ExportStep(ExportStep);

    "Run a script of UI steps in the app, as its scenario files do: Click(ui(\"name\")) on a \
        named element, Click(xyz(x, y, z)) on a 3D point, ClickAt(x, y), DoubleClick, RightClick, Drag(from, to), \
        MoveTo(x, y), Hover, Key(\"Ctrl+Z\"), Type(\"text\"), Wait(frames), WaitFor(\"name\"), \
        ExpectText(\"name\", \"text\"), Custom(\"command\"), Command(\"tool\", \"{json params}\") (one of these \
        tools) and Screenshot(\"label\"). Each step waits for the app \
        to settle. Returns the screenshots' paths (the last one as an image), or the step that failed."
    run_scenario => RunScenario(RunScenario);

    "A PCB Studio board: its outline's box, thickness, nets, every footprint (reference, \
        value, place in mm, angle, side, on the board or not, its pads with their place, net and drill), \
        counts of tracks, vias and zones, and the Part Studios it is derived into with their warnings \
        (components running into parts)."
    get_board => GetBoard(BoardName);

    "Fitting a board derived into a Part Studio: for each part, the pins and holes \
        standing square to the board (where their axes meet it, in the layout's mm, and radius), and the \
        collisions of its components with the parts."
    get_board_fit => GetBoardFit(BoardFit);

    "Derive a PCB Studio board into a Part Studio: the board's solid and its \
        components' 3D models as parts, at a mate connector or on a sketch's plane. It follows the layout."
    derive_board => DeriveBoard(DeriveBoard);

    "Create a PCB board from a Part Studio sketch: its outline is the sketch's largest \
        region (holes included) and follows the sketch; the board is derived onto the sketch's plane."
    create_board_from_sketch => CreateBoardFromSketch(CreateBoardFromSketch);

    "Move a footprint on a board (one undo step): to x, y (mm), angle, side; or with pad \
        and onto_part, so the pad is centred on a pin or hole of that part (in a Part Studio the board is \
        derived into). Its tracks' ends follow. Returns the board."
    move_footprint => MoveFootprint(MoveFootprint);

    "Add a footprint to a board: a copy of one on it, with a new reference, value, place \
        and pad nets."
    add_footprint => AddFootprint(AddFootprint);

    "Remove a footprint from a board, and its symbol from the schematic."
    remove_footprint => RemoveFootprint(RemoveFootprint);

    "Make a board's outline follow a Part Studio sketch (its largest region), moving the \
        layout's parts with the outline's centre; or, without a sketch, break the link."
    link_outline => LinkOutline(LinkOutline);

    "Take a screenshot of the whole cadrs window (toolbar, feature list, 3D view, dialogs) \
        as it is now, scaled to fit max_size pixels. Use it to check what a change looks like."
    screenshot => Screenshot(Screenshot);
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
    fn every_command_is_a_tool_and_parses_by_name() {
        let listed = commands();
        assert!(listed.len() > 40, "{} commands", listed.len());
        for c in &listed {
            assert!(!c.description.is_empty(), "{} has no description", c.name);
            assert_eq!(c.parameters["type"], "object", "{}", c.name);
            // Known by name: without its parameters it is a parameter error at worst.
            match Call::parse(&c.name, serde_json::Value::Null) {
                Ok(call) => assert_eq!(call.name(), c.name),
                Err(e) => assert!(e.starts_with("bad parameters"), "{}: {e}", c.name),
            }
        }
        let e = Call::parse("no_such_command", serde_json::Value::Null).unwrap_err();
        assert!(e.contains("unknown command"), "{e}");
        assert!(Call::parse("get_document", serde_json::json!({})).is_ok());
        assert!(Call::parse("get_document", serde_json::json!({ "x": 1 })).is_err());
        let call = Call::parse(
            "add_sketch",
            serde_json::json!({ "plane": "Top", "entities": [] }),
        )
        .unwrap();
        assert!(matches!(&call, Call::AddSketch(p) if p.plane == Some(Plane::Top)));
    }

    #[test]
    fn hostile_parameters_never_panic_the_parser() {
        let mut rng = fuzz::Rng::new(102);
        let mut parsed = 0;
        for c in commands() {
            for _ in 0..300 {
                let params = fuzz::params(&c.parameters, &mut rng);
                if let Ok(call) = Call::parse(&c.name, params) {
                    assert_eq!(call.name(), c.name);
                    parsed += 1;
                }
            }
        }
        // Most are refused, but enough get through to reach the app's fuzz test.
        assert!(parsed > 1000, "only {parsed} parsed");
    }

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
