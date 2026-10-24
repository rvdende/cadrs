//! Work on the rebuild session as named requests ([`Task`]), for work that needs the kernel
//! session itself rather than a rebuild's result (a section's caps, an assembly's interference,
//! an import plan, ...).
//!
//! A task is a value (its parameters) with a name and a [`Task::run`] on the [`Rebuilder`].
//! On the kernel thread it simply runs. With the kernel in its own process
//! ([`super::process`]) it is sent there as its name and its encoded parameters, and the result
//! comes back encoded: the kernel process finds the task by its name, so every task must be
//! registered there ([`register`]; the tasks of `cadrs_core` always are). A task the kernel
//! process doesn't know, or one it crashed on, has no result (`None` from [`PendingJob`]), as
//! a job on a worker that stopped.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, RwLock, mpsc};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{Job, PendingJob, Rebuilder};

/// Work on the rebuild session, by name (see the module docs).
pub trait Task: Serialize + DeserializeOwned + Send + 'static {
    /// Its name, unique among the tasks: the kernel process finds [`Task::run`] by it.
    const NAME: &'static str;
    type Output: Serialize + DeserializeOwned + Send + 'static;
    fn run(self, r: &mut Rebuilder) -> Self::Output;
}

/// A task's [`Task::run`] on encoded parameters, giving its encoded output.
type Handler = fn(&[u8], &mut Rebuilder) -> Result<Vec<u8>, String>;

fn handle<T: Task>(input: &[u8], r: &mut Rebuilder) -> Result<Vec<u8>, String> {
    let task: T = super::process::decode(input)?;
    super::process::encode(&task.run(r))
}

fn registry() -> &'static RwLock<HashMap<&'static str, Handler>> {
    static REGISTRY: OnceLock<RwLock<HashMap<&'static str, Handler>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut m: HashMap<&'static str, Handler> = HashMap::new();
        let mut add = |name: &'static str, h: Handler| {
            m.insert(name, h);
        };
        add(super::StageOf::NAME, handle::<super::StageOf>);
        add(super::ClearCache::NAME, handle::<super::ClearCache>);
        add(
            super::SaveSnapshotNow::NAME,
            handle::<super::SaveSnapshotNow>,
        );
        add(
            super::exchange::PlanImport::NAME,
            handle::<super::exchange::PlanImport>,
        );
        add(
            super::exchange::ExportFiles::NAME,
            handle::<super::exchange::ExportFiles>,
        );
        add(crate::section::Caps::NAME, handle::<crate::section::Caps>);
        add(
            crate::cam::geometry::CamModel::NAME,
            handle::<crate::cam::geometry::CamModel>,
        );
        add(
            crate::assembly::interference::Check::NAME,
            handle::<crate::assembly::interference::Check>,
        );
        add(
            crate::assembly::interference::ExportStep::NAME,
            handle::<crate::assembly::interference::ExportStep>,
        );
        add(
            crate::samples::StepFixture::NAME,
            handle::<crate::samples::StepFixture>,
        );
        RwLock::new(m)
    })
}

/// Makes a task of another crate known, so the kernel process can run it. The app registers
/// them at start, in the app and in the kernel process alike (the same executable).
pub fn register<T: Task>() {
    if let Ok(mut r) = registry().write() {
        r.insert(T::NAME, handle::<T>);
    }
}

/// Runs the task named `name` on encoded parameters (in the kernel process).
pub(super) fn run_named(name: &str, input: &[u8], r: &mut Rebuilder) -> Result<Vec<u8>, String> {
    let handler = registry().read().ok().and_then(|m| m.get(name).copied());
    match handler {
        Some(h) => h(input, r),
        None => Err(format!("the kernel process doesn't know the task {name}")),
    }
}

/// Runs `task` on the rebuild session, after the jobs queued before it.
pub fn run<T: Task>(task: T) -> PendingJob<T::Output> {
    if !super::process::enabled() {
        return super::run_on_worker(move |r| task.run(r));
    }
    let (reply, rx) = mpsc::channel::<T::Output>();
    let input = match super::process::encode(&task) {
        Ok(i) => i,
        // Dropping `reply` reports no result.
        Err(_) => return PendingJob { rx: Mutex::new(rx) },
    };
    let done = Box::new(move |output: Option<Vec<u8>>| {
        if let Some(out) = output.and_then(|o| super::process::decode::<T::Output>(&o).ok()) {
            let _ = reply.send(out);
        }
    });
    let job = Job::Task {
        name: T::NAME,
        input,
        done,
    };
    let _ = super::worker().lock().map(|tx| tx.send(job));
    PendingJob { rx: Mutex::new(rx) }
}
