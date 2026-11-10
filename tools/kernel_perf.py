#!/usr/bin/env python3
"""Times the native kernel against OCCT 7.8 on the same operations and inputs.

    tools/kernel_perf.py [--out DIR] [--reps N] [--timeout SECS] [--only CAT,...] [--report-only]
                         [--known] [--occt-bench EXE] [--commit SHA]

1. Builds `crates/cadrs_kernel/examples/kernel_bench.rs` and the conformance test binary
   (release, `$CARGO_TARGET_DIR`), and `tools/occt_bench` against the static OCCT
   (`$OCCT`, see tools/occt_bench/build.sh).
2. Writes the cases (`<out>/cases.txt`):
   - SMLib's fillet, boolean and offset cases that pass (`tests/smlib_<suite>.rs`);
   - tessellation and mass properties of the largest SMLib models;
   - STEP import and export of a range of files of OCCT's dataset;
   - with `--known`, the known failures of the same suites too (category `known_<category>`), sampled
     alike: timed to their result or their error, reported apart and kept out of the means.
   `kernel_bench prep` then writes the case files of each side: OCCT reads the SMLib models as
   STEP that the native kernel wrote.
3. Runs each case in its own process (a hung case times out alone), `--reps` runs, medians:
   the native kernel on one thread (`RAYON_NUM_THREADS=1`) and on rayon's default pool, OCCT
   on one thread (as DRAW runs by default). The conformance suite is timed from its test binary
   (`--report-time`) against `bench/occt_conformance_times.txt` (the same cases on the former
   OCCT backend; times of whole tests).
4. Writes `<out>/results.json` and the report `PERF.md`: per case the CPU time (user +
   system, all threads) of each side on one thread and the ratio, per category the geometric
   mean of the ratios over the cases both sides complete; the load average while measuring, and
   the commit measured (`--commit`, else `$CADRS_COMMIT`, else `git rev-parse` of the tree, with
   `+dirty` for a modified one).

CPU time is what's compared: the machine is shared, and under load wall times stretch with
the queue while CPU times don't.
"""

import argparse
import json
import math
import os
import re
import resource
import statistics
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KERNEL = os.path.join(REPO, "crates", "cadrs_kernel")
TESTS = os.path.join(KERNEL, "tests")
SMLIB = os.path.join(TESTS, "data", "smlib")
DATA = os.environ.get("CADRS_OCCT_DATA", os.path.join(REPO, "..", "occt-dataset", "opencascade-dataset-7.8.0"))
OCCT_CONF = os.path.join(REPO, "..", "bench", "occt_conformance_times.txt")

# STEP files of OCCT's dataset, 50 kB to 3.5 MB.
STEP_FILES = [
    "trj6_pm4-ec-203.stp", "r0901_ec.stp", "nist_ftc_08_asme1_ap242-2.stp", "ec_soapbox-A.stp", "ec_turbine-A.stp",
    "as1_motor.step", "trj4_s1-pe-214.stp", "trj3_as1-ec-214.stp", "bug27453_165-174_piattello.stp", "r103ec.stp",
    "bug24595_281390.stp", "trj3_s1-ct-214.stp",
]
TESS_MODELS = 10


