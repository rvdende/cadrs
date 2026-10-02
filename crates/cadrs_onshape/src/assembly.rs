//! Onshape assemblies to cadrs Assembly tabs: instances of parts and subassemblies (of this
//! document, or linked from another at a version), placed where Onshape's solved occurrence
//! transforms put them; fixed and hidden instances; mate connectors, fastened (and other) mates
//! and groups.
//!
//! - **Placement.** Every instance gets its occurrence's transform (Onshape's solved position),
//!   so the assembly looks as it does in Onshape without solving. A subassembly instance is
//!   rigid when its parts sit where its tab puts them; otherwise it becomes flexible with its
//!   children's placements as overrides (one level; deeper differences are reported).
//! - **Parts.** An instance names an Onshape part; its cadrs part is the one the Part Studio's
//!   import matched to it by volume and box ([`crate::eval::part_map`]).
//! - **Other documents.** An instance of another document's part or assembly is a linked
//!   instance: that document is imported here (once per run) and the element copied into this
//!   document ([`cadrs_core::external::snapshot`]), with a reference at Onshape's version.
//! - **Mates.** Onshape gives each mated entity's frame in its part's coordinates (`matedCS`),
//!   so a mate joins two frame connectors ([`MateConnector::at`]) on the occurrences it names.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_core::assembly::commands::{AddMateFeature, InsertInstance, SetLocalConnector};
use cadrs_core::assembly::folders::{CreateAssemblyFolder, FolderList};
use cadrs_core::assembly::connector::{ConnectorFrame, LocalConnector, LocalConnectorId, MateConnector};
use cadrs_core::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateType};
use cadrs_core::assembly::structure::derive;
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::document::Document;
use cadrs_core::external::{InsertLinked, SourceRef, snapshot};
use cadrs_core::history_log::VersionId;
use cadrs_core::ids::{DocumentId, ElementId, FeatureId};
use serde_json::Value;

use crate::ids::stable_u128;
use crate::raw::{RawDocument, RawElement, read_json};
use crate::report::{ElementReport, FeatureReport, Outcome};
use crate::studio::{DocStudio, Studio};

/// The cadrs id of instance `inst` of Onshape assembly `element` of document `doc`.
pub fn instance_id(doc: &str, element: &str, inst: &str) -> InstanceId {
    InstanceId::from_u128(stable_u128(&[doc, element, "instance", inst]))
}

/// An Onshape transform (16 numbers, row-major, metres) as a pose.
pub fn pose_of(t: &Value) -> Option<Pose> {
    let a: Vec<f64> = t.as_array()?.iter().filter_map(Value::as_f64).collect();
    if a.len() < 12 {
        return None;
    }
    Some(Pose { rotation: [[a[0], a[1], a[2]], [a[4], a[5], a[6]], [a[8], a[9], a[10]]], translation: [a[3] * 1000.0, a[7] * 1000.0, a[11] * 1000.0] })
}

/// An Onshape coordinate system (`origin` in metres, unit axes) as a connector frame.
fn frame_of(cs: &Value) -> Option<ConnectorFrame> {
    let v = |k: &str, s: f64| -> Option<[f64; 3]> {
        let a: Vec<f64> = cs[k].as_array()?.iter().filter_map(Value::as_f64).collect();
        (a.len() == 3).then(|| [a[0] * s, a[1] * s, a[2] * s])
    };
    Some(ConnectorFrame { origin: v("origin", 1000.0)?, z: v("zAxis", 1.0)?, x: v("xAxis", 1.0)? })
}

/// A frame moved by a pose.
fn moved(p: &Pose, f: &ConnectorFrame) -> ConnectorFrame {
    ConnectorFrame { origin: p.apply(f.origin), z: p.rotate(f.z), x: p.rotate(f.x) }
}

fn close(a: &Pose, b: &Pose) -> bool {
    let r = (0..3).all(|i| (0..3).all(|j| (a.rotation[i][j] - b.rotation[i][j]).abs() < 1e-6));
    r && (0..3).all(|i| (a.translation[i] - b.translation[i]).abs() < 1e-4)
}

/// Another document imported in this run (for linked instances): the cadrs document.
type Imported = Option<Arc<Document>>;

