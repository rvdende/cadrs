//! A localhost job server for the Onshape tab: it hands out batches of translated cases, the tab
//! runs each through evalFeatureScript with the person's own session, and posts the response
//! back; results land in each case's directory as parasolid.json.
//!
//! In a cad.onshape.com tab's console (Chrome asks once to allow local network access):
//!   eval(await (await fetch('http://127.0.0.1:8766/client.js', {targetAddressSpace: 'loopback'})).text())
//!
//! Onshape limits session requests (a 429 with a Retry-After of about a day after ~1,280 requests
//! on 2026-09-29), so batches are several cases each and the client stops at the first 429. Every
//! request is logged in <dir>/onshape-requests.jsonl.

use crate::{CaseSet, emit, flag};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Onshape's typed FeatureScript values (BTFSValue*) as plain JSON.
pub fn unfs(v: &Value) -> Value {
    let t = v.get("btType").and_then(|t| t.as_str()).unwrap_or("");
    if t.contains("Map") && !t.contains("MapEntry") {
        let mut m = serde_json::Map::new();
        for e in v["value"].as_array().into_iter().flatten() {
            let key = match unfs(&e["key"]) {
                Value::String(s) => s,
                other => other.to_string(),
            };
            m.insert(key, unfs(&e["value"]));
        }
        return Value::Object(m);
    }
    if t.contains("Array") {
        return Value::Array(
            v["value"]
                .as_array()
                .into_iter()
                .flatten()
                .map(unfs)
                .collect(),
        );
    }
    if t.contains("Undefined") {
        return Value::Null;
    }
    v.get("value").cloned().unwrap_or(Value::Null)
}

/// Integral floats back to integers (FeatureScript numbers are all floats).
fn tidy(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 && !n.is_i64() => json!(f as i64),
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(tidy).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, tidy(v))).collect()),
        o => o,
    }
}

struct Job {
    id: usize,
    cases: Vec<i64>,
    script: String,
}

struct State {
    queue: VecDeque<Job>,
    inflight: BTreeMap<usize, (Job, Instant)>,
    stop: bool,
    names: BTreeMap<i64, (String, Vec<i64>)>,
    dir: PathBuf,
    set: PathBuf,
    next_job: usize,
}

/// How many failures a case may be replayed with (each one a step Parasolid failed at inside
/// the generator's own `try`) before it is left as it is.
const MAX_REPLAYS: usize = 10;

const CLIENT_JS: &str = r#"
(async () => {
  const S = 'http://127.0.0.1:PORT';
  const opt = {targetAddressSpace: 'loopback'};
  const xs = document.cookie.split(';').map(c => c.trim()).find(c => /^XSRF-TOKEN=/i.test(c));
  const H = {Accept: 'application/json', 'Content-Type': 'application/json', 'X-XSRF-TOKEN': decodeURIComponent(xs.split('=').slice(1).join('='))};
  const cfg = await (await fetch(S + '/config', opt)).json();
  const dj = await (await fetch('/api/v10/documents?filter=0&limit=20&sortColumn=modifiedAt&sortOrder=desc', {headers: H})).json();
  const d = (dj.items || []).find(x => x.name === cfg.hostDocument);
  if (!d) { console.log('[cq2fs] no document named', cfg.hostDocument); return; }
  const els = await (await fetch(`/api/v10/documents/d/${d.id}/w/${d.defaultWorkspace.id}/elements?elementType=PARTSTUDIO`, {headers: H})).json();
  const url = `/api/v10/partstudios/d/${d.id}/w/${d.defaultWorkspace.id}/e/${els[0].id}/featurescript`;
  window.cq2fs = {running: true, jobs: 0, last: null};
  console.log('[cq2fs] running against', cfg.hostDocument);
  while (window.cq2fs.running) {
    const job = await (await fetch(S + '/job', opt)).json();
    if (job.done) { console.log('[cq2fs] done'); break; }
    const t0 = Date.now();
    let status = 0, text = '';
    try {
      const r = await fetch(url, {method: 'POST', headers: H, credentials: 'include', body: JSON.stringify({script: job.script, queries: {}})});
      status = r.status; text = await r.text();
    } catch (e) { status = -1; text = String(e); }
    const ack = await (await fetch(S + '/result?id=' + job.id + '&ms=' + (Date.now() - t0), {...opt, method: 'POST', body: JSON.stringify({status, body: text})})).json();
    window.cq2fs.jobs += 1; window.cq2fs.last = {id: job.id, status, ms: Date.now() - t0, left: ack.left};
    console.log('[cq2fs] job', job.id, 'status', status, (Date.now() - t0) + ' ms', ack.left + ' left');
    if (ack.stop) { console.log('[cq2fs] stopping:', ack.why); break; }
    await new Promise(res => setTimeout(res, cfg.delayMs));
  }
  window.cq2fs.running = false;
})();
"#;

