#!/usr/bin/env python3
"""Brings brep-bench's cases (https://brepbench.org) into the kernel's tests: copies its cadrs
runner's implementation of every case (kernels/cadrs/src: bench.rs, shared/, cases/) into
crates/cadrs_kernel/tests/brep_bench/, and writes cases.rs there: a module per case and a #[test]
per case, which builds it and grades it as the harness does (`check` in that directory's main.rs),
against the volume and area its description gives (none for an unsettled case).

    tools/brep_bench_sync.py [BREP_BENCH]

BREP_BENCH is a brep-bench checkout, by default ../ext/brep-bench beside this repository's parent
(~/work/ext/brep-bench for ~/work/cadrs). Run it after the cases or the runner change there; the
files it writes are committed, so the tests need no checkout. Standard library only (tomllib:
Python 3.11 or later)."""

import os
import shutil
import subprocess
import sys
import tomllib

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "crates", "cadrs_kernel", "tests", "brep_bench")
KEYWORDS = {"as", "box", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
            "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static",
            "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await",
            "dyn", "abstract", "become", "do", "final", "macro", "override", "priv", "typeof", "unsized",
            "virtual", "yield", "try", "gen"}


def header(path):
    """A case's TOML header (between its +++ lines)."""
    text = open(path, encoding="utf-8").read()
    _, head, _ = text.split("+++", 2)
    return tomllib.loads(head)


def ident(name):
    return f"r#{name}" if name in KEYWORDS else name


def main():
    bench = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(REPO, "..", "ext", "brep-bench"))
    src = os.path.join(bench, "kernels", "cadrs", "src")
    if not os.path.isdir(src):
        sys.exit(f"brep_bench_sync: {src} is missing (give the brep-bench checkout)")
    commit = subprocess.run(["git", "-C", bench, "rev-parse", "--short", "HEAD"], capture_output=True,
                            text=True).stdout.strip() or "?"
    # The runner's sources, as they are there (bench.rs, shared/, cases/).
    for d in ("shared", "cases"):
        shutil.rmtree(os.path.join(OUT, d), ignore_errors=True)
        shutil.copytree(os.path.join(src, d), os.path.join(OUT, d))
    shutil.copy(os.path.join(src, "bench.rs"), os.path.join(OUT, "bench.rs"))
    # The table: each case's module and test, with its expected measures.
    lines = [f"// Written by tools/brep_bench_sync.py from brep-bench {commit}: don't edit.", ""]
    count = 0
    cases_dir = os.path.join(src, "cases")
    for category in sorted(os.listdir(cases_dir)):
        cdir = os.path.join(cases_dir, category)
        if not os.path.isdir(cdir):
            continue
        lines.append(f"pub mod {ident(category)} {{")
        for f in sorted(os.listdir(cdir)):
            if not f.endswith(".rs"):
                continue
            name = f[:-3]
            case = f"{category}/{name.replace('_', '-')}"
            md = os.path.join(bench, "cases", category, name.replace("_", "-") + ".md")
            meta = header(md) if os.path.exists(md) else None
            if meta is None:
                expect = "None"
            elif meta.get("settled", True):
                expect = f"Some(({float(meta['volume']['value'])!r}, {float(meta['area']['value'])!r}))"
            else:
                expect = "None"
            m = ident(name)
            lines += [
                f"    pub mod {m};",
                "    #[test]",
                f"    fn {m}() {{",
                f'        crate::check("cases::{ident(category)}::{m}", "{case}", |k| {m}::build(k).map_err(crate::Refusal::from), {expect});',
                "    }",
            ]
            count += 1
        lines.append("}")
    text = "\n".join(lines) + "\n"
    open(os.path.join(OUT, "cases.rs"), "w", encoding="utf-8").write(text)
    print(f"brep_bench_sync: {count} cases from brep-bench {commit} into {os.path.relpath(OUT, REPO)}")


if __name__ == "__main__":
    main()