fn externals() -> &'static std::sync::Mutex<HashMap<String, Imported>> {
    static S: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Imported>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// Onshape document `doc` (under `root`) imported as a cadrs document, once per run (`None` if
/// it wasn't scraped, or while it is being imported: a cycle).
fn external(root: &std::path::Path, doc: &str) -> Imported {
    if let Some(d) = externals().lock().ok()?.get(doc) {
        return d.clone();
    }
    externals().lock().ok()?.insert(doc.to_string(), None);
    let raw = crate::raw::documents(root).into_iter().find(|d| d.id == doc)?;
    let imported = crate::import::nested_import(|| crate::import::import_document(&raw, "import", &crate::import::Options::nested(None)));
    let d = Some(Arc::new(imported.doc));
    externals().lock().ok()?.insert(doc.to_string(), d.clone());
    d
}

/// The name of version `version` of scraped document `doc`.
fn version_name(root: &std::path::Path, doc: &str, version: &str) -> String {
    read_json(&root.join(doc).join("versions.json"))
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .find(|v| v["id"].as_str() == Some(version))
        .and_then(|v| v["name"].as_str().map(String::from))
        .unwrap_or_default()
}

/// The instances placed: Onshape instance id → (cadrs id, its subassembly's Onshape document and
/// element).
type Placed = HashMap<String, (InstanceId, Option<(String, String)>)>;

/// A resolved Onshape instance: its cadrs source (and the copies and reference when linked).
struct Source {
    source: InstanceSource,
    link: Option<(SourceRef, cadrs_core::external::LinkSnapshot)>,
    /// For a subassembly: its Onshape document and element (whose instance ids its children use).
    sub: Option<(String, String)>,
    /// For a part: how far (mm) the cadrs part sits from Onshape's ([`crate::eval::part_map`]).
    offset: [f64; 3],
}

/// The pose that puts a cadrs part where Onshape's `pose` puts Onshape's part, the cadrs part
/// sitting `offset` from it.
fn corrected(pose: Pose, offset: [f64; 3]) -> Pose {
    Pose::translation([-offset[0], -offset[1], -offset[2]]).then(&pose)
}

/// How far (mm) the cadrs part of Onshape part instance `inst` sits from Onshape's.
fn part_offset(inst: &Value) -> [f64; 3] {
    let (Some(d), Some(e), Some(p)) = (inst["documentId"].as_str(), inst["elementId"].as_str(), inst["partId"].as_str()) else { return [0.0; 3] };
    crate::eval::part_map(d, e).and_then(|m| m.get(p).map(|x| x.1)).unwrap_or([0.0; 3])
}

/// Imports every assembly of `raw`, subassembly tabs before the tabs that use them.
/// Where an assembly of this document holds an instance of the Part Studio `studio` (an Onshape
/// element id) at its top level: the cadrs assembly, the instance's cadrs id and its pose
/// (Onshape's occurrence transform). The anchor of the studio's assembly context.
pub fn context_anchor(raw: &RawDocument, elements: &[RawElement], studio: &str) -> Option<(ElementId, InstanceId, Pose)> {
    for el in elements.iter().filter(|e| e.kind == "ASSEMBLY") {
        let def = read_json(&el.dir.join("definition.json"))?;
        let root = &def["rootAssembly"];
        for inst in root["instances"].as_array().into_iter().flatten() {
            if inst["elementId"].as_str() != Some(studio) || inst["documentId"].as_str() != Some(raw.id.as_str()) || inst["suppressed"].as_bool() == Some(true) {
                continue;
            }
            let oid = inst["id"].as_str()?;
            let occ = root["occurrences"].as_array().into_iter().flatten().find(|o| o["path"].as_array().is_some_and(|p| p.len() == 1 && p[0].as_str() == Some(oid)))?;
            let pose = pose_of(&occ["transform"])?;
            return Some((ElementId::from_u128(stable_u128(&[&raw.id, &el.id])), instance_id(&raw.id, &el.id, oid), pose));
        }
    }
    None
}

