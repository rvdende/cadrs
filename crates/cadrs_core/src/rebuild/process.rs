//! The kernel in a process of its own, so that a crash in it (a stack overflow or an abort,
//! which no `catch_unwind` stops) costs a rebuild, not the app.
//!
//! **Off by default**: the jobs run on the kernel thread of this process ([`super::run`]), as
//! tests and tools want. The app turns it on at start ([`use_process`]), and its executable
//! runs as the kernel process when started with `--kernel-process` ([`main`]).
//!
//! - **The kernel process** is the same worker loop as the thread ([`super::run`], with the
//!   rebuild session, its cache and the session snapshots), fed by requests from the app.
//!   Every job of [`super::Job`] is one request and one reply; a closure ([`super::Job::Run`])
//!   can't be sent, so kernel work other than rebuilds, exports and drawing views is a
//!   [`super::task::Task`].
//! - **The connection** is a TCP socket on the loopback interface (or to a kernel server,
//!   below): the app listens on a port of its own, starts the kernel process with the port and
//!   a random token, and takes the one connection that sends the token. (Not the process's
//!   stdout, which stays free for its messages.) Messages are MessagePack with named fields
//!   (they carry document types, whose
//!   optional fields are skipped when empty), each after its length.
//! - **While it works** the kernel process reports progress (the "Rebuilding…" indicator), the
//!   feature it is computing, and the files it adds to its blob cache (a board's skin), which
//!   the app needs too. Before each request the app sends the files of its blob cache the
//!   kernel process doesn't have yet (imports, pictures, footprint models).
//! - **A crash** closes the connection. A rebuild that crashed while computing a feature
//!   *poisons* that feature's cache key (the feature with everything above it): the app starts
//!   a new kernel process, tells it the poisoned keys, and asks again; that feature now fails
//!   with "The geometry kernel crashed" instead of being computed, and the rest of the list
//!   rebuilds (up to [`MAX_CRASHES`] times per request). Changing the feature, or anything above
//!   it, gives it a new key, so it is computed again. Other jobs that crash fail (no result).
//! - **A page** (the browser build) starts no processes: its jobs run inline
//!   (`super::inline`), and only the messages' encoding here is used.
//! - **On another machine** (issue #146: a compute workstation for the others' rebuilds): there
//!   `cadrs --kernel-server` runs [`serve`], which takes connections that send its token and
//!   starts a kernel process of its own for each, relaying between the two. The app connects to
//!   it instead of starting one ([`use_server`]); a crash closes the connection as before, and
//!   the app connects again for a new kernel process. The connection is plain TCP with a token:
//!   for a network one trusts (an authenticated client can still send it large messages). Both
//!   ends must be the same cadrs release (the Hello carries it); the server says why it turns
//!   a client away. The documents folder and the session snapshots stay the app's (a remote
//!   kernel gets neither: a Derived feature from another document doesn't build there). A
//!   machine that vanishes without closing the connection (asleep, off the network) isn't
//!   noticed: the rebuild waits for it.

// The app's side of the connection is unused in a page.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};
use std::time::Duration;
use web_time::Instant;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::{Build, Job, PROGRESS_DONE, PROGRESS_TOTAL, Rebuilder};
use crate::document::Feature;
use crate::export::{StepFile, StepRequest};

/// The argument that makes the app's executable the kernel process (followed by the port and
/// the token).
pub const ARG: &str = "--kernel-process";

/// How many times one rebuild may crash the kernel process (each time poisoning another
/// feature) before it is given up.
pub const MAX_CRASHES: usize = 8;

/// What a feature the kernel crashed on fails with.
pub const CRASHED: &str = "The geometry kernel crashed while building this feature";

/// How long a new kernel process has to connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------------------------
// Encoding

/// `x` as a message body.
pub(crate) fn encode<T: Serialize + ?Sized>(x: &T) -> Result<Vec<u8>, String> {
    rmp_serde::to_vec_named(x).map_err(|e| e.to_string())
}

/// A message body as a `T`.
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    rmp_serde::from_slice(bytes).map_err(|e| e.to_string())
}

