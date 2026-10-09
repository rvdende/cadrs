//! Files dragged onto the window: logged as they come (`DRAG DETECTED`, `DROP DETECTED`), and on
//! Wayland read off the compositor by hand.
//!
//! Bevy's [`FileDragAndDrop`] message is what the rest of the app listens to
//! ([`crate::picture`]). winit sends it on Windows, macOS and X11, but its Wayland backend has no
//! `wl_data_device` code: on a Wayland session a drop is never delivered. This module is that
//! missing half (after the robot2 `substrate` crate's `filedrop`), and writes the same
//! [`FileDragAndDrop::DroppedFile`] messages, so a drop takes one path from there on.
//!
//! - **winit's connection, not a new one.** A drag offer goes to the client that owns the
//!   surface under the pointer; a second `wl_display` connection would be a second client and
//!   hear nothing. So the `wl_display` is taken from the window's raw display handle and wrapped
//!   with `Backend::from_foreign_display`, sharing winit's socket. Events are read on an event
//!   queue of this module's own: a proxy belongs to the queue it was made on, so neither side
//!   consumes the other's events.
//! - **A thread.** `wl_data_offer.receive` hands the source a pipe and the paths arrive when the
//!   source writes them, so reading a drop blocks; it happens on this module's thread, and the
//!   paths cross back over a channel.
//! - **Where it was dropped.** The drag's `enter` and `motion` events carry the pointer's
//!   position on the window, which the window's own cursor position doesn't follow during a
//!   drag; [`DropPosition`] keeps the last one for whoever places what was dropped.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::FileDragAndDrop;

pub struct FileDropPlugin;

impl Plugin for FileDropPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DropPosition>().add_systems(Update, log_drops);
        #[cfg(target_os = "linux")]
        app.add_systems(PreUpdate, (wayland::listen, wayland::deliver).chain());
    }
}

/// Where the pointer was on the window (logical pixels) when files were last dragged over or
/// dropped on it, if the platform said (Wayland does; elsewhere the window's cursor position is
/// current during a drag).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct DropPosition(pub Option<Vec2>);

/// Logs every drag and drop the app is told about.
fn log_drops(mut drops: MessageReader<FileDragAndDrop>) {
    for d in drops.read() {
        match d {
            FileDragAndDrop::HoveredFile { path_buf, .. } => {
                info!("DRAG DETECTED: file {} ({})", path_buf.display(), kind(path_buf));
            }
            FileDragAndDrop::DroppedFile { path_buf, .. } => {
                info!("DROP DETECTED: file {} ({})", path_buf.display(), kind(path_buf));
            }
            FileDragAndDrop::HoveredFileCanceled { .. } => info!("DRAG CANCELLED"),
        }
    }
}

/// What a dropped file is, by its extension: for the log.
fn kind(path: &std::path::Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if cadrs_core::picture::ImageFormat::of_path(path).is_some() => format!("picture, .{ext}"),
        Some(ext) => format!(".{ext}"),
        None => "no extension".into(),
    }
}

/// The `file://` entries of an RFC 2483 URI list (what a file manager offers a dragged file
/// as), as local paths. Comments and other URIs (a browser's `https://`) are left out.
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.strip_prefix("file://"))
        // The authority (empty, or `localhost`) ends at the first `/`, where the path begins.
        .map(|rest| rest.split_once('/').map_or(rest.to_string(), |(_, path)| format!("/{path}")))
        .map(|path| PathBuf::from(percent_decode(&path)))
        .collect()
}

