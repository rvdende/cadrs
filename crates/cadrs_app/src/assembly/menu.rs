//! Assembly context menus (A3.3, A3.4, A3.7, A4.2–A4.4, A4.6, X15; `ex1-step6.png`,
//! `ex1-step7.png`, `ex1-step9.png`, `ex2-step13.png`):
//!
//! - **Instance menu**, from the Instances list (`ex1-step9.png`) or on an instance in the view
//!   (`ex1-step6.png`): Hide / Show, Hide other instances, Hide all instances, Isolate…, Make
//!   transparent…, Suppress / Unsuppress, Fix / Unfix, Switch to <Part Studio> (or to the
//!   subassembly's tab), Find in instance list, Clear selection, Zoom to fit, Zoom to selection
//!   and Delete work; P3B.4: **Move to new subassembly**, **Create empty subassembly**, **Add
//!   selection to folder…** and, on a subassembly, **Dissolve subassembly** and **Make flexible
//!   / Make rigid** (A16.2, A17, A18). The other items of Onshape's menu are shown disabled
//!   until their milestones (drawings P3C.1, export P3F.2, versions P3D.3, section view P3E.3,
//!   …). "Add comment" and "Create task…" are collaboration (out of scope) and stay disabled.
//! - On the **triad** the menu starts with the handle's own items: the origin's **Move to
//!   origin**, an arrow's **Align with Z** / **Anti-align with Z**, a ring's **Rotate 90°** /
//!   **Rotate 180°** (see [`super::triad`]).
//! - **Empty space** (`ex2-step13.png`): Show all, Show all instances, Paste (P3B.2), Create
//!   Drawing (P3C.1), Zoom to fit, Isometric.
//!
//! The menu acts on the selected instances when the right-clicked one is among them, else on it
//! alone (as the Parts list does).

use bevy::prelude::*;
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::commands::{DeleteInstances, SetInstancesFixed, SetInstancesHidden};
use cadrs_core::assembly::structure::{DissolveSubassembly, MoveToNewSubassembly, SetInstancesSuppressed, SetSubassemblyFlexible};
use cadrs_core::ElementId;
use cadrs_ui::menu::{ContextMenuAnchor, Menu, MenuAction, MenuEntry, MenuItem};
use cadrs_ui::{Theme, open_context_menu};

use super::triad::TriadHandle;
use crate::parts::PartCache;
use crate::viewport::{Pick, Selection};
use crate::{ActiveDocument, AppState};

pub struct AssemblyMenuPlugin;

impl Plugin for AssemblyMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_menu_action);
    }
}

/// What an open assembly menu is for.
#[derive(Component, Debug, Clone)]
struct AsmMenu {
    element: ElementId,
    /// The right-clicked instance (`None`: empty space).
    instance: Option<InstanceId>,
    /// The instances the items act on.
    targets: Vec<InstanceId>,
    /// Where it was opened (Paste puts the instance there).
    at: Vec2,
}

/// The instances a menu on `instance` acts on: the selection if `instance` is in it.
fn targets(world: &World, instance: InstanceId) -> Vec<InstanceId> {
    let selected = super::selected_instances(world.resource::<Selection>());
    if selected.contains(&instance) { selected } else { vec![instance] }
}

/// The right-click in the view (P3B.1): on the triad, its handle's menu; on an instance, the
/// instance menu (the instance is selected while the menu is open); else the empty-space menu.
pub fn open_viewport_menu(world: &mut World, at: Vec2, pick: Option<Pick>) {
    if let Some((instance, handle)) = super::triad::handle_at(world, at) {
        open_instance_menu(world, at, instance, handle);
        return;
    }
    match pick.as_ref().and_then(super::instance_of) {
        Some(instance) => {
            if !super::selected_instances(world.resource::<Selection>()).contains(&instance)
                && let Some(p) = pick
            {
                world.resource_mut::<Selection>().0 = vec![p];
            }
            open_instance_menu(world, at, instance, TriadHandle::View);
        }
        None => open_empty_menu(world, at),
    }
}

fn names(world: &World, instance: InstanceId) -> (String, String, String) {
    let cache = world.resource::<PartCache>();
    let doc = world.resource::<ActiveDocument>();
    let inst = doc.active_element().and_then(|e| e.assembly_model()?.instance(instance).cloned());
    let name = match &inst {
        Some(i) if i.source.is_composite() || i.suppressed => i.name(&cadrs_core::assembly::source_part_name(&doc.doc, &i.source, None)),
        _ => cache.part_name(instance.part_id()).unwrap_or("instance").to_string(),
    };
    let studio = doc
        .active_element()
        .and_then(|e| e.assembly_model()?.instance(instance))
        .and_then(|i| doc.doc.element(i.source.element()))
        .map(|e| e.name.clone())
        .unwrap_or_default();
    let asm = doc.active_element().map(|e| e.name.clone()).unwrap_or_default();
    (name, studio, asm)
}

