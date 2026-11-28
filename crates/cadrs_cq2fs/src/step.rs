//! STEP (and Parasolid) files of Onshape's own builds of a case set.
//!
//!   cq2fs step <set.json> --doc <document url> [--batch 10] [--port 8766] [--only i,j] [--rerun]
//!   cq2fs step <set.json> --doc <document url> --verify   (each STEP imported back and measured)
//!
//! In the document (a Part Studio holds solids, which evalFeatureScript's scratch context can't
//! export), each batch of cases gets a Feature Studio "cq2fs <set> <k>" with the runtime, the
//! cases and a generator feature (emit::feature_studio), and a Part Studio of the same name with
//! that feature in it: every case built, the rest of what it made deleted, its result moved
//! `emit::SLOT` mm further along X than the case before and its parts named `c<i>.<n>`. The tab
//! then exports the Part Studio as STEP (AP242) and as Parasolid, and asks Onshape for each
//! part's mass properties; all of it lands in `<dir>/step/<set>-<k>.*`. As featurescript-engine
//! does it: one feature builds the batch, one export saves it.
//!
//! In the cad.onshape.com tab's console:
//!   eval(await (await fetch('http://127.0.0.1:8766/step.js', {targetAddressSpace: 'loopback'})).text())

use crate::{CaseSet, emit, flag};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const STEP_JS: &str = r#"
(async () => {
  const S = 'http://127.0.0.1:PORT';
  const opt = {targetAddressSpace: 'loopback'};
  const xs = document.cookie.split(';').map(c => c.trim()).find(c => /^XSRF-TOKEN=/i.test(c));
  const H = {Accept: 'application/json', 'Content-Type': 'application/json', 'X-XSRF-TOKEN': decodeURIComponent(xs.split('=').slice(1).join('='))};
  const api = async (method, path, body) => {
    const r = await fetch(path, {method, headers: H, credentials: 'include', body: body === undefined ? undefined : JSON.stringify(body)});
    const t = await r.text();
    if (!r.ok) throw new Error(method + ' ' + path + ' -> ' + r.status + ' ' + t.slice(0, 300));
    try { return JSON.parse(t); } catch (e) { return t; }
  };
  const post = (what, body) => fetch(S + what, {...opt, method: 'POST', body: JSON.stringify(body)});
  const job = await (await fetch(S + '/stepjob', opt)).json();
  const {did, wid} = job;
  const D = `d/${did}/w/${wid}`;
  window.cq2fsStep = {batch: null, done: 0, error: null};
  const els = async () => await api('GET', `/api/v10/documents/${D}/elements`);
  const wait = ms => new Promise(r => setTimeout(r, ms));
  for (const b of job.batches) {
    window.cq2fsStep.batch = b.k;
    try {
      const name = `cq2fs ${job.set} ${b.k}`;
      let list = await els();
      let fs = list.find(e => e.name === name && e.elementType === 'FEATURESTUDIO');
      if (!fs) fs = await api('POST', `/api/v10/featurestudios/${D}`, {name});
      const cur = await api('GET', `/api/v10/featurestudios/${D}/e/${fs.id}`);
      const version = (cur.contents.match(/FeatureScript (\d+);/) || [])[1];
      const src = await (await fetch(S + `/studio?k=${b.k}&version=${version}`, opt)).text();
      const up = await api('POST', `/api/v10/featurestudios/${D}/e/${fs.id}`, {contents: src, serializationVersion: cur.serializationVersion, sourceMicroversion: cur.sourceMicroversion, rejectMicroversionSkew: false});
      const errors = (up.notices || []).filter(n => n.level === 'ERROR').map(n => (n.message && n.message.message) || JSON.stringify(n).slice(0, 300));
      if (errors.length) { await post(`/stepresult?k=${b.k}`, {error: 'feature studio: ' + errors.slice(0, 5).join(' | ')}); continue; }
      const specs = await api('GET', `/api/v10/featurestudios/${D}/e/${fs.id}/featurespecs`);
      const spec = (specs.featureSpecs || []).find(f => f.featureType === 'cq2fsCases');
      if (!spec) { await post(`/stepresult?k=${b.k}`, {error: 'no cq2fsCases feature spec', specs: JSON.stringify(specs).slice(0, 500)}); continue; }
      list = await els();
      let ps = list.find(e => e.name === name && e.elementType === 'PARTSTUDIO');
      if (!ps) ps = await api('POST', `/api/v10/partstudios/${D}`, {name});
      const feats = await api('GET', `/api/v10/partstudios/${D}/e/${ps.id}/features`);
      for (const f of (feats.features || [])) await api('DELETE', `/api/v10/partstudios/${D}/e/${ps.id}/features/featureid/${f.featureId}`);
      const added = await api('POST', `/api/v10/partstudios/${D}/e/${ps.id}/features`, {feature: {btType: 'BTMFeature-134', featureType: 'cq2fsCases', name: 'cq2fs cases', namespace: spec.namespace,
        parameters: [{btType: 'BTMParameterString-149', parameterId: 'cases', value: b.cases.join(',')}]}});
      const state = added.featureState || {};
      const exportAs = async (formatName, extra) => {
        const t = await api('POST', `/api/v10/partstudios/${D}/e/${ps.id}/translations`, {formatName, storeInDocument: false, ...extra});
        let st = t;
        for (let n = 0; n < 240 && st.requestState === 'ACTIVE'; n++) { await wait(2500); st = await api('GET', `/api/v10/translations/${t.id}`); }
        if (st.requestState !== 'DONE') throw new Error(formatName + ' translation ' + st.requestState + ' ' + (st.failureReason || ''));
        const r = await fetch(`/api/v10/documents/d/${did}/externaldata/${st.resultExternalDataIds[0]}`, {headers: H, credentials: 'include'});
        return await r.text();
      };
      const step = await exportAs('STEP', {stepVersionString: 'AP242'});
      const xt = await exportAs('PARASOLID', {});
      const parts = await api('GET', `/api/v10/parts/${D}/e/${ps.id}`);
      const mass = await api('GET', `/api/v10/partstudios/${D}/e/${ps.id}/massproperties?massAsGroup=false`);
      await post(`/stepresult?k=${b.k}`, {featureState: state, step, xt,
        parts: parts.map(p => ({partId: p.partId, name: p.name})), mass: mass.bodies || mass});
      window.cq2fsStep.done += 1;
      console.log('[cq2fs step] batch', b.k, 'done');
    } catch (e) {
      window.cq2fsStep.error = String(e);
      await post(`/stepresult?k=${b.k}`, {error: String(e)});
      if (/ 429 /.test(String(e))) break;
    }
    await wait(1500);
  }
  await fetch(S + '/stepdone', opt);
  window.cq2fsStep.batch = 'done';
})();
"#;

