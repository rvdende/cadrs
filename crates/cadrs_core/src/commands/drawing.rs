//! Drawing commands (P3C.1): creating a drawing tab (or pasting a copied tab) and every edit of
//! a drawing, so all of them are undoable.

use cadrs_drawing::DrawingOp;

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind};
use crate::ids::ElementId;

/// Inserts a ready-made element (a new drawing from the Create Drawing dialog, or a tab pasted
/// from the clipboard) right of `after`, or at the end.
#[derive(Debug, Clone)]
pub struct InsertElement {
    pub element: Element,
    pub after: Option<ElementId>,
    /// The undo label ("Create Drawing", "Paste tab").
    pub label: String,
}

impl Command for InsertElement {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if doc.element(self.element.id).is_some() {
            return Err(CommandError::Invalid("element id already in use".into()));
        }
        if self.element.name.trim().is_empty() {
            return Err(CommandError::Invalid("name must not be empty".into()));
        }
        super::insert_after(doc, self.after, self.element.clone());
        Ok(())
    }
}

/// Applies a [`DrawingOp`] to a Drawing tab.
#[derive(Debug, Clone)]
pub struct EditDrawing {
    pub element: ElementId,
    pub op: DrawingOp,
}

impl Command for EditDrawing {
    fn label(&self) -> String {
        self.op.label()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let ElementKind::Drawing(d) = &mut el.kind else {
            return Err(CommandError::Invalid("not a drawing".into()));
        };
        d.apply(&self.op).map_err(CommandError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::History;
    use cadrs_drawing::{Drawing, SheetId, template};

    fn doc_with_drawing() -> (Document, ElementId) {
        let mut doc = Document::new("Doc");
        let t = template::builtin("ANSI_A_INCH.dwt").unwrap();
        let el = Element::drawing("Drawing 1", Drawing::from_template(&t, None));
        let id = el.id;
        let mut h = History::default();
        let first = doc.elements[0].id;
        h.execute(
            &mut doc,
            &InsertElement {
                element: el,
                after: Some(first),
                label: "Create Drawing".into(),
            },
        )
        .unwrap();
        (doc, id)
    }

    #[test]
    fn create_drawing_is_undoable() {
        let mut doc = Document::new("Doc");
        let mut h = History::default();
        let t = template::builtin("ISO_A3_MM.dwt").unwrap();
        let el = Element::drawing("Drawing 1", Drawing::from_template(&t, None));
        let id = el.id;
        let first = doc.elements[0].id;
        h.execute(
            &mut doc,
            &InsertElement {
                element: el,
                after: Some(first),
                label: "Create Drawing".into(),
            },
        )
        .unwrap();
        assert_eq!(doc.elements[1].id, id, "right of the active tab");
        assert!(doc.elements[1].drawing_data().is_some());
        h.undo(&mut doc);
        assert!(doc.element(id).is_none());
        h.redo(&mut doc);
        assert!(doc.element(id).is_some());
    }

    #[test]
    fn drawing_edits_are_undoable() {
        let (mut doc, id) = doc_with_drawing();
        let mut h = History::default();
        let s = SheetId::new();
        h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::InsertSheet { id: s, after: None } })
            .unwrap();
        assert_eq!(h.undo_label(), Some("Insert sheet"));
        assert_eq!(doc.element(id).unwrap().drawing_data().unwrap().sheets.len(), 2);
        h.undo(&mut doc);
        assert_eq!(doc.element(id).unwrap().drawing_data().unwrap().sheets.len(), 1);
        // A refused edit changes nothing and records nothing.
        let only = doc.element(id).unwrap().drawing_data().unwrap().sheets[0].id;
        assert!(
            h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::DeleteSheet { id: only } })
                .is_err()
        );
        assert!(!h.can_undo());
        // Not a drawing.
        let ps = doc.elements[0].id;
        assert!(
            h.execute(&mut doc, &EditDrawing { element: ps, op: DrawingOp::SetLocked(true) })
                .is_err()
        );
    }

    #[test]
    fn annotation_edits_are_undoable() {
        use cadrs_drawing::annotation::{Annotation, AnnotationKind, EdgeRef, Shape};
        use cadrs_drawing::{NamedView, ObjectRef, Scale, View};
        let (mut doc, id) = doc_with_drawing();
        let mut h = History::default();
        let sheet = doc.element(id).unwrap().drawing_data().unwrap().sheets[0].id;
        let r = ObjectRef { element: doc.elements[0].id.0, part: None };
        let v = View::base(r, NamedView::Front, Scale::new(1, 1), [100.0, 100.0]);
        let vid = v.id;
        h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::InsertView { sheet, view: v } }).unwrap();
        let edge = EdgeRef { edge: None, face: None, shape: Shape::Circle { center: [0.0, 0.0], radius: 5.0, arc: None } };
        let a = Annotation::new(AnnotationKind::Centermark(edge));
        let annotations = |doc: &Document| doc.element(id).unwrap().drawing_data().unwrap().view(vid).unwrap().1.annotations.clone();
        h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::AddAnnotation { view: vid, annotation: a.clone() } })
            .unwrap();
        assert_eq!(h.undo_label(), Some("Insert centermark"));
        assert_eq!(annotations(&doc), vec![a.clone()]);
        h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::DeleteAnnotations { ids: vec![(vid, a.id)] } })
            .unwrap();
        assert!(annotations(&doc).is_empty());
        h.undo(&mut doc);
        assert_eq!(annotations(&doc), vec![a.clone()]);
        h.undo(&mut doc);
        assert!(annotations(&doc).is_empty());
        h.redo(&mut doc);
        assert_eq!(annotations(&doc), vec![a]);
    }

    #[test]
    fn table_merge_and_unmerge_are_undoable() {
        use cadrs_drawing::rich::RichText;
        use cadrs_drawing::table::{Corner, Merge, Table};
        let (mut doc, id) = doc_with_drawing();
        let mut h = History::default();
        let d = doc.element(id).unwrap().drawing_data().unwrap().clone();
        let sheet = d.sheets[0].id;
        let t = Table::new(3, 4, true, true, Corner::TopLeft, [20.0, 180.0], &d.style);
        let tid = t.id;
        let table = |doc: &Document| doc.element(id).unwrap().drawing_data().unwrap().sheets[0].tables[0].clone();
        let run = |h: &mut History, doc: &mut Document, table: Table, label: &str| {
            h.execute(doc, &EditDrawing { element: id, op: DrawingOp::SetTable { sheet, table, label: label.into() } })
                .unwrap();
        };
        h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::AddTable { sheet, table: t.clone() } }).unwrap();
        assert_eq!(h.undo_label(), Some("Insert table"));
        let filled = table(&doc).set_cell((2, 0), RichText::plain("Bolt")).unwrap();
        run(&mut h, &mut doc, filled, "Edit cell");
        let merged = table(&doc).merge((2, 0), (2, 1)).unwrap();
        run(&mut h, &mut doc, merged, "Merge cells");
        let m = Merge { row: 2, col: 0, rows: 1, cols: 2 };
        assert_eq!(table(&doc).merge_at(2, 1), Some(&m));
        let unmerged = table(&doc).unmerge((2, 1)).unwrap();
        run(&mut h, &mut doc, unmerged, "Unmerge cell");
        assert_eq!(h.undo_label(), Some("Unmerge cell"));
        assert!(table(&doc).merge_at(2, 1).is_none());
        // Undo the unmerge: merged again; undo the merge: two cells, the text kept.
        h.undo(&mut doc);
        assert_eq!(table(&doc).merge_at(2, 1), Some(&m));
        h.undo(&mut doc);
        assert!(table(&doc).merge_at(2, 1).is_none());
        assert_eq!(table(&doc).cells[2][0], RichText::plain("Bolt"));
        // Redo both.
        h.redo(&mut doc);
        assert_eq!(table(&doc).merge_at(2, 0), Some(&m));
        h.redo(&mut doc);
        assert!(table(&doc).merge_at(2, 0).is_none());
        assert_eq!(table(&doc).id, tid);
    }

    #[test]
    fn drawings_save_and_load() {
        let (doc, id) = doc_with_drawing();
        let text = ron::to_string(&doc).unwrap();
        let back: Document = ron::from_str(&text).unwrap();
        assert_eq!(back, doc);
        assert!(back.element(id).unwrap().drawing_data().is_some());
    }
}
