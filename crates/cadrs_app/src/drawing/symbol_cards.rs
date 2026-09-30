//! The cards of the GD&T, datum, surface finish and weld tools (P3C.8, X14): while one of those
//! tools is active its card sits at the top right of the sheet with the symbol's settings, read
//! live into [`SymbolSpecs`]; a pick on an edge and a click place the symbol with them.
//!
//! - **Feature control frame**: the characteristic (the 14 ASME symbols), the tolerance, Ø, the
//!   material-condition modifier and datums 1–3.
//! - **Datum feature**: the letter (the next unused one by default).
//! - **Surface finish**: basic, material removal required or prohibited, and the value.
//! - **Weld**: the arrow-side and other-side symbols (fillet, V-groove, square), the size and
//!   all around.

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_drawing::annotation_more::{FinishKind, Gdt, Modifier, WeldKind};
use cadrs_ui::prelude::*;
use cadrs_ui::{CheckboxState, Select, SelectState};

use super::annotations::{AnnTool, AnnotationUi};
use super::note_bar::{card_bundle, header};
use super::{DrawingUi, sheet_area};
use crate::viewport::ViewportRect;
use crate::AppState;

pub struct SymbolCardsPlugin;

impl Plugin for SymbolCardsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SymbolSpecs>()
            .add_systems(Update, read_cards.run_if(in_state(AppState::Document)));
    }
}

/// The symbol tools' settings.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct SymbolSpecs {
    pub gdt: Gdt,
    pub tolerance: String,
    pub diameter: bool,
    pub modifier: Option<Modifier>,
    pub datums: [String; 3],
    pub datum_letter: String,
    pub finish: FinishKind,
    pub finish_value: String,
    pub weld_arrow: WeldKind,
    pub weld_other: WeldKind,
    pub weld_size: String,
    pub all_around: bool,
}

impl Default for SymbolSpecs {
    fn default() -> Self {
        Self {
            gdt: Gdt::Position,
            tolerance: "0.05".into(),
            diameter: true,
            modifier: Some(Modifier::Mmc),
            datums: ["A".into(), "B".into(), String::new()],
            datum_letter: "A".into(),
            finish: FinishKind::RemovalRequired,
            finish_value: "Ra 1.6".into(),
            weld_arrow: WeldKind::Fillet,
            weld_other: WeldKind::None,
            weld_size: "5".into(),
            all_around: false,
        }
    }
}

/// A symbol tool's card.
#[derive(Component, Clone, Copy, PartialEq)]
struct SymbolCard(AnnTool);

const MODIFIERS: [(Option<Modifier>, &str); 4] =
    [(None, "None"), (Some(Modifier::Mmc), "Maximum material (M)"), (Some(Modifier::Lmc), "Least material (L)"), (Some(Modifier::Rfs), "Regardless of size (S)")];

fn row(p: &mut ChildSpawnerCommands, t: &Theme, label: &str, child: impl Bundle) {
    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
        r.spawn((t.text(label, t.font_sm, FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(74.0), ..default() }));
        r.spawn(child);
    });
}

fn close_cards(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<SymbolCard>>();
    let all: Vec<Entity> = q.iter(world).collect();
    for e in all {
        world.entity_mut(e).despawn();
    }
}

fn cancel(world: &mut World) {
    close_cards(world);
    let mut ui = world.resource_mut::<AnnotationUi>();
    ui.tool = AnnTool::None;
    ui.reset_picks();
}