/// The STEP files checked by Onshape itself (`--verify`): each batch's STEP imported into the
/// document as a new Part Studio and every part measured there by Parasolid, so the file is
/// held to what evalFeatureScript built by a reader that isn't cadrs's.
const VERIFY_JS: &str = r#"
(async () => {
  const S = 'http://127.0.0.1:PORT';
  const opt = {targetAddressSpace: 'loopback'};
  const xs = document.cookie.split(';').map(c => c.trim()).find(c => /^XSRF-TOKEN=/i.test(c));
  const X = decodeURIComponent(xs.split('=').slice(1).join('='));
  const H = {Accept: 'application/json', 'Content-Type': 'application/json', 'X-XSRF-TOKEN': X};
  const api = async (method, path, body) => {
    const r = await fetch(path, {method, headers: H, credentials: 'include', body: body === undefined ? undefined : JSON.stringify(body)});
    const t = await r.text();
    if (!r.ok) throw new Error(method + ' ' + path + ' -> ' + r.status + ' ' + t.slice(0, 300));
    try { return JSON.parse(t); } catch (e) { return t; }
  };
  const post = (what, body) => fetch(S + what, {...opt, method: 'POST', body: JSON.stringify(body)});
  const wait = ms => new Promise(r => setTimeout(r, ms));
  const job = await (await fetch(S + '/verifyjob', opt)).json();
  const {did, wid} = job;
  const D = `d/${did}/w/${wid}`;
  window.cq2fsVerify = {batch: null, done: 0, error: null};
  const measure = `function(context is Context, queries) {
    var out = [];
    for (var b in evaluateQuery(context, qAllModifiableSolidBodies()))
      out = append(out, {"name": getProperty(context, {"entity": b, "propertyType": PropertyType.NAME}),
                         "vol": evVolume(context, {"entities": b, "accuracy": "HIGH"}) / millimeter ^ 3});
    return out;
  }`;
  const unfs = (v) => { if (!v) return v; const t = v.btType || ''; if (t.includes('Map')) { const o = {}; for (const e of v.value) o[unfs(e.key)] = unfs(e.value); return o; } if (t.includes('Array')) return v.value.map(unfs); return v.value; };
  for (const b of job.batches) {
    window.cq2fsVerify.batch = b.k;
    try {
      const text = await (await fetch(S + `/stepfile?k=${b.k}`, opt)).text();
      const form = new FormData();
      form.append('file', new Blob([text], {type: 'application/octet-stream'}), `cq2fs ${job.set} ${b.k} reimport.step`);
      form.append('translate', 'true');
      form.append('flattenAssemblies', 'true');
      form.append('allowFaultyParts', 'true');
      const r = await fetch(`/api/v10/translations/${D}`, {method: 'POST', headers: {Accept: 'application/json', 'X-XSRF-TOKEN': X}, credentials: 'include', body: form});
      const t = await r.json();
      if (!r.ok) throw new Error('import -> ' + r.status + ' ' + JSON.stringify(t).slice(0, 300));
      let st = t;
      for (let n = 0; n < 240 && st.requestState === 'ACTIVE'; n++) { await wait(2500); st = await api('GET', `/api/v10/translations/${t.id}`); }
      if (st.requestState !== 'DONE') throw new Error('import ' + st.requestState + ' ' + (st.failureReason || ''));
      const els = await api('GET', `/api/v10/documents/${D}/elements`);
      const ps = els.find(e => (st.resultElementIds || []).includes(e.id) && e.elementType === 'PARTSTUDIO');
      if (!ps) throw new Error('no Part Studio among ' + JSON.stringify(st.resultElementIds));
      const m = await api('POST', `/api/v10/partstudios/${D}/e/${ps.id}/featurescript`, {script: measure, queries: {}});
      await post(`/verifyresult?k=${b.k}`, {element: ps.id, parts: unfs(m.result)});
      window.cq2fsVerify.done += 1;
    } catch (e) {
      window.cq2fsVerify.error = String(e);
      await post(`/verifyresult?k=${b.k}`, {error: String(e)});
      if (/ 429 /.test(String(e))) break;
    }
    await wait(1500);
  }
  await fetch(S + '/stepdone', opt);
  window.cq2fsVerify.batch = 'done';
})();
"#;