def smlib_cases(with_known=False):
    out = []

    def passing(suite, pat):
        """(name, match, category prefix) of the suite's passing cases, and of its known failures
        (`known_failure(…)`, ignored or not) with `with_known`."""
        src = open(os.path.join(TESTS, f"smlib_{suite}.rs")).read()
        for t in re.split(r"\n#\[test\]\n", src)[1:]:
            name = re.search(r"fn (\w+)\(", t).group(1)
            # One line per statement whether or not rustfmt spread the call over several.
            flat = re.sub(r",\s*([)\]}])", r"\1", re.sub(r"([(\[{])\s+", r"\1", re.sub(r"\s*\n\s*", " ", t)))
            m = re.search(r"(?<!known_failure\()" + pat, flat)
            if m and not flat.startswith("known_failure("):
                if not t.lstrip().startswith("#[ignore"):
                    yield name, m, ""
                continue
            m = re.search(r"known_failure\(" + pat, flat)
            if m and with_known:
                yield name, m, "known_"

    for name, m, kn in passing("fillets", r'blend\("([^"]+)", &\[([^\]]*)\], (true|false), Op::Fillet\(([-0-9.e]+)\)'):
        ranges = [f"{a}-{b}" for a, b in re.findall(r"\((\d+), (\d+)\)", m.group(2))]
        inc = "1" if m.group(3) == "true" else "0"
        out.append((f"fillets/{name}", f"{kn}smlib_fillet", [f"m = smb {SMLIB}/fillets/{m.group(1)}", f"*r = smbfillet m {m.group(4)} {inc} {' '.join(ranges)}"]))
    ops = {"U": "bfuse", "I": "bcommon", "D": "bcut"}
    for name, m, kn in passing("booleans", r'chain\(&\[([^\]]*)\], ([UID]),'):
        files = re.findall(r'"([^"]+)"', m.group(1))
        lines = [f"b{i} = smb {SMLIB}/booleans/{f}" for i, f in enumerate(files)]
        acc = "b0"
        for i in range(1, len(files)):
            lines.append(f"*r{i} = {ops[m.group(2)]} {acc} b{i}")
            acc = f"r{i}"
        out.append((f"booleans/{name}", f"{kn}smlib_boolean", lines))
    for name, m, kn in passing("offset", r'offset\("([^"]+)", ([-0-9.e]+), None, None,'):
        out.append((f"offset/{name}", f"{kn}smlib_offset", [f"m = smb {SMLIB}/offset/{m.group(1)}", f"*r = offset m {m.group(2)}"]))
    print(f"smlib: {len(out)} fillet, boolean and offset cases", file=sys.stderr)
    return out


def model_cases():
    files = []
    for d in os.listdir(SMLIB):
        for f in os.listdir(os.path.join(SMLIB, d)):
            if f.endswith(".smb"):
                p = os.path.join(SMLIB, d, f)
                files.append((os.path.getsize(p), p))
    files.sort(reverse=True)
    out = []
    for _, p in files[:TESS_MODELS]:
        stem = os.path.basename(p)[:-4]
        out.append((f"tess/{stem}", "tess", [f"m = smb {p}", "*tess m 0.001 0.5"]))
        out.append((f"mass/{stem}", "mass", [f"m = smb {p}", "*mass m"]))
        # OCCT's default rules are coarse (1e-3 off on the blade hubs); adaptive to 1e-9 is the
        # native kernel's precision class.
        out.append((f"mass_eps9/{stem}", "mass_eps9", [f"m = smb {p}", "*mass m 1e-9"]))
    return out


def step_cases(out_dir):
    out = []
    for f in STEP_FILES:
        p = os.path.join(DATA, "step", f)
        if not os.path.exists(p):
            continue
        stem = re.sub(r"\W", "_", f.rsplit(".", 1)[0])
        out.append((f"step_read/{stem}", "step_read", [f"*m = stepread {p}"]))
        out.append((f"step_write/{stem}", "step_write", [f"m = stepread {p}", f"*stepwrite m {out_dir}/written_{stem}.step"]))
    return out


def write_cases(path, cases):
    with open(path, "w") as f:
        for name, cat, lines in cases:
            f.write(f"case {name} {cat}\n")
            for l in lines:
                f.write(l + "\n")
            f.write("end\n")


def case_names(path):
    return [(l.split()[1], l.split()[2]) for l in open(path) if l.startswith("case ")]


def loadavg():
    with open("/proc/loadavg") as f:
        return float(f.read().split()[0])


def run_side(label, cmd, cases_file, names, env, timeout, reps):
    """Runs each case in its own process; returns {name: {...}} and the load seen."""
    res, loads = {}, [loadavg()]
    t0 = time.time()
    for i, (name, cat) in enumerate(names):
        try:
            p = subprocess.run(cmd + [cases_file, "--case", name, "--reps", str(reps)], env=env, capture_output=True, text=True, timeout=timeout)
            line = next((l for l in p.stdout.splitlines() if l.startswith("RESULT\t")), None)
            if line is None:
                r = {"ok": False, "msg": f"crashed (exit {p.returncode})"}
            else:
                f = line.split("\t")
                r = {"ok": f[3] == "ok", "wall": float(f[4]), "cpu": float(f[5]), "runs": int(f[6]), "msg": f[7] if len(f) > 7 else ""}
        except subprocess.TimeoutExpired:
            r = {"ok": False, "msg": f"timeout {timeout} s"}
        r["category"] = cat
        res[name] = r
        if i % 20 == 0:
            loads.append(loadavg())
            print(f"[{label}] {i + 1}/{len(names)} {time.time() - t0:.0f} s, load {loads[-1]:.1f}", file=sys.stderr)
    loads.append(loadavg())
    return res, loads