/// `%20` and the like, as a URI spells awkward path characters.
fn percent_decode(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match hex {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(target_os = "linux")]
mod wayland {
    use std::io::Read;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::mpsc::{Receiver, Sender, channel};

    use bevy::prelude::*;
    use bevy::window::{FileDragAndDrop, PrimaryWindow, RawHandleWrapper};
    use raw_window_handle::RawDisplayHandle;
    use wayland_client::backend::Backend;
    use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
    use wayland_client::protocol::wl_data_device_manager::{self, WlDataDeviceManager};
    use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
    use wayland_client::protocol::wl_registry::{self, WlRegistry};
    use wayland_client::protocol::wl_seat::WlSeat;
    use wayland_client::{Connection, Dispatch, QueueHandle};

    use super::DropPosition;

    /// The type a file manager offers dragged files as. Only this one is asked for: some
    /// sources offer the same paths as `text/plain` too, and taking both would drop twice.
    const URI_LIST: &str = "text/uri-list";

    /// What the listener thread tells the app.
    enum Event {
        /// The pointer is over the window at this position during a drag.
        At(Vec2),
        /// Files dropped.
        Dropped(Vec<PathBuf>),
    }

    /// The channel from the listener thread (and the mark that it runs).
    #[derive(Resource)]
    pub(super) struct Drops(Mutex<Receiver<Event>>);

    /// Starts the listener once there is a window on Wayland (at build time there is none).
    pub(super) fn listen(window: Query<&RawHandleWrapper, With<PrimaryWindow>>, existing: Option<Res<Drops>>, mut commands: Commands) {
        if existing.is_some() {
            return;
        }
        let Ok(handle) = window.single() else { return };
        // Elsewhere winit delivers drops itself.
        let RawDisplayHandle::Wayland(wayland) = handle.get_display_handle() else { return };
        // Only used to open a second view onto the connection the window owns, which outlives
        // the thread (a send to the gone app ends it).
        let display = wayland.display.as_ptr() as usize;
        let (tx, rx) = channel();
        if let Err(e) = std::thread::Builder::new().name("wayland-file-drop".into()).spawn(move || run(display, tx)) {
            warn!("file drop: cannot start the Wayland listener: {e}");
            return;
        }
        commands.insert_resource(Drops(Mutex::new(rx)));
        info!("file drop: listening for Wayland drags");
    }

    /// Turns what the listener heard into [`FileDragAndDrop`] messages.
    pub(super) fn deliver(
        drops: Option<Res<Drops>>,
        window: Query<Entity, With<PrimaryWindow>>,
        mut position: ResMut<DropPosition>,
        mut out: MessageWriter<FileDragAndDrop>,
    ) {
        let Some(drops) = drops else { return };
        let Ok(rx) = drops.0.lock() else { return };
        let window = window.single().unwrap_or(Entity::PLACEHOLDER);
        for event in rx.try_iter() {
            match event {
                Event::At(at) => position.0 = Some(at),
                Event::Dropped(paths) => {
                    for path_buf in paths {
                        out.write(FileDragAndDrop::DroppedFile { window, path_buf });
                    }
                }
            }
        }
    }

    /// The listener: a data device on winit's connection, read on a queue of its own.
    fn run(display: usize, tx: Sender<Event>) {
        // SAFETY: `display` is the `wl_display` winit created (the window's raw display handle).
        // `from_foreign_display` borrows it (it doesn't disconnect it when dropped), so winit
        // stays its only owner.
        let backend = unsafe { Backend::from_foreign_display(display as *mut _) };
        let connection = Connection::from_backend(backend);
        let mut queue = connection.new_event_queue();
        let handle = queue.handle();
        // A registry of our own: the compositor announces every global to each registry, so
        // this sees the seat and the data device manager without touching winit's.
        let _registry = connection.display().get_registry(&handle, ());
        let mut state = State { seat: None, manager: None, device: None, offer: None, tx };
        loop {
            if queue.blocking_dispatch(&mut state).is_err() {
                // The connection is gone: the app is closing.
                return;
            }
            state.arm(&handle);
        }
    }

    struct State {
        seat: Option<WlSeat>,
        manager: Option<WlDataDeviceManager>,
        /// Kept alive: dropping it would stop the seat's drags reaching us.
        device: Option<WlDataDevice>,
        /// The drag over the window, if any.
        offer: Option<Offer>,
        tx: Sender<Event>,
    }

    /// A drag in progress.
    struct Offer {
        offer: WlDataOffer,
        /// The types the source offers (for the log).
        types: Vec<String>,
        /// The `enter` serial (`motion` has none, and re-accepting needs one).
        serial: u32,
    }

    impl Offer {
        fn has_uris(&self) -> bool {
            self.types.iter().any(|t| t == URI_LIST)
        }
    }

    impl State {
        /// A data device as soon as there are a seat and a manager to ask.
        fn arm(&mut self, handle: &QueueHandle<Self>) {
            if self.device.is_some() {
                return;
            }
            if let (Some(seat), Some(manager)) = (&self.seat, &self.manager) {
                self.device = Some(manager.get_data_device(seat, handle, ()));
                info!("file drop: Wayland data device bound");
            }
        }
    }

    impl Dispatch<WlRegistry, ()> for State {
        fn event(state: &mut Self, registry: &WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, handle: &QueueHandle<Self>) {
            let wl_registry::Event::Global { name, interface, version } = event else { return };
            match interface.as_str() {
                // Only something to hang a data device on.
                "wl_seat" if state.seat.is_none() => state.seat = Some(registry.bind(name, version.min(5), handle, ())),
                // Version 3 for `finish`, which tells the source the drop was taken (a file
                // manager may otherwise wait on it).
                "wl_data_device_manager" => state.manager = Some(registry.bind(name, version.min(3), handle, ())),
                _ => {}
            }
        }
    }

    impl Dispatch<WlDataDevice, ()> for State {
        fn event(state: &mut Self, _: &WlDataDevice, event: wl_data_device::Event, _: &(), connection: &Connection, _: &QueueHandle<Self>) {
            match event {
                // A drag entered the window: accepting a type is what shows "copy" rather than
                // "no" on the pointer.
                wl_data_device::Event::Enter { serial, id, x, y, .. } => {
                    let Some(offer) = id else { return };
                    let types = state.offer.take().filter(|o| o.offer == offer).map(|o| o.types).unwrap_or_default();
                    let pending = Offer { offer, types, serial };
                    info!(
                        "DRAG DETECTED: Wayland drag entered at ({x:.0}, {y:.0}), types {:?}, {}",
                        pending.types,
                        if pending.has_uris() { "files: accepted" } else { "no files: declined" }
                    );
                    if pending.has_uris() {
                        pending.offer.accept(serial, Some(URI_LIST.to_string()));
                        pending.offer.set_actions(wl_data_device_manager::DndAction::Copy, wl_data_device_manager::DndAction::Copy);
                    } else {
                        pending.offer.accept(serial, None);
                    }
                    let _ = state.tx.send(Event::At(Vec2::new(x as f32, y as f32)));
                    state.offer = Some(pending);
                }
                // Re-accepted as the pointer moves, or some compositors count the drag as
                // declined by the time it is released.
                wl_data_device::Event::Motion { x, y, .. } => {
                    if let Some(pending) = &state.offer
                        && pending.has_uris()
                    {
                        pending.offer.accept(pending.serial, Some(URI_LIST.to_string()));
                        let _ = state.tx.send(Event::At(Vec2::new(x as f32, y as f32)));
                    }
                }
                wl_data_device::Event::Leave => {
                    if let Some(pending) = state.offer.take() {
                        info!("DRAG CANCELLED: the Wayland drag left the window");
                        pending.offer.destroy();
                    }
                }
                wl_data_device::Event::Drop => {
                    let Some(pending) = state.offer.take() else { return };
                    if !pending.has_uris() {
                        info!("DROP DETECTED: Wayland drop of {:?}: no files, ignored", pending.types);
                        pending.offer.destroy();
                        return;
                    }
                    match receive(connection, &pending.offer) {
                        Ok(text) => {
                            let paths = super::parse_uri_list(&text);
                            info!("DROP DETECTED: Wayland drop of {} file(s): {paths:?}", paths.len());
                            if state.tx.send(Event::Dropped(paths)).is_err() {
                                return;
                            }
                            // The transfer is done (a file manager may show it in flight until
                            // it hears so).
                            pending.offer.finish();
                        }
                        Err(e) => warn!("DROP DETECTED: cannot read the Wayland drop: {e}"),
                    }
                    pending.offer.destroy();
                }
                _ => {}
            }
        }

        // An offer arrives as a new object before the `enter` that uses it: its types are
        // learned here.
        fn event_created_child(_opcode: u16, handle: &QueueHandle<Self>) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
            handle.make_data::<WlDataOffer, ()>(())
        }
    }

    impl Dispatch<WlDataOffer, ()> for State {
        fn event(state: &mut Self, offer: &WlDataOffer, event: wl_data_offer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
            // The source lists its types, one event each, before the drag enters.
            if let wl_data_offer::Event::Offer { mime_type } = event {
                match &mut state.offer {
                    Some(pending) if pending.offer == *offer => pending.types.push(mime_type),
                    _ => state.offer = Some(Offer { offer: offer.clone(), types: vec![mime_type], serial: 0 }),
                }
            }
        }
    }

    /// Reads an offer's URI list off the pipe the source writes into. The write end is closed
    /// once handed over, or the read would wait for this process's own copy of it.
    fn receive(connection: &Connection, offer: &WlDataOffer) -> std::io::Result<String> {
        let (mut read, write) = std::io::pipe()?;
        {
            use std::os::fd::AsFd;
            offer.receive(URI_LIST.to_string(), write.as_fd());
        }
        // The request must reach the compositor before this thread blocks on the read.
        connection.flush().map_err(|e| std::io::Error::other(format!("flushing the receive request: {e}")))?;
        drop(write);
        let mut text = String::new();
        read.read_to_string(&mut text)?;
        Ok(text)
    }

    wayland_client::delegate_noop!(State: ignore WlSeat);
    wayland_client::delegate_noop!(State: WlDataDeviceManager);
}

#[cfg(test)]
mod tests {
    use super::*;

    // What a file manager sends: CRLF line ends, one entry per file.
    #[test]
    fn a_uri_list_becomes_paths() {
        let list = "file:///home/someone/a.png\r\nfile:///tmp/b.JPG\r\n";
        assert_eq!(parse_uri_list(list), vec![PathBuf::from("/home/someone/a.png"), PathBuf::from("/tmp/b.JPG")]);
    }

    #[test]
    fn escapes_are_decoded() {
        assert_eq!(parse_uri_list("file:///tmp/two%20words%2Bmore.png"), vec![PathBuf::from("/tmp/two words+more.png")]);
    }

    // A browser's drag lists URLs too: only local files are taken, and `localhost` keeps the
    // path's root.
    #[test]
    fn only_local_files_are_taken() {
        let list = "#comment\r\nhttps://example.com/x.png\r\nfile://localhost/tmp/ok.png\r\n";
        assert_eq!(parse_uri_list(list), vec![PathBuf::from("/tmp/ok.png")]);
    }
}
