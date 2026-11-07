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

/// What [`Rebuilder::mesh_parts`] made of mesh files' triangles.
#[derive(Default)]
struct MeshParts {
    /// Each body, its name and the index of the mesh it came from.
    bodies: Vec<(BodyId, String, usize)>,
    /// The connected pieces of the meshes, those that made surfaces, and those left out.
    pieces: usize,
    sheets: usize,
    failed: usize,
    last_err: String,
}

impl Rebuilder {
    /// Edge and vertex names for a body whose faces are named `faces`.
    pub(super) fn linked_names(
        &self,
        body: BodyId,
        faces: Vec<FaceName>,
    ) -> Result<BodyNames, String> {
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
    fn place_linked(
        &mut self,
        id: FeatureId,
        made: Vec<Linked>,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        let next = (**state).clone();
        let (np, ns) = (next.next_part, next.next_surface);
        let placed: Vec<Placed> = made
            .iter()
            .map(|m| {
                (
                    m.id,
                    Piece {
                        body: m.piece.body,
                        names: m.piece.names.clone(),
                        from: HashSet::new(),
                        volume: m.piece.volume,
                    },
                )
            })
            .collect();
        let mut out = self.finish(id, placed, next, id.0, state.geoms.clone(), PartKind::Solid)?;
        let st = Arc::get_mut(&mut out.state).expect("a new state");
        st.next_part = np;
        st.next_surface = ns;
        for m in made {
            let Some(p) = st.parts.iter_mut().find(|q| q.part.id == m.id) else {
                continue;
            };
            p.part_mut().name = m.name;
            p.part_mut().kind = m.kind;
            if let Some(pal) = m.palette {
                p.part_mut().palette = pal;
            }
            p.part_mut().derived = m.derived;
            if !m.connectors.is_empty() {
                Arc::make_mut(&mut p.part_mut().solid).connectors = m.connectors;
            }
        }
        Ok(out)
    }

    /// Bodies of mesh files' triangles (`meshes`: a name and its triangles each, in the file's
    /// units, `scale` mm each): a solid per closed piece of each, else (with `sheets`) a surface.
    /// Each body comes with its name ("name", "name (2)", …) and the index of its mesh.
    fn mesh_parts(
        &mut self,
        meshes: &[(String, Vec<crate::import::Triangle>)],
        scale: f64,
        sheets: bool,
    ) -> MeshParts {
        let s = scale;
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for t in meshes.iter().flat_map(|m| &m.1) {
            for p in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k] * s);
                    hi[k] = hi[k].max(p[k] * s);
                }
            }
        }
        let diag = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt();
        let tol = (diag * 1e-6).max(1e-6);
        let mut out = MeshParts::default();
        let mut used: HashMap<String, usize> = HashMap::new();
        for (k, (stem, tris)) in meshes.iter().enumerate() {
            for piece in crate::import::stl_pieces(tris) {
                out.pieces += 1;
                let pts: Vec<[Point3<f64>; 3]> = piece
                    .iter()
                    .map(|t| t.map(|p| Point3::new(p[0] * s, p[1] * s, p[2] * s)))
                    .collect();
                let body = match self.kernel.mesh_solid(&pts, tol) {
                    Ok(b) => b,
                    Err(e) => {
                        out.last_err = e.to_string();
                        if !sheets {
                            out.failed += 1;
                            continue;
                        }
                        match self.kernel.mesh_sheet(&pts, tol) {
                            Ok(b) => {
                                out.sheets += 1;
                                b
                            }
                            Err(e) => {
                                out.failed += 1;
                                out.last_err = e.to_string();
                                continue;
                            }
                        }
                    }
                };
                let n = used.entry(stem.clone()).or_default();
                *n += 1;
                let name = if *n == 1 {
                    stem.clone()
                } else {
                    format!("{stem} ({n})")
                };
                out.bodies.push((body, name, k));
            }
        }
        out
    }

    /// The Import feature: a part per body of the file.
    pub(in crate::rebuild) fn import(
        &mut self,
        id: FeatureId,
        x: &ImportFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let bytes = crate::blobs::get(&x.blob)
            .ok_or_else(|| format!("The data of {} is missing", x.file_name))?;
        let mut warning = None;
        let mut bodies: Vec<(BodyId, String)> = Vec::new();
        // GLB: each body's colour (in the order of `bodies`).
        let mut colors: Vec<Option<crate::appearance::Appearance>> = Vec::new();
        let release = |k: &mut cadrs_kernel::DefaultKernel, b: &[(BodyId, String)]| {
            b.iter().for_each(|(b, _)| k.release(*b))
        };
        match x.format {
            // IGES, BREP, and STEP with its assembly structure: the XDE reader (`import.rs`).
            ImportFormat::Iges | ImportFormat::Brep => {
                bodies =
                    self.read_structured(x, &bytes, x.structure.unwrap_or(ImportMode::Flatten))?
            }
            ImportFormat::Step => {
                bodies =
                    self.read_structured(x, &bytes, x.structure.unwrap_or(ImportMode::Flatten))?;
                if x.structure.is_none() {
                    // Preserve the ordinary STEP import's solid-before-surface ordering and
                    // naming, while sharing the color-aware reader with structured imports.
                    bodies
                        .sort_by_key(|(body, _)| self.kernel.solid_count(*body).unwrap_or(0) == 0);
                    let counts: Vec<_> = bodies
                        .iter()
                        .map(|(body, _)| self.kernel.faces(*body).map(|f| f.len()).unwrap_or(0))
                        .collect();
                    let occurrences =
                        crate::import::step_occurrences(&String::from_utf8_lossy(&bytes));
                    let names = crate::import::assign_names(&counts, &occurrences, &x.stem());
                    for ((_, name), original) in bodies.iter_mut().zip(names) {
                        *name = original;
                    }
                }
            }
            ImportFormat::Stl => {
                let tris = crate::import::parse_stl(&bytes)?;
                let m = self.mesh_parts(&[(x.stem(), tris)], x.scale(), false);
                bodies = m.bodies.into_iter().map(|(b, n, _)| (b, n)).collect();
                if bodies.is_empty() {
                    return Err(format!("The mesh does not make a solid: {}", m.last_err));
                }
                if m.failed > 0 {
                    warning = Some(format!(
                        "{} of {} pieces of the mesh are not closed solids and were left out",
                        m.failed, m.pieces
                    ));
                }
            }
            ImportFormat::Glb => {
                let meshes = crate::glb::read(&bytes)?;
                // A part per closed piece of each mesh, else a surface; each named after its
                // node, in its material's colour.
                let named: Vec<(String, Vec<crate::import::Triangle>)> = meshes
                    .iter()
                    .map(|m| (m.name.clone(), m.triangles.clone()))
                    .collect();
                let m = self.mesh_parts(&named, x.scale(), true);
                for (b, name, k) in m.bodies {
                    bodies.push((b, name));
                    colors.push(meshes[k].color.map(crate::glb::appearance_of));
                }
                if bodies.is_empty() {
                    return Err(format!(
                        "The meshes make no solids or surfaces: {}",
                        m.last_err
                    ));
                }
                if m.sheets > 0 {
                    warning = Some(format!(
                        "{} of {} pieces of the meshes are not closed and came in as surfaces",
                        m.sheets, m.pieces
                    ));
                }
            }
        }
        if x.y_axis_up {
            // The file's +Y becomes +Z: a quarter turn about X.
            let turn = cadrs_kernel::Transform::rotation(
                nalgebra::Vector3::x() * std::f64::consts::FRAC_PI_2,
            );
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
            let faces = (0..n)
                .map(|i| {
                    FaceName::new(
                        id.0,
                        FaceOrigin::Imported {
                            body: k as u32,
                            face: i as u32,
                        },
                    )
                })
                .collect();
            let names = match self.linked_names(*body, faces) {
                Ok(n) => n,
                Err(e) => {
                    release(&mut self.kernel, &bodies);
                    return Err(e);
                }
            };
            let solid = self.kernel.solid_count(*body).unwrap_or(1) > 0;
            let volume = self
                .kernel
                .mass_properties(*body)
                .map(|m| m.volume)
                .unwrap_or(0.0);
            made.push(Linked {
                id: PartId::new(id, k as u32),
                piece: Piece {
                    body: *body,
                    names,
                    from: HashSet::new(),
                    volume,
                },
                name: name.clone(),
                kind: if solid {
                    PartKind::Solid
                } else {
                    PartKind::Surface
                },
                palette: None,
                // The file's colour, shown unless the part is given its own.
                derived: colors.get(k).copied().flatten().map(|c| {
                    let mut props = crate::document::PartProps::new(PartId::new(id, k as u32));
                    props.appearance = Some(c);
                    Arc::new(props)
                }),
                connectors: Vec::new(),
            });
        }
        let mut out = self.place_linked(id, made, state)?;
        out.warning = warning;
        Ok(out)
    }
}