def conformance(binary, reps, threads):
    """Median per-test time (ms) of the conformance suite: with `threads` = 1 the CPU time
    (user + system) of a process running just that test, less an empty run's; else the wall
    time `--report-time` gives on rayon's default pool. (One rayon thread's wall time is no use
    under load: every hand-off to the pool waits for the scheduler.)"""
    env = dict(os.environ, RUSTC_BOOTSTRAP="1")
    times = {}
    if threads == 1:
        env["RAYON_NUM_THREADS"] = "1"
        names = [l.split(": test")[0] for l in subprocess.run([binary, "--list"], env=env, capture_output=True, text=True).stdout.splitlines() if l.endswith(": test")]

        def cpu(args):
            r0 = resource.getrusage(resource.RUSAGE_CHILDREN)
            subprocess.run([binary, "--test-threads=1", "-q"] + args, env=env, capture_output=True)
            r1 = resource.getrusage(resource.RUSAGE_CHILDREN)
            return (r1.ru_utime - r0.ru_utime + r1.ru_stime - r0.ru_stime) * 1e3

        empty = statistics.median(cpu(["--exact", "no_such_test"]) for _ in range(max(reps, 5)))
        for n in names:
            times[n] = statistics.median(max(cpu(["--exact", n]) - empty, 0.0) for _ in range(reps))
        return times
    for _ in range(reps):
        p = subprocess.run([binary, "--test-threads=1", "-Zunstable-options", "--report-time", "--format", "json"], env=env, capture_output=True, text=True)
        for l in p.stdout.splitlines():
            try:
                e = json.loads(l)
            except ValueError:
                continue
            if e.get("type") == "test" and e.get("event") == "ok" and "exec_time" in e:
                times.setdefault(e["name"], []).append(e["exec_time"] * 1e3)
    return {n: statistics.median(v) for n, v in times.items()}


def occt_conformance():
    out = {}
    if os.path.exists(OCCT_CONF):
        for l in open(OCCT_CONF):
            f = l.split()
            if len(f) == 2 and f[0].startswith("occt::"):
                out[f[0][6:]] = float(f[1]) * 1e3
    return out


def gmean(xs):
    return math.exp(sum(math.log(x) for x in xs) / len(xs)) if xs else float("nan")


# Below this CPU time a case is mostly fixed overhead on both sides; the ratio is floored there.
FLOOR_MS = 0.05


# What each category times, and what to keep in mind reading it.
NOTES = [
    "**The categories.**",
    "- `smlib_<suite>`: SMLib's fillet, boolean and offset cases that pass. OCCT reads the models as STEP",
    "  the native kernel wrote, the fillet's edges picked by their middle points; OCCT's own failures",
    "  are listed below.",
    "- `tess`, `mass`: the largest SMLib models; the chord is 0.001 of the bounding box's diagonal, the",
    "  angle 0.5 rad. OCCT's default mass properties (fixed Gauss rules) are fast but coarse (1e-3 off in",
    "  volume on the blade hubs, 7e-5 on the vases); `mass_eps9` times OCCT adaptive to 1e-9, the",
    "  native kernel's precision class.",
    "- `step_read`, `step_write`: files of OCCT's dataset, read into bodies, and written back.",
    "",
]


# Measurements by hand (profiles of the documents and of single operations), kept as written.
BY_HAND = [
    '## Measured by hand',
    '',
    'Not generated: profiles of the documents and of single operations, measured on 2026-10-22/23',
    "with `CADRS_PROFILE` timers (`perf` can't sample on the build boxes), the documents being the",
    '173 Part Studios of the private test corpus run by the `checkall` example on gray (i9-13900KF).',
    '',
    '- **Where document time goes.** Booleans are ~38% of it (90 s of 239 s over 126 documents).',
    "  Within the booleans' runs (65 s over all documents, kernel-perf 1665ac2): face–face 20%,",
    '  classify 14%, edge–face 13%, shells 11%, import 10%, unify 8%, the crossing checks 7%,',
    '  edge–edge 5%, empty solids 4%, split 4%. One document (a gridfinity holder) spends 2.7 s in',
    '  `curve_curve` between two branches of one intersection that run together over 5 µm.',
    '- **Multi-tool booleans** (several tools at once) are 2.6% of document time (6.2 s); the parity',
    "  rays' per-face box loop is at most 0.6% of the worst document. Neither is worth a change yet.",
    '- **Surface intersections** are 22 s over the documents: B-spline pairs 17 s (Other×Other 8.6 s,',
    '  Cylinder×Other 5.4 s, Other×Plane 3.1 s); quadric pairs left to the marcher 3.9 s (Cylinder×',
    '  Sphere 1.4, Cylinder×Cylinder 1.0, Cylinder×Torus 0.5); Plane×Plane 86 590 calls, 0.1 s.',
    "  The 22 marched Cone×Plane calls (~40 ms each, planes tangent along a countersink's ruling)",
    '  now have a closed form.',
    '- **Sweeps.** Sections per sweep went from 1013 / 412 to 111 / 43 (graded density, exact frames).',
    '- **STEP import** (trj6): 1.6 s orienting faces by volume and ~0.4 s fitting p-curves before the',
    '  per-face parity orientation and parallel fits.',
    '- **Booleans sampled** (231): all finish in one run; a median of 9 face pairs intersected; 2.2',
    '  rays per classified point; p-curve fits 14% of the time; the empty-solid volume dominated two.',
    "- **SSI culls** (helical B-splines): a squish bottle's 4×50 union 3.2 s → 90 ms, edge–face",
    '  2.5 → 0.8 s, shells 553 → 86 ms.',
    '',
]