fn write_msg<T: Serialize + ?Sized>(w: &mut impl Write, x: &T) -> io::Result<()> {
    let body = encode(x).map_err(io::Error::other)?;
    w.write_all(&(body.len() as u64).to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

fn read_msg<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<T> {
    let mut len = [0u8; 8];
    r.read_exact(&mut len)?;
    let mut body = vec![0u8; u64::from_le_bytes(len) as usize];
    r.read_exact(&mut body)?;
    decode(&body).map_err(io::Error::other)
}

/// A feature list as sent to the kernel process: the features, and what their saved form leaves
/// out ([`crate::sheetmetal::SheetMetalModelFeature::picking`]: the Sheet metal models a dialog
/// takes picks on, built without making parts).
#[derive(Serialize, Deserialize)]
pub(crate) struct Features {
    list: Vec<Feature>,
    picking: Vec<crate::ids::FeatureId>,
}

impl Features {
    pub(crate) fn of(list: Vec<Feature>) -> Self {
        let picking = list
            .iter()
            .filter(|f| matches!(&f.kind, crate::document::FeatureKind::SheetMetalModel(x) if x.picking))
            .map(|f| f.id)
            .collect();
        Self { list, picking }
    }

    pub(crate) fn into_list(mut self) -> Vec<Feature> {
        for f in &mut self.list {
            if let crate::document::FeatureKind::SheetMetalModel(x) = &mut f.kind
                && self.picking.contains(&f.id)
            {
                x.picking = true;
            }
        }
        self.list
    }
}

/// The app to the kernel process.
#[derive(Serialize, Deserialize)]
enum Request {
    /// The app to a kernel server ([`serve`]), first: its token and cadrs version (both ends
    /// must be the same release: the messages carry the document's types).
    Hello {
        token: String,
        version: String,
    },
    /// Where the documents (a Derived feature's sources in other documents) and the session
    /// snapshots are.
    Configure {
        documents: Option<PathBuf>,
        snapshots: Option<PathBuf>,
    },
    /// Files of the app's blob cache.
    Blobs(Vec<(String, Vec<u8>)>),
    /// Cache keys of features not to compute (they crashed it).
    Poison(Vec<u64>),
    /// Stop the rebuild running early ([`super::Pending::stop`]); sent while it runs, so it is
    /// read as it comes, not after.
    Stop,
    Rebuild {
        features: Features,
        persist: bool,
    },
    Export {
        features: Features,
        request: StepRequest,
    },
    Project {
        features: Features,
        request: crate::views::ViewRequest,
    },
    ProjectAssembly {
        state: crate::drawing_assembly::AssemblyState,
        request: crate::views::ViewRequest,
    },
    Task {
        name: String,
        input: Vec<u8>,
    },
}

/// The kernel process to the app: what it is doing, then the reply.
#[derive(Serialize, Deserialize)]
pub(super) enum Event {
    /// The token it was started with (its first message).
    Hello(String),
    /// A kernel server turned the app away, and why (the token, the version, its kernel
    /// process).
    Refused(String),
    /// [`super::progress`].
    Progress(usize, usize),
    /// The cache key of the feature it is computing now (`None`: none).
    Computing(Option<u64>),
    /// A file it added to its blob cache.
    Blob(String, Vec<u8>),
    /// The build, its parts' meshes taken out ([`each_solid`]): in the same order, each mesh's
    /// id and the mesh, or only the id of one sent before ([`SOLID_WINDOW`]).
    Rebuilt(Arc<Build>, SentMeshes),
    /// The parts of the rebuild running, as far as it has got ([`super::Pending::partial`]): a
    /// build of only its parts, their meshes sent as [`Event::Rebuilt`]'s are.
    Partial(Arc<Build>, SentMeshes),
    Exported(Result<Vec<StepFile>, String>),
    Projected(Result<Arc<crate::views::ViewGeometry>, String>),
    TaskDone(Result<Vec<u8>, String>),
}

// ---------------------------------------------------------------------------------------------
// The parts' meshes, sent once

/// How many of the meshes sent last both sides keep: the kernel process sends one of them again
/// as its id alone, and the app keeps them (by id) to hand out again. A rebuild sends only the
/// meshes the features it computed made (as the kernel thread hands out its cached parts' meshes
/// again), so an edit at the end of a long list costs little.
const SOLID_WINDOW: u64 = 4096;

/// A build's meshes as sent ([`Event::Rebuilt`]): each one's id, and the mesh unless sent before.
type SentMeshes = Vec<(u64, Option<Arc<crate::solid::Solid>>)>;

/// Every mesh of `b` the app tells changes by (the parts', the stage's), in one order.
fn each_solid(b: &mut Build, mut f: impl FnMut(&mut Arc<crate::solid::Solid>)) {
    for p in &mut b.parts {
        f(&mut p.solid);
    }
    if let Some((_, st)) = &mut b.stage {
        for p in &mut st.before {
            f(&mut p.solid);
        }
        f(&mut st.tool);
        for m in &mut st.more {
            f(m);
        }
    }
}

/// The meshes the kernel process has sent (by their address there, which they keep while held
/// here), with their ids.
#[derive(Default)]
struct SentSolids {
    by_ptr: HashMap<usize, (u64, Arc<crate::solid::Solid>)>,
    next: u64,
}

impl SentSolids {
    /// `b` without its meshes, and the meshes in [`each_solid`]'s order: the ones sent before
    /// (within the window) as ids.
    fn strip(&mut self, b: &Build) -> (Build, SentMeshes) {
        let empty = Arc::new(crate::solid::Solid::default());
        let mut b = b.clone();
        let mut out = Vec::new();
        each_solid(&mut b, |s| {
            let solid = std::mem::replace(s, empty.clone());
            let ptr = Arc::as_ptr(&solid) as usize;
            match self.by_ptr.get(&ptr) {
                Some((id, _)) if id + SOLID_WINDOW > self.next => out.push((*id, None)),
                _ => {
                    let id = self.next;
                    self.next += 1;
                    self.by_ptr.insert(ptr, (id, solid.clone()));
                    out.push((id, Some(solid)));
                }
            }
        });
        let next = self.next;
        self.by_ptr.retain(|_, (id, _)| *id + SOLID_WINDOW > next);
        (b, out)
    }
}

/// The meshes the app received from the kernel process now running, by id.
#[derive(Default)]
struct ReceivedSolids {
    by_id: std::collections::BTreeMap<u64, Arc<crate::solid::Solid>>,
}

impl ReceivedSolids {
    /// Puts the meshes back into `b`.
    fn restore(
        &mut self,
        b: &mut Build,
        solids: SentMeshes,
        recent: &mut Recent,
    ) -> Result<(), String> {
        let mut it = solids.into_iter();
        let mut missing = None;
        each_solid(b, |s| match it.next() {
            Some((id, Some(new))) => {
                // A mesh equal to one of an earlier kernel process's keeps its pointer too.
                let new = recent.solid(&new);
                self.by_id.insert(id, new.clone());
                *s = new;
            }
            Some((id, None)) => match self.by_id.get(&id) {
                Some(old) => *s = old.clone(),
                None => missing = Some(id),
            },
            None => missing = Some(u64::MAX),
        });
        if let Some(&last) = self.by_id.keys().next_back() {
            self.by_id.retain(|id, _| id + SOLID_WINDOW > last);
        }
        match missing {
            Some(id) => Err(format!("the geometry kernel sent an unknown mesh ({id})")),
            None => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The app's side

/// How to start the kernel process.
#[derive(Debug, Clone)]
enum Launch {
    /// `program args… --kernel-process <port> <token>` on this machine.
    Spawn {
        program: PathBuf,
        args: Vec<OsString>,
    },
    /// A kernel server ([`serve`]) on another machine: its address (`host:port`) and token.
    Server { addr: String, token: String },
}

static LAUNCH: OnceLock<Launch> = OnceLock::new();

/// Runs the kernel in a process of its own from now on: `program args… --kernel-process <port>
/// <token>`, which must call [`main`]. Only before the first job (the worker starts with the
/// first one); later calls change nothing.
pub fn use_process(program: impl Into<PathBuf>, args: Vec<OsString>) {
    let _ = LAUNCH.set(Launch::Spawn {
        program: program.into(),
        args,
    });
}

/// Runs the kernel on a kernel server ([`serve`], `cadrs --kernel-server`) at `addr`
/// (`host:port`) from now on, with its token. Only before the first job, as [`use_process`].
pub fn use_server(addr: impl Into<String>, token: impl Into<String>) {
    let _ = LAUNCH.set(Launch::Server {
        addr: addr.into(),
        token: token.into(),
    });
}

/// Whether the kernel runs in its own process.
pub fn enabled() -> bool {
    LAUNCH.get().is_some()
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Config {
    documents: Option<PathBuf>,
    snapshots: Option<PathBuf>,
}

/// The configuration, and a count of its changes (sent again to the process when it changed).
static CONFIG: Mutex<(Config, u64)> = Mutex::new((
    Config {
        documents: None,
        snapshots: None,
    },
    0,
));

fn set_config(f: impl FnOnce(&mut Config)) {
    if let Ok(mut c) = CONFIG.lock() {
        let before = c.0.clone();
        f(&mut c.0);
        if c.0 != before {
            c.1 += 1;
        }
    }
}

/// The documents folder, for a Derived feature's sources in other documents (the kernel
/// process reads them itself, as [`crate::derived::set_document_loader`] does in the app).
pub fn set_documents(root: Option<PathBuf>) {
    set_config(|c| c.documents = root);
}

/// The session snapshots' folder (as [`super::session::set_store`] in the app).
pub fn set_snapshots(dir: Option<PathBuf>) {
    set_config(|c| c.snapshots = dir);
}

/// A running kernel process and the connection to it.
struct Conn {
    /// The process, when it runs here (not on a kernel server).
    child: Option<Child>,
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
    /// The blobs it has.
    blobs: HashSet<String>,
    /// The configuration change it has.
    config: u64,
    /// The meshes it sent.
    solids: ReceivedSolids,
}

/// Starts `program args… --kernel-process <port> <token>` here and takes its connection (the
/// one that sends the token). `served`: for a kernel server, whose clients' folders aren't
/// this machine's (see [`main`]).
fn spawn_kernel(
    program: &PathBuf,
    args: &[OsString],
    served: bool,
) -> io::Result<(Child, TcpStream)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let token = uuid::Uuid::new_v4().to_string();
    let mut command = Command::new(program);
    command
        .args(args)
        .arg(ARG)
        .arg(port.to_string())
        .arg(&token)
        .stdin(Stdio::null());
    if served {
        command.env(SERVED_ENV, "1");
    }
    let mut child = command.spawn()?;
    listener.set_nonblocking(true)?;
    let start = Instant::now();
    loop {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_nonblocking(false)?;
                // Only the process it started may connect (and something else that connects
                // can't hold it up).
                s.set_read_timeout(Some(Duration::from_secs(10)))?;
                let mut r = BufReader::new(s.try_clone()?);
                match read_msg::<Event>(&mut r) {
                    Ok(Event::Hello(t)) if t == token => {
                        s.set_read_timeout(None)?;
                        return Ok((child, s));
                    }
                    _ => continue,
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Some(status) = child.try_wait()? {
                    return Err(io::Error::other(format!(
                        "the kernel process exited at start ({status})"
                    )));
                }
                if start.elapsed() > CONNECT_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(io::Error::other("the kernel process didn't connect"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Connects to a kernel server ([`serve`]) with its token and waits until its kernel process
/// runs (the server answers Hello then, or closes the connection).
fn connect_server(addr: &str, token: &str) -> io::Result<TcpStream> {
    use std::net::ToSocketAddrs;
    if token.is_empty() {
        return Err(io::Error::other(
            "no token for the kernel server (CADRS_KERNEL_TOKEN)",
        ));
    }
    // Every address the name has ("localhost" is ::1 and 127.0.0.1): the first that answers.
    let mut last = io::Error::other(format!("no address for {addr}"));
    let mut stream = None;
    for to in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&to, Duration::from_secs(10)) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = e,
        }
    }
    let s = stream.ok_or(last)?;
    s.set_nodelay(true)?;
    let hello = Request::Hello {
        token: token.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    write_msg(&mut BufWriter::new(s.try_clone()?), &hello)?;
    // Read unbuffered (nothing else comes before the first request) and capped: something
    // other than a kernel server on that port (an SSH banner) mustn't make it allocate.
    let not_server = || io::Error::other(format!("{addr} is not a kernel server"));
    let body = read_capped(&s, MAX_HELLO, CONNECT_TIMEOUT).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => {
            io::Error::other(format!("the kernel server at {addr} closed the connection"))
        }
        io::ErrorKind::InvalidData => not_server(),
        _ => e,
    })?;
    match decode::<Event>(&body).map_err(|_| not_server())? {
        Event::Hello(_) => Ok(s),
        Event::Refused(why) => Err(io::Error::other(format!(
            "the kernel server at {addr} refused: {why}"
        ))),
        _ => Err(not_server()),
    }
}

/// One message from `s` of at most `cap` bytes, all of it within `within` (a peer that trickles
/// bytes can't hold the reader longer).
fn read_capped(s: &TcpStream, cap: u64, within: Duration) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + within;
    let read_all = |buf: &mut [u8]| -> io::Result<()> {
        let mut at = 0;
        while at < buf.len() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "too slow"));
            }
            s.set_read_timeout(Some(left))?;
            match (&*s).read(&mut buf[at..]) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => at += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    };
    let mut len = [0u8; 8];
    read_all(&mut len)?;
    let len = u64::from_le_bytes(len);
    if len > cap {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too long",
        ));
    }
    let mut body = vec![0u8; len as usize];
    read_all(&mut body)?;
    s.set_read_timeout(None)?;
    Ok(body)
}

