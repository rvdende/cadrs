//! Import and export jobs on the rebuild worker (P3F.2), with the kernel session:
//!
//! - [`plan_import`]: reads a STEP or IGES file and says what it holds ([`ImportPlan`]: part
//!   names, solid counts, occurrences and their placements), releasing the bodies again. The
//!   Import feature reads the file for real when its Part Studio rebuilds.
//! - [`export_files`]: writes parts (of a Part Studio) or instances (of an assembly, each a
//!   part of its studio at a placement) to **STEP** or **IGES** (exact B-rep, product names,
//!   optionally as an assembly of instances), or to **STL** (binary or ASCII) or **OBJ** from
//!   the kernel's tessellation at the asked chord and angle tolerance. Y axis up and the units
//!   apply to every format.

use super::*;
use crate::assembly::Pose;
use crate::import::{ImportFormat, ImportPlan, PlanOccurrence, PlanPart};

/// An export format for 3D models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    Step,
    Iges,
    Stl { binary: bool },
    Obj,
}

impl ModelFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ModelFormat::Step => "step",
            ModelFormat::Iges => "igs",
            ModelFormat::Stl { .. } => "stl",
            ModelFormat::Obj => "obj",
        }
    }

    pub fn is_mesh(self) -> bool {
        matches!(self, ModelFormat::Stl { .. } | ModelFormat::Obj)
    }
}

/// One thing to export: a part of a Part Studio (its features), where it goes (`None`: where
/// it is in its studio), its name, and what it is an instance of (instances of the same
/// source are one part of an assembly file).
#[derive(Debug, Clone)]
pub struct ExportItem {
    pub features: Arc<Vec<Feature>>,
    pub part: PartId,
    pub name: String,
    pub pose: Option<Pose>,
    /// The source (studio, part), and its name, for an assembly file.
    pub source: (crate::ids::ElementId, PartId),
    pub source_name: String,
}

/// What to export.
#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub format: ModelFormat,
    /// The file (or assembly) name.
    pub name: String,
    pub items: Vec<ExportItem>,
    /// Turn the models so +Y is up (the Top plane's +Z).
    pub y_up: bool,
    /// One file per item, else one file.
    pub individual: bool,
    /// STEP/IGES of instances: one assembly with each distinct part once and its instances
    /// (else every item as a body where it is).
    pub assembly: bool,
    /// Meshes: the chord tolerance (mm) and the angle tolerance (radians).
    pub deflection: f64,
    pub angle: f64,
    /// Meshes: file units per mm (1 for mm, 1/25.4 for inches).
    pub scale: f64,
}

impl ExportRequest {
    pub fn new(format: ModelFormat, name: impl Into<String>, items: Vec<ExportItem>) -> Self {
        Self { format, name: name.into(), items, y_up: false, individual: false, assembly: false, deflection: 0.1, angle: 15f64.to_radians(), scale: 1.0 }
    }
}

/// An exported file: the item's name when it holds one item, its extension and its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportFile {
    pub part: Option<String>,
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

/// Reads `bytes` (a `format` file) on the worker and says what it holds.
pub fn plan_import(format: ImportFormat, bytes: Vec<u8>) -> PendingJob<Result<ImportPlan, String>> {
    run_on_worker(move |r| r.plan_import(format, &bytes))
}

/// Writes the files of `req` on the worker.
pub fn export_files(req: ExportRequest) -> PendingJob<Result<Vec<ExportFile>, String>> {
    run_on_worker(move |r| r.export_files(&req))
}

fn pose_of(m: &cadrs_kernel::Motion) -> Pose {
    let l = &m.linear;
    Pose {
        rotation: [[l[(0, 0)], l[(0, 1)], l[(0, 2)]], [l[(1, 0)], l[(1, 1)], l[(1, 2)]], [l[(2, 0)], l[(2, 1)], l[(2, 2)]]],
        translation: [m.translation.x, m.translation.y, m.translation.z],
    }
}

fn motion_of(p: &Pose) -> cadrs_kernel::Motion {
    cadrs_kernel::Motion { linear: p.rotation_matrix(), translation: nalgebra::Vector3::from(p.translation) }
}

/// Y up: −90° about X, so +Z becomes +Y.
fn y_up_pose() -> Pose {
    Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], -std::f64::consts::FRAC_PI_2)
}

impl Rebuilder {
    /// [`plan_import`], here.
    #[cfg(feature = "occt")]
    pub fn plan_import(&mut self, format: ImportFormat, bytes: &[u8]) -> Result<ImportPlan, String> {
        use cadrs_kernel::Kernel;
        let kernel_format = format.kernel().ok_or_else(|| format!("{} files have no assembly structure", format.label()))?;
        let model = self.kernel.import_model(kernel_format, bytes).map_err(|e| e.to_string())?;
        let plan = self.plan_of(format, &model);
        for p in &model.parts {
            self.kernel.release(p.body);
        }
        Ok(plan)
    }

    /// What a model the kernel read holds (its bodies are kept).
    #[cfg(feature = "occt")]
    pub(crate) fn plan_of(&self, format: ImportFormat, model: &cadrs_kernel::exchange::ImportedModel) -> ImportPlan {
        use cadrs_kernel::Kernel;
        let parts = model
            .parts
            .iter()
            .map(|p| PlanPart { name: p.name.clone(), solids: self.kernel.solid_count(p.body).unwrap_or(1) })
            .collect();
        let occurrences = model.occurrences.iter().map(|o| PlanOccurrence { part: o.part, pose: pose_of(&o.placement), name: o.name.clone() }).collect();
        ImportPlan { format, name: model.name.clone(), parts, occurrences }
    }

