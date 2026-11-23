//! Onshape's Sheet metal model (`sheetMetalStart`) onto cadrs's. A child of `features`, so it
//! shares its parameter helpers.

use cadrs_core::command::CommandError;
use cadrs_core::document::FeatureKind;
use cadrs_core::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use cadrs_sketch::units::Quantity;
use serde_json::Value;

use crate::import::PartStudio;
use crate::refs::{self, Pick};
use crate::report::FeatureReport;
use crate::sketch::param;

impl PartStudio<'_> {
    /// Onshape's Sheet metal model, Thicken: sketch regions and faces given the sheet's
    /// thickness. (Convert and Extrude are not imported yet.)
    pub(crate) fn sheet_metal_start(
        &mut self,
        f: &Value,
        fid: &str,
        fr: &mut FeatureReport,
    ) -> Result<(), CommandError> {
        let process = Self::text(f, "process");
        if process != "THICKEN" {
            return Err(CommandError::Invalid(format!(
                "sheet metal models by {} are not imported yet",
                process.to_lowercase()
            )));
        }
        let mut x = SheetMetalModelFeature {
            operation: SheetMetalOp::Thicken,
            tangent_propagation: Self::flag(f, "tangentPropagation"),
            flip_thickness: Self::flag(f, "oppositeDirection"),
            keep_input: Self::flag(f, "keepInputParts"),
            ..Default::default()
        };
        let mut lost = 0;
        for p in refs::picks(param(f, "regions")) {
            match p {
                Pick::WholeSketch(s) => match self.features.get(&s) {
                    Some(id) => x.region_sketches.push(*id),
                    None => lost += 1,
                },
                Pick::Query(q) => {
                    let rs = self.region_of(&q);
                    if !rs.is_empty() {
                        for r in rs {
                            if !x.regions.iter().any(|x| x.key() == r.key()) {
                                x.regions.push(r);
                            }
                        }
                    } else if let Some(face) = self.face_of(&q) {
                        if !x.faces.iter().any(|f| f.face == face.face) {
                            x.faces.push(face);
                        }
                    } else {
                        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                            eprintln!("LOST {} | {}", f["name"], self.describe(&q));
                        }
                        lost += 1;
                    }
                }
                Pick::Opaque(_) => lost += 1,
            }
        }
        if x.region_sketches.is_empty() && x.regions.is_empty() && x.faces.is_empty() {
            return Err(CommandError::Invalid(
                "none of its regions could be translated".into(),
            ));
        }
        Self::lost_note(fr, lost, "region selections");
        let (thickness, thickness_expr) = self.quantity(f, "thickness", Quantity::Length)?;
        x.params.thickness = thickness;
        if let Ok((r, _)) = self.quantity(f, "radius", Quantity::Length) {
            x.params.bend_radius = r;
        }
        if let Ok((g, _)) = self.quantity(f, "minimalClearance", Quantity::Length) {
            x.params.minimal_gap = g;
        }
        let number = |id: &str| -> Option<f64> {
            let p = param(f, id)?;
            p["expression"]
                .as_str()
                .and_then(|e| e.trim().parse().ok())
                .or_else(|| p["value"].as_f64())
        };
        if let Some(k) = number("kFactor") {
            x.params.k_factor = k;
        }
        if let Some(k) = number("kFactorRolled") {
            x.params.rolled_k_factor = k;
        }
        x.params.flip_direction_up = Self::flag(f, "flipDirectionUp");
        x.exprs = SheetMetalExprs::of(&x.params);
        x.exprs.thickness = thickness_expr;
        self.add(
            f,
            fid,
            "Sheet metal model",
            FeatureKind::SheetMetalModel(x),
            fr,
        )
    }
}
