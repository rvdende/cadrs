//! Hole-callout symbols in any text (P3.6, PS15.10): `⌴` (counterbore), `⌵` (countersink) and
//! `↧` (depth) are not in Inter, so a text holding them is split into spans, the symbols set in
//! cadrs's own "cadrs Symbols" face (`assets/fonts/cadrs-symbols.ttf`) at the text's size and
//! colour. The owner keeps writing the whole string to [`Text`]; this keeps the spans in step.

use bevy::prelude::*;
use bevy::text::FontSource;

/// The family of the symbols face.
pub const SYMBOL_FAMILY: &str = "cadrs Symbols";

/// The characters drawn from the symbols face.
pub fn is_symbol(c: char) -> bool {
    matches!(c, '⌴' | '⌵' | '↧')
}

pub struct SymbolsPlugin;

impl Plugin for SymbolsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, (split_symbol_text, follow_root_style).chain().before(bevy::ui::UiSystems::Prepare));
    }
}

/// On a text split into spans: the whole string it was given.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct SymbolSource(pub String);

/// A span made by the split.
#[derive(Component, Debug, Clone, Copy)]
pub struct SymbolSpan;

/// The string cut into runs of plain text and of symbols (`true`).
pub fn runs(s: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for c in s.chars() {
        let sym = is_symbol(c);
        match out.last_mut() {
            Some((run, k)) if *k == sym => run.push(c),
            _ => out.push((c.to_string(), sym)),
        }
    }
    out
}

#[allow(clippy::type_complexity)]
fn split_symbol_text(
    mut q: Query<(Entity, &mut Text, &TextFont, &TextColor, Option<&SymbolSource>, Option<&Children>), (Changed<Text>, Without<TextSpan>)>,
    q_spans: Query<(), With<SymbolSpan>>,
    mut commands: Commands,
) {
    for (e, mut text, font, color, source, children) in &mut q {
        // Our own write of the first run.
        if let Some(src) = source
            && runs(&src.0).first().is_some_and(|(r, sym)| !sym && *r == text.0)
        {
            continue;
        }
        let has = text.0.chars().any(is_symbol);
        if source.is_none() && !has {
            continue;
        }
        // Drop the old spans.
        for c in children.into_iter().flatten() {
            if q_spans.contains(*c) {
                commands.entity(*c).despawn();
            }
        }
        if !has {
            commands.entity(e).remove::<SymbolSource>();
            continue;
        }
        let full = text.0.clone();
        let mut parts = runs(&full);
        // The root keeps the first plain run (empty if the text starts with a symbol).
        let first = if parts.first().is_some_and(|(_, sym)| !sym) { parts.remove(0).0 } else { String::new() };
        text.0 = first;
        commands.entity(e).insert(SymbolSource(full));
        for (run, sym) in parts {
            let mut f = font.clone();
            if sym {
                f.font = FontSource::Family(SYMBOL_FAMILY.into());
            }
            let span = commands.spawn((TextSpan(run), f, *color, SymbolSpan)).id();
            commands.entity(e).add_child(span);
        }
    }
}

/// The spans follow their text's colour and size (hover, selection, errors).
#[allow(clippy::type_complexity)]
fn follow_root_style(
    q: Query<(&TextColor, &TextFont, &Children), (With<SymbolSource>, Or<(Changed<TextColor>, Changed<TextFont>)>)>,
    mut q_spans: Query<(&mut TextColor, &mut TextFont), (With<SymbolSpan>, Without<SymbolSource>)>,
) {
    for (color, font, children) in &q {
        for c in children {
            if let Ok((mut sc, mut sf)) = q_spans.get_mut(*c) {
                sc.set_if_neq(*color);
                if sf.font_size != font.font_size || sf.weight != font.weight {
                    sf.font_size = font.font_size;
                    sf.weight = font.weight;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callouts_split_into_runs() {
        assert_eq!(
            runs("Ø 5.3 mm THRU | ⌴Ø 9.75 mm ↧ 5 mm"),
            vec![
                ("Ø 5.3 mm THRU | ".to_string(), false),
                ("⌴".to_string(), true),
                ("Ø 9.75 mm ".to_string(), false),
                ("↧".to_string(), true),
                (" 5 mm".to_string(), false),
            ]
        );
        assert!(runs("Extrude 1").iter().all(|(_, s)| !s));
    }
}
