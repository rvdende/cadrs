//! The **version graph** picker (P3G.1, ER1.7, ER3.2, ER X3; `external-references/
//! lesson-referencing-versions-within-a-document.png`): a legend (**Workspace** as an open
//! circle, **Version** as a filled marker), then one branch drawn top to bottom: **Main** (the
//! workspace) at the top, the versions newest first, **Start** at the bottom. A click on a
//! selectable node triggers [`VersionGraphSelect`]; the selected node is pale blue.
//!
//! Built on [`TimelineRow`] (the History panel's rail). Names: the graph `<name>`, its legend
//! `<name>-legend`, node `k` (top first) `<name>-node-<k>`. Release candidates and releases are
//! out of scope (release management), so the legend shows only Workspace and Version.
//!
//! Modeled on gpui-component's `List` with a leading decoration column.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;

use crate::theme::Theme;
use crate::timeline::{TimelineMarker, TimelineRow};
use crate::tooltip::Tooltip;

pub struct VersionGraphPlugin;

impl Plugin for VersionGraphPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_node_activate);
    }
}

/// What a node stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionNodeKind {
    /// The workspace ("Main").
    Workspace,
    Version,
    /// A version Update all references made (P3G.2, ER4.7): an open square, and "Auto
    /// version" in the legend.
    AutoVersion,
    /// The document's first state.
    Start,
}

/// One node of the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionNode {
    pub title: String,
    pub subtitle: Option<String>,
    pub kind: VersionNodeKind,
    /// It can be picked (a version; the workspace only for same-document references).
    pub selectable: bool,
    /// Why it can't be picked (its tooltip).
    pub reason: Option<String>,
    pub selected: bool,
}

impl VersionNode {
    pub fn new(title: impl Into<String>, kind: VersionNodeKind) -> Self {
        Self { title: title.into(), subtitle: None, kind, selectable: matches!(kind, VersionNodeKind::Version | VersionNodeKind::AutoVersion), reason: None, selected: false }
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.subtitle = Some(s.into());
        self
    }

    pub fn selectable(mut self, s: bool) -> Self {
        self.selectable = s;
        self
    }

    pub fn reason(mut self, r: impl Into<String>) -> Self {
        self.reason = Some(r.into());
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }
}

/// A node row: its graph's name and its index (top first).
#[derive(Component, Debug, Clone)]
pub struct VersionGraphNode {
    pub graph: Cow<'static, str>,
    pub index: usize,
    pub selectable: bool,
}

/// A selectable node was clicked. Targets the node's row.
#[derive(EntityEvent, Debug, Clone)]
pub struct VersionGraphSelect {
    pub entity: Entity,
    /// The graph's name.
    pub graph: Cow<'static, str>,
    pub index: usize,
}

/// Builder for the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionGraph {
    name: Cow<'static, str>,
    nodes: Vec<VersionNode>,
}

impl VersionGraph {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self { name: name.into(), nodes: Vec::new() }
    }

    /// Adds a node below the others.
    pub fn node(mut self, n: VersionNode) -> Self {
        self.nodes.push(n);
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let VersionGraph { name, nodes } = self;
        let root_name = name.to_string();
        (
            Name::new(root_name),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::vertical(Val::Px(4.0)), ..default() },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let auto = nodes.iter().any(|n| n.kind == VersionNodeKind::AutoVersion);
                legend(p, &t, &name, auto);
                let last = nodes.len().saturating_sub(1);
                for (k, n) in nodes.into_iter().enumerate() {
                    let marker = match n.kind {
                        VersionNodeKind::Workspace => TimelineMarker::Workspace,
                        VersionNodeKind::Version => TimelineMarker::Version,
                        VersionNodeKind::AutoVersion => TimelineMarker::AutoVersion,
                        VersionNodeKind::Start => TimelineMarker::Start,
                    };
                    let mut row = TimelineRow::new(format!("{name}-node-{k}"), n.title.clone())
                        .marker(marker)
                        .first(k == 0)
                        .last(k == last)
                        .selected(n.selected)
                        .muted(!n.selectable);
                    if let Some(s) = &n.subtitle {
                        row = row.subtitle(s.clone());
                    }
                    let mut e = p.spawn((row.build(&t), VersionGraphNode { graph: name.clone(), index: k, selectable: n.selectable }));
                    if let Some(r) = n.reason {
                        e.insert(Tooltip::new(r));
                    }
                }
            })),
        )
    }
}

/// "○ Workspace  ◆ Version", and "□ Auto version" when the graph has one.
fn legend(p: &mut ChildSpawner, t: &Theme, name: &str, auto: bool) {
    let line = Color::srgb_u8(0x2f, 0x5f, 0xb3);
    p.spawn((
        Name::new(format!("{name}-legend")),
        Node { padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(2.0), Val::Px(6.0)), column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() },
        Pickable::IGNORE,
    ))
    .with_children(|r| {
        let mut items = vec![("Workspace", true, false), ("Version", false, false)];
        if auto {
            items.push(("Auto version", false, true));
        }
        for (label, circle, open) in items {
            r.spawn((Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }, Pickable::IGNORE)).with_children(|c| {
                let size = if circle || open { 10.0 } else { 8.0 };
                c.spawn((
                    Node {
                        width: Val::Px(size),
                        height: Val::Px(size),
                        border: UiRect::all(Val::Px(if circle || open { 2.0 } else { 0.0 })),
                        border_radius: if circle { BorderRadius::MAX } else { BorderRadius::all(Val::Px(1.5)) },
                        ..default()
                    },
                    BackgroundColor(if circle || open { Color::WHITE } else { line }),
                    BorderColor::all(line),
                    Pickable::IGNORE,
                ));
                c.spawn((t.text(label, 11.0, FontWeight::NORMAL, t.foreground), Pickable::IGNORE));
            });
        }
    });
}

fn on_node_activate(a: On<Activate>, q: Query<&VersionGraphNode>, mut commands: Commands) {
    let Ok(n) = q.get(a.entity) else { return };
    if !n.selectable {
        return;
    }
    commands.trigger(VersionGraphSelect { entity: a.entity, graph: n.graph.clone(), index: n.index });
}
