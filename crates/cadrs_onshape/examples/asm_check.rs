//! Compares where an imported assembly puts each part with where Onshape does: for every part
//! occurrence (at any depth) of Onshape's `definition.json`, the box of Onshape's part vertices
//! (`bodydetails.json`) moved by Onshape's occurrence transform against the box of the cadrs
//! part's vertices moved by the cadrs occurrence pose. Prints the occurrences that differ.
//!
//! `cargo run -r -p cadrs_onshape --example asm_check -- <store dir> <raw dir> <Onshape document id> <assembly element id>`

use std::collections::HashMap;

use cadrs_core::Store;
use cadrs_core::assembly::structure::{derive, occurrences};
use cadrs_onshape::assembly::{instance_id, pose_of};
use serde_json::Value;

fn read(p: &std::path::Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

fn bbox(points: impl Iterator<Item = [f64; 3]>) -> Option<[f64; 6]> {
    let mut b = [f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN];
    let mut any = false;
    for p in points {
        any = true;
        for k in 0..3 {
            b[k] = b[k].min(p[k]);
            b[k + 3] = b[k + 3].max(p[k]);
        }
    }
    any.then_some(b)
}

fn centre(b: &[f64; 6]) -> [f64; 3] {
    [(b[0] + b[3]) / 2.0, (b[1] + b[4]) / 2.0, (b[2] + b[5]) / 2.0]
}

fn size(b: &[f64; 6]) -> [f64; 3] {
    [b[3] - b[0], b[4] - b[1], b[5] - b[2]]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let store = Store::new(args.next().expect("a document store"));
    let raw = std::path::PathBuf::from(args.next().expect("the raw dir"));
    let did = args.next().expect("an Onshape document id");
    let eid = args.next().expect("an assembly element id");
    let asm_id = cadrs_core::ElementId::from_u128(cadrs_onshape::ids::stable_u128(&[&did, &eid]));
    let (lib, errors) = store.list();
    for (path, e) in &errors {
        eprintln!("{}: {e}", path.display());
    }
    let doc = lib
        .entries
        .iter()
        .filter_map(|e| store.load(e.id).inspect_err(|err| eprintln!("{}: {err}", e.id)).ok())
        .map(|f| f.document)
        .find(|d| d.element(asm_id).is_some())
        .expect("no imported document holds that assembly");
    let asm = doc.element(asm_id).and_then(|e| e.assembly_model()).expect("an assembly");
    let def = read(&raw.join(&did).join(&eid).join("definition.json")).expect("definition.json");
    let root = &def["rootAssembly"];
    // Onshape: path → world transform; instance trees by (document, element).
    let mut world: HashMap<Vec<String>, cadrs_core::assembly::Pose> = HashMap::new();
    for o in root["occurrences"].as_array().into_iter().flatten() {
        let path: Vec<String> = o["path"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
        if let Some(p) = pose_of(&o["transform"]) {
            world.insert(path, p);
        }
    }
    let subs: HashMap<(String, String), Vec<Value>> = def["subAssemblies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| Some(((x["documentId"].as_str()?.to_string(), x["elementId"].as_str()?.to_string()), x["instances"].as_array().cloned().unwrap_or_default())))
        .collect();
    // cadrs: occurrence id → (element, part, pose).
    let occ: HashMap<_, _> = occurrences(&doc, asm).into_iter().map(|o| (o.id, o)).collect();
    let mut builds: HashMap<cadrs_core::ElementId, std::sync::Arc<cadrs_core::rebuild::Build>> = HashMap::new();
    let mut details: HashMap<(String, String), Option<Value>> = HashMap::new();
    // Walk Onshape's instance tree: (path, cadrs ids chain, instance).
    let mut stack: Vec<(Vec<String>, Vec<cadrs_core::assembly::InstanceId>, Value)> = root["instances"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            let id = i["id"].as_str().unwrap_or_default().to_string();
            (vec![id.clone()], vec![instance_id(&did, &eid, &id)], i.clone())
        })
        .collect();
    let (mut checked, mut bad, mut missing) = (0, 0, 0);
    while let Some((path, chain, inst)) = stack.pop() {
        let (d, e) = (inst["documentId"].as_str().unwrap_or_default().to_string(), inst["elementId"].as_str().unwrap_or_default().to_string());
        if inst["type"].as_str() == Some("Assembly") {
            for c in subs.get(&(d.clone(), e.clone())).into_iter().flatten() {
                let cid = c["id"].as_str().unwrap_or_default().to_string();
                let mut p = path.clone();
                p.push(cid.clone());
                let mut ch = chain.clone();
                ch.push(instance_id(&d, &e, &cid));
                stack.push((p, ch, c.clone()));
            }
            continue;
        }
        if inst["suppressed"].as_bool() == Some(true) {
            continue;
        }
        let names: Vec<String> = {
            // Names along the path, for the report.
            let mut out = Vec::new();
            let mut cur: Vec<Value> = root["instances"].as_array().cloned().unwrap_or_default();
            for p in &path {
                let Some(x) = cur.iter().find(|x| x["id"].as_str() == Some(p)).cloned() else { break };
                out.push(x["name"].as_str().unwrap_or_default().to_string());
                cur = subs.get(&(x["documentId"].as_str().unwrap_or_default().to_string(), x["elementId"].as_str().unwrap_or_default().to_string())).cloned().unwrap_or_default();
            }
            out
        };
        let label = names.join(" / ");
        // The cadrs occurrence id: derive(top, derive(a, derive(b, …))).
        let mut ids = chain.clone();
        let mut acc = ids.pop().unwrap();
        while let Some(x) = ids.pop() {
            acc = derive(x, acc);
        }
        let Some(o) = occ.get(&acc) else {
            missing += 1;
            println!("MISSING  {label}");
            continue;
        };
        let Some(ow) = world.get(&path) else { continue };
        let det = details.entry((d.clone(), e.clone())).or_insert_with(|| read(&raw.join(&d).join(&e).join("bodydetails.json")));
        let pid = inst["partId"].as_str().unwrap_or_default();
        let Some(body) = det.as_ref().and_then(|v| v["bodies"].as_array()?.iter().find(|b| b["id"].as_str() == Some(pid)).cloned()) else {
            println!("NO-ONSHAPE-BODY  {label}");
            continue;
        };
        let ob = bbox(body["vertices"].as_array().into_iter().flatten().filter_map(|v| {
            let p = &v["point"];
            Some(ow.apply([p["x"].as_f64()? * 1000.0, p["y"].as_f64()? * 1000.0, p["z"].as_f64()? * 1000.0]))
        }));
        let build = builds.entry(o.element).or_insert_with(|| cadrs_core::rebuild::build(doc.element(o.element).map(|e| e.features()).unwrap_or_default()));
        let Some(part) = build.part(o.part) else {
            missing += 1;
            for f in doc.element(o.element).map(|e| e.features()).unwrap_or_default() {
                if let cadrs_core::document::FeatureKind::Derived(x) = &f.kind {
                    let sb = cadrs_core::rebuild::build(&x.studio);
                    println!("  derived {}: its source builds {} parts, errors {:?}", f.name, sb.parts.len(), sb.errors);
                }
            }
            println!("NO-CADRS-PART  {label}: wanted {:?}, the studio has {:?}, errors {:?}", o.part, build.parts.iter().map(|p| p.id).collect::<Vec<_>>(), build.errors);
            continue;
        };
        let cb = bbox(part.solid.vertices.iter().map(|v| o.pose.apply(v.point)));
        // In the parts' own coordinates (their Part Studios').
        let ol = bbox(body["vertices"].as_array().into_iter().flatten().filter_map(|v| {
            let p = &v["point"];
            Some([p["x"].as_f64()? * 1000.0, p["y"].as_f64()? * 1000.0, p["z"].as_f64()? * 1000.0])
        }));
        let cl = bbox(part.solid.vertices.iter().map(|v| v.point));
        let (Some(ob), Some(cb)) = (ob, cb) else { continue };
        checked += 1;
        let (oc, cc) = (centre(&ob), centre(&cb));
        let dc = ((oc[0] - cc[0]).powi(2) + (oc[1] - cc[1]).powi(2) + (oc[2] - cc[2]).powi(2)).sqrt();
        let (os, cs) = (size(&ob), size(&cb));
        let ds = (0..3).map(|k| (os[k] - cs[k]).abs()).fold(0.0, f64::max);
        if dc > 0.5 || ds > 0.5 {
            bad += 1;
            println!(
                "OFF {dc:8.2} mm  size Δ {ds:6.2}  {label}  [{}]\n    onshape centre {:.2?} size {:.2?}\n    cadrs   centre {:.2?} size {:.2?}  (cadrs part {})\n    local: onshape centre {:.2?} size {:.2?}, cadrs centre {:.2?} size {:.2?}  studio {d}/{e}",
                pid,
                oc,
                os,
                cc,
                cs,
                part.name,
                ol.map(|b| centre(&b)),
                ol.map(|b| size(&b)),
                cl.map(|b| centre(&b)),
                cl.map(|b| size(&b))
            );
        }
    }
    println!("{checked} parts checked, {bad} off, {missing} missing");
    // The assembly's own mate connectors: Onshape's frame origin (its occurrence's transform on
    // the connector's coordinate system) against cadrs's (the occurrence pose on its frame).
    let solids = cadrs_core::assembly::document_occurrence_solids(&doc, asm_id);
    for f in root["features"].as_array().into_iter().flatten().filter(|f| f["featureType"].as_str() == Some("mateConnector")) {
        let data = &f["featureData"];
        let name = data["name"].as_str().unwrap_or_default();
        let path: Vec<String> = data["occurrence"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
        let o = &data["mateConnectorCS"]["origin"];
        let local = [o[0].as_f64().unwrap_or(0.0) * 1000.0, o[1].as_f64().unwrap_or(0.0) * 1000.0, o[2].as_f64().unwrap_or(0.0) * 1000.0];
        let onshape = world.get(&path).map(|w| w.apply(local));
        let cadrs = asm.connectors.iter().find(|c| c.name == name).and_then(|c| {
            let occ = occ.get(&c.connector.instance)?;
            let frame = c.connector.local_frame(solids.get(&c.connector.instance).map(|s| &**s));
            Some(occ.pose.apply(frame.origin))
        });
        let d = match (onshape, cadrs) {
            (Some(a), Some(b)) => format!("{:.2} mm", ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()),
            _ => "-".into(),
        };
        println!("CONNECTOR {name}: off by {d}; onshape {onshape:.2?}, cadrs {cadrs:.2?}");
    }
}