impl Conn {
    fn start(launch: &Launch, poisoned: &[u64]) -> io::Result<Conn> {
        let (child, stream) = match launch {
            Launch::Spawn { program, args } => {
                let (child, stream) = spawn_kernel(program, args, false)?;
                (Some(child), stream)
            }
            Launch::Server { addr, token } => (None, connect_server(addr, token)?),
        };
        stream.set_nodelay(true)?;
        let mut conn = Conn {
            child,
            reader: BufReader::new(stream.try_clone()?),
            writer: BufWriter::new(stream),
            blobs: HashSet::new(),
            config: u64::MAX,
            solids: ReceivedSolids::default(),
        };
        if !poisoned.is_empty() {
            write_msg(&mut conn.writer, &Request::Poison(poisoned.to_vec()))?;
        }
        Ok(conn)
    }

    /// Sends what changed since the last request: the configuration and new blobs.
    fn sync(&mut self) -> io::Result<()> {
        let (config, version) = CONFIG.lock().map(|c| c.clone()).unwrap_or_default();
        if version != self.config {
            // A kernel server's machine has folders of its own: it gets neither.
            let here = self.child.is_some();
            write_msg(
                &mut self.writer,
                &Request::Configure {
                    documents: config.documents.filter(|_| here),
                    snapshots: config.snapshots.filter(|_| here),
                },
            )?;
            self.config = version;
        }
        let new: Vec<String> = crate::blobs::hashes()
            .into_iter()
            .filter(|h| !self.blobs.contains(h))
            .collect();
        if !new.is_empty() {
            let blobs: Vec<(String, Vec<u8>)> = new
                .iter()
                .filter_map(|h| Some((h.clone(), crate::blobs::get(h)?.to_vec())))
                .collect();
            write_msg(&mut self.writer, &Request::Blobs(blobs))?;
            self.blobs.extend(new);
        }
        Ok(())
    }

