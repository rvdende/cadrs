//! Named views (P3E.3a, TD6.5, PS2.9): a camera saved under a name in a Part Studio or an
//! Assembly tab, from the view cube menu's **Named views…**, and restored from the same panel.
//!
//! A view is stored with its tab ([`crate::Element::named_views`]), so it is saved with the
//! document, copied with the tab and undone like any other edit ([`AddNamedView`],
//! [`DeleteNamedView`]). Documents from before it load with no named views (the field is
//! additive).
//!
//! The camera is the viewport's: an azimuth and elevation (degrees, the direction from the
//! focus toward the eye), a roll, the focus point (mm) and a zoom (mm per logical pixel), and
//! whether it is a perspective view, and the render mode it was saved in (the app's slug for it,
//! `shaded`, `hidden-removed`…; views saved before it have none and keep the tab's mode).

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;

/// A saved camera.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedView {
    pub name: String,
    pub azimuth: f32,
    pub elevation: f32,
    #[serde(default)]
    pub roll: f32,
    pub focus: [f32; 3],
    /// mm per logical pixel.
    pub scale: f32,
    #[serde(default)]
    pub perspective: bool,
    /// The render mode's slug, if saved with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<String>,
}

/// Saves the camera `view` under its name in the tab `element`, replacing a view of the same
/// name (names compare without case or surrounding spaces).
#[derive(Debug, Clone)]
pub struct AddNamedView {
    pub element: ElementId,
    pub view: NamedView,
}

impl Command for AddNamedView {
    fn label(&self) -> String {
        format!("Save named view {}", self.view.name.trim())
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = self.view.name.trim();
        if name.is_empty() {
            return Err(CommandError::Invalid("A named view needs a name".into()));
        }
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        let view = NamedView { name: name.to_string(), ..self.view.clone() };
        match el.named_views.iter_mut().find(|v| same_name(&v.name, name)) {
            Some(v) => *v = view,
            None => el.named_views.push(view),
        }
        Ok(())
    }
}

/// Deletes the named view `name` of the tab `element`.
#[derive(Debug, Clone)]
pub struct DeleteNamedView {
    pub element: ElementId,
    pub name: String,
}

impl Command for DeleteNamedView {
    fn label(&self) -> String {
        format!("Delete named view {}", self.name.trim())
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        let before = el.named_views.len();
        el.named_views.retain(|v| !same_name(&v.name, &self.name));
        if el.named_views.len() == before {
            return Err(CommandError::Invalid(format!("No named view {}", self.name.trim())));
        }
        Ok(())
    }
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The next default name: "View n" for the n-th view of the tab ("View 2" once one is saved,
/// whatever it was called), the next free number if that one is taken.
pub fn next_name(views: &[NamedView]) -> String {
    (views.len() + 1..).map(|i| format!("View {i}")).find(|n| !views.iter().any(|v| same_name(&v.name, n))).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::History;
    use crate::document::Element;

    fn view(name: &str, az: f32) -> NamedView {
        NamedView { name: name.into(), azimuth: az, elevation: 30.0, roll: 0.0, focus: [1.0, 2.0, 3.0], scale: 0.25, perspective: true, render: Some("translucent".into()) }
    }

    /// Saving, replacing and deleting named views go through the undo layer, and a document
    /// with named views survives a save and reload; a document saved before them loads with
    /// none.
    #[test]
    fn the_default_name_counts_on() {
        assert_eq!(next_name(&[]), "View 1");
        // After a save under another name, the field offers View 2.
        assert_eq!(next_name(&[view("Upright hole", 0.0)]), "View 2");
        // A taken number is skipped.
        assert_eq!(next_name(&[view("View 2", 0.0)]), "View 3");
    }

    #[test]
    fn named_views_are_undone_and_persist() {
        let mut doc = Document::new("Views");
        let el = doc.elements[0].id;
        let mut h = History::default();
        h.execute(&mut doc, &AddNamedView { element: el, view: view(" Front detail ", 10.0) }).unwrap();
        h.execute(&mut doc, &AddNamedView { element: el, view: view("Back", 190.0) }).unwrap();
        // Two views saved: the third is offered.
        assert_eq!(next_name(&doc.elements[0].named_views), "View 3");
        // Same name: replaced, not added.
        h.execute(&mut doc, &AddNamedView { element: el, view: view("front DETAIL", 20.0) }).unwrap();
        let names: Vec<&str> = doc.elements[0].named_views.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["front DETAIL", "Back"]);
        assert_eq!(doc.elements[0].named_views[0].azimuth, 20.0);
        assert!(h.execute(&mut doc, &AddNamedView { element: el, view: view("  ", 0.0) }).is_err());

        // Survives a save and reload (RON, as `document.ron`).
        let text = ron::ser::to_string(&doc).unwrap();
        let back: Document = ron::from_str(&text).unwrap();
        assert_eq!(back.elements[0].named_views, doc.elements[0].named_views);

        h.execute(&mut doc, &DeleteNamedView { element: el, name: "back".into() }).unwrap();
        assert_eq!(doc.elements[0].named_views.len(), 1);
        h.undo(&mut doc).unwrap();
        assert_eq!(doc.elements[0].named_views.len(), 2);
        h.undo(&mut doc).unwrap();
        assert_eq!(doc.elements[0].named_views[0].azimuth, 10.0);
        h.undo(&mut doc).unwrap();
        h.undo(&mut doc).unwrap();
        assert!(doc.elements[0].named_views.is_empty());

        // An element with no named views doesn't write the field, and one from before reads.
        let plain = Element::part_studio("Part Studio 1");
        let text = ron::ser::to_string(&plain).unwrap();
        assert!(!text.contains("named_views"), "{text}");
        let old: Element = ron::from_str(&text).unwrap();
        assert!(old.named_views.is_empty());
    }
}