    #[cfg(not(feature = "occt"))]
    pub fn plan_import(&mut self, _format: ImportFormat, _bytes: &[u8]) -> Result<ImportPlan, String> {
        Err("Importing needs the solid-modelling kernel".into())
    }

    /// [`export_files`], here.
    #[cfg(feature = "occt")]
    pub fn export_files(&mut self, req: &ExportRequest) -> Result<Vec<ExportFile>, String> {
        use cadrs_kernel::Kernel;
        use cadrs_kernel::exchange as x;
        if req.items.is_empty() {
            return Err("Nothing to export".into());
        }
        let up = req.y_up.then(y_up_pose);
        let place = |p: Option<Pose>| -> Pose {
            let p = p.unwrap_or(Pose::IDENTITY);
            match &up {
                Some(u) => p.then(u),
                None => p,
            }
        };
        let mut held: Vec<BodyId> = Vec::new();
        let result = (|| -> Result<Vec<ExportFile>, String> {
            let mut body_of = |this: &mut Self, features: &[Feature], part: PartId, pose: &Pose| -> Result<BodyId, String> {
                let b = this.placed_body(features, part, pose.rotation, pose.translation)?;
                held.push(b);
                Ok(b)
            };
            match req.format {
                ModelFormat::Step | ModelFormat::Iges => {
                    let format = if req.format == ModelFormat::Step { x::ExchangeFormat::Step } else { x::ExchangeFormat::Iges };
                    let ext = req.format.extension();
                    if req.assembly {
                        // Each distinct source once, where its studio has it; the items as its
                        // instances.
                        let mut sources: Vec<(crate::ids::ElementId, PartId)> = Vec::new();
                        let mut parts: Vec<(BodyId, String)> = Vec::new();
                        let mut instances = Vec::new();
                        for it in &req.items {
                            let k = match sources.iter().position(|s| *s == it.source) {
                                Some(k) => k,
                                None => {
                                    let b = body_of(self, &it.features, it.part, &Pose::IDENTITY)?;
                                    sources.push(it.source);
                                    parts.push((b, it.source_name.clone()));
                                    sources.len() - 1
                                }
                            };
                            instances.push(x::ExportInstance { part: k, placement: motion_of(&place(it.pose)), name: it.name.clone() });
                        }
                        let bytes = self.kernel.export_model(format, &req.name, &parts, &instances).map_err(|e| e.to_string())?;
                        return Ok(vec![ExportFile { part: None, extension: ext, bytes }]);
                    }
                    let mut bodies = Vec::new();
                    for it in &req.items {
                        bodies.push((body_of(self, &it.features, it.part, &place(it.pose))?, it.name.clone()));
                    }
                    let groups: Vec<&[(BodyId, String)]> = if req.individual { bodies.chunks(1).collect() } else { vec![&bodies[..]] };
                    groups
                        .into_iter()
                        .map(|g| {
                            let bytes = self.kernel.export_model(format, &req.name, g, &[]).map_err(|e| e.to_string())?;
                            Ok(ExportFile { part: (g.len() == 1 && req.individual).then(|| g[0].1.clone()), extension: ext, bytes })
                        })
                        .collect()
                }
                ModelFormat::Stl { .. } | ModelFormat::Obj => {
                    let quality = cadrs_kernel::Tessellation { deflection: req.deflection.max(1e-4), angle: req.angle.clamp(1e-3, 1.0) };
                    let mut meshes = Vec::new();
                    for it in &req.items {
                        let b = body_of(self, &it.features, it.part, &place(it.pose))?;
                        let mut m = self.kernel.tessellate(b, quality).map_err(|e| e.to_string())?;
                        if (req.scale - 1.0).abs() > 1e-15 {
                            m.positions.iter_mut().for_each(|p| *p = nalgebra::Point3::from(p.coords * req.scale));
                        }
                        meshes.push((it.name.clone(), m));
                    }
                    let write = |group: &[(String, cadrs_kernel::TriMesh)], name: &str| -> Vec<u8> {
                        match req.format {
                            ModelFormat::Obj => {
                                let list: Vec<(&str, &cadrs_kernel::TriMesh)> = group.iter().map(|(n, m)| (n.as_str(), m)).collect();
                                x::write_obj(&list).into_bytes()
                            }
                            ModelFormat::Stl { binary } => {
                                let mut all = cadrs_kernel::TriMesh::default();
                                for (_, m) in group {
                                    let base = all.positions.len() as u32;
                                    all.positions.extend_from_slice(&m.positions);
                                    all.normals.extend_from_slice(&m.normals);
                                    all.indices.extend(m.indices.iter().map(|t| t.map(|i| i + base)));
                                }
                                if binary { x::write_stl_binary(&all, &format!("cadrs {name}")) } else { x::write_stl_ascii(&all, name).into_bytes() }
                            }
                            _ => Vec::new(),
                        }
                    };
                    let ext = req.format.extension();
                    Ok(if req.individual {
                        meshes.chunks(1).map(|g| ExportFile { part: Some(g[0].0.clone()), extension: ext, bytes: write(g, &g[0].0) }).collect()
                    } else {
                        vec![ExportFile { part: None, extension: ext, bytes: write(&meshes, &req.name) }]
                    })
                }
            }
        })();
        for b in held {
            self.release_body(b);
        }
        result
    }

    #[cfg(not(feature = "occt"))]
    pub fn export_files(&mut self, _req: &ExportRequest) -> Result<Vec<ExportFile>, String> {
        Err("Exporting needs the solid-modelling kernel".into())
    }
}
