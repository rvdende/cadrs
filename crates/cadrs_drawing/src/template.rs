//! Drawing templates (D1.3–D1.6, X2): the built-in ANSI and ISO set and local custom ones.
//!
//! A template fixes a sheet's standard, size, orientation, units and projection, and the
//! drawing properties a new drawing starts with. The built-in set is generated here rather
//! than read from files: one template per standard size, orientation and unit, named like
//! Onshape's (`ANSI_A_INCH.dwt`, `ANSI_A_Portrait_MM.dwt`, `ISO_A3_MM.dwt`). The border, zones
//! and title block are cadrs's own layout (see [`crate::standard::frame`] and
//! [`crate::title_block`]).

use serde::{Deserialize, Serialize};

use crate::standard::{Orientation, Projection, SheetFormat, SheetSize, Standard};
use crate::style::DrawingStyle;

/// The units a drawing's dimensions are shown in (the template's `_INCH` / `_MM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DrawingUnits {
    Inch,
    Millimeter,
}

impl DrawingUnits {
    pub const ALL: [DrawingUnits; 2] = [DrawingUnits::Inch, DrawingUnits::Millimeter];

    /// "INCH" / "MM", as in template names.
    pub fn tag(self) -> &'static str {
        match self {
            DrawingUnits::Inch => "INCH",
            DrawingUnits::Millimeter => "MM",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DrawingUnits::Inch => "Inches",
            DrawingUnits::Millimeter => "Millimeters",
        }
    }
}

/// Where a template comes from (the Create Drawing dialog's source list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TemplateSource {
    /// Shipped with cadrs.
    BuiltIn,
    /// Made in the Custom template tab and kept locally ("My templates").
    Custom,
}

/// A drawing template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    /// The file name, e.g. `ANSI_A_INCH.dwt`.
    pub name: String,
    pub format: SheetFormat,
    pub units: DrawingUnits,
    pub projection: Projection,
    pub source: TemplateSource,
}

impl Template {
    /// A template of the given format and units with its standard's projection.
    pub fn standard(size: SheetSize, orientation: Orientation, units: DrawingUnits) -> Self {
        let std = size.standard();
        let portrait = if orientation == Orientation::Portrait {
            "_Portrait"
        } else {
            ""
        };
        Self {
            name: format!(
                "{}_{}{}_{}.dwt",
                std.label(),
                size.letter(),
                portrait,
                units.tag()
            ),
            format: SheetFormat::new(size, orientation),
            units,
            projection: std.default_projection(),
            source: TemplateSource::BuiltIn,
        }
    }

    /// A custom template (the Custom template tab), named after its settings.
    pub fn custom(
        size: SheetSize,
        orientation: Orientation,
        units: DrawingUnits,
        projection: Projection,
    ) -> Self {
        let mut t = Self::standard(size, orientation, units);
        let angle = match projection {
            Projection::First => "FirstAngle",
            Projection::Third => "ThirdAngle",
        };
        t.name = t.name.replace(".dwt", &format!("_{angle}.dwt"));
        t.name = format!("Custom_{}", t.name);
        t.projection = projection;
        t.source = TemplateSource::Custom;
        t
    }

    pub fn standard_kind(&self) -> Standard {
        self.format.size.standard()
    }

    /// The table's "Document" column.
    pub fn document_label(&self) -> String {
        match self.source {
            TemplateSource::BuiltIn => {
                format!("cadrs {} Drawing Templates", self.standard_kind().label())
            }
            TemplateSource::Custom => "My templates".into(),
        }
    }

    /// The table's "Owner" column.
    pub fn owner_label(&self) -> &'static str {
        match self.source {
            TemplateSource::BuiltIn => "cadrs",
            TemplateSource::Custom => "Me",
        }
    }

    /// The drawing properties a drawing made from this template starts with.
    pub fn style(&self) -> DrawingStyle {
        DrawingStyle::for_units(self.units, self.standard_kind())
    }
}

/// Every built-in template: for each standard and size, landscape then portrait, each in
/// inches then millimetres (the order of Onshape's list: `ANSI_A_INCH`, `ANSI_A_MM`,
/// `ANSI_A_Portrait_INCH`, `ANSI_A_Portrait_MM`, `ANSI_B_INCH`, …).
pub fn builtin_templates() -> Vec<Template> {
    let mut v = Vec::new();
    for size in SheetSize::ALL {
        for orientation in Orientation::ALL {
            for units in DrawingUnits::ALL {
                v.push(Template::standard(size, orientation, units));
            }
        }
    }
    v
}

/// The built-in template called `name`.
pub fn builtin(name: &str) -> Option<Template> {
    builtin_templates().into_iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_onshape() {
        let names: Vec<String> = builtin_templates().into_iter().map(|t| t.name).collect();
        assert_eq!(names.len(), 40);
        assert_eq!(
            &names[..5],
            [
                "ANSI_A_INCH.dwt",
                "ANSI_A_MM.dwt",
                "ANSI_A_Portrait_INCH.dwt",
                "ANSI_A_Portrait_MM.dwt",
                "ANSI_B_INCH.dwt"
            ]
        );
        assert!(names.contains(&"ISO_A0_MM.dwt".to_string()));
        assert_eq!(builtin("ISO_A4_MM.dwt").unwrap().projection, Projection::First);
        assert_eq!(builtin("ANSI_C_MM.dwt").unwrap().projection, Projection::Third);
    }

    #[test]
    fn custom_names_carry_the_projection() {
        let t = Template::custom(
            SheetSize::IsoA3,
            Orientation::Landscape,
            DrawingUnits::Millimeter,
            Projection::Third,
        );
        assert_eq!(t.name, "Custom_ISO_A3_MM_ThirdAngle.dwt");
        assert_eq!(t.source, TemplateSource::Custom);
    }
}
