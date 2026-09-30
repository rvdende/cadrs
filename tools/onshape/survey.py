#!/usr/bin/env python3
"""Tallies what the scraped documents use, to decide what the importer must support first.

    python3 tools/onshape/survey.py            # summary over raw/
    python3 tools/onshape/survey.py --docs     # plus one line per document
"""
import collections
import glob
import json
import os
import sys

ROOT = os.path.expanduser(os.environ.get("CADRS_ONSHAPE_ROOT", "~/work/cadrs_onshape"))
RAW = os.path.join(ROOT, "raw")


def load(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def short(bt):
    return (bt or "?").split("-")[0].removeprefix("BTCurveGeometry").removeprefix("BTM").removeprefix("BT")


def main():
    per_doc = "--docs" in sys.argv
    docs = {}
    for page in sorted(glob.glob(os.path.join(RAW, "_index", "documents-*.json"))):
        for d in (load(page) or {}).get("items", []):
            docs[d["id"]] = d

    element_types = collections.Counter()
    feature_types = collections.Counter()
    feature_docs = collections.defaultdict(set)
    entity_types = collections.Counter()
    constraint_types = collections.Counter()
    mate_types = collections.Counter()
    custom = collections.Counter()
    scraped = 0
    lines = []

    for did, d in sorted(docs.items(), key=lambda kv: kv[1].get("name", "")):
        elements = load(os.path.join(RAW, did, "elements.json"))
        if elements is None:
            lines.append(f"  (not scraped) {d['name']}")
            continue
        scraped += 1
        kinds = collections.Counter(e["elementType"] for e in elements)
        element_types.update(kinds)
        n_features = 0
        doc_features = collections.Counter()
        for e in elements:
            f = load(os.path.join(RAW, did, e["id"], "features.json"))
            if not f:
                continue
            for feat in f.get("features", []):
                ft = feat.get("featureType", "?")
                if e["elementType"] == "ASSEMBLY":
                    mt = next((p.get("value") for p in feat.get("parameters", []) if p.get("parameterId") == "mateType"), None)
                    mate_types[mt or ft] += 1
                    continue
                n_features += 1
                if feat.get("namespace"):
                    custom[ft] += 1
                    ft = f"custom:{ft}"
                feature_types[ft] += 1
                doc_features[ft] += 1
                feature_docs[ft].add(did)
                for ent in feat.get("entities", []) or []:
                    entity_types[short((ent.get("geometry") or {}).get("btType") or ent.get("btType"))] += 1
                for c in feat.get("constraints", []) or []:
                    constraint_types[c.get("constraintType", "?")] += 1
        kinds_s = " ".join(f"{k.lower()}×{n}" for k, n in sorted(kinds.items()))
        top = ", ".join(f"{k}×{n}" for k, n in doc_features.most_common(6))
        lines.append(f"  {d['name'][:40]:40} {kinds_s:45} {n_features:4} features  {top}")

    print(f"documents listed: {len(docs)}, scraped: {scraped}")
    print("\nelement types:", dict(element_types.most_common()))
    print("\npart studio feature types (count, documents):")
    for ft, n in feature_types.most_common():
        print(f"  {ft:32} {n:5}  {len(feature_docs[ft]):4} docs")
    print("\nsketch entity types:", dict(entity_types.most_common()))
    print("\nsketch constraint types:", dict(constraint_types.most_common()))
    print("\nassembly features / mate types:", dict(mate_types.most_common()))
    if per_doc:
        print("\ndocuments:")
        print("\n".join(lines))


if __name__ == "__main__":
    main()
