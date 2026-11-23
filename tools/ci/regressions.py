#!/usr/bin/env python3
"""A suite's run against its baseline: every case that passed there must pass here.

    regressions.py RESULTS BASELINE [--update] [--name NAME]

RESULTS is a suite's results (JSON with `cases`, each with an id under `id` or `case` and an
`outcome`): brep-bench's `results/cadrs.json`, or brep-conformance's run (tools/conformance's
`examples/suite.rs --out`). BASELINE is `{"outcomes": {id: outcome}}`, committed beside the job
that runs the suite (tools/ci/*.baseline.json).

Prints the cases that stopped passing (regressions: a pass in the baseline, anything else now,
or gone), the ones that started (gains) and the ones new to the suite; exits 1 on any
regression. A baseline is never moved by CI: with --update, the run is written as the new
baseline (for a fix's gains, or a suite moved to a new commit), to be committed with them.
Standard library only.
"""

import json
import sys
from collections import Counter


def outcomes(results):
    out = {}
    for c in results["cases"]:
        key = c.get("id") or c.get("case")
        if key is not None:
            out[key] = c.get("outcome", "?")
    return out


def main():
    args = sys.argv[1:]
    if len(args) < 2 or args[0].startswith("-"):
        print(__doc__)
        return 2
    flag = lambda f: args[args.index(f) + 1] if f in args and args.index(f) + 1 < len(args) else None
    name = flag("--name") or args[1]
    with open(args[0]) as f:
        now = outcomes(json.load(f))
    if "--update" in args:
        with open(args[1], "w") as f:
            json.dump({"outcomes": dict(sorted(now.items()))}, f, indent=1, sort_keys=False)
            f.write("\n")
        print(f"{name}: baseline written, {len(now)} cases {dict(Counter(now.values()))}")
        return 0
    with open(args[1]) as f:
        base = json.load(f)["outcomes"]
    lost = [(k, o, now.get(k, "gone")) for k, o in sorted(base.items())
            if o == "pass" and now.get(k) != "pass"]
    gained = [(k, o, now[k]) for k, o in sorted(base.items())
              if o != "pass" and now.get(k) == "pass"]
    new = [(k, now[k]) for k in sorted(now) if k not in base]
    print(f"{name}: {len(now)} cases {dict(Counter(now.values()))}; "
          f"baseline {len(base)} {dict(Counter(base.values()))}")
    for k, was, is_ in lost:
        print(f"  REGRESSION {k}: {was} -> {is_}")
    for k, was, is_ in gained:
        print(f"  gain       {k}: {was} -> {is_}")
    for k, o in new:
        print(f"  new        {k}: {o}")
    if gained or new:
        print(f"  (to keep them: regressions.py RESULTS {args[1]} --update, and commit it)")
    if lost:
        print(f"{name}: {len(lost)} regression(s)")
        return 1
    print(f"{name}: no regressions")
    return 0


if __name__ == "__main__":
    sys.exit(main())
