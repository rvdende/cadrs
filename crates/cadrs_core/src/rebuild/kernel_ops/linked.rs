//! Rebuilding the Import feature (Onshape import): parts that come from outside the feature
//! list, a file ([`crate::import`]). It makes new parts through `finish` and then gives them the
//! file's body names; it doesn't use up a "Part N" number, as in Onshape. (The Derived feature,
//! which the importer also uses, is rebuilt by `kernel_ops/derived.rs`.) IGES files, and STEP
//! files read with their assembly structure (P3F.2), are read in `import.rs`.

use super::*;
use cadrs_kernel::Kernel;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_sketch::{FaceName, FaceOrigin};
use nalgebra::Point3;

use crate::import::{ImportFeature, ImportFormat, ImportMode};

/// A new part of a linked feature: its id, piece, name, kind, palette entry and inherited
/// settings.
struct Linked {
    id: PartId,
    piece: Piece,
    name: String,
    kind: PartKind,
    palette: Option<u32>,
    derived: Option<Arc<crate::document::PartProps>>,
    connectors: Vec<crate::solid::SolidConnector>,
}

impl Rebuilder {
    /// Edge and vertex names for a body whose faces are named `faces`.
    fn linked_names(&self, body: BodyId, faces: Vec<FaceName>) -> Result<BodyNames, String> {
        let edges = self.kernel.edges(body).map_err(|e| e.to_string())?;
        let vertices = self.kernel.vertices(body).map_err(|e| e.to_string())?;
        Ok(BodyNames {
            edges: naming::name_edges(&faces, &edges, naming::joint_tolerance(&edges)),
            vertices: naming::name_vertices(&faces, &edges, &vertices),
            faces,
            aliases: Vec::new(),
        })
    }

    /// The parts after `state` plus `made`, named and dressed as they say.
    fn place_linked(&mut self, id: FeatureId, made: Vec<Linked>, state: &Arc<State>) -> Result<Output, String> {
        let next = (**state).clone();
        let (np, ns) = (next.next_part, next.next_surface);
        let placed: Vec<Placed> = made.iter().map(|m| (m.id, Piece { body: m.piece.body, names: m.piece.names.clone(), from: HashSet::new(), volume: m.piece.volume })).collect();
        let mut out = self.finish(id, placed, next, id.0, state.geoms.clone(), PartKind::Solid)?;
        let st = Arc::get_mut(&mut out.state).expect("a new state");
        st.next_part = np;
        st.next_surface = ns;
        for m in made {
            let Some(p) = st.parts.iter_mut().find(|q| q.part.id == m.id) else { continue };
            p.part.name = m.name;
            p.part.kind = m.kind;
            if let Some(pal) = m.palette {
                p.part.palette = pal;
            }
            p.part.derived = m.derived;
            if !m.connectors.is_empty() {
                Arc::make_mut(&mut p.part.solid).connectors = m.connectors;
            }
        }
        Ok(out)
    }

    /// The Import feature: a part per body of the file.
    pub(in crate::rebuild) fn import(&mut self, id: FeatureId, x: &ImportFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let bytes = crate::blobs::get(&x.blob).ok_or_else(|| format!("The data of {} is missing", x.file_name))?;
        let mut warning = None;
        let mut bodies: Vec<(BodyId, String)> = Vec::new();
        let release = |k: &mut cadrs_kernel::backend::occt::OcctKernel, b: &[(BodyId, String)]| b.iter().for_each(|(b, _)| k.release(*b));
        match x.format {
            // IGES, and STEP with its assembly structure: the XDE reader (`import.rs`).
            ImportFormat::Iges => bodies = self.read_structured(x, &bytes, x.structure.unwrap_or(ImportMode::Flatten))?,
            ImportFormat::Step if x.structure.is_some() => {
                bodies = self.read_structured(x, &bytes, x.structure.unwrap_or(ImportMode::Flatten))?;
            }
            ImportFormat::Step => {
                let made = self.kernel.import_step(&bytes).map_err(|e| format!("Import failed: {e}"))?;
                let counts: Vec<usize> = made.iter().map(|b| self.kernel.faces(*b).map(|f| f.len()).unwrap_or(0)).collect();
                let occurrences = crate::import::step_occurrences(&String::from_utf8_lossy(&bytes));
                let names = crate::import::assign_names(&counts, &occurrences, &x.stem());
                bodies = made.into_iter().zip(names).collect();
            }
            ImportFormat::Stl => {
                let tris = crate::import::parse_stl(&bytes)?;
                let s = x.scale();
                let pieces = crate::import::stl_pieces(&tris);
                let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                for t in &tris {
                    for p in t {
                        for k in 0..3 {
                            lo[k] = lo[k].min(p[k] * s);
                            hi[k] = hi[k].max(p[k] * s);
                        }
                    }
                }
                let diag = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt();
                let tol = (diag * 1e-6).max(1e-6);
                let stem = x.stem();
                let mut failed = 0;
                let mut last_err = String::new();
                for piece in &pieces {
                    let pts: Vec<[Point3<f64>; 3]> = piece.iter().map(|t| t.map(|p| Point3::new(p[0] * s, p[1] * s, p[2] * s))).collect();
                    match self.kernel.mesh_solid(&pts, tol) {
                        Ok(b) => {
                            let name = if bodies.is_empty() { stem.clone() } else { format!("{stem} ({})", bodies.len() + 1) };
                            bodies.push((b, name));
                        }
                        Err(e) => {
                            failed += 1;
                            last_err = e.to_string();
                        }
                    }
                }
                if bodies.is_empty() {
                    return Err(format!("The mesh does not make a solid: {last_err}"));
                }
                if failed > 0 {
                    warning = Some(format!("{failed} of {} pieces of the mesh are not closed solids and were left out", pieces.len()));
                }
            }
        }
        if x.y_axis_up {
            // The file's +Y becomes +Z: a quarter turn about X.
            let turn = cadrs_kernel::Transform::rotation(nalgebra::Vector3::x() * std::f64::consts::FRAC_PI_2);
            for i in 0..bodies.len() {
                match self.kernel.transform(bodies[i].0, &turn) {
                    Ok(r) => {
                        self.kernel.release(bodies[i].0);
                        bodies[i].0 = r.bodies[0];
                    }
                    Err(e) => {
                        release(&mut self.kernel, &bodies);
                        return Err(format!("Turning Y up failed: {e}"));
                    }
                }
            }
        }
        let mut made = Vec::new();
        for (k, (body, name)) in bodies.iter().enumerate() {
            let n = self.kernel.faces(*body).map(|f| f.len()).unwrap_or(0);
            let faces = (0..n).map(|i| FaceName::new(id.0, FaceOrigin::Imported { body: k as u32, face: i as u32 })).collect();
            let names = match self.linked_names(*body, faces) {
                Ok(n) => n,
                Err(e) => {
                    release(&mut self.kernel, &bodies);
                    return Err(e);
                }
            };
            let solid = self.kernel.solid_count(*body).unwrap_or(1) > 0;
            let volume = self.kernel.mass_properties(*body).map(|m| m.volume).unwrap_or(0.0);
            made.push(Linked {
                id: PartId::new(id, k as u32),
                piece: Piece { body: *body, names, from: HashSet::new(), volume },
                name: name.clone(),
                kind: if solid { PartKind::Solid } else { PartKind::Surface },
                palette: None,
                derived: None,
                connectors: Vec::new(),
            });
        }
        let mut out = self.place_linked(id, made, state)?;
        out.warning = warning;
        Ok(out)
    }
}