def report(R, path):
    rows, cats, known = [], {}, []
    for name, ours in R["cadrs_1t"].items():
        occ = R["occt_1t"].get(name, {})
        mt = R["cadrs_mt"].get(name, {})
        cat = ours["category"]
        if cat.startswith("known_"):
            known.append((cat[6:], name, ours, occ))
            continue
        ratio = None
        if ours.get("ok") and occ.get("ok"):
            ratio = max(ours["cpu"], FLOOR_MS) / max(occ["cpu"], FLOOR_MS)
            cats.setdefault(cat, []).append(ratio)
        rows.append((cat, name, ours, mt, occ, ratio))
    # Conformance.
    conf_rows = []
    oc = R.get("occt_conformance", {})
    for name, t in sorted(R.get("conformance_1t", {}).items()):
        short = name.split("::")[-1]
        o = oc.get(short.replace("s", ""))
        tm = R.get("conformance_mt", {}).get(name)
        ratio = max(t, FLOOR_MS) / max(o, FLOOR_MS) if o is not None else None
        if ratio:
            cats.setdefault("conformance", []).append(ratio)
        conf_rows.append(("conformance", short, t, tm, o, ratio))

    L = []
    L.append("# Kernel performance against OCCT")
    L.append("")
    L.append("Generated by `tools/kernel_perf.py`; don't edit by hand. Re-run it after kernel changes that")
    L.append("matter for speed.")
    L.append("")
    m = R["meta"]
    L.append(f"Measured {m['date']} on {m['cpu']} ({m['cores']} threads), native kernel at `{m['git'] or '?'}`")
    L.append(f"(`{m.get('toolchain', '?')}`), OCCT 7.8.0 (static, `-O2`). Load average (1 min) while measuring:")
    L.append(f"native kernel {m['load_cadrs_1t']}, OCCT {m['load_occt_1t']} (other jobs may share the machine; hence CPU time).")
    L.append("")
    L.append("**How.** A case is a DRAW-like script both sides read: `kernel_bench` (an example of the")
    L.append("crate) for the native kernel, `tools/occt_bench` for OCCT, each command mapped onto the")
    L.append("same DRAW/OCCT calls on the same input files. Only its marked lines (the operation, not")
    L.append("loading its input) are timed. Each case runs in its own process, at least "
             f"{m['reps']} times and")
    L.append("until its runs have taken a second (fewer when one run is slow). **Time is CPU time**")
    L.append("(user + system, all threads; the mean over the runs) on one thread: `RAYON_NUM_THREADS=1`")
    L.append("for the native kernel, OCCT's default (no parallel mode, as DRAW). The `mt` column is the")
    L.append(f"native kernel's median wall time on rayon's default pool ({m['cores']} threads, under the")
    L.append(f"load above). Ratio = native / OCCT; times under {FLOOR_MS} ms count as {FLOOR_MS} ms in it.")
    L.append("Only cases both kernels complete enter the means; the others are listed with their error.")
    L.append("")
    L.append("## Summary")
    L.append("")
    L.append("| Category | Cases timed | Geometric mean ratio | Median ratio | Native faster in |")
    L.append("|---|---:|---:|---:|---:|")
    allr = []
    for cat in sorted(cats):
        v = cats[cat]
        allr += v
        L.append(f"| {cat} | {len(v)} | {gmean(v):.2f} | {statistics.median(v):.2f} | {sum(1 for x in v if x < 1)} |")
    L.append(f"| **all** | {len(allr)} | **{gmean(allr):.2f}** | {statistics.median(allr):.2f} | {sum(1 for x in allr if x < 1)} |")
    L.append("")
    L.extend(NOTES)
    L.append("Conformance compares whole tests: the native kernel's CPU time for a process running the one")
    L.append("test (one rayon thread, less an empty run's) with the same tests' times on the former OCCT")
    L.append("backend (`bench/occt_conformance_times.txt`, wall times to 1 ms). They include the tests'")
    L.append("own checks (mass properties and meshes at 0.01 mm), not only the modelling operations.")
    L.append("")
    L.extend(BY_HAND)
    worst = sorted([r for r in rows + conf_rows if r[5] is not None], key=lambda r: -r[5])[:20]
    L.append("## The 20 worst ratios")
    L.append("")
    L.append("| Case | Category | Native ms | OCCT ms | Ratio |")
    L.append("|---|---|---:|---:|---:|")
    for cat, name, ours, mt, occ, ratio in worst:
        ot = ours["cpu"] if isinstance(ours, dict) else ours
        oo = occ["cpu"] if isinstance(occ, dict) else occ
        L.append(f"| `{name}` | {cat} | {ot:.2f} | {oo:.2f} | {ratio:.1f} |")
    L.append("")
    L.append("## Failures")
    L.append("")
    L.append("Cases one side doesn't complete (they pass in the native kernel's test suites, so a native")
    L.append("failure here is a difference between the suite and the bench, e.g. a time-out).")
    L.append("")
    L.append("| Case | Native | OCCT |")
    L.append("|---|---|---|")
    for cat, name, ours, mt, occ, ratio in rows:
        if ratio is None:
            a = f"{ours['cpu']:.2f} ms" if ours.get("ok") else ours.get("msg", "")[:80]
            b = f"{occ['cpu']:.2f} ms" if occ.get("ok") else occ.get("msg", "missing")[:80]
            L.append(f"| `{name}` | {a} | {b} |")
    L.append("")
    if known:
        L.append("## Known failures")
        L.append("")
        L.append("Cases on the suites' known-failure lists: the native kernel's CPU time on one thread to its")
        L.append("result (which the suite's checks then reject) or to its error, and OCCT's. Not in the means.")
        L.append("")
        L.append("| Case | Category | Native ms | Native outcome | OCCT ms |")
        L.append("|---|---|---:|---|---:|")
        for cat, name, ours, occ in sorted(known, key=lambda r: -r[2].get("cpu", 0.0)):
            t = f"{ours['cpu']:.2f}" if "cpu" in ours and (ours.get("ok") or ours["cpu"] > 0) else ""
            how = "result" if ours.get("ok") else ours.get("msg", "")[:70].replace("|", "/")
            o = f"{occ['cpu']:.2f}" if occ.get("ok") else "fail"
            L.append(f"| `{name}` | {cat} | {t} | {how} | {o} |")
        L.append("")
    L.append("## All cases")
    L.append("")
    L.append("CPU ms on one thread; `mt` wall ms on the default pool.")
    L.append("")
    L.append("| Case | Category | Native | Native mt | OCCT | Ratio |")
    L.append("|---|---|---:|---:|---:|---:|")
    for cat, name, ours, mt, occ, ratio in sorted(rows, key=lambda r: (r[0], r[1])):
        f = lambda d, k: f"{d[k]:.2f}" if d.get("ok") else "fail"
        L.append(f"| `{name}` | {cat} | {f(ours, 'cpu')} | {f(mt, 'wall')} | {f(occ, 'cpu')} | {'' if ratio is None else f'{ratio:.2f}'} |")
    for cat, name, t, tm, o, ratio in conf_rows:
        L.append(f"| `{name}` | {cat} | {t:.2f} | {'' if tm is None else f'{tm:.2f}'} | {'' if o is None else f'{o:.2f}'} | {'' if ratio is None else f'{ratio:.2f}'} |")
    with open(path, "w") as f:
        f.write("\n".join(L) + "\n")
    return cats, worst