/// Opens the card of `tool` (after the tool started).
pub fn open(world: &mut World, tool: AnnTool) {
    close_cards(world);
    if world.resource::<AnnotationUi>().tool != tool {
        return;
    }
    // A datum symbol takes the next letter no datum of the sheet uses yet.
    if tool == AnnTool::Datum
        && let Some(letter) = next_datum(world)
    {
        world.resource_mut::<SymbolSpecs>().datum_letter = letter;
    }
    let spec = world.resource::<SymbolSpecs>().clone();
    let area = {
        let ui = world.resource::<DrawingUi>();
        sheet_area(world.resource::<ViewportRect>(), ui)
    };
    let t = world.resource::<Theme>().clone();
    let at = Vec2::new(area.max.x - 270.0, area.min.y + 14.0);
    let (title, prefix) = match tool {
        AnnTool::Gdt => ("Feature control frame", "gdt-card"),
        AnnTool::Datum => ("Datum feature", "datum-card"),
        AnnTool::SurfaceFinish => ("Surface finish", "finish-card"),
        AnnTool::Weld => ("Weld symbol", "weld-card"),
        _ => return,
    };
    let mut commands = world.commands();
    commands
        .spawn((card_bundle(&t, prefix, at), SymbolCard(tool)))
        .insert(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x),
            top: Val::Px(at.y),
            width: Val::Px(256.0),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        })
        .with_children(|c| {
            header(c, &t, title, prefix, None, cancel);
            c.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(8.0)), row_gap: Val::Px(6.0), ..default() })
                .with_children(|b| match tool {
                    AnnTool::Gdt => {
                        let mut s = Select::new("gdt-symbol").width(Val::Px(150.0));
                        for g in Gdt::ALL {
                            s = s.option(g.label(), true);
                        }
                        let i = Gdt::ALL.iter().position(|g| *g == spec.gdt).unwrap_or(0);
                        row(b, &t, "Symbol", s.selected(i).build(&t));
                        row(b, &t, "Tolerance", TextInput::new("gdt-tolerance").value(spec.tolerance.clone()).width(Val::Px(90.0)).select_all_on_focus().build(&t));
                        b.spawn(Checkbox::new("gdt-diameter").label("Diameter (Ø)").checked(spec.diameter).build(&t));
                        let mut m = Select::new("gdt-modifier").width(Val::Px(150.0));
                        for (_, l) in MODIFIERS {
                            m = m.option(l, true);
                        }
                        let mi = MODIFIERS.iter().position(|(x, _)| *x == spec.modifier).unwrap_or(0);
                        row(b, &t, "Modifier", m.selected(mi).build(&t));
                        for (k, d) in spec.datums.iter().enumerate() {
                            row(
                                b,
                                &t,
                                &format!("Datum {}", k + 1),
                                TextInput::new(format!("gdt-datum-{}", k + 1)).value(d.clone()).width(Val::Px(50.0)).select_all_on_focus().build(&t),
                            );
                        }
                    }
                    AnnTool::Datum => {
                        row(b, &t, "Letter", TextInput::new("datum-letter").value(spec.datum_letter.clone()).width(Val::Px(50.0)).select_all_on_focus().build(&t));
                    }
                    AnnTool::SurfaceFinish => {
                        let mut s = Select::new("finish-kind").width(Val::Px(160.0));
                        for k in FinishKind::ALL {
                            s = s.option(k.label(), true);
                        }
                        let i = FinishKind::ALL.iter().position(|k| *k == spec.finish).unwrap_or(0);
                        row(b, &t, "Type", s.selected(i).build(&t));
                        row(b, &t, "Value", TextInput::new("finish-value").value(spec.finish_value.clone()).width(Val::Px(90.0)).select_all_on_focus().build(&t));
                    }
                    AnnTool::Weld => {
                        for (name, label, v) in [("weld-arrow-side", "Arrow side", spec.weld_arrow), ("weld-other-side", "Other side", spec.weld_other)] {
                            let mut s = Select::new(name).width(Val::Px(150.0));
                            for k in WeldKind::ALL {
                                s = s.option(k.label(), true);
                            }
                            let i = WeldKind::ALL.iter().position(|k| *k == v).unwrap_or(0);
                            row(b, &t, label, s.selected(i).build(&t));
                        }
                        row(b, &t, "Size", TextInput::new("weld-size").value(spec.weld_size.clone()).width(Val::Px(60.0)).select_all_on_focus().build(&t));
                        b.spawn(Checkbox::new("weld-all-around").label("All around").checked(spec.all_around).build(&t));
                    }
                    _ => {}
                });
            c.spawn((
                t.text("Pick an edge, then click to place", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                Node { margin: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(0.0), Val::Px(8.0)), ..default() },
            ));
        });
    world.flush();
}

/// The first datum letter no datum symbol of the active sheet uses.
fn next_datum(world: &World) -> Option<String> {
    let doc = world.get_resource::<crate::ActiveDocument>()?;
    let (id, d) = super::active_drawing(doc)?;
    let sheet = d.sheets.get(world.resource::<DrawingUi>().sheet_index(id, d))?;
    let used: Vec<&str> = sheet
        .views
        .iter()
        .flat_map(|v| v.annotations.iter())
        .filter_map(|a| match &a.kind {
            cadrs_drawing::annotation::AnnotationKind::Datum(x) => Some(x.letter.as_str()),
            _ => None,
        })
        .collect();
    (0..).map(cadrs_drawing::view_kinds::letter).find(|l| !used.contains(&l.as_str()))
}

/// Reads the open card into [`SymbolSpecs`]; closes it when its tool ends.
fn read_cards(
    ann: Res<AnnotationUi>,
    q_card: Query<(Entity, &SymbolCard)>,
    q_text: Query<(&Name, &bevy::text::EditableText)>,
    q_select: Query<(&Name, &SelectState)>,
    q_check: Query<(&Name, &CheckboxState)>,
    mut specs: ResMut<SymbolSpecs>,
    mut commands: Commands,
) {
    let Some((e, card)) = q_card.iter().next() else {
        return;
    };
    if ann.tool != card.0 {
        commands.entity(e).despawn();
        return;
    }
    let mut s = specs.clone();
    for (n, t) in &q_text {
        let v = t.value().to_string();
        match n.as_str() {
            "gdt-tolerance-field" => s.tolerance = v,
            "gdt-datum-1-field" => s.datums[0] = v.trim().to_string(),
            "gdt-datum-2-field" => s.datums[1] = v.trim().to_string(),
            "gdt-datum-3-field" => s.datums[2] = v.trim().to_string(),
            "datum-letter-field" => s.datum_letter = v.trim().to_uppercase(),
            "finish-value-field" => s.finish_value = v,
            "weld-size-field" => s.weld_size = v.trim().to_string(),
            _ => {}
        }
    }
    for (n, st) in &q_select {
        match n.as_str() {
            "gdt-symbol" => s.gdt = Gdt::ALL.get(st.selected).copied().unwrap_or(s.gdt),
            "gdt-modifier" => s.modifier = MODIFIERS.get(st.selected).map(|m| m.0).unwrap_or(s.modifier),
            "finish-kind" => s.finish = FinishKind::ALL.get(st.selected).copied().unwrap_or(s.finish),
            "weld-arrow-side" => s.weld_arrow = WeldKind::ALL.get(st.selected).copied().unwrap_or(s.weld_arrow),
            "weld-other-side" => s.weld_other = WeldKind::ALL.get(st.selected).copied().unwrap_or(s.weld_other),
            _ => {}
        }
    }
    for (n, c) in &q_check {
        match n.as_str() {
            "gdt-diameter" => s.diameter = c.checked,
            "weld-all-around" => s.all_around = c.checked,
            _ => {}
        }
    }
    if *specs != s {
        *specs = s;
    }
}