/// The document and workspace ids of an Onshape document URL.
fn doc_ids(url: &str) -> Result<(String, String)> {
    let part = |key: &str| -> Option<String> {
        url.split(&format!("/{key}/"))
            .nth(1)
            .map(|s| s.split(['/', '?']).next().unwrap_or("").to_string())
    };
    let did = part("documents").context("no /documents/<id> in the URL")?;
    let wid = part("w").context("no /w/<workspace> in the URL")?;
    Ok((did, wid))
}

pub fn run(args: &[String]) -> Result<()> {
    let set = CaseSet::load(&PathBuf::from(args.first().context("set.json")?))?;
    let (did, wid) = doc_ids(&flag(args, "--doc").context("--doc <document url>")?)?;
    let port: u16 = flag(args, "--port")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(8766);
    let size: usize = flag(args, "--batch")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(10);
    let rerun = args.iter().any(|a| a == "--rerun");
    let verify = args.iter().any(|a| a == "--verify");
    let only: Option<Vec<i64>> =
        flag(args, "--only").map(|s| s.split(',').map(|x| x.parse().unwrap()).collect());
    let set_name = set.path.file_stem().unwrap().to_string_lossy().to_string();
    let out_dir = set.dir.join("step");
    std::fs::create_dir_all(&out_dir)?;
    // the cases Onshape built in evalFeatureScript, in the set's order, in batches
    let mut cases = Vec::new();
    for (i, name) in &set.cases {
        if only.as_ref().is_some_and(|o| !o.contains(i)) {
            continue;
        }
        let p: Value = std::fs::read_to_string(set.case_dir(*i, name).join("parasolid.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({}));
        if p["status"] != "ok" {
            continue;
        }
        let t = emit::translate(&set.tape(*i, name)?).map_err(anyhow::Error::msg)?;
        cases.push(t);
    }
    let mut batches: Vec<(usize, Vec<emit::Translated>)> = Vec::new();
    let mut it = cases.into_iter().peekable();
    let mut k = 0;
    while it.peek().is_some() {
        let b: Vec<emit::Translated> = it.by_ref().take(size).collect();
        let have_step = out_dir.join(format!("{set_name}-{k}.step")).exists();
        let have_check = out_dir
            .join(format!("{set_name}-{k}.reimport.json"))
            .exists();
        if (verify && have_step && (rerun || !have_check)) || (!verify && (rerun || !have_step)) {
            batches.push((k, b));
        }
        k += 1;
    }
    if batches.is_empty() {
        eprintln!("every batch has its STEP already (--rerun makes them again)");
        return Ok(());
    }
    let job = json!({"did": did, "wid": wid, "set": set_name,
        "batches": batches.iter().map(|(k, b)| json!({"k": k, "cases": b.iter().map(|t| t.i).collect::<Vec<_>>()})).collect::<Vec<_>>()});
    eprintln!(
        "cq2fs step: {} batches of up to {size} for document {did}",
        batches.len()
    );
    let script = if verify { "verify.js" } else { "step.js" };
    eprintln!(
        "  in a cad.onshape.com tab: eval(await (await fetch('http://127.0.0.1:{port}/{script}', {{targetAddressSpace: 'loopback'}})).text())"
    );
    let batches = Arc::new(batches);
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let finished = Arc::new(Mutex::new(false));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let mut rd = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        rd.read_line(&mut line)?;
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let url = parts.next().unwrap_or("").to_string();
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if rd.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0u8; len];
        rd.read_exact(&mut body)?;
        let q = |key: &str| {
            url.split(&format!("{key}="))
                .nth(1)
                .map(|s| s.split('&').next().unwrap_or("").to_string())
        };
        let (ctype, text) = if method == "OPTIONS" {
            ("text/plain", String::new())
        } else if url.starts_with("/step.js") {
            (
                "text/javascript",
                STEP_JS.replace("PORT", &port.to_string()),
            )
        } else if url.starts_with("/verify.js") {
            (
                "text/javascript",
                VERIFY_JS.replace("PORT", &port.to_string()),
            )
        } else if url.starts_with("/verifyjob") || url.starts_with("/stepjob") {
            ("application/json", job.to_string())
        } else if url.starts_with("/studio") {
            let k: usize = q("k").and_then(|s| s.parse().ok()).unwrap_or(0);
            let version = q("version").unwrap_or_default();
            if !version.chars().all(|c| c.is_ascii_digit()) || version.is_empty() {
                bail!("bad FeatureScript version {version:?}");
            }
            let b = batches
                .iter()
                .find(|(kk, _)| *kk == k)
                .context("no such batch")?;
            let refs: Vec<&emit::Translated> = b.1.iter().collect();
            let src = emit::feature_studio(&refs, &version);
            std::fs::write(out_dir.join(format!("{set_name}-{k}.fs")), &src)?;
            ("text/plain", src)
        } else if url.starts_with("/stepresult") {
            let k: usize = q("k").and_then(|s| s.parse().ok()).unwrap_or(0);
            let mut v: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
            if let Some(step) = v.get("step").and_then(|s| s.as_str()) {
                std::fs::write(out_dir.join(format!("{set_name}-{k}.step")), step)?;
            }
            if let Some(xt) = v.get("xt").and_then(|s| s.as_str()) {
                std::fs::write(out_dir.join(format!("{set_name}-{k}.x_t")), xt)?;
            }
            if let Value::Object(m) = &mut v {
                m.remove("step");
                m.remove("xt");
                m.insert(
                    "cases".into(),
                    json!(
                        batches.iter().find(|(kk, _)| *kk == k).map(|b| b
                            .1
                            .iter()
                            .map(|t| t.i)
                            .collect::<Vec<_>>())
                    ),
                );
                m.insert("slot_mm".into(), json!(emit::SLOT));
            }
            std::fs::write(
                out_dir.join(format!("{set_name}-{k}.json")),
                serde_json::to_string_pretty(&v)? + "\n",
            )?;
            eprintln!(
                "batch {k}: {}",
                v.get("error").and_then(|e| e.as_str()).unwrap_or("saved")
            );
            ("application/json", json!({"ok": true}).to_string())
        } else if url.starts_with("/stepfile") {
            let k: usize = q("k").and_then(|s| s.parse().ok()).unwrap_or(0);
            (
                "application/octet-stream",
                std::fs::read_to_string(out_dir.join(format!("{set_name}-{k}.step")))?,
            )
        } else if url.starts_with("/verifyresult") {
            let k: usize = q("k").and_then(|s| s.parse().ok()).unwrap_or(0);
            let v: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
            std::fs::write(
                out_dir.join(format!("{set_name}-{k}.reimport.json")),
                serde_json::to_string_pretty(&v)? + "\n",
            )?;
            eprintln!(
                "batch {k} re-imported: {}",
                v.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("measured")
            );
            ("application/json", json!({"ok": true}).to_string())
        } else if url.starts_with("/stepdone") {
            *finished.lock().unwrap() = true;
            ("application/json", json!({"ok": true}).to_string())
        } else {
            ("text/plain", "not found".into())
        };
        let mut s = stream;
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Private-Network: true\r\nAccess-Control-Allow-Headers: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nConnection: close\r\n\r\n{text}",
            text.len()
        );
        if *finished.lock().unwrap() {
            eprintln!("all batches done");
            return Ok(());
        }
    }
    Ok(())
}