pub fn import_assemblies(s: &mut DocStudio, raw: &RawDocument, elements: &[RawElement], reports: &mut [Option<ElementReport>]) {
    let asms: Vec<usize> = (0..elements.len()).filter(|&i| elements[i].kind == "ASSEMBLY").collect();
    // Same-document subassemblies first.
    let uses = |i: usize| -> Vec<String> {
        let def = read_json(&elements[i].dir.join("definition.json")).unwrap_or(Value::Null);
        def["rootAssembly"]["instances"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|x| x["type"].as_str() == Some("Assembly") && x["documentId"].as_str() == Some(raw.id.as_str()))
            .filter_map(|x| x["elementId"].as_str().map(String::from))
            .collect()
    };
    let mut left = asms.clone();
    while !left.is_empty() {
        let k = left.iter().position(|&i| uses(i).iter().all(|e| !left.iter().any(|&j| elements[j].id == *e))).unwrap_or(0);
        let i = left.remove(k);
        let Some(er) = reports.get_mut(i).and_then(Option::as_mut) else { continue };
        import_assembly(s, raw, &elements[i], er);
    }
}

fn import_assembly(s: &mut DocStudio, raw: &RawDocument, el: &RawElement, er: &mut ElementReport) {
    let Some(def) = read_json(&el.dir.join("definition.json")) else {
        er.notes.push("no assembly definition was scraped".into());
        return;
    };
    let root = &def["rootAssembly"];
    let instances = root["instances"].as_array().cloned().unwrap_or_default();
    if instances.is_empty() {
        return;
    }
    let raw_root = raw.dir.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let asm = ElementId::from_u128(stable_u128(&[&raw.id, &el.id]));
    // Occurrences: path → (pose, fixed, hidden).
    let mut occ: HashMap<Vec<String>, (Pose, bool, bool)> = HashMap::new();
    for o in root["occurrences"].as_array().into_iter().flatten() {
        let path: Vec<String> = o["path"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
        if let Some(p) = pose_of(&o["transform"]) {
            occ.insert(path, (p, o["fixed"].as_bool() == Some(true), o["hidden"].as_bool() == Some(true)));
        }
    }
    // The subassembly definitions (their instances), by Onshape document and element.
    let subs: HashMap<(String, String), Vec<Value>> = def["subAssemblies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| Some(((x["documentId"].as_str()?.to_string(), x["elementId"].as_str()?.to_string()), x["instances"].as_array().cloned().unwrap_or_default())))
        .collect();
    let mut placed = 0;
    let mut problems: HashMap<String, usize> = HashMap::new();
    // Onshape instance id → (cadrs id, its subassembly's Onshape document and element).
    let mut ids: Placed = HashMap::new();
    for inst in &instances {
        let oid = inst["id"].as_str().unwrap_or_default().to_string();
        let name = inst["name"].as_str().unwrap_or_default();
        let id = instance_id(&raw.id, &el.id, &oid);
        let src = match resolve(s, raw, &raw_root, inst) {
            Ok(x) => x,
            Err(why) => {
                *problems.entry(why).or_default() += 1;
                continue;
            }
        };
        let (pose, fixed, hidden) = occ.get(&vec![oid.clone()]).copied().unwrap_or((Pose::IDENTITY, false, false));
        let pose = corrected(pose, src.offset);
        let mut instance = Instance::new(id, src.source, pose);
        instance.fixed = fixed;
        instance.hidden = hidden;
        instance.suppressed = inst["suppressed"].as_bool() == Some(true);
        // A subassembly whose parts sit elsewhere than its tab says: flexible, with overrides.
        if let Some((d, e)) = &src.sub {
            let tab = match &src.link {
                Some((_, snap)) => snap.element().cloned(),
                None => s.doc.element(src.source.element()).cloned(),
            };
            let children = tab.map(|t| t.assembly.instances.clone()).unwrap_or_default();
            let mut overrides = Vec::new();
            for child in subs.get(&(d.clone(), e.clone())).into_iter().flatten() {
                let Some(cid) = child["id"].as_str() else { continue };
                let Some((world, ..)) = occ.get(&vec![oid.clone(), cid.to_string()]) else { continue };
                let tab_id = instance_id(d, e, cid);
                let Some(c) = children.iter().find(|c| c.id == tab_id) else { continue };
                let local = corrected(world.then(&pose.inverse()), part_offset(child));
                if !close(&local, &c.pose) {
                    overrides.push((tab_id, local));
                }
            }
            if !overrides.is_empty() {
                instance.flexible = true;
                instance.overrides = overrides;
            }
        }
        let result = match &src.link {
            Some((r, snap)) => s.run(&InsertLinked { element: asm, snapshot: snap.clone(), instances: vec![instance], reference: *r }),
            None => s.run(&InsertInstance { element: asm, instance }),
        };
        match result {
            Ok(()) => {
                placed += 1;
                ids.insert(oid, (id, src.sub.clone()));
            }
            Err(e) => *problems.entry(format!("{name}: {e}")).or_default() += 1,
        }
    }
    let _ = &problems;
    folders(s, asm, el, &ids, er);
    // The occurrence id of an Onshape path (a part at any depth).
    let occurrence = |path: &[String]| -> Option<InstanceId> {
        let (top, sub) = ids.get(path.first()?)?;
        let mut id = *top;
        let mut sub = sub.clone();
        let mut inner: Vec<InstanceId> = Vec::new();
        for p in &path[1..] {
            let (d, e) = sub?;
            inner.push(instance_id(&d, &e, p));
            // Deeper: that child's own subassembly (its definition lists it by document and element).
            sub = subs.get(&(d, e)).and_then(|xs| xs.iter().find(|x| x["id"].as_str() == Some(p.as_str()))).and_then(|x| {
                if x["type"].as_str() == Some("Assembly") { Some((x["documentId"].as_str()?.to_string(), x["elementId"].as_str()?.to_string())) } else { None }
            });
        }
        // derive(A, derive(B, C)): each level names its occurrence in its own tab.
        if let Some(last) = inner.pop() {
            let mut acc = last;
            while let Some(x) = inner.pop() {
                acc = derive(x, acc);
            }
            id = derive(id, acc);
        }
        Some(id)
    };
    // The Onshape instance a path ends at (a part's, for its offset).
    let leaf = |path: &[String]| -> Option<&Value> {
        let top = path.first()?;
        let mut cur = instances.iter().find(|x| x["id"].as_str() == Some(top.as_str()))?;
        for p in &path[1..] {
            let key = (cur["documentId"].as_str()?.to_string(), cur["elementId"].as_str()?.to_string());
            cur = subs.get(&key)?.iter().find(|x| x["id"].as_str() == Some(p.as_str()))?;
        }
        Some(cur)
    };
    // A frame in Onshape's part coordinates, in the cadrs part's.
    let local = |path: &[String], f: ConnectorFrame| -> ConnectorFrame { moved(&Pose::translation(leaf(path).map(part_offset).unwrap_or([0.0; 3])), &f) };
    let path_of = |v: &Value| -> Vec<String> { v.as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect() };
    let world = |path: &[String]| occ.get(path).map(|x| x.0);
    let (mut mates, mut mates_done) = (0, 0);
    for f in root["features"].as_array().into_iter().flatten() {
        let data = &f["featureData"];
        let name = data["name"].as_str().unwrap_or("Mate").to_string();
        let kind = f["featureType"].as_str().unwrap_or_default();
        let fid = f["id"].as_str().unwrap_or_default();
        let mut fr = FeatureReport { name: name.clone(), kind: kind.to_string(), outcome: Outcome::Full, notes: Vec::new() };
        if f["suppressed"].as_bool() == Some(true) {
            fr.outcome = Outcome::Suppressed;
            er.features.push(fr);
            continue;
        }
        let mid = MateId(uuid::Uuid::from_u128(stable_u128(&[&raw.id, &el.id, "mate", fid])));
        let result: Result<(), String> = match kind {
            "mateConnector" => (|| {
                let path = path_of(&data["occurrence"]);
                let owner = occurrence(&path).ok_or("its instance was not imported")?;
                let frame = local(&path, frame_of(&data["mateConnectorCS"]).ok_or("no frame")?);
                let c = LocalConnector { id: LocalConnectorId(mid.0), name: name.clone(), connector: MateConnector::at(owner, frame) };
                s.run(&SetLocalConnector { element: asm, connector: c }).map_err(|e| e.to_string())
            })(),
            "mateGroup" => (|| {
                let mut members: Vec<InstanceId> = Vec::new();
                for o in data["occurrences"].as_array().into_iter().flatten() {
                    let path = path_of(&o["occurrence"]);
                    let top = path.first().and_then(|p| ids.get(p)).map(|x| x.0).ok_or("an instance of it was not imported")?;
                    if !members.contains(&top) {
                        members.push(top);
                    }
                }
                let feature = MateFeature::new(mid, name.clone(), MateKind::Group { instances: members });
                s.run(&AddMateFeature { element: asm, feature, poses: Vec::new() }).map_err(|e| e.to_string())
            })(),
            "mate" => {
                mates += 1;
                (|| {
                    let t = match data["mateType"].as_str().unwrap_or_default() {
                        "FASTENED" => MateType::Fastened,
                        "REVOLUTE" => MateType::Revolute,
                        "SLIDER" => MateType::Slider,
                        "CYLINDRICAL" => MateType::Cylindrical,
                        "PIN_SLOT" => MateType::PinSlot,
                        "PLANAR" => MateType::Planar,
                        "BALL" => MateType::Ball,
                        "PARALLEL" => MateType::Parallel,
                        other => return Err(format!("{} mates are not imported yet", other.to_lowercase())),
                    };
                    let ents = data["matedEntities"].as_array().cloned().unwrap_or_default();
                    if ents.len() != 2 {
                        return Err("it doesn't join two entities".into());
                    }
                    let mut cs = Vec::new();
                    let mut worlds = Vec::new();
                    for e in &ents {
                        let path = path_of(&e["matedOccurrence"]);
                        let frame = frame_of(&e["matedCS"]).ok_or("no frame")?;
                        let conn = if path.is_empty() { MateConnector::at(InstanceId::ORIGIN, frame) } else { MateConnector::at(occurrence(&path).ok_or("an instance of it was not imported")?, local(&path, frame)) };
                        worlds.push(if path.is_empty() { Some(frame) } else { world(&path).map(|p| moved(&p, &frame)) });
                        cs.push(conn);
                    }
                    let m = Mate::new(t, cs[0], cs[1]);
                    let feature = MateFeature::new(mid, name.clone(), MateKind::Mate(m));
                    s.run(&AddMateFeature { element: asm, feature, poses: Vec::new() }).map_err(|e| e.to_string())?;
                    // Onshape's positions are kept; a fastened mate they don't satisfy (an offset
                    // Onshape's featureData doesn't list) is noted.
                    if t == MateType::Fastened
                        && let (Some(a), Some(b)) = (worlds[0], worlds[1])
                        && a.origin.iter().zip(b.origin).any(|(x, y)| (x - y).abs() > 1e-3)
                    {
                        return Err("imported, but Onshape's positions don't satisfy it (an offset?)".into());
                    }
                    Ok(())
                })()
            }
            other => Err(format!("{other} is not imported yet")),
        };
        match result {
            Ok(()) => {
                if kind == "mate" {
                    mates_done += 1;
                }
            }
            Err(why) if why.starts_with("imported, but") => {
                if kind == "mate" {
                    mates_done += 1;
                }
                fr.outcome = Outcome::Partial;
                fr.notes.push(why);
            }
            Err(why) => {
                fr.outcome = Outcome::Skipped;
                fr.notes.push(why);
            }
        }
        er.features.push(fr);
    }
    er.notes.retain(|n| !n.starts_with("assembly contents are not imported"));
    let mut problems: Vec<(String, usize)> = problems.into_iter().collect();
    problems.sort();
    for (why, n) in problems {
        er.notes.push(if n > 1 { format!("{n} instances: {why}") } else { format!("an instance: {why}") });
    }
    er.assembly = Some(crate::report::AssemblyCounts { instances: (placed, instances.len()), mates: (mates_done, mates) });
}

/// The instance folders (`folders.json`, read from Onshape's instance list by
/// `tools/onshape/folders.js`: the REST API doesn't return them). cadrs folders don't nest, so a
/// folder in a folder is merged into the outer one.
fn folders(s: &mut DocStudio, asm: ElementId, el: &RawElement, ids: &Placed, er: &mut ElementReport) {
    let Some(v) = read_json(&el.dir.join("folders.json")) else { return };
    let all = v["folders"].as_array().cloned().unwrap_or_default();
    let by_id: HashMap<&str, &Value> = all.iter().filter_map(|f| Some((f["id"].as_str()?, f))).collect();
    // A folder's instances, its subfolders' too (in list order).
    fn gather<'a>(f: &'a Value, by_id: &HashMap<&str, &'a Value>, out: &mut Vec<&'a str>, nested: &mut usize) {
        for m in f["members"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            match by_id.get(m) {
                Some(sub) => {
                    *nested += 1;
                    gather(sub, by_id, out, nested);
                }
                None => out.push(m),
            }
        }
    }
    for f in all.iter().filter(|f| f["parent"].is_null()) {
        let (Some(fid), Some(name)) = (f["id"].as_str(), f["name"].as_str()) else { continue };
        let (mut members, mut nested) = (Vec::new(), 0);
        gather(f, &by_id, &mut members, &mut nested);
        let items: Vec<FeatureId> = members.iter().filter_map(|m| ids.get(*m)).map(|x| cadrs_core::assembly::folders::item(x.0)).collect();
        let folder = FeatureId(uuid::Uuid::from_u128(stable_u128(&[&el.id, "folder", fid])));
        let cmd = CreateAssemblyFolder { element: asm, list: FolderList::Instances, folder, name: Some(name.to_string()), items };
        if let Err(e) = s.run(&cmd) {
            er.notes.push(format!("folder {name}: {e}"));
        } else if nested > 0 {
            er.notes.push(format!("folder {name}: its {nested} subfolder(s) merged into it (cadrs folders don't nest)"));
        }
    }
}

/// The cadrs source of an Onshape instance.
fn resolve(s: &DocStudio, raw: &RawDocument, root: &std::path::Path, inst: &Value) -> Result<Source, String> {
    if inst["isStandardContent"].as_bool() == Some(true) {
        return Err("standard content is not imported yet".into());
    }
    let doc = inst["documentId"].as_str().unwrap_or_default().to_string();
    let el = inst["elementId"].as_str().unwrap_or_default().to_string();
    let here = doc == raw.id;
    let element = ElementId::from_u128(stable_u128(&[&doc, &el]));
    let ty = inst["type"].as_str().unwrap_or_default();
    let part = || -> Result<(cadrs_core::ids::PartId, [f64; 3]), String> {
        let pid = inst["partId"].as_str().unwrap_or_default();
        let map = crate::eval::part_map(&doc, &el).ok_or("its Part Studio was not imported")?;
        map.get(pid).copied().ok_or_else(|| "its part didn't come across (not matched in its Part Studio)".to_string())
    };
    if here {
        return match ty {
            "Part" => {
                if s.doc.element(element).is_none() {
                    return Err("its Part Studio was not imported".into());
                }
                let (part, offset) = part()?;
                Ok(Source { source: InstanceSource::Part { element, part }, link: None, sub: None, offset })
            }
            "Assembly" => Ok(Source { source: InstanceSource::Assembly { element }, link: None, sub: Some((doc, el)), offset: [0.0; 3] }),
            other => Err(format!("{other} instances are not imported yet")),
        };
    }
    // Another document: import it, and link its element at Onshape's version.
    let ext = external(root, &doc).ok_or("its document was not scraped (or links back here)")?;
    let version = inst["documentVersion"].as_str().unwrap_or_default();
    let vid = VersionId(uuid::Uuid::from_u128(stable_u128(&[&doc, "version", version])));
    let r = SourceRef::version(Some(DocumentId::from_u128(stable_u128(&[&doc]))), element, vid);
    let snap = snapshot(&ext, r, &version_name(root, &doc, version)).map_err(|e| e.to_string())?;
    let (source, offset) = match ty {
        "Part" => {
            let (part, offset) = part()?;
            (InstanceSource::Part { element: snap.root, part }, offset)
        }
        "Assembly" => (InstanceSource::Assembly { element: snap.root }, [0.0; 3]),
        other => return Err(format!("{other} instances are not imported yet")),
    };
    let sub = (ty == "Assembly").then(|| (doc.clone(), el.clone()));
    Ok(Source { source, link: Some((r, snap)), sub, offset })
}