fn respond(mut s: TcpStream, status: &str, ctype: &str, body: &str) {
    let _ = write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Private-Network: true\r\nAccess-Control-Allow-Headers: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn record_result(st: &mut State, job: &Job, status: i64, text: &str, now: u64) -> Result<()> {
    let resp: Value = serde_json::from_str(text).unwrap_or(json!({}));
    let res = unfs(&resp["result"]);
    let notices: Vec<Value> = resp["notices"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| json!({"level": n["level"], "msg": n["message"]["message"].as_str().map(String::from).unwrap_or_else(|| n["message"].to_string())}))
        .collect();
    for i in &job.cases {
        let (name, guarded) = &st.names[i];
        let mut r = res
            .get(format!("c{i}"))
            .cloned()
            .unwrap_or_else(|| json!({"status": "eval-error", "http": status, "notices": notices}));
        if let Value::Object(m) = &mut r {
            m.insert(
                "source".into(),
                json!("Onshape evalFeatureScript (Parasolid), case.fs"),
            );
            m.insert("evaluated".into(), json!(now));
            m.insert("guarded_steps".into(), json!(guarded));
        }
        let dir = st.dir.join(format!("{i}_{name}"));
        // Parasolid failed inside the generator's own `try`: CadQuery's caller would have
        // caught it there, so replay the generator with that call raising and run the path its
        // handler takes (the recorder's CQFailure; trace.py --fail).
        let at = r["at"].as_f64().map(|x| x as i64);
        let err = r["err"].as_str().unwrap_or("").to_string();
        if r["status"] == "error"
            && let Some(at) = at
            && !err.contains("cq-unsupported")
            && let Ok(tape) = std::fs::read_to_string(dir.join("tape.json"))
                .map(|t| serde_json::from_str::<Value>(&t).unwrap_or(json!({})))
            && tape["entries"][(at - 1) as usize]["guarded"] == true
        {
            let mut fails: Vec<i64> = tape["fail_at"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
                .unwrap_or_default();
            if fails.len() < MAX_REPLAYS && !fails.contains(&at) {
                fails.push(at);
                let set = CaseSet::load(&st.set)?;
                crate::record_cases(&set, &[*i], &BTreeMap::from([(*i, fails.clone())]))?;
                let rec = set.tape(*i, name)?;
                eprintln!(
                    "case {i}: Parasolid failed at guarded step {at} ({err}); replaying with failures at {fails:?}"
                );
                match emit::translate(&rec) {
                    Ok(t) => {
                        std::fs::write(dir.join("case.fs"), emit::batch(&[&t]))?;
                        let id = st.next_job;
                        st.next_job += 1;
                        st.names.insert(*i, (name.clone(), t.guarded.clone()));
                        st.queue.push_back(Job {
                            id,
                            cases: vec![*i],
                            script: emit::batch(&[&t]),
                        });
                        continue;
                    }
                    Err(e) => {
                        r = json!({"status": "untranslated", "err": e, "after_replaying": fails});
                    }
                }
            }
        }
        std::fs::write(
            dir.join("parasolid.json"),
            serde_json::to_string_pretty(&tidy(r))? + "\n",
        )?;
    }
    Ok(())
}

fn handle(
    stream: TcpStream,
    state: &Arc<Mutex<State>>,
    port: u16,
    host: &str,
    delay: u64,
) -> Result<()> {
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
    let body = String::from_utf8_lossy(&body).to_string();
    if method == "OPTIONS" {
        respond(stream, "204 No Content", "text/plain", "");
    } else if url.starts_with("/client.js") {
        respond(
            stream,
            "200 OK",
            "text/javascript",
            &CLIENT_JS.replace("PORT", &port.to_string()),
        );
    } else if url.starts_with("/config") {
        respond(
            stream,
            "200 OK",
            "application/json",
            &json!({"hostDocument": host, "delayMs": delay}).to_string(),
        );
    } else if url.starts_with("/job") {
        let mut st = state.lock().unwrap();
        let stale: Vec<usize> = st
            .inflight
            .iter()
            .filter(|(_, (_, t))| t.elapsed().as_secs() > 900)
            .map(|(k, _)| *k)
            .collect();
        for k in stale {
            let (j, _) = st.inflight.remove(&k).unwrap();
            st.queue.push_back(j);
        }
        let v = if st.stop {
            json!({"done": true})
        } else if let Some(j) = st.queue.pop_front() {
            let v = json!({"id": j.id, "cases": j.cases, "script": j.script});
            st.inflight.insert(j.id, (j, Instant::now()));
            v
        } else {
            json!({"done": true})
        };
        drop(st);
        respond(stream, "200 OK", "application/json", &v.to_string());
    } else if url.starts_with("/result") {
        let id: usize = url
            .split("id=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(usize::MAX);
        let ms: u64 = url
            .split("ms=")
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let posted: Value = serde_json::from_str(&body).unwrap_or(json!({}));
        let status = posted["status"].as_i64().unwrap_or(0);
        let text = posted["body"].as_str().unwrap_or("");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut st = state.lock().unwrap();
        let (mut stop, mut why) = (false, String::new());
        if let Some((job, _)) = st.inflight.remove(&id) {
            let mut logline = json!({"t": now, "set": st.set.file_name().map(|s| s.to_string_lossy().to_string()), "job": id, "cases": job.cases, "status": status, "ms": ms, "bytes": text.len()});
            if status == 200 {
                record_result(&mut st, &job, status, text, now)?;
            } else {
                logline["text"] = json!(text.chars().take(400).collect::<String>());
                if status == 429 || status == 401 || status == 403 {
                    stop = true;
                    st.stop = true;
                    why = format!("HTTP {status}");
                }
                st.queue.push_front(job);
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(st.dir.join("onshape-requests.jsonl"))?;
            writeln!(f, "{logline}")?;
            eprintln!(
                "job {id}: status {status}, {ms} ms; {} jobs left",
                st.queue.len() + st.inflight.len()
            );
            if st.queue.is_empty() && st.inflight.is_empty() {
                eprintln!("all jobs answered");
            }
        }
        let left = st.queue.len() + st.inflight.len();
        drop(st);
        respond(
            stream,
            "200 OK",
            "application/json",
            &json!({"ok": true, "stop": stop, "why": why, "left": left}).to_string(),
        );
    } else {
        respond(stream, "404 Not Found", "text/plain", "not found");
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let set = CaseSet::load(&PathBuf::from(args.first().context("set.json")?))?;
    let port: u16 = flag(args, "--port")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(8766);
    let batch: usize = flag(args, "--batch")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(4);
    let host = flag(args, "--host").unwrap_or_else(|| "test_shell".to_string());
    let delay: u64 = flag(args, "--delay")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(2000);
    let rerun = args.iter().any(|a| a == "--rerun");
    let rerun_errors = args.iter().any(|a| a == "--rerun-errors");
    let only: Option<Vec<i64>> =
        flag(args, "--only").map(|s| s.split(',').map(|x| x.parse().unwrap()).collect());

    let mut todo = vec![];
    let mut translated = BTreeMap::new();
    let mut names = BTreeMap::new();
    for (i, name) in &set.cases {
        if only.as_ref().is_some_and(|o| !o.contains(i)) {
            continue;
        }
        let dir = set.case_dir(*i, name);
        let rec = set.tape(*i, name)?;
        match emit::translate(&rec) {
            Ok(t) => {
                std::fs::write(dir.join("case.fs"), emit::batch(&[&t]))?;
                let prev: Option<Value> = std::fs::read_to_string(dir.join("parasolid.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());
                let errored = prev.as_ref().is_some_and(|p| p["status"] != "ok");
                if rerun || prev.is_none() || (rerun_errors && errored) {
                    todo.push(*i);
                }
                names.insert(*i, (name.clone(), t.guarded.clone()));
                translated.insert(*i, t);
            }
            Err(e) => {
                eprintln!("case {i} {name}: not translated: {e}");
                std::fs::remove_file(dir.join("case.fs")).ok();
                std::fs::write(
                    dir.join("parasolid.json"),
                    serde_json::to_string_pretty(&json!({"status": "untranslated", "err": e}))?
                        + "\n",
                )?;
            }
        }
    }
    let mut queue = VecDeque::new();
    let mut next_job = 0;
    for (n, chunk) in todo.chunks(batch.max(1)).enumerate() {
        next_job = n + 1;
        let ts: Vec<&emit::Translated> = chunk.iter().map(|i| &translated[i]).collect();
        queue.push_back(Job {
            id: n,
            cases: chunk.to_vec(),
            script: emit::batch(&ts),
        });
    }
    eprintln!(
        "cq2fs serve: {} cases in {} jobs on 127.0.0.1:{port}; host document {host:?}",
        todo.len(),
        queue.len()
    );
    eprintln!(
        "  in a cad.onshape.com tab: eval(await (await fetch('http://127.0.0.1:{port}/client.js', {{targetAddressSpace: 'loopback'}})).text())"
    );
    let state = Arc::new(Mutex::new(State {
        queue,
        inflight: BTreeMap::new(),
        stop: false,
        names,
        dir: set.dir.clone(),
        set: set.path.clone(),
        next_job,
    }));
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let state = state.clone();
        let host = host.clone();
        std::thread::spawn(move || {
            if let Err(e) = handle(stream, &state, port, &host, delay) {
                eprintln!("request failed: {e:#}");
            }
        });
    }
    Ok(())
}