/// The instance menu. `handle` is where it was opened: the list (`None`), the view (`View`),
/// or a triad handle (its own items first).
pub fn open_instance_menu(world: &mut World, at: Vec2, instance: InstanceId, handle: TriadHandle) {
    let Some(element) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) else {
        return;
    };
    let Some(inst) = world
        .resource::<ActiveDocument>()
        .active_element()
        .and_then(|e| e.assembly_model()?.instance(instance).cloned())
    else {
        return;
    };
    let targets = targets(world, instance);
    let (name, studio, asm) = names(world, instance);
    // Create Drawing of … names the part (a standard content part by its Name, not its
    // "Standard content" studio; P3C wrap-up) or the subassembly.
    let drawing_of = {
        let doc = &world.resource::<ActiveDocument>().doc;
        match inst.source {
            cadrs_core::assembly::InstanceSource::Part { element, part } => {
                let n = cadrs_core::properties::text(doc, cadrs_core::properties::PropertyOwner::Part { element, part }, cadrs_core::properties::PropertyKey::Name, None);
                if n.trim().is_empty() { cadrs_core::assembly::source_part_name(doc, &inst.source, None) } else { n }
            }
            _ => studio.clone(),
        }
    };
    let transparent = super::view_parts(world.resource::<PartCache>(), &[instance]).iter().any(|p| world.resource::<PartCache>().transparent.contains(p));
    let hide = if inst.hidden {
        MenuItem::new("asm-show", "Show").icon("visible")
    } else {
        MenuItem::new("asm-hide", "Hide").icon("hidden")
    };
    let is_sub = inst.source.is_assembly();
    // P3B.5: a standard content instance (A19.8, A19.9); it has no tab to switch to.
    let is_std = cadrs_core::assembly::standard::standard_of(&world.resource::<ActiveDocument>().doc, &inst.source).is_some();
    let suppress = if inst.suppressed { MenuItem::new("asm-unsuppress", "Unsuppress") } else { MenuItem::new("asm-suppress", "Suppress") };
    // P3G.1: a linked instance's source is a frozen copy: no tab to switch to or edit in context.
    let is_linked = inst.link.is_some();
    let is_version = inst.link.is_some_and(|l| matches!(l.at, cadrs_core::external::RefAt::Version(_)));
    let pinned = inst.link.is_some_and(|l| l.pinned);
    const LINKED: &str = "A linked instance shows a frozen version of its source: there is no tab to switch to";
    // P3G.3 (DV1.9): its source document at the version, read-only; not while the source can't
    // be reached (DV1.7).
    let open_linked = {
        let doc = &world.resource::<ActiveDocument>().doc;
        let site = cadrs_core::link_update::RefSite::Instance { element, instance };
        match cadrs_core::link_update::use_at(doc, site) {
            Some(u) if crate::linked_session::can_open(doc, world.resource::<crate::linked::LinkStatus>(), &u) => None,
            Some(u) if u.is_version() => Some(world.resource::<crate::linked::LinkStatus>().state(u.reference.document_or(doc.id)).message().unwrap_or("The linked document can't be opened")),
            _ => Some("Only a reference to a version opens its document"),
        }
    };
    let switch = if is_std {
        MenuItem::new("asm-switch-to", "Switch to Standard content").icon("standard-content").disabled(true)
    } else if is_linked {
        MenuItem::new("asm-switch-to", format!("Switch to {studio}")).icon(if is_sub { "assembly" } else { "part-studio" }).disabled(true).tooltip(LINKED)
    } else {
        MenuItem::new("asm-switch-to", format!("Switch to {studio}")).icon(if is_sub { "assembly" } else { "part-studio" })
    };
    let std_items = |menu: Menu| -> Menu {
        if !is_std {
            return menu;
        }
        menu.item(MenuItem::new("asm-std-edit", "Edit standard content instance…").icon("standard-content"))
            .item(MenuItem::new("asm-select-same-config", "Select instances with same configuration"))
            .item(MenuItem::new("asm-select-same-part", "Select instances with same part and same configuration"))
    };
    let copy_label = if targets.len() > 1 { format!("Copy {} items", targets.len()) } else { format!("Copy {name}") };
    let fix = if inst.fixed {
        MenuItem::new("asm-unfix", "Unfix").icon("constraint-fix")
    } else {
        MenuItem::new("asm-fix", "Fix").icon("constraint-fix")
    };
    let transparent_item = if transparent {
        MenuItem::new("asm-opaque", "Make opaque")
    } else {
        MenuItem::new("asm-transparent", "Make transparent…")
    };
    // P3B.9: items of other stages stay disabled, saying what they wait for.
    let later = |id: &'static str, label: String, why: &str| MenuItem::new(id, label).disabled(true).tooltip(why.to_string());
    const COLLAB: &str = "Collaboration (comments, tasks) is out of scope";
    const RELEASE: &str = "Release management is out of scope";
    const TESSELLATION: &str = "Parts are always drawn from their full tessellation";
    let parts_only = targets.iter().all(|t| {
        world.resource::<ActiveDocument>().active_element().and_then(|e| e.assembly_model()?.instance(*t)).is_some_and(|i| i.source.part().is_some())
    });
    // MC2.14, MC4.1, MC4.6, MC3.3: Edit in context (▸ its contexts in this assembly and New
    // context), Update context ▸ its contexts, Set as primary instance.
    // MC5: an instance of a part from another document (a version reference) is edited in
    // context in its own document.
    let external = cadrs_core::assembly::context::linked_studio_of(&world.resource::<ActiveDocument>().doc, element, instance).is_some();
    let contexts = if external { super::linked_context::contexts_of(world, element, instance) } else { super::managed_context::contexts_of(world.resource::<ActiveDocument>(), element, instance) };
    let no_edit = is_sub || is_std || (is_linked && !external);
    let in_context = if contexts.is_empty() || no_edit {
        let mut item = MenuItem::new("asm-edit-in-context", "Edit in context").icon("part-studio").disabled(no_edit);
        if is_linked && !external {
            item = item.tooltip("A version of this document is edited at its tab");
        }
        item
    } else {
        let mut items: Vec<cadrs_ui::MenuEntry> = contexts.iter().map(|c| MenuItem::new(format!("asm-edit-in-context-{}", c.id), c.label.clone()).icon("part-studio").into()).collect();
        items.push(MenuItem::new("asm-edit-in-context-new", "New context").icon("plus").into());
        MenuItem::new("asm-edit-in-context", "Edit in context").icon("part-studio").submenu(items)
    };
    let update_context = (!contexts.is_empty()).then(|| {
        let items = contexts
            .iter()
            .map(|c| {
                let mut item = MenuItem::new(format!("asm-update-context-{}", c.id), c.label.clone()).icon("part-studio").disabled(c.no_primary);
                if c.no_primary {
                    item = item.tooltip(cadrs_core::assembly::context::NO_PRIMARY);
                } else if c.stale {
                    item = item.dot(Color::srgb(0.17, 0.49, 0.91)).tooltip("An update is available");
                }
                item.into()
            })
            .collect();
        MenuItem::new("asm-update-context", "Update context").icon("restore").submenu(items)
    });
    let set_primary = (!external && contexts.iter().any(|c| !c.primary)).then(|| MenuItem::new("asm-set-primary", "Set as primary instance").icon("origin"));
    let mut menu = Menu::new("assembly-context-menu").min_width(210.0).item_height(20.0);
    match handle {
        TriadHandle::Origin => menu = menu.item(MenuItem::new("asm-move-to-origin", "Move to origin")),
        TriadHandle::Arrow(_) => {
            menu = menu
                .item(MenuItem::new("asm-align-z", "Align with Z"))
                .item(MenuItem::new("asm-anti-align-z", "Anti-align with Z"));
        }
        TriadHandle::Ring(_) => {
            menu = menu
                .item(MenuItem::new("asm-rotate-90", "Rotate 90°"))
                .item(MenuItem::new("asm-rotate-180", "Rotate 180°"));
        }
        TriadHandle::Plane(_) | TriadHandle::View | TriadHandle::None => {}
    }
    if handle == TriadHandle::None {
        // The Instances list's menu (`ex1-step9.png`).
        menu = menu
            .item(MenuItem::new("asm-properties", "Properties…").icon("properties"))
            .item(hide)
            .item(MenuItem::new("asm-hide-others", "Hide other instances"))
            .item(MenuItem::new("asm-hide-all", "Hide all instances"))
            .item(MenuItem::new("asm-isolate", "Isolate…"))
            .item(transparent_item)
            .item(suppress)
            .item(fix)
            .item(MenuItem::new("asm-show-mates", "Show mates"))
            .item(MenuItem::new("asm-hide-mates", "Hide mates"))
            .item(MenuItem::new("asm-interference", "Check interference").icon("intersection"))
            .item(later("asm-tessellation", "Use best available tessellation".into(), TESSELLATION))
            .item(MenuItem::new("asm-connector", "Add mate connector to instance origin…").icon("mate-connector"))
            .item(MenuItem::new("asm-replace", "Replace instances…").disabled(!parts_only).icon("transform"));
        menu = std_items(menu).item(in_context.clone());
        if let Some(u) = update_context.clone() {
            menu = menu.item(u);
        }
        if let Some(p) = set_primary.clone() {
            menu = menu.item(p);
        }
        menu = menu
            .item(switch.clone())
            .separator()
            .item(MenuItem::new("asm-copy", copy_label).icon("copy"))
            .item(MenuItem::new("asm-move-to-new", "Move to new subassembly"));
        if is_sub {
            // A16.2: Lock / follow position to ▸ the current position or a Named position of
            // the subassembly's tab.
            let positions: Vec<(usize, String)> = world
                .resource::<ActiveDocument>()
                .doc
                .element(inst.source.element())
                .and_then(|e| e.assembly_model())
                .map(|a| a.named_positions.iter().enumerate().map(|(k, p)| (k, p.name.clone())).collect())
                .unwrap_or_default();
            let followed = inst.follow.and_then(|f| {
                world.resource::<ActiveDocument>().doc.element(inst.source.element())?.assembly_model()?.named_positions.iter().position(|p| p.id == f)
            });
            let mut follow: Vec<MenuEntry> = vec![MenuItem::new("asm-follow-none", "Current position").checked(followed.is_none() && !inst.flexible).into()];
            for (k, n) in positions {
                follow.push(MenuItem::new(format!("asm-follow-{k}"), n).checked(followed == Some(k)).into());
            }
            menu = menu
                .item(MenuItem::new("asm-dissolve", "Dissolve subassembly"))
                .item(if inst.flexible { MenuItem::new("asm-rigid", "Make rigid").icon("lock-filled") } else { MenuItem::new("asm-flexible", "Make flexible") })
                .item(MenuItem::new("asm-follow", "Lock / follow position to").submenu(follow));
        }
        // A2.4: a rigid Part Studio instance's Edit adds or removes parts.
        if inst.source.is_studio() {
            menu = menu.item(MenuItem::new("asm-studio-edit", "Edit…").icon("edit"));
        }
        // A linked instance's items (ER2.4, ER5.1, ER5.7, ER6.7).
        if is_linked {
            let pin = if pinned {
                MenuItem::new("asm-unpin", "Unpin reference").icon("location")
            } else {
                MenuItem::new("asm-pin", "Pin reference").icon("location").disabled(!is_version)
            };
            menu = menu
                .item(MenuItem::new("asm-update-linked", "Update linked document…").icon("link"))
                .item(match open_linked {
                    None => MenuItem::new("asm-open-linked", "Open linked document").icon("open-external"),
                    Some(why) => later("asm-open-linked", "Open linked document".into(), why).icon("open-external"),
                })
                .item(pin);
        }
        // ER3.3: a same-document instance to a version (or a linked one to another version).
        let version_item = if is_std {
            later("asm-version", "Change to version…".into(), "Standard content has no versions").icon("versions")
        } else {
            MenuItem::new("asm-version", "Change to version…").icon("versions")
        };
        menu = menu
            .item(version_item)
            .item(MenuItem::new("asm-export", "Export…").icon("file-export"))
            .item(MenuItem::new("asm-where-used", "Where used…").icon("find"))
            .item(later("asm-revisions", "Revision history…".into(), RELEASE))
            .item(later("asm-task", "Create task…".into(), COLLAB))
            .separator()
            .item(later("asm-comment", "Add comment".into(), COLLAB).icon("comments"))
            .separator()
            .item(MenuItem::new("asm-zoom-selection", "Zoom to selection"))
            .separator()
            .item(MenuItem::new("asm-drawing", format!("Create Drawing of {drawing_of}…")).icon("file-new"))
            .separator()
            .item(MenuItem::new("asm-folder", "Add selection to folder…"))
            .item(MenuItem::new("asm-empty-sub", "Create empty subassembly"))
            .separator()
            .item(MenuItem::new("asm-delete", "Delete").icon("remove-circle"));
    } else {
        // In the view and on the triad (`ex1-step6.png`, `ex1-step7.png`).
        let part = name.clone();
        menu = menu
            .item(hide)
            .item(MenuItem::new("asm-hide-others", "Hide other instances"))
            .item(MenuItem::new("asm-hide-all", "Hide all instances"))
            .item(MenuItem::new("asm-isolate", "Isolate…"))
            .item(transparent_item)
            .item(MenuItem::new("asm-section", "Section view…").icon("section-view"))
            .item(if inst.suppressed { MenuItem::new("asm-unsuppress", format!("Unsuppress {part}")) } else { MenuItem::new("asm-suppress", format!("Suppress {part}")) })
            .item(fix)
            .item(MenuItem::new("asm-show-mates", "Show mates"))
            .item(MenuItem::new("asm-hide-mates", "Hide mates"))
            .item(MenuItem::new("asm-interference", "Check interference").icon("intersection"))
            .item(later("asm-tessellation", "Use best available tessellation".into(), TESSELLATION))
            .separator()
            .item(MenuItem::new("asm-copy", format!("Copy {part}")).icon("copy"))
            .item(MenuItem::new("asm-drawing", format!("Create Drawing of {drawing_of}…")).icon("file-new"))
            .item(MenuItem::new("asm-drawing-asm", format!("Create Drawing of {asm}…")).icon("file-new"))
            .item(MenuItem::new("asm-export", "Export…").icon("file-export"))
            .separator()
            .item(MenuItem::new("asm-find", "Find in instance list"))
            .item(MenuItem::new("asm-connector", "Add mate connector to instance origin…").icon("mate-connector"))
            .item(MenuItem::new("asm-replace", format!("Replace {part}…")).disabled(!parts_only).icon("transform"));
        menu = std_items(menu).item(in_context);
        if let Some(u) = update_context {
            menu = menu.item(u);
        }
        if let Some(p) = set_primary {
            menu = menu.item(p);
        }
        menu = menu
            .item(switch)
            .item(MenuItem::new("asm-where-used", format!("Where used for {studio}…")).icon("find"))
            .separator()
            .item(MenuItem::new("asm-clear-selection", "Clear selection"))
            .item(later("asm-select-other", "Select other…".into(), "Select other (picking hidden faces) is not part of the assemblies course"))
            .separator()
            .item(later("asm-comment", "Add comment".into(), COLLAB).icon("comments"))
            .separator()
            .item(MenuItem::new("asm-zoom-fit", "Zoom to fit"))
            .item(MenuItem::new("asm-zoom-selection", "Zoom to selection"))
            .item(later("asm-normal-to", "View normal to".into(), "View normal to (a face) is not part of the assemblies course"))
            .separator()
            .item(MenuItem::new("asm-delete", format!("Delete {part}")).icon("remove-circle"));
    }
    // 28 items and 6 separators, plus the handle's own items: opened high enough to fit.
    let extra = match handle {
        TriadHandle::Origin => 1,
        TriadHandle::Arrow(_) | TriadHandle::Ring(_) => 2,
        _ => 0,
    };
    let extra = extra + if is_sub && handle == TriadHandle::None { 3 } else { 0 } + if is_std { 3 } else { 0 } + usize::from(inst.source.is_studio());
    let mut at = fit(world, at, 29 + extra, 6);
    // On the triad, the menu opens beside the handle so it doesn't cover it: to the right past
    // the triad, as `ex1-step6.png` shows it, or to the left of the pointer when there is no room
    // (Final part 4: it opened to the left over the triad).
    if matches!(handle, TriadHandle::Origin | TriadHandle::Arrow(_) | TriadHandle::Plane(_) | TriadHandle::Ring(_)) {
        // The menu is as wide as its longest item ("Replace <instance>…", about 6.6 px a
        // character plus the icon column and padding).
        let longest = (name.chars().count() + 10).max(studio.chars().count() + 26).max(30);
        let width = (longest as f32 * 6.6 + 64.0).max(220.0);
        let mut q = world.query::<&Window>();
        let window_w = q.iter(world).next().map_or(f32::MAX, |w| w.width());
        at.x = if at.x + 90.0 + width < window_w - 4.0 { at.x + 90.0 } else { (at.x - width - 24.0).max(4.0) };
    }
    if matches!(handle, TriadHandle::Origin | TriadHandle::Arrow(_) | TriadHandle::Plane(_) | TriadHandle::Ring(_)) {
        world.resource_mut::<super::triad::Triad>().menu_handle = Some(handle);
    }
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
    commands.entity(anchor).insert((
        AsmMenu { element, instance: Some(instance), targets, at },
        MenuHandle(handle),
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// Where to open a menu of `items` 20 px rows and `separators` so it stays in the window
/// (a long menu opened low moves up, as Onshape's does).
fn fit(world: &mut World, at: Vec2, items: usize, separators: usize) -> Vec2 {
    let height = 20.0 * items as f32 + 9.0 * separators as f32 + 12.0;
    let mut q = world.query::<&Window>();
    let Some(h) = q.iter(world).next().map(|w| w.height()) else { return at };
    Vec2::new(at.x, at.y.min(h - height - 6.0).max(4.0))
}

/// Which triad handle a menu was opened on.
#[derive(Component, Debug, Clone, Copy)]
struct MenuHandle(TriadHandle);

/// Right-click on empty space in an assembly (`ex2-step13.png`).
pub fn open_empty_menu(world: &mut World, at: Vec2) {
    let Some(element) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) else {
        return;
    };
    let asm = world.resource::<ActiveDocument>().active_element().map(|e| e.name.clone()).unwrap_or_default();
    let clip = world.resource::<super::InstanceClipboard>().0.as_ref().map(|c| c.name.clone());
    let paste = match clip {
        Some(n) => MenuItem::new("asm-paste", format!("Paste {n}")),
        None => MenuItem::new("asm-paste", "Paste").disabled(true),
    };
    let menu = Menu::new("assembly-empty-menu")
        .min_width(180.0)
        .item_height(20.0)
        .item(MenuItem::new("asm-show-all", "Show all"))
        .item(MenuItem::new("asm-show-all-instances", "Show all instances"))
        .item(paste)
        .item(MenuItem::new("asm-interference-all", "Check interference").icon("intersection"))
        .item(MenuItem::new("asm-drawing-asm", format!("Create Drawing of {asm}…")).icon("file-new"))
        .item(MenuItem::new("asm-zoom-fit", "Zoom to fit"))
        .item(MenuItem::new("asm-isometric", "Isometric"));
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
    commands.entity(anchor).insert((
        AsmMenu { element, instance: None, targets: Vec::new(), at },
        MenuHandle(TriadHandle::None),
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

fn on_menu_action(ev: On<MenuAction>, q: Query<(&AsmMenu, &MenuHandle), With<ContextMenuAnchor>>, mut commands: Commands) {
    let Ok((menu, handle)) = q.get(ev.entity) else {
        return;
    };
    let menu = menu.clone();
    let handle = handle.0;
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| act(world, &menu, handle, &item));
}

fn all_instances(world: &World, element: ElementId) -> Vec<InstanceId> {
    world
        .resource::<ActiveDocument>()
        .doc
        .element(element)
        .and_then(|e| e.assembly_model())
        .map(|a| a.instances.iter().map(|i| i.id).collect())
        .unwrap_or_default()
}

/// Shows every hidden instance (Show all instances, Shift+Y).
pub fn show_all_instances(world: &mut World, element: ElementId) {
    let hidden: Vec<InstanceId> = world
        .resource::<ActiveDocument>()
        .doc
        .element(element)
        .and_then(|e| e.assembly_model())
        .map(|a| a.instances.iter().filter(|i| i.hidden).map(|i| i.id).collect())
        .unwrap_or_default();
    if !hidden.is_empty() {
        super::run(world, &SetInstancesHidden { element, instances: hidden, hidden: false });
    }
}

fn act(world: &mut World, menu: &AsmMenu, handle: TriadHandle, item: &str) {
    let element = menu.element;
    let targets = menu.targets.clone();
    let parts: Vec<cadrs_core::PartId> = super::view_parts(world.resource::<PartCache>(), &targets);
    match item {
        // P3B.6: the instance's part or subassembly properties.
        "asm-properties" => {
            if let Some(i) = menu.instance {
                crate::properties_dialog::open_for_instance(world, i);
            }
        }
        "asm-hide" | "asm-show" => {
            super::run(world, &SetInstancesHidden { element, instances: targets, hidden: item == "asm-hide" });
        }
        "asm-hide-others" => {
            let others: Vec<InstanceId> = all_instances(world, element).into_iter().filter(|i| !targets.contains(i)).collect();
            if !others.is_empty() {
                super::run(world, &SetInstancesHidden { element, instances: others, hidden: true });
            }
        }
        "asm-hide-all" => {
            let all = all_instances(world, element);
            if !all.is_empty() {
                super::run(world, &SetInstancesHidden { element, instances: all, hidden: true });
            }
        }
        "asm-isolate" => world.resource_mut::<PartCache>().isolate(Some(parts)),
        "asm-transparent" | "asm-opaque" => {
            world.resource_mut::<PartCache>().set_transparent(&parts, item == "asm-transparent");
        }
        "asm-show-mates" | "asm-hide-mates" => {
            let model = world.resource::<ActiveDocument>().doc.element(element).and_then(|e| e.assembly_model().cloned());
            if let Some(model) = model {
                world.resource_mut::<super::mate_display::MateDisplay>().set_for_instances(&model, &targets, item == "asm-show-mates");
            }
        }
        "asm-fix" | "asm-unfix" => {
            super::run(world, &SetInstancesFixed { element, instances: targets, fixed: item == "asm-fix" });
        }
        "asm-suppress" | "asm-unsuppress" => {
            super::run(world, &SetInstancesSuppressed { element, instances: targets, suppressed: item == "asm-suppress" });
        }
        // A17.3, A21.4: the selection (with its mates) into a new Assembly tab.
        "asm-move-to-new" => {
            let top: Vec<InstanceId> = all_instances(world, element).into_iter().filter(|i| targets.contains(i)).collect();
            if !top.is_empty()
                && super::run(world, &MoveToNewSubassembly { element, instances: top, new_element: ElementId::new(), instance: InstanceId::new(), name: None, after: None })
            {
                world.resource_mut::<Selection>().0.clear();
            }
        }
        // A17.3, A21.6: an empty subassembly after the right-clicked instance.
        "asm-empty-sub" => {
            super::run(world, &MoveToNewSubassembly { element, instances: vec![], new_element: ElementId::new(), instance: InstanceId::new(), name: None, after: menu.instance });
        }
        "asm-dissolve" => {
            if let Some(sub) = menu.instance
                && super::run(world, &DissolveSubassembly { element, sub })
            {
                world.resource_mut::<Selection>().0.clear();
            }
        }
        "asm-studio-edit" => {
            if let Some(i) = menu.instance {
                super::studio_edit::open(world, i);
            }
        }
        "asm-follow-none" => {
            if let Some(i) = menu.instance {
                super::run(world, &cadrs_core::assembly::positions::SetFollowPosition { element, instance: i, follow: None });
                super::run(world, &SetSubassemblyFlexible { element, instances: vec![i], flexible: false });
            }
        }
        x if x.starts_with("asm-follow-") => {
            let (Some(i), Some(k)) = (menu.instance, x.trim_start_matches("asm-follow-").parse::<usize>().ok()) else { return };
            let doc = world.resource::<ActiveDocument>();
            let follow = doc
                .doc
                .element(element)
                .and_then(|e| e.assembly_model()?.instance(i))
                .and_then(|inst| doc.doc.element(inst.source.element())?.assembly_model()?.named_positions.get(k).map(|p| p.id));
            if let Some(f) = follow {
                super::run(world, &cadrs_core::assembly::positions::SetFollowPosition { element, instance: i, follow: Some(f) });
            }
        }
        "asm-flexible" | "asm-rigid" => {
            if let Some(i) = menu.instance {
                super::run(world, &SetSubassemblyFlexible { element, instances: vec![i], flexible: item == "asm-flexible" });
            }
        }
        // A18.3: the Folder name popup, beside the first selected row.
        "asm-folder" => {
            let items = targets.iter().map(|i| cadrs_core::assembly::folders::item(*i)).collect();
            super::folders::add_to_folder(world, cadrs_core::assembly::folders::FolderList::Instances, items, Some(menu.at));
        }
        // P3B.9 (X15).
        "asm-interference" => super::interference::check(world, element, targets.clone()),
        "asm-interference-all" => super::interference::check(world, element, Vec::new()),
        "asm-connector" => {
            if let Some(i) = menu.instance {
                super::connector_tool::open_at_instance_origin(world, i);
            }
        }
        "asm-replace" => super::replace_dialog::open(world, targets.clone()),
        x if x.starts_with("asm-update-context-") && menu.instance.is_some_and(|i| cadrs_core::assembly::context::linked_studio_of(&world.resource::<ActiveDocument>().doc, element, i).is_some()) => {
            if let (Some(i), Ok(id)) = (menu.instance, x.trim_start_matches("asm-update-context-").parse()) {
                super::linked_context::update_from_assembly(world, element, i, id);
            }
        }
        x if x.starts_with("asm-update-context-") => {
            if let Some(i) = menu.instance
                && let Some(studio) = cadrs_core::assembly::context::studio_of(&world.resource::<ActiveDocument>().doc, element, i)
                && let Ok(id) = x.trim_start_matches("asm-update-context-").parse()
            {
                super::in_context::update_context(world, studio, id);
            }
        }
        x if x.starts_with("asm-edit-in-context") => {
            let Some(i) = menu.instance else { return };
            let which = match x.trim_start_matches("asm-edit-in-context").trim_start_matches('-') {
                "" | "new" => super::in_context::Which::New,
                n => match n.parse() {
                    Ok(id) => super::in_context::Which::Context(id),
                    Err(_) => return,
                },
            };
            if cadrs_core::assembly::context::linked_studio_of(&world.resource::<ActiveDocument>().doc, element, i).is_some() {
                super::linked_context::edit_linked(world, element, i, which);
            } else {
                super::in_context::edit_in_context(world, element, i, which);
            }
        }
        "asm-set-primary" => {
            if let Some(i) = menu.instance
                && let Some(studio) = cadrs_core::assembly::context::studio_of(&world.resource::<ActiveDocument>().doc, element, i)
            {
                let contexts = super::managed_context::contexts_of(world.resource::<ActiveDocument>(), element, i).into_iter().filter(|c| !c.primary).map(|c| c.id).collect();
                super::run(world, &cadrs_core::assembly::context::SetPrimaryInstance { studio, contexts, instance: i });
            }
        }
        // P3G.2: the Reference manager for the instances acted on, and pinning.
        "asm-update-linked" => {
            let sites = crate::reference_manager::instance_sites(world, element, &targets, true);
            crate::reference_manager::open(world, crate::reference_manager::Scope::Sites(sites), 0);
        }
        "asm-version" => {
            let sites = crate::reference_manager::instance_sites(world, element, &targets, false);
            crate::reference_manager::open_for_versions(world, sites);
        }
        "asm-pin" | "asm-unpin" => {
            let sites = crate::reference_manager::instance_sites(world, element, &targets, true);
            crate::reference_manager::set_pinned(world, sites, item == "asm-pin");
        }
        "asm-open-linked" => {
            if let Some(i) = menu.instance.or(targets.first().copied()) {
                crate::linked_session::open_site(world, cadrs_core::link_update::RefSite::Instance { element, instance: i });
            }
        }
        "asm-where-used" => {
            if let Some(i) = menu.instance {
                super::where_used::open(world, i);
            }
        }
        "asm-export" => crate::export_dialog::open_export_dialog(world, parts.clone()),
        "asm-delete" if super::run(world, &DeleteInstances { element, instances: targets.clone() }) => {
            world.resource_mut::<Selection>().0.retain(|p| super::instance_of(p).is_none_or(|i| !targets.contains(&i)));
        }
        // A19.9: Edit standard content instance, on the targets (bulk edit).
        "asm-std-edit" => super::standard::open_edit_dialog(world, targets.clone()),
        "asm-select-same-config" | "asm-select-same-part" => {
            if let Some(i) = menu.instance {
                let same = world
                    .resource::<ActiveDocument>()
                    .doc
                    .element(element)
                    .and_then(|e| e.assembly_model())
                    .map(|a| cadrs_core::assembly::standard::same_configuration(a, i, item == "asm-select-same-part"))
                    .unwrap_or_default();
                world.resource_mut::<Selection>().0 = same.into_iter().map(|i| Pick::Part(i.part_id())).collect();
            }
        }
        "asm-switch-to" => {
            if let Some(i) = menu.instance {
                switch_to(world, i);
            }
        }
        "asm-find" => {
            if let Some(i) = menu.instance {
                world.resource_mut::<Selection>().0 = vec![Pick::Part(i.part_id())];
            }
        }
        "asm-clear-selection" => world.resource_mut::<Selection>().0.clear(),
        "asm-copy" => {
            // P3G.1 (ER6.7): every instance the menu acts on.
            if !menu.targets.is_empty() {
                super::copy_instances(world, &menu.targets);
            } else if let Some(i) = menu.instance {
                super::copy_instance(world, i);
            }
        }
        "asm-paste" => super::paste_instance(world, Some(menu.at)),
        // D1.2 (P3C.5): Create Drawing of the assembly, or of the instance's part.
        "asm-drawing-asm" => {
            let r = cadrs_drawing::ObjectRef { element: element.0, part: None };
            crate::drawing::create_dialog::open_create_drawing(world, Some(r));
        }
        "asm-drawing" => {
            let source = menu
                .instance
                .and_then(|i| world.resource::<ActiveDocument>().doc.element(element)?.assembly_model()?.instance(i).map(|x| x.source));
            let r = match source {
                Some(cadrs_core::assembly::InstanceSource::Part { element: e, part }) => {
                    Some(cadrs_drawing::ObjectRef { element: e.0, part: Some((part.feature.0, part.index)) })
                }
                Some(cadrs_core::assembly::InstanceSource::Assembly { element: e })
                | Some(cadrs_core::assembly::InstanceSource::Studio { element: e }) => Some(cadrs_drawing::ObjectRef { element: e.0, part: None }),
                None => None,
            };
            if r.is_some() {
                crate::drawing::create_dialog::open_create_drawing(world, r);
            }
        }
        "asm-zoom-fit" => crate::viewport::zoom_to_fit(world),
        "asm-zoom-selection" => zoom_to(world, &parts),
        // P3E.3a (A3.3, X15): a section through the instance.
        "asm-section" => crate::section_view::open_for_instance(world, &parts),
        "asm-show-all" => {
            show_all_instances(world, element);
            let mut cache = world.resource_mut::<PartCache>();
            cache.isolate(None);
            let all: Vec<_> = cache.transparent.iter().copied().collect();
            cache.set_transparent(&all, false);
        }
        "asm-show-all-instances" => show_all_instances(world, element),
        "asm-isometric" => {
            let pts = world.get_resource::<ActiveDocument>().map(super::shown_points).unwrap_or_default();
            let size = world.resource::<crate::viewport::ViewportRect>().0.size();
            let mut pts = pts;
            pts.push(Vec3::ZERO);
            let mut view = world.resource_mut::<crate::viewport::ViewportView>();
            let to = view
                .target()
                .oriented(crate::camera::StandardView::Isometric)
                .fitted(&pts, size, crate::viewport::ASM_FIT_FILL);
            view.animate_to(to);
        }
        "asm-move-to-origin" | "asm-align-z" | "asm-anti-align-z" | "asm-rotate-90" | "asm-rotate-180" => {
            super::triad::triad_action(world, handle, item);
        }
        _ => {}
    }
}

/// Zooms to the parts `parts` (Zoom to selection).
fn zoom_to(world: &mut World, parts: &[cadrs_core::PartId]) {
    let pts: Vec<Vec3> = {
        let cache = world.resource::<PartCache>();
        parts
            .iter()
            .filter_map(|p| cache.part(*p))
            .flat_map(|p| p.solid.positions.iter().map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)))
            .collect()
    };
    if pts.is_empty() {
        return;
    }
    let size = world.resource::<crate::viewport::ViewportRect>().0.size();
    let mut view = world.resource_mut::<crate::viewport::ViewportView>();
    let to = view.target().fitted(&pts, size, crate::viewport::ASM_FIT_FILL);
    view.animate_to(to);
}

/// **Switch to** from a BOM row (TD9.4, P3E.5): the row's Part Studio, its part selected, or
/// its subassembly's tab.
pub fn switch_to_owner(world: &mut World, owner: cadrs_core::properties::PropertyOwner) {
    use cadrs_core::properties::PropertyOwner;
    let (element, picks) = match owner {
        PropertyOwner::Part { element, part } => (element, vec![Pick::Part(part)]),
        PropertyOwner::Assembly { element } => (element, Vec::new()),
        PropertyOwner::Item { .. } => return,
    };
    if world.resource::<ActiveDocument>().doc.element(element).is_none() {
        return;
    }
    world.resource_mut::<ActiveDocument>().set_active(element);
    world.insert_resource(super::PendingSelection(Some((element, picks))));
}

/// **Switch to** (A4.6, X11): the instance's Part Studio becomes the active tab with its part
/// selected (highlighted); a subassembly's tab opens.
pub fn switch_to(world: &mut World, instance: InstanceId) {
    let Some(source) = world
        .resource::<ActiveDocument>()
        .active_element()
        .and_then(|e| e.assembly_model()?.instance(instance))
        .map(|i| i.source)
    else {
        return;
    };
    let element = source.element();
    if world.resource::<ActiveDocument>().doc.element(element).is_none() {
        return;
    }
    world.resource_mut::<ActiveDocument>().set_active(element);
    // A17 / A4.6: a subassembly opens its tab; a part is selected in its studio.
    let picks = source.part().map(|p| vec![Pick::Part(p)]).unwrap_or_default();
    world.insert_resource(super::PendingSelection(Some((element, picks))));
}