    /// Ends the process (if it is still running) and says how it ended.
    fn stop(self) -> String {
        let Some(mut child) = self.child else {
            // On a kernel server: closing the connection ends its process there.
            let _ = self.writer.get_ref().shutdown(std::net::Shutdown::Both);
            return "the kernel server's connection closed".into();
        };
        // A crashed process has exited, or is about to.
        for _ in 0..50 {
            if let Ok(Some(status)) = child.try_wait() {
                return status.to_string();
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = child.kill();
        child
            .wait()
            .map_or_else(|e| e.to_string(), |s| format!("stopped, {s}"))
    }
}

/// What the builds received lately had, handed out again when a new build has the same: the
/// app tells what changed by pointer (a part's [`crate::solid::Solid`]) and by contents laid out
/// in order (a `HashMap`'s order, which every map read anew changes), and the kernel thread's
/// builds keep both for what didn't change (they are its cache's).
#[derive(Default)]
struct Recent {
    solids: std::collections::VecDeque<Arc<crate::solid::Solid>>,
    sheet_metal: std::collections::VecDeque<crate::sheetmetal::SheetMetalContext>,
    derived: std::collections::VecDeque<crate::derived::DerivedOutput>,
}

/// How many of each [`Recent`] keeps.
const RECENT: usize = 512;

/// The value in `recent` equal to `x` (made the newest), else `x` (kept).
fn settle<T: PartialEq + Clone>(
    recent: &mut std::collections::VecDeque<T>,
    x: &T,
    same_size: impl Fn(&T, &T) -> bool,
) -> T {
    if let Some(i) = recent.iter().position(|r| same_size(r, x) && r == x) {
        let r = recent.remove(i).expect("found");
        recent.push_front(r.clone());
        return r;
    }
    recent.push_front(x.clone());
    recent.truncate(RECENT);
    x.clone()
}

impl Recent {
    fn solid(&mut self, s: &Arc<crate::solid::Solid>) -> Arc<crate::solid::Solid> {
        settle(&mut self.solids, s, |a, b| {
            a.positions.len() == b.positions.len() && a.indices.len() == b.indices.len()
        })
    }

    /// `b` with the values of earlier builds where it has the same (its meshes are
    /// [`ReceivedSolids::restore`]'s).
    fn settle(&mut self, b: &mut Build) {
        for c in &mut b.sheet_metal {
            *c = settle(&mut self.sheet_metal, c, |a, b| a.feature == b.feature);
        }
        for d in b.derived.values_mut() {
            *d = settle(&mut self.derived, d, |a, b| a.parts == b.parts);
        }
    }
}

/// How a request to the kernel process went wrong.
enum Failure {
    /// It crashed (or couldn't start); while computing the feature with this key, if any.
    Crashed { computing: Option<u64>, why: String },
}

/// The app's side of the worker: the jobs of [`super::worker`] sent to the kernel process.
struct Remote {
    launch: Launch,
    conn: Option<Conn>,
    /// Features that crashed the kernel process (their cache keys), for every new one.
    poisoned: Vec<u64>,
    /// A session in this process, for closures ([`super::run_on_worker`]), which can't be sent.
    local: Option<Rebuilder>,
    recent: Recent,
    /// The stop of the rebuild being asked for ([`super::Pending::stop`]): sent on to the kernel
    /// process once set, while the reply is awaited.
    stop: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl Remote {
    /// Sends `request` and waits for its reply, handling the events before it.
    fn call(&mut self, request: &Request) -> Result<Event, Failure> {
        if self.conn.is_none() {
            match Conn::start(&self.launch, &self.poisoned) {
                Ok(c) => self.conn = Some(c),
                Err(e) => {
                    return Err(Failure::Crashed {
                        computing: None,
                        why: format!("The geometry kernel couldn't start: {e}"),
                    });
                }
            }
        }
        let conn = self.conn.as_mut().expect("just started");
        let mut computing = None;
        let stop = self.stop.clone();
        let mut watch = None;
        let result = (|| -> io::Result<Event> {
            conn.sync()?;
            write_msg(&mut conn.writer, request)?;
            // While the reply is awaited: a Stop once the request's stop is set (by a thread of
            // its own, joined before the next request, so a late one can't stop that).
            if let Some(stop) = stop
                && let Ok(stream) = conn.writer.get_ref().try_clone()
            {
                let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let finished = done.clone();
                let thread = std::thread::spawn(move || {
                    let mut w = BufWriter::new(stream);
                    while !finished.load(Ordering::Relaxed) {
                        if stop.load(Ordering::Relaxed) {
                            let _ = write_msg(&mut w, &Request::Stop);
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                });
                watch = Some((done, thread));
            }
            loop {
                match read_msg::<Event>(&mut conn.reader)? {
                    Event::Progress(done, total) => {
                        PROGRESS_DONE.store(done, Ordering::Relaxed);
                        PROGRESS_TOTAL.store(total, Ordering::Relaxed);
                    }
                    Event::Computing(k) => computing = k,
                    Event::Partial(b, solids) => {
                        let mut b = Arc::unwrap_or_clone(b);
                        if conn
                            .solids
                            .restore(&mut b, solids, &mut self.recent)
                            .is_ok()
                        {
                            super::keep_partial(b.parts);
                        }
                    }
                    Event::Blob(hash, bytes) => {
                        crate::blobs::insert_known(&hash, bytes);
                        conn.blobs.insert(hash);
                    }
                    Event::Hello(_) => {}
                    reply => return Ok(reply),
                }
            }
        })();
        if let Some((done, thread)) = watch {
            done.store(true, Ordering::Relaxed);
            let _ = thread.join();
        }
        result.map_err(|e| {
            let how = self.conn.take().map_or_else(|| e.to_string(), Conn::stop);
            eprintln!("cadrs: the geometry kernel process ended ({how}); the next request starts a new one");
            PROGRESS_DONE.store(0, Ordering::Relaxed);
            PROGRESS_TOTAL.store(0, Ordering::Relaxed);
            Failure::Crashed { computing, why: how }
        })
    }

    fn rebuild(&mut self, features: Vec<Feature>, persist: bool) -> Build {
        let request = Request::Rebuild {
            features: Features::of(features),
            persist,
        };
        let Request::Rebuild {
            features: Features { list: features, .. },
            ..
        } = &request
        else {
            unreachable!()
        };
        for _ in 0..MAX_CRASHES {
            match self.call(&request) {
                Ok(Event::Rebuilt(b, solids)) => {
                    let mut b = Arc::unwrap_or_clone(b);
                    let restored = match self.conn.as_mut() {
                        Some(c) => c.solids.restore(&mut b, solids, &mut self.recent),
                        None => Err("the geometry kernel stopped".into()),
                    };
                    if let Err(e) = restored {
                        return Build::failed(features, &e);
                    }
                    self.recent.settle(&mut b);
                    return b;
                }
                Ok(_) => {
                    return Build::failed(features, "The geometry kernel answered something else");
                }
                // Ask again, without the feature it crashed on.
                Err(Failure::Crashed {
                    computing: Some(key),
                    ..
                }) => {
                    if !self.poisoned.contains(&key) {
                        self.poisoned.push(key);
                    }
                }
                Err(Failure::Crashed {
                    computing: None,
                    why,
                }) => {
                    return Build::failed(features, &format!("The geometry kernel stopped: {why}"));
                }
            }
        }
        Build::failed(features, "The geometry kernel crashed too many times")
    }

    fn run(&mut self, rx: mpsc::Receiver<Job>) {
        while let Ok(job) = rx.recv() {
            match job {
                Job::Rebuild {
                    features,
                    ticket,
                    reply,
                    cancelled,
                    persist,
                } => {
                    if cancelled.load(Ordering::Relaxed) {
                        let _ = reply.send(Arc::new(Build {
                            stopped: true,
                            ..Build::default()
                        }));
                        continue;
                    }
                    super::start_progress(ticket);
                    self.stop = Some(cancelled);
                    let build = self.rebuild(features, persist);
                    self.stop = None;
                    super::clear_partial();
                    let _ = reply.send(Arc::new(build));
                }
                Job::Export {
                    features,
                    request,
                    reply,
                } => {
                    let out = match self.call(&Request::Export {
                        features: Features::of(features),
                        request,
                    }) {
                        Ok(Event::Exported(r)) => r,
                        Ok(_) => Err("The geometry kernel answered something else".into()),
                        Err(Failure::Crashed { .. }) => {
                            Err("The geometry kernel crashed while exporting".into())
                        }
                    };
                    let _ = reply.send(out);
                }
                Job::Project {
                    features,
                    request,
                    reply,
                    cancelled,
                } => {
                    if cancelled.load(Ordering::Relaxed) {
                        continue;
                    }
                    let out = match self.call(&Request::Project {
                        features: Features::of(features),
                        request: *request,
                    }) {
                        Ok(Event::Projected(r)) => r,
                        Ok(_) => Err("The geometry kernel answered something else".into()),
                        Err(Failure::Crashed { .. }) => {
                            Err("The geometry kernel crashed while projecting the view".into())
                        }
                    };
                    let _ = reply.send(out);
                }
                Job::ProjectAssembly {
                    state,
                    request,
                    reply,
                    cancelled,
                } => {
                    if cancelled.load(Ordering::Relaxed) {
                        continue;
                    }
                    let out = match self.call(&Request::ProjectAssembly {
                        state: *state,
                        request: *request,
                    }) {
                        Ok(Event::Projected(r)) => r,
                        Ok(_) => Err("The geometry kernel answered something else".into()),
                        Err(Failure::Crashed { .. }) => {
                            Err("The geometry kernel crashed while projecting the assembly".into())
                        }
                    };
                    let _ = reply.send(out);
                }
                Job::Task { name, input, done } => {
                    let out = match self.call(&Request::Task {
                        name: name.into(),
                        input,
                    }) {
                        Ok(Event::TaskDone(Ok(o))) => Some(o),
                        _ => None,
                    };
                    done(out);
                }
                Job::Run(f) => {
                    let r = self.local.get_or_insert_with(Rebuilder::new);
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(r))).is_err() {
                        self.local = None;
                    }
                }
            }
        }
        if let Some(c) = self.conn.take() {
            c.stop();
        }
    }
}

/// The worker loop with the kernel in its own process (see [`super::worker`]).
pub(super) fn run_remote(rx: mpsc::Receiver<Job>) {
    let Some(launch) = LAUNCH.get().cloned() else {
        return super::run(rx);
    };
    Remote {
        launch,
        conn: None,
        poisoned: Vec::new(),
        local: None,
        recent: Recent::default(),
        stop: None,
    }
    .run(rx);
}

// ---------------------------------------------------------------------------------------------
// A kernel server

/// Set for a kernel process a kernel server started: it takes no folders from the app.
const SERVED_ENV: &str = "CADRS_KERNEL_SERVED";

/// The longest Hello read (a token and a version), so a stranger can't make either end allocate.
const MAX_HELLO: u64 = 4096;

/// Clients a kernel server serves at once (each has a kernel process).
const MAX_CLIENTS: usize = 32;

/// A kernel server (`cadrs --kernel-server`): on `listen` (`host:port`), each connection that
/// sends `token` gets a kernel process of its own (`program args… --kernel-process …`), and the
/// two are relayed until either ends. Returns only if it can't listen.
pub fn serve(listen: &str, token: &str, program: PathBuf, args: Vec<OsString>) -> io::Result<()> {
    let listener = TcpListener::bind(listen)?;
    eprintln!("cadrs kernel server on {}", listener.local_addr()?);
    serve_on(listener, token.to_string(), program, args);
    Ok(())
}

fn serve_on(listener: TcpListener, token: String, program: PathBuf, args: Vec<OsString>) {
    let clients = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for client in listener.incoming() {
        let Ok(client) = client else { continue };
        if clients.load(Ordering::SeqCst) >= MAX_CLIENTS {
            let _ = write_msg(&mut &client, &Event::Refused("too many clients".into()));
            continue;
        }
        clients.fetch_add(1, Ordering::SeqCst);
        let (token, program, args, count) = (
            token.clone(),
            program.clone(),
            args.clone(),
            clients.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("cadrs-kernel-client".into())
            .spawn(move || {
                let peer = client
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                match relay(client, &token, &program, &args) {
                    Ok(()) => eprintln!("kernel server: {peer} done"),
                    Err(e) => eprintln!("kernel server: {peer}: {e}"),
                }
                count.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            clients.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// `a == b` in a time that doesn't depend on where they differ (a token).
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// One client: its Hello, a kernel process, then bytes both ways until either side closes.
fn relay(client: TcpStream, token: &str, program: &PathBuf, args: &[OsString]) -> io::Result<()> {
    client.set_nodelay(true)?;
    let refuse = |why: &str| {
        let _ = write_msg(&mut &client, &Event::Refused(why.to_string()));
        io::Error::other(why.to_string())
    };
    let body = read_capped(&client, MAX_HELLO, Duration::from_secs(10))
        .map_err(|e| io::Error::other(format!("not a cadrs client ({e})")))?;
    match decode::<Request>(&body) {
        Ok(Request::Hello { token: t, version }) if same_secret(&t, token) => {
            let ours = env!("CARGO_PKG_VERSION");
            if version != ours {
                return Err(refuse(&format!(
                    "it runs cadrs {ours} and this is cadrs {version}: use the same release"
                )));
            }
        }
        Ok(Request::Hello { .. }) => return Err(refuse("wrong token")),
        _ => return Err(io::Error::other("not a cadrs client")),
    }
    let (mut child, kernel) =
        spawn_kernel(program, args, true).map_err(|e| refuse(&format!("its kernel: {e}")))?;
    let streams = (|| -> io::Result<_> {
        write_msg(&mut &client, &Event::Hello(String::new()))?;
        Ok((client.try_clone()?, kernel.try_clone()?))
    })();
    let (mut from_client, mut to_kernel) = match streams {
        Ok(s) => s,
        Err(e) => {
            // No zombie: the process it started ends with the client it was for.
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let up = std::thread::spawn(move || {
        let _ = io::copy(&mut from_client, &mut to_kernel);
        let _ = to_kernel.shutdown(std::net::Shutdown::Both);
    });
    let (mut from_kernel, mut to_client) = (kernel, client);
    let _ = io::copy(&mut from_kernel, &mut to_client);
    let _ = to_client.shutdown(std::net::Shutdown::Both);
    let _ = up.join();
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The kernel process's side

/// Sends events to the app (set in the kernel process).
type Sink = Box<dyn Fn(&Event) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();

/// Features not to compute: their cache keys (in the kernel process).
static POISONED: RwLock<Option<HashSet<u64>>> = RwLock::new(None);

/// Sends the parts of the rebuild running to the app (set in the kernel process).
type PartialSink = Box<dyn Fn(&[crate::parts::Part]) + Send + Sync>;

static PARTIAL_SINK: OnceLock<PartialSink> = OnceLock::new();

/// Sends `parts`, those of the rebuild running as far as it has got, to the app: `false` outside
/// the kernel process (the caller keeps them, [`super::keep_partial`]).
pub(super) fn send_partial(parts: &[crate::parts::Part]) -> bool {
    match PARTIAL_SINK.get() {
        Some(s) => {
            s(parts);
            true
        }
        None => false,
    }
}

/// Tells the app (from the kernel process; nothing elsewhere).
pub(super) fn emit(e: &Event) {
    if let Some(s) = SINK.get() {
        s(e);
    }
}

/// Whether the feature with this cache key crashed a kernel process before (in the kernel
/// process; never elsewhere).
pub(super) fn is_poisoned(key: u64) -> bool {
    POISONED
        .read()
        .is_ok_and(|p| p.as_ref().is_some_and(|p| p.contains(&key)))
}

thread_local! {
    /// The cache keys of the features being computed, outermost first (a Derived feature's
    /// source rebuilds inside it).
    static COMPUTING: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Reports that `f`, with the cache key `key`, is being computed until the guard is dropped.
pub(super) fn computing(key: u64, f: &Feature) -> Computing {
    if SINK.get().is_none() {
        return Computing(false);
    }
    COMPUTING.with(|c| c.borrow_mut().push(key));
    emit(&Event::Computing(Some(key)));
    // For tests of the crash handling: crash on the feature with this name.
    static CRASH_ON: OnceLock<Option<String>> = OnceLock::new();
    if CRASH_ON
        .get_or_init(|| std::env::var("CADRS_KERNEL_CRASH_ON").ok())
        .as_deref()
        == Some(f.name.as_str())
    {
        std::process::abort();
    }
    Computing(true)
}

/// See [`computing`].
pub(super) struct Computing(bool);

impl Drop for Computing {
    fn drop(&mut self) {
        if self.0 {
            let outer = COMPUTING.with(|c| {
                let mut c = c.borrow_mut();
                c.pop();
                c.last().copied()
            });
            emit(&Event::Computing(outer));
        }
    }
}

/// The kernel process: `args` are the ones after [`ARG`] (the port and the token). Returns the
/// exit code (when the app closes the connection).
pub fn main(args: &[String]) -> i32 {
    let (Some(port), Some(token)) = (
        args.first().and_then(|p| p.parse::<u16>().ok()),
        args.get(1),
    ) else {
        eprintln!("usage: {ARG} <port> <token>");
        return 2;
    };
    let stream = match TcpStream::connect(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("kernel process: cannot connect: {e}");
            return 1;
        }
    };
    let _ = stream.set_nodelay(true);
    let (Ok(read), Ok(write)) = (stream.try_clone(), stream.try_clone()) else {
        return 1;
    };
    let writer = Arc::new(Mutex::new(BufWriter::new(write)));
    if write_msg(
        &mut *writer.lock().expect("unshared"),
        &Event::Hello(token.clone()),
    )
    .is_err()
    {
        return 1;
    }
    let sink = writer.clone();
    let _ = SINK.set(Box::new(move |e: &Event| {
        if let Ok(mut w) = sink.lock() {
            let _ = write_msg(&mut *w, e);
        }
    }));
    let blob_sink = writer.clone();
    crate::blobs::set_insert_hook(Box::new(move |hash: &str, bytes: &[u8]| {
        if let Ok(mut w) = blob_sink.lock() {
            let _ = write_msg(&mut *w, &Event::Blob(hash.to_string(), bytes.to_vec()));
        }
    }));
    // The same worker loop as the app's kernel thread.
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("cadrs-rebuild".into())
        .spawn(move || super::run(rx))
        .expect("failed to start the rebuild thread");
    // The requests, read as they come: a Stop sets the stop of the rebuild running then (the
    // loop below waits for that rebuild), the others queue for the loop.
    let running: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>> = Arc::default();
    let (requests, queued) = mpsc::channel::<Request>();
    let stop_of = running.clone();
    std::thread::Builder::new()
        .name("cadrs-kernel-requests".into())
        .spawn(move || {
            let mut reader = BufReader::new(read);
            while let Ok(request) = read_msg::<Request>(&mut reader) {
                if let Request::Stop = request {
                    if let Ok(r) = stop_of.lock()
                        && let Some(stop) = r.as_ref()
                    {
                        stop.store(true, Ordering::Relaxed);
                    }
                    continue;
                }
                if requests.send(request).is_err() {
                    break;
                }
            }
        })
        .expect("failed to start the request reader");
    // The meshes sent, shared with the rebuild thread's parts sent as it goes.
    let sent = Arc::new(Mutex::new(SentSolids::default()));
    let partial_sent = sent.clone();
    let partial_writer = writer.clone();
    let _ = PARTIAL_SINK.set(Box::new(move |parts: &[crate::parts::Part]| {
        let b = Build {
            parts: parts.to_vec(),
            ..Build::default()
        };
        let Ok(mut sent) = partial_sent.lock() else {
            return;
        };
        let (b, solids) = sent.strip(&b);
        if let Ok(mut w) = partial_writer.lock() {
            let _ = write_msg(&mut *w, &Event::Partial(Arc::new(b), solids));
        }
    }));
    let send = |e: &Event| -> bool {
        writer
            .lock()
            .is_ok_and(|mut w| write_msg(&mut *w, e).is_ok())
    };
    loop {
        let Ok(request) = queued.recv() else {
            return 0;
        };
        let reply = match request {
            Request::Configure {
                documents,
                snapshots,
            } => {
                // A kernel server's process: the paths are the client's, not this machine's.
                let (documents, snapshots) = if std::env::var_os(SERVED_ENV).is_some() {
                    (None, None)
                } else {
                    (documents, snapshots)
                };
                crate::derived::set_document_loader(
                    documents.map(|d| crate::derived::store_loader(crate::Store::new(d))),
                );
                #[cfg(feature = "brep")]
                super::session::set_store(snapshots.map(|d| {
                    Arc::new(crate::blob_store::DiskStore::new(d))
                        as Arc<dyn crate::blob_store::BlobStore>
                }));
                #[cfg(not(feature = "brep"))]
                let _ = snapshots;
                continue;
            }
            Request::Blobs(blobs) => {
                for (hash, bytes) in blobs {
                    crate::blobs::insert_known(&hash, bytes);
                }
                continue;
            }
            Request::Poison(keys) => {
                if let Ok(mut p) = POISONED.write() {
                    p.get_or_insert_with(HashSet::new).extend(keys);
                }
                continue;
            }
            // (Read by the request reader as it comes; a Hello is a kernel server's.)
            Request::Stop | Request::Hello { .. } => continue,
            Request::Rebuild { features, persist } => {
                let (reply, rx) = mpsc::channel();
                let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                if let Ok(mut r) = running.lock() {
                    *r = Some(cancelled.clone());
                }
                let _ = tx.send(Job::Rebuild {
                    features: features.into_list(),
                    ticket: 0,
                    reply,
                    cancelled,
                    persist,
                });
                let got = rx.recv();
                if let Ok(mut r) = running.lock() {
                    *r = None;
                }
                match got {
                    Ok(b) => {
                        let Ok(mut sent) = sent.lock() else {
                            return 1;
                        };
                        let (b, solids) = sent.strip(&b);
                        Event::Rebuilt(Arc::new(b), solids)
                    }
                    Err(_) => return 1,
                }
            }
            Request::Export { features, request } => {
                let (reply, rx) = mpsc::channel();
                let _ = tx.send(Job::Export {
                    features: features.into_list(),
                    request,
                    reply,
                });
                Event::Exported(
                    rx.recv()
                        .unwrap_or_else(|_| Err("The rebuild thread stopped".into())),
                )
            }
            Request::Project { features, request } => {
                let (reply, rx) = mpsc::channel();
                let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let _ = tx.send(Job::Project {
                    features: features.into_list(),
                    request: Box::new(request),
                    reply,
                    cancelled,
                });
                Event::Projected(
                    rx.recv()
                        .unwrap_or_else(|_| Err("The rebuild thread stopped".into())),
                )
            }
            Request::ProjectAssembly { state, request } => {
                let (reply, rx) = mpsc::channel();
                let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let _ = tx.send(Job::ProjectAssembly {
                    state: Box::new(state),
                    request: Box::new(request),
                    reply,
                    cancelled,
                });
                Event::Projected(
                    rx.recv()
                        .unwrap_or_else(|_| Err("The rebuild thread stopped".into())),
                )
            }
            Request::Task { name, input } => {
                let (reply, rx) = mpsc::channel();
                let _ = tx.send(Job::Run(Box::new(move |r: &mut Rebuilder| {
                    let _ = reply.send(super::task::run_named(&name, &input, r));
                })));
                Event::TaskDone(
                    rx.recv()
                        .unwrap_or_else(|_| Err("the task panicked".into())),
                )
            }
        };
        if !send(&reply) {
            return 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What crosses to the kernel process comes back the same: every feature list of the
    /// stored fixtures (all the feature kinds they use), and a rebuild's result.
    #[test]
    fn feature_lists_and_builds_survive_the_trip() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let dir = tempfile::tempdir().unwrap();
        let store = crate::Store::new(dir.path());
        let mut lists = 0;
        for entry in cadrs_fs::read_dir(&fixtures).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "cadrs") {
                continue;
            }
            let file = crate::native::import(&path, &store, "test", 0)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for el in &file.document.elements {
                let list = el.features().to_vec();
                if list.is_empty() {
                    continue;
                }
                let back: Features = decode(&encode(&Features::of(list.clone())).unwrap()).unwrap();
                assert!(
                    back.into_list() == list,
                    "{}: {} changed on the way",
                    path.display(),
                    el.name
                );
                lists += 1;
            }
        }
        assert!(lists > 30, "only {lists} feature lists");

        // A Sheet metal model taking picks stays so (its saved form leaves that out).
        let mut list = crate::samples::sheetmetal_legacy::channel();
        let mut picking = 0;
        for f in &mut list {
            if let crate::document::FeatureKind::SheetMetalModel(x) = &mut f.kind {
                x.picking = true;
                picking += 1;
            }
        }
        assert!(picking > 0);
        let back: Features = decode(&encode(&Features::of(list.clone())).unwrap()).unwrap();
        assert!(back.into_list() == list);

        for list in [
            crate::samples::bracket_pair::bracket(),
            crate::samples::sheetmetal_legacy::channel(),
            crate::samples::sheetmetal_legacy::case_round(),
        ] {
            let build = Rebuilder::new().rebuild(&list);
            assert!(!build.parts.is_empty());
            let back: Build = decode(&encode(&build).unwrap()).unwrap();
            assert!(back == build, "the build changed on the way");
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod server_tests {
    use super::*;

    #[test]
    fn a_kernel_server_wants_its_token() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            serve_on(
                listener,
                "secret".into(),
                PathBuf::from("/nonexistent/cadrs"),
                Vec::new(),
            )
        });
        let e = connect_server(&addr, "wrong").unwrap_err();
        assert!(e.to_string().contains("refused: wrong token"), "{e}");
        // The right token but no kernel to start: refused as well, saying why.
        let e = connect_server(&addr, "secret").unwrap_err();
        assert!(e.to_string().contains("refused: its kernel"), "{e}");
        // No token at all is never sent.
        assert!(connect_server(&addr, "").is_err());
        // A stranger announcing a huge message is dropped without allocating it.
        let mut s = TcpStream::connect(&addr).unwrap();
        s.write_all(&u64::MAX.to_le_bytes()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(s.read(&mut buf).unwrap_or(0), 0);
        // And the app, pointed at something that isn't a kernel server, doesn't allocate what
        // its first bytes announce either.
        let other = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let other_addr = other.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = other.accept() {
                let _ = s.write_all(b"SSH-2.0-OpenSSH_9.6\r\n");
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        let e = connect_server(&other_addr, "secret").unwrap_err();
        assert!(e.to_string().contains("is not a kernel server"), "{e}");
        assert!(same_secret("abc", "abc") && !same_secret("abc", "abd") && !same_secret("a", "ab"));
    }
}
