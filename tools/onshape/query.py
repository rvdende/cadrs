#!/usr/bin/env python3
"""Decoder for the `qCompressed(...)` query strings in Onshape feature JSON (prototype; the
importer's Rust decoder follows this one).

The payload is either plain (`%...`) or zlib+base64 (`&<hexlen>$<base64>`). Grammar, as
reverse-engineered from the scraped data:

    value  := 'M' hex (value value)*    a map of n key/value pairs
            | 'A' hex value*            an array of n values
            | 'S' segs '$' chars        a string; segs are '.'-separated hex lengths of new
                                        segments or '-'hex back references; the segments
                                        join with '.'
            | 'R' hex                   a back reference to a value (see `Reader`)
            | 'E' hex                   a string value taken from the name table
            | 'B' hex '$' chars value   a new type name, then a value of that type
            | 'C' hex value             a value of a type already in the table
            | 'D' number                a number
            | 'T' | 'F'                 true / false (not entered in the value table)
            | 'N'                       null (assumed)

See `Reader` for the two back-reference tables.

    python3 tools/onshape/query.py <features.json>   # prints every feature's decoded queries
"""
import base64
import json
import re
import sys
import zlib


class Typed:
    def __init__(self, type_name, value):
        self.type, self.value = type_name, value

    def __repr__(self):
        return f"{self.type}({self.value!r})"


class Reader:
    """Two back-reference tables, as the data shows them:
    - `names` (indexed by `C<n>` and by `-<n>` string segments): every type name, every new
      string segment and every joined compound string, in the order met;
    - `values` (indexed by `R<n>`): every value (strings including map keys, numbers,
      untyped maps and arrays, typed values) in the order it finishes; a typed value's own
      payload is not entered separately.
    """

    def __init__(self, s):
        self.s, self.i, self.names, self.values = s, 0, [], []

    def hex(self):
        m = re.compile(r"-?[0-9a-f]+").match(self.s, self.i)
        self.i = m.end()
        return int(m.group(0), 16)

    def number(self):
        m = re.compile(r"-?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][-+]?[0-9]+)?").match(self.s, self.i)
        self.i = m.end()
        t = m.group(0)
        return float(t) if any(c in t for c in ".eE") else int(t)

    def chars(self, n):
        out = self.s[self.i:self.i + n]
        self.i += n
        return out

    def string(self):
        segs = []
        while True:
            segs.append(self.hex())
            c = self.s[self.i]
            self.i += 1
            if c == "$":
                break
            assert c == ".", f"bad string at {self.i}"
        parts = []
        for n in segs:
            if n < 0:
                parts.append(self.names[-n])
            else:
                parts.append(self.chars(n))
                self.names.append(parts[-1])
        joined = ".".join(parts)
        if len(parts) > 1:
            self.names.append(joined)
        return joined

    def value(self, typed_payload=False):
        """Reads one value. Every value enters `values` when it finishes, except a typed
        value's own payload (the typed wrapper enters instead)."""
        c = self.s[self.i]
        self.i += 1
        if c == "R":
            return self.values[self.hex()]
        if c == "M":
            n = self.hex()
            v = {}
            for _ in range(n):
                k = self.value()
                v[k if isinstance(k, str) else repr(k)] = self.value()
        elif c == "A":
            v = [self.value() for _ in range(self.hex())]
        elif c == "S":
            v = self.string()
        elif c == "E":
            v = self.names[self.hex()]
        elif c in "BC":
            if c == "B":
                n = self.hex()
                assert self.s[self.i] == "$"
                self.i += 1
                name = self.chars(n)
                self.names.append(name)
            else:
                name = self.names[self.hex()]
            v = Typed(name, self.value(typed_payload=True))
        elif c == "D":
            v = self.number()
        elif c in "TF":
            v = c == "T"
            if not BOOLS_INTERNED:
                return v
        elif c == "N":
            return None
        else:
            raise ValueError(f"unknown tag {c!r} at {self.i - 1}: …{self.s[self.i - 20:self.i + 20]}…")
        if not typed_payload:
            self.values.append(v)
        return v


BOOLS_INTERNED = False


def payload(query_string):
    """The serialized query inside `qCompressed(...)`, or None for other query forms (such as
    `qSketchRegion(id + "<feature>", true)`)."""
    m = re.search(r'qCompressed\(1\.0,"(.*)",(?:true|false)\)', query_string) or re.search(r'qCompressed\(1\.0,"(.*)"', query_string)
    if not m:
        return None
    s = m.group(1)
    if s.startswith("&"):
        b = s.split("$", 1)[1]
        return zlib.decompress(base64.b64decode(b + "=" * (-len(b) % 4))).decode()
    return s


def decode(query_string):
    s = payload(query_string)
    if s is None:
        return None
    assert s.startswith("%"), s[:20]
    r = Reader(s[1:])
    v = r.value()
    assert r.i == len(r.s), f"trailing: {r.s[r.i:r.i + 40]}"
    return v


def plain(v):
    if isinstance(v, Typed):
        if v.type == "Query":
            return plain(v.value)
        if v.type == "Id":
            return ".".join(v.value) if isinstance(v.value, list) else v.value
        return plain(v.value)
    if isinstance(v, dict):
        return {k: plain(x) for k, x in v.items()}
    if isinstance(v, list):
        return [plain(x) for x in v]
    return v


if __name__ == "__main__":
    for feat in json.load(open(sys.argv[1]))["features"]:
        for p in feat.get("parameters", []):
            for q in p.get("queries") or []:
                if "queryString" in q:
                    print(f"== {feat['name']} / {p['parameterId']}")
                    print(json.dumps(plain(decode(q["queryString"])), indent=1))