def cargo_build(target):
    env = dict(os.environ, CARGO_TARGET_DIR=target)
    env.setdefault("CARGO_BUILD_JOBS", "3")
    subprocess.run(["cargo", "build", "-r", "-p", "cadrs_kernel", "--example", "kernel_bench"], cwd=REPO, env=env, check=True)
    p = subprocess.run(["cargo", "test", "-r", "-p", "cadrs_kernel", "--test", "conformance", "--no-run", "--message-format=json"], cwd=REPO, env=env, check=True, capture_output=True, text=True)
    conf = None
    for l in p.stdout.splitlines():
        e = json.loads(l)
        if e.get("reason") == "compiler-artifact" and e.get("target", {}).get("name") == "conformance" and e.get("executable"):
            conf = e["executable"]
    return os.path.join(target, "release", "examples", "kernel_bench"), conf


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=os.path.join(REPO, "target", "kernel_perf"))
    ap.add_argument("--reps", type=int, default=5)
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--only", help="categories to (re)run, comma-separated; others keep their results")
    ap.add_argument("--report-only", action="store_true")
    ap.add_argument("--doc", default=os.path.join(KERNEL, "PERF.md"))
    ap.add_argument("--known", action="store_true", help="also time the known failures (reported apart)")
    ap.add_argument("--occt-bench", help="a built tools/occt_bench to use instead of building one")
    ap.add_argument("--commit", default=os.environ.get("CADRS_COMMIT", ""), help="the commit measured (default $CADRS_COMMIT, else git)")
    ap.add_argument("--cases-only", action="store_true", help="write the case files to --out, and stop")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    rpath = os.path.join(a.out, "results.json")
    R = json.load(open(rpath)) if os.path.exists(rpath) else {}
    if not a.report_only:
        target = os.environ.get("CARGO_TARGET_DIR", os.path.join(REPO, "target"))
        bench, conf_bin = cargo_build(target)
        occt_bin = a.occt_bench or os.path.join(a.out, "occt_bench")
        if not a.occt_bench and not a.cases_only:
            subprocess.run([os.path.join(REPO, "tools", "occt_bench", "build.sh"), occt_bin], check=True)
        cases = smlib_cases(a.known) + model_cases() + step_cases(a.out)
        only = set(a.only.split(",")) if a.only else None
        if only:
            cases = [c for c in cases if c[1] in only or (c[1].startswith("known_") and c[1][6:] in only)]
        raw = os.path.join(a.out, "cases.txt")
        write_cases(raw, cases)
        ours_f, occt_f = os.path.join(a.out, "cadrs.cases"), os.path.join(a.out, "occt.cases")
        subprocess.run([bench, "prep", raw, ours_f, occt_f, os.path.join(a.out, "step")], check=True)
        names = case_names(ours_f)
        if a.cases_only:
            print(f"{len(names)} cases in {ours_f} and {occt_f}", file=sys.stderr)
            return
        one = dict(os.environ, RAYON_NUM_THREADS="1")
        for key, cmd, f, env in [("cadrs_1t", [bench, "run"], ours_f, one), ("occt_1t", [occt_bin], occt_f, one), ("cadrs_mt", [bench, "run"], ours_f, dict(os.environ))]:
            res, loads = run_side(key, cmd, f, names, env, a.timeout, a.reps)
            old = {n: r for n, r in R.get(key, {}).items() if only and r["category"].removeprefix("known_") not in only}
            R[key] = {**old, **res}
            R.setdefault("meta", {})[f"load_{key}"] = f"{min(loads):.0f}–{max(loads):.0f}"
        if not only or "conformance" in only:
            R["conformance_1t"] = conformance(conf_bin, a.reps, 1)
            R["conformance_mt"] = conformance(conf_bin, a.reps, None)
            R["occt_conformance"] = occt_conformance()
        git = a.commit
        if not git:
            git = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=REPO, capture_output=True, text=True).stdout.strip()
            dirty = subprocess.run(["git", "status", "--porcelain", "--untracked-files=no"], cwd=REPO, capture_output=True, text=True).stdout.strip()
            if git and dirty:
                git += "+dirty"
        toolchain = subprocess.run(["rustc", "--version"], cwd=REPO, capture_output=True, text=True).stdout.strip()
        cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), "?")
        R["meta"].update({"date": time.strftime("%Y-%m-%d"), "git": git, "cpu": cpu, "cores": os.cpu_count(), "reps": a.reps, "toolchain": toolchain})
        json.dump(R, open(rpath, "w"), indent=1)
    cats, worst = report(R, a.doc)
    for c in sorted(cats):
        print(f"{c:16} {len(cats[c]):4} cases  geomean {gmean(cats[c]):.2f}")


if __name__ == "__main__":
    main()
