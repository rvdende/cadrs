function(context is Context, queries) {
// cqrt: CadQuery 2.8's Workplane semantics, ported to FeatureScript so Parasolid can run CADEvolve
// generators. Each function names the cq.py / selectors.py method it follows; read them side by side.
//
// This file is the body of a lambda (Onshape's evalFeatureScript takes one function), so every
// helper is a const function value; they take the context `c` and the state box `st` explicitly.
//
// Conventions
// - Lengths are plain numbers in millimetres; units are attached only when calling Onshape (`* MM`).
// - Bodies are never modified in place: an operation copies its inputs (opPattern) and works on the
//   copies, so every shape value (a query) and every selected face or edge stays valid, as CadQuery's
//   values are immutable.
// - Stack objects: {"t": "vec", "p"} a point; {"t": "loc", "T"} a Location (local, as CadQuery keeps
//   it); {"t": "edge", ...} / {"t": "wire", ...} edges and wires as data (world coordinates);
//   {"t": "solid", "q"} bodies; {"t": "face" | "edgeq" | "vertex", "q", "body"} topology of a body.
// - Workplanes live in st[].wps by their tape id: {"plane", "objs", "parent", "ctx"}; a context
//   (CQContext) in st[].ctx by index: {"pw" pending wires, "pe" pending edges, "fp" firstPoint, "tags"}.

const MM = millimeter;
const CQTOL = 0.0001;   // CQContext.tolerance and the selectors' tolerance
const TOL6 = 1e-6;      // cq.py's TOL

const cqErr = function(msg) { throw regenError("cq: " ~ msg); };
const cqUnsup = function(msg) { throw regenError("cq-unsupported: " ~ msg); };

const nid = function(st, prefix) {
    var s = st[];
    s.n = s.n + 1;
    st[] = s;
    return s.base + (prefix ~ s.n);   // under the case's own id (a feature's ops must be under its id)
};

const v3 = function(a) {
    if (a == undefined) cqErr("missing vector");
    if (a is map && a.vec != undefined) a = a.vec;
    if (size(a) == 2) return vector(a[0], a[1], 0);
    if (size(a) == 3) return vector(a[0], a[1], a[2]);
    cqErr("not a vector: " ~ toString(a));
};
const isNum = function(x) { return x is number; };
const unitOf = function(v) { return v / norm(v); };

// ---- points and directions through Onshape transforms (unitless mm <-> lengths)
const tP = function(T, p) { return (T * (p * MM)) / MM; };
const tD = function(T, d) { return T.linear * d; };
const lenV = function(p) { return p * MM; };
const unlen = function(p) { return vector(p[0] / MM, p[1] / MM, p[2] / MM); };

// ---- Plane (geom.py). Frame: x orthogonalised as gp_Ax3(origin, zDir, xDir) does.
const plMake = function(o, x, z) {
    var zz = normalize(z);
    var xp = x - zz * dot(x, zz);
    if (norm(xp) < 1e-12) cqErr("xDir parallel to the normal");
    return {"o": o, "x": normalize(xp), "z": zz};
};
const plY = function(p) { return cross(p.z, p.x); };
const plW = function(p, l) {            // Plane.toWorldCoords
    var v = v3(l);
    return p.o + p.x * v[0] + plY(p) * v[1] + p.z * v[2];
};
const plL = function(p, w) {            // Plane.toLocalCoords (points)
    var d = w - p.o;
    return vector(dot(d, p.x), dot(d, plY(p)), dot(d, p.z));
};
const plT = function(p) { return toWorld(coordSystem(p.o * MM, p.x, p.z)); };   // Location(plane): local -> world
const plFromTape = function(m) { return plMake(v3(m.origin), v3(m.xDir), v3(m.zDir)); };
const plRotated = function(p, r) {      // Plane.rotated: T = T1 * T2 * T3 about the plane's x, y, z
    var rv = v3(r);
    var T = rotationMatrix3d(p.x, rv[0] * degree) * rotationMatrix3d(plY(p), rv[1] * degree) * rotationMatrix3d(p.z, rv[2] * degree);
    return plMake(p.o, T * p.x, T * p.z);
};
// gp_Ax2(P, V): OCCT's X direction for a normal alone (Plane(normal=...) with xDir None)
const defaultXDir = function(n) {
    const A = n[0]; const B = n[1]; const C = n[2];
    const Aa = abs(A); const Ba = abs(B); const Ca = abs(C);
    var d;
    if (Ba <= Aa && Ba <= Ca) d = (Aa > Ca) ? vector(-C, 0, A) : vector(C, 0, -A);
    else if (Aa <= Ba && Aa <= Ca) d = (Ba > Ca) ? vector(0, -C, B) : vector(0, C, -B);
    else d = (Aa > Ba) ? vector(-B, A, 0) : vector(B, -A, 0);
    return normalize(d);
};
const locFromTape = function(m) {       // {"R": 3x3 rows, "t": [x, y, z]} -> Transform (local)
    return transform(matrix(m.R), v3(m.t) * MM);
};

// ---- state
const wpGet = function(st, i) {
    var w = st[].wps[i];
    if (w == undefined) cqErr("no workplane " ~ i);
    return w;
};
const wpPut = function(st, i, w) { var s = st[]; s.wps[i] = w; st[] = s; };
const ctxGet = function(st, w) { return st[].ctx[w.ctx]; };
const ctxPut = function(st, w, cx) { var s = st[]; s.ctx[w.ctx] = cx; st[] = s; };
const newObject = function(st, out, selfI, objs) {      // Workplane.newObject
    var w = wpGet(st, selfI);
    wpPut(st, out, {"plane": w.plane, "objs": objs, "parent": selfI, "ctx": w.ctx});
};
const shapeGet = function(st, i) {
    var s = st[].shapes[i];
    if (s == undefined) cqErr("no shape " ~ i);
    return s;
};
const logStep = function(c, st, id, op, msg) {
    var s = st[];
    s.log = append(s.log, {"id": id, "op": op, "msg": msg});
    st[] = s;
};
const warn = function(st, msg) {
    var s = st[];
    if (!isIn(msg, s.warn)) s.warn = append(s.warn, msg);
    st[] = s;
};

// ---- data edges / wires (world coordinates, mm)
const eStart = function(e) { return (e.k == "circle" || e.k == "ellipse") ? e.s : (e.k == "spline" ? e.pts[0] : e.a); };
const eEnd = function(e) { return (e.k == "circle" || e.k == "ellipse") ? e.s : (e.k == "spline" ? e.pts[size(e.pts) - 1] : e.b); };
const eLine = function(a, b) { return {"t": "edge", "k": "line", "a": a, "b": b}; };
const eArc = function(a, m, b) { return {"t": "edge", "k": "arc", "a": a, "m": m, "b": b}; };
const eXform = function(T, e) {
    if (e.k == "line") return eLine(tP(T, e.a), tP(T, e.b));
    if (e.k == "arc") return eArc(tP(T, e.a), tP(T, e.m), tP(T, e.b));
    if (e.k == "circle") return {"t": "edge", "k": "circle", "c": tP(T, e.c), "n": tD(T, e.n), "r": e.r, "s": tP(T, e.s)};
    if (e.k == "ellipse") return {"t": "edge", "k": "ellipse", "c": tP(T, e.c), "n": tD(T, e.n), "xa": tD(T, e.xa), "r1": e.r1, "r2": e.r2, "s": tP(T, e.s)};
    if (e.k == "spline") {
        var ps = [];
        for (var p in e.pts) ps = append(ps, tP(T, p));
        var ts = undefined;
        if (e.tans != undefined) { ts = []; for (var d in e.tans) ts = append(ts, d == undefined ? undefined : tD(T, d)); }
        return {"t": "edge", "k": "spline", "pts": ps, "tans": ts, "periodic": e.periodic};
    }
    cqErr("edge kind " ~ e.k);
};
const wXform = function(T, w) {
    var es = [];
    for (var e in w.edges) es = append(es, eXform(T, e));
    return {"t": "wire", "edges": es, "closed": w.closed, "constr": w.constr};
};
// circumcentre of three points, for arcs
const circum = function(a, m, b) {
    const u = m - a; const v = b - a;
    const n = cross(u, v);
    const nn = dot(n, n);
    if (nn < 1e-24) cqErr("arc through collinear points");
    return a + (cross(n, u) * dot(v, v) + cross(v, n) * dot(u, u)) / (2 * nn);
};
// length and centroid of a data edge (GProp linear properties)
const eLenCen = function(e) {
    if (e.k == "line") return {"L": norm(e.b - e.a), "C": (e.a + e.b) / 2};
    if (e.k == "circle") return {"L": 2 * PI * e.r, "C": e.c};
    if (e.k == "ellipse") {
        const h = ((e.r1 - e.r2) / (e.r1 + e.r2)) ^ 2;
        return {"L": PI * (e.r1 + e.r2) * (1 + 3 * h / (10 + sqrt(4 - 3 * h))), "C": e.c};
    }
    if (e.k == "arc") {
        const cc = circum(e.a, e.m, e.b);
        const r = norm(e.a - cc);
        const ua = unitOf(e.a - cc); const ub = unitOf(e.b - cc); const um = unitOf(e.m - cc);
        const n = unitOf(cross(e.a - cc, e.m - cc) + cross(e.m - cc, e.b - cc));
        var ang = atan2(dot(cross(ua, ub), n), dot(ua, ub)) / radian;
        if (ang <= 0) ang += 2 * PI;
        const bis = unitOf(rotationMatrix3d(n, ang / 2 * radian) * ua);
        const half = ang / 2;
        return {"L": r * ang, "C": cc + bis * (r * sin(half * radian) / half)};
    }
    if (e.k == "spline") {   // by its control polygon: only used to centre a wire made of it
        var L = 0; var acc = vector(0, 0, 0);
        for (var j = 0; j + 1 < size(e.pts); j += 1) { const l = norm(e.pts[j + 1] - e.pts[j]); L += l; acc += (e.pts[j] + e.pts[j + 1]) / 2 * l; }
        return {"L": L, "C": acc / L};
    }
    cqErr("edge kind " ~ e.k);
};
const wCenter = function(w) {
    var L = 0; var acc = vector(0, 0, 0);
    for (var e in w.edges) { const lc = eLenCen(e); L += lc.L; acc += lc.C * lc.L; }
    if (L <= 0) cqErr("empty wire");
    return acc / L;
};
// wire plane: circle/ellipse normal, else Newell's normal of the polygon through the points
const wPoints = function(w) {
    var ps = [];
    for (var e in w.edges) {
        if (e.k == "line") ps = concatenateArrays([ps, [e.a, e.b]]);
        else if (e.k == "arc") ps = concatenateArrays([ps, [e.a, e.m, e.b]]);
        else if (e.k == "circle" || e.k == "ellipse") ps = append(ps, e.c);
        else if (e.k == "spline") ps = concatenateArrays([ps, e.pts]);
    }
    return ps;
};
const wPlane = function(w) {
    for (var e in w.edges) if (e.k == "circle" || e.k == "ellipse") return plMake(e.c, e.k == "ellipse" ? e.xa : defaultXDir(unitOf(e.n)), e.n);
    const ps = wPoints(w);
    var n = vector(0, 0, 0);
    for (var i = 0; i < size(ps); i += 1) {
        const p = ps[i]; const q = ps[(i + 1) % size(ps)];
        n += vector((p[1] - q[1]) * (p[2] + q[2]), (p[2] - q[2]) * (p[0] + q[0]), (p[0] - q[0]) * (p[1] + q[1]));
    }
    if (norm(n) < 1e-12) cqErr("Cannot build face(s): wires not planar");
    var cen = vector(0, 0, 0);
    for (var p in ps) cen += p;
    cen = cen / size(ps);
    return plMake(cen, defaultXDir(unitOf(n)), n);
};

// ---- topology helpers (Parasolid answers)
const ents = function(c, q) { return evaluateQuery(c, q); };
const entObj = function(t, q, body) { return {"t": t, "q": q, "body": body}; };
const centroid = function(c, q) { return unlen(evApproximateCentroid(c, {"entities": q})); };
const objCenter = function(c, o) {      // Shape.Center() / Vector.Center()
    if (o.t == "vec") return o.p;
    if (o.t == "vertex") return unlen(evVertexPoint(c, {"vertex": o.q}));
    if (o.t == "face" || o.t == "edgeq" || o.t == "solid") return centroid(c, o.q);
    if (o.t == "wire") return wCenter(o);
    if (o.t == "edge") return eLenCen(o).C;
    cqErr("Center() of " ~ o.t);
};
const faceGeom = function(c, q) {       // Face.geomType()
    const s = evSurfaceDefinition(c, {"face": q});
    if (s is Plane) return "PLANE";
    if (s is Cylinder) return "CYLINDER";
    if (s is Cone) return "CONE";
    if (s is Sphere) return "SPHERE";
    if (s is Torus) return "TORUS";
    if (s is BSplineSurface) return "BSPLINE";
    return "OTHER";
};
const edgeGeom = function(c, q) {       // Edge.geomType()
    const d = evCurveDefinition(c, {"edge": q});
    if (d is Line) return "LINE";
    if (d is Circle) return "CIRCLE";
    if (d is Ellipse) return "ELLIPSE";
    if (d is BSplineCurve) return "BSPLINE";
    return "OTHER";
};
const geomType = function(c, o) {
    if (o.t == "face") return faceGeom(c, o.q);
    if (o.t == "edgeq") return edgeGeom(c, o.q);
    if (o.t == "edge") return o.k == "line" ? "LINE" : (o.k == "ellipse" ? "ELLIPSE" : "CIRCLE");
    return "OTHER";
};
const faceNormalAt = function(c, q, p) {   // Face.normalAt(point): outward for a solid's faces
    if (faceGeom(c, q) == "PLANE") return evPlane(c, {"face": q}).normal;
    const d = evDistance(c, {"side0": p * MM, "side1": q});
    return evFaceTangentPlane(c, {"face": q, "parameter": d.sides[1].parameter}).normal;
};
const faceNormal0 = function(c, q) {    // Face.normalAt() with no point: at the middle of its UV bounds
    if (faceGeom(c, q) == "PLANE") return evPlane(c, {"face": q}).normal;
    return evFaceTangentPlane(c, {"face": q, "parameter": vector(0.5, 0.5)}).normal;
};
const edgeTangent = function(c, q) {    // Edge.tangentAt() (locationParam 0.5)
    return evEdgeTangentLine(c, {"edge": q, "parameter": 0.5, "arcLengthParameterization": true}).direction;
};
const bodiesOf = function(c, o) {       // owning bodies of a shape object
    if (o.t == "solid") return o.q;
    if (o.body != undefined) return o.body;
    cqErr("no body for " ~ o.t);
};

// _collectProperty: the unique sub-shapes of every object on the stack, in stack order
const collect = function(c, objs, kind) {
    var cand = [];    // [type, query, body] in stack order, duplicates removed below
    for (var o in objs) {
        if (o.t == "solid") {
            for (var b in ents(c, o.q)) {
                if (kind == "Solids") cand = append(cand, ["solid", b, b]);
                if (kind == "Faces") for (var f in ents(c, qOwnedByBody(b, EntityType.FACE))) cand = append(cand, ["face", f, b]);
                if (kind == "Edges") for (var e in ents(c, qOwnedByBody(b, EntityType.EDGE))) cand = append(cand, ["edgeq", e, b]);
                if (kind == "Vertices") for (var v in ents(c, qOwnedByBody(b, EntityType.VERTEX))) cand = append(cand, ["vertex", v, b]);
            }
        } else if (o.t == "face") {
            if (kind == "Faces") cand = append(cand, ["face", o.q, o.body]);
            if (kind == "Edges") for (var e in ents(c, qAdjacent(o.q, AdjacencyType.EDGE, EntityType.EDGE))) cand = append(cand, ["edgeq", e, o.body]);
            if (kind == "Vertices") for (var v in ents(c, qAdjacent(o.q, AdjacencyType.VERTEX, EntityType.VERTEX))) cand = append(cand, ["vertex", v, o.body]);
        } else if (o.t == "edgeq") {
            if (kind == "Edges") cand = append(cand, ["edgeq", o.q, o.body]);
            if (kind == "Vertices") for (var v in ents(c, qAdjacent(o.q, AdjacencyType.VERTEX, EntityType.VERTEX))) cand = append(cand, ["vertex", v, o.body]);
        } else if (o.t == "vertex") {
            if (kind == "Vertices") cand = append(cand, ["vertex", o.q, o.body]);
        } else if (o.t == "wire" || o.t == "edge") {
            if (kind == "Edges" || kind == "Vertices" || kind == "Wires") cqUnsup("selecting " ~ kind ~ " of pending wires");
        }
    }
    var out = [];
    var seen = {};
    for (var x in cand) {
        const k = toString(x[1]);
        if (seen[k] != true) {
            seen[k] = true;
            out = append(out, entObj(x[0], x[1], x[2]));
        }
    }
    return out;
};

// ---- selectors (selectors.py). `sel` is the compiled form cq2fs emits.
const objKey = function(o) { return toString(o.q); };
const selFilter = function(self, c, sel, objs) {
    const k = sel.k;
    if (k == "and" || k == "or" || k == "exc") {
        const a = self(self, c, sel.a, objs);
        const b = self(self, c, sel.b, objs);
        var inB = {};
        for (var o in b) inB[objKey(o)] = true;
        var inA = {};
        for (var o in a) inA[objKey(o)] = true;
        var res = [];
        if (k == "and") { for (var o in a) if (inB[objKey(o)] == true) res = append(res, o); }
        else if (k == "exc") { for (var o in a) if (inB[objKey(o)] != true) res = append(res, o); }
        else {
            res = a;
            for (var o in b) if (inA[objKey(o)] != true) { res = append(res, o); inA[objKey(o)] = true; }
        }
        return res;
    }
    if (k == "not") {
        const na = self(self, c, sel.a, objs);
        var inN = {};
        for (var o in na) inN[objKey(o)] = true;
        var rn = [];
        for (var o in objs) if (inN[objKey(o)] != true) rn = append(rn, o);
        return rn;
    }
    if (k == "type") {
        var r = [];
        for (var o in objs) if (geomType(c, o) == sel.ty) r = append(r, o);
        return r;
    }
    if (k == "dir" || k == "par" || k == "perp") {     // BaseDirSelector subclasses
        const d = v3(sel.v);
        var r = [];
        for (var o in objs) {
            var tv;
            if (o.t == "face" && faceGeom(c, o.q) == "PLANE") tv = faceNormal0(c, o.q);
            else if (o.t == "edgeq" && edgeGeom(c, o.q) == "LINE") tv = edgeTangent(c, o.q);
            else continue;
            var ok;
            if (k == "par") ok = norm(cross(d, tv)) < CQTOL;
            else {
                const ang = acos(max(-1, min(1, dot(d, tv) / (norm(d) * norm(tv))))) / radian;
                ok = (k == "dir") ? ang < CQTOL : abs(ang - PI / 2) < CQTOL;
            }
            if (ok) r = append(r, o);
        }
        return r;
    }
    if (k == "nth") {     // _NthSelector.filter / cluster; key "center" (CenterNth), "radius", "length", "area"
        var pool = objs;
        if (sel.parallel == true) pool = self(self, c, {"k": "par", "v": sel.v}, objs);
        if (size(pool) == 0) cqErr("Can not return the Nth element of an empty list");
        var keyed = [];
        for (var o in pool) {
            var key;
            if (sel.key == "center") key = dot(objCenter(c, o), v3(sel.v));
            else if (sel.key == "radius") {
                if (o.t != "edgeq") continue;
                const cd = evCurveDefinition(c, {"edge": o.q});
                if (!(cd is Circle)) continue;
                key = cd.radius / MM;
            } else if (sel.key == "length") {
                if (o.t != "edgeq") cqErr("LengthNthSelector supports only Edges and Wires");
                key = evLength(c, {"entities": o.q}) / MM;
            } else if (sel.key == "area") {
                if (o.t != "face" && o.t != "solid") cqErr("AreaNthSelector: unsupported object");
                key = evArea(c, {"entities": o.t == "face" ? o.q : qOwnedByBody(o.q, EntityType.FACE)}) / (MM * MM);
            }
            keyed = append(keyed, {"key": key, "o": o, "i": size(keyed)});
        }
        if (size(keyed) == 0) cqErr("Can not return the Nth element of an empty list");
        keyed = sort(keyed, function(a, b) { return (a.key != b.key) ? a.key - b.key : a.i - b.i; });
        var clusters = [[keyed[0].o]];
        var start = keyed[0].key;
        for (var j = 1; j < size(keyed); j += 1) {
            if (abs(keyed[j].key - start) <= CQTOL) clusters[size(clusters) - 1] = append(clusters[size(clusters) - 1], keyed[j].o);
            else { clusters = append(clusters, [keyed[j].o]); start = keyed[j].key; }
        }
        if (sel.max != true) {
            var rev = [];
            for (var j = size(clusters) - 1; j >= 0; j -= 1) rev = append(rev, clusters[j]);
            clusters = rev;
        }
        var n = sel.n;
        if (n < 0) n += size(clusters);
        if (n < 0 || n >= size(clusters)) cqErr("Attempted to access index " ~ sel.n ~ " of a list with length " ~ size(clusters));
        return clusters[n];
    }
    cqUnsup("selector " ~ k);
};
const select = function(c, st, w, kind, sel, tagW) {    // Workplane._selectObjects
    const src = (tagW == undefined) ? w : tagW;
    const objs = collect(c, src.objs, kind);
    if (sel == undefined) return objs;
    return selFilter(selFilter, c, sel, objs);
};

// ---- _findType((Solid,), searchStack, searchParents): the solids as one query, or undefined
const findSolidQ = function(st, i, searchStack, searchParents) {
    var cur = i;
    var first = true;
    while (cur != undefined) {
        const w = wpGet(st, cur);
        if (searchStack || !first) {
            var qs = [];
            for (var o in w.objs) if (o.t == "solid") qs = append(qs, o.q);
            if (size(qs) > 0) return qUnion(qs);
        }
        first = false;
        if (!searchParents) break;
        cur = w.parent;
    }
    return undefined;
};
const findSolid = function(st, i) {
    const q = findSolidQ(st, i, true, true);
    if (q == undefined) cqErr("Cannot find a solid on the stack or in the parent chain");
    return q;
};

// ---- body operations, always on copies
const copyBodies = function(c, st, q, T) {
    if (size(ents(c, q)) == 0) cqErr("no bodies to copy");
    const id = nid(st, "cp");
    opPattern(c, id, {"entities": q, "transforms": [T == undefined ? identityTransform() : T], "instanceNames": ["i"]});
    return qCreatedBy(id, EntityType.BODY);
};
// copy `base` with its selected entities tagged, so the copies' entities can be found
const copyWithEntities = function(c, st, base, entsQ) {
    const tagv = "s" ~ st[].n;
    var all = [];
    for (var e in entsQ) all = append(all, e.q);
    if (size(all) > 0) setAttribute(c, {"entities": qUnion(all), "name": "cqsel", "attribute": tagv});
    const b = copyBodies(c, st, base, undefined);
    return {"base": b, "tag": tagv};
};
const taggedIn = function(body, kind, tagv) {
    return qHasAttributeWithValue(qOwnedByBody(body, kind), "cqsel", tagv);
};
const boolOp = function(c, st, kind, baseQ, toolQ) {
    const b = copyBodies(c, st, baseQ, undefined);
    if (toolQ == undefined || size(ents(c, toolQ)) == 0) {
        if (kind == "union") return b;
        if (kind == "cut") return b;
        cqErr("intersect with nothing");
    }
    const t = copyBodies(c, st, toolQ, undefined);
    if (kind == "union") {
        opBoolean(c, nid(st, "bo"), {"tools": qUnion([b, t]), "operationType": BooleanOperationType.UNION});
        return qUnion([b, t]);
    }
    if (kind == "cut") {
        opBoolean(c, nid(st, "bo"), {"targets": b, "tools": t, "operationType": BooleanOperationType.SUBTRACTION});
        return b;
    }
    if (kind == "intersect") {
        // the tools are consumed, as SUBTRACTION's are
        opBoolean(c, nid(st, "bo"), {"targets": b, "tools": t, "operationType": BooleanOperationType.SUBTRACT_COMPLEMENT});
        return b;
    }
    cqErr("boolean " ~ kind);
};
// Compound.fuse() of a compound with itself: merge its overlapping solids
const selfFuse = function(c, st, q) {
    if (size(ents(c, q)) < 2) return q;
    const b = copyBodies(c, st, q, undefined);
    opBoolean(c, nid(st, "bo"), {"tools": b, "operationType": BooleanOperationType.UNION});
    return b;
};
const solidObj = function(q) { return {"t": "solid", "q": q}; };
const isShapeObj = function(o) { return o.t == "solid" || o.t == "face" || o.t == "edgeq" || o.t == "vertex" || o.t == "wire" || o.t == "edge"; };

// ---- faces from pending wires (wiresToFaces) as sketch regions; or selected faces
// Edge.makeSpline through the points (GeomAPI_Interpolate in CadQuery): Onshape's fit spline
// through the same points, with the end tangents where given. Both interpolate, but OCCT's
// parametrization may not be Onshape's: the result carries a warning.
const cqSkSpline = function(sk, nm, e, to2, P, st) {
    warn(st, "spline: Onshape's interpolation through the points, not OCCT's GeomAPI_Interpolate");
    var pts = [];
    for (var p in e.pts) pts = append(pts, to2(p));
    var def = {"points": pts};
    if (e.periodic == true) def.closed = true;
    if (e.tans != undefined && size(e.tans) >= 2) {
        const d2 = function(d) { const l = plL(P, P.o + d); return vector(l[0], l[1]) * MM; };
        if (e.tans[0] != undefined) def.startDerivative = d2(e.tans[0]);
        if (e.tans[size(e.tans) - 1] != undefined) def.endDerivative = d2(e.tans[size(e.tans) - 1]);
        if (size(e.tans) > 2) warn(st, "spline: tangents at inner points not given to Onshape");
    }
    skFitSpline(sk, nm, def);
};

const sketchRegions = function(c, st, wires) {
    const P = wPlane(wires[0]);
    const scale = 1;
    for (var w in wires) {
        if (!w.closed) cqErr("Cannot build face(s): wire is not closed");
        for (var p in wPoints(w)) if (abs(dot(p - P.o, P.z)) > 1e-6 * max(1, norm(p))) cqErr("Cannot build face(s): wires not planar");
        for (var e in w.edges) if (e.k == "circle" || e.k == "ellipse") if (norm(cross(unitOf(e.n), P.z)) > 1e-9) cqErr("Cannot build face(s): wires not planar");
    }
    const id = nid(st, "sk");
    var sk = newSketchOnPlane(c, id, {"sketchPlane": plane(P.o * MM, P.z, P.x)});
    const to2 = function(p) { const l = plL(P, p); return vector(l[0], l[1]) * MM; };
    var j = 0;
    for (var w in wires) {
        for (var e in w.edges) {
            j += 1;
            const nm = "e" ~ j;
            if (e.k == "line") {
                if (norm(e.b - e.a) > 1e-9) skLineSegment(sk, nm, {"start": to2(e.a), "end": to2(e.b)});
            } else if (e.k == "arc") skArc(sk, nm, {"start": to2(e.a), "mid": to2(e.m), "end": to2(e.b)});
            else if (e.k == "circle") skCircle(sk, nm, {"center": to2(e.c), "radius": e.r * MM});
            else if (e.k == "ellipse") {
                const xa = plL(P, P.o + e.xa);
                var major = vector(xa[0], xa[1]);
                var r1 = e.r1; var r2 = e.r2;
                if (r2 > r1) { major = vector(-major[1], major[0]); r1 = e.r2; r2 = e.r1; }
                skEllipse(sk, nm, {"center": to2(e.c), "majorRadius": r1 * MM, "minorRadius": r2 * MM, "majorAxis": normalize(major)});
            } else if (e.k == "spline") cqSkSpline(sk, nm, e, to2, P, st);
        }
    }
    skSolve(sk);
    const regions = qSketchRegion(id, true);
    const n = size(ents(c, regions));
    if (n == 0) cqErr("Cannot build face(s): no closed region");
    // a wire inside another: its region a hole in the other's face (an inner wire)
    return {"q": regions, "inner": size(ents(c, qSketchRegion(id, false))) > n};
};
const getFaces = function(c, st, selfI) {     // Workplane._getFaces
    const w = wpGet(st, selfI);
    var fq = [];
    for (var o in w.objs) if (o.t == "face") fq = append(fq, o.q);
    if (size(fq) > 0) {
        warn(st, "faces on the stack extruded: inner wires not checked for a taper");
        return {"q": qUnion(fq), "inner": false};
    }
    var cx = ctxGet(st, w);
    if (size(cx.pw) == 0) cqErr("No pending wires present");
    const wires = cx.pw;
    cx.pw = [];
    ctxPut(st, w, cx);
    return sketchRegions(c, st, wires);
};

// ---- eachpoint's placements (useLocalCoordinates=True): one world Transform per stack object
const placements = function(c, w) {
    const L = plT(w.plane);
    if (size(w.objs) == 0) return [L];
    var out = [];
    for (var o in w.objs) {
        if (o.t == "loc") out = append(out, L * o.T);
        else out = append(out, plT(plMake(objCenter(c, o), w.plane.x, w.plane.z)));
    }
    return out;
};
const addPendingWires = function(st, w, wires) {
    var cx = ctxGet(st, w);
    for (var x in wires) if (x.constr != true) cx.pw = append(cx.pw, x);
    ctxPut(st, w, cx);
};
const addPendingEdge = function(st, w, e) {    // Workplane._addPendingEdge
    var cx = ctxGet(st, w);
    cx.pe = append(cx.pe, e);
    if (cx.fp == undefined) cx.fp = plL(w.plane, eStart(e));
    ctxPut(st, w, cx);
};
const fromPoint = function(c, w, useLocal) {    // Workplane._findFromPoint
    var p;
    if (size(w.objs) == 0) p = w.plane.o;
    else {
        const o = w.objs[size(w.objs) - 1];
        if (o.t == "edge") p = eEnd(o);
        else if (o.t == "vec") p = o.p;
        else if (o.t == "vertex") p = objCenter(c, o);
        else cqErr("Cannot convert object type " ~ o.t ~ " to vector.");
    }
    return useLocal ? plL(w.plane, p) : p;
};

// ---- _combineWithBase(obj, mode, clean)
const combineWithBase = function(c, st, out, selfI, q, mode) {
    if (mode == true || mode == "a") {          // _fuseWithBase
        const base = findSolidQ(st, selfI, true, true);
        if (base != undefined) newObject(st, out, selfI, [solidObj(boolOp(c, st, "union", base, q))]);
        else newObject(st, out, selfI, [solidObj(selfFuse(c, st, q))]);
    } else if (mode == "cut" || mode == "s") {  // _cutFromBase
        const base = findSolidQ(st, selfI, true, true);
        if (base != undefined) newObject(st, out, selfI, [solidObj(boolOp(c, st, "cut", base, q))]);
        else newObject(st, out, selfI, [solidObj(q)]);
    } else if (mode == false) newObject(st, out, selfI, [solidObj(q)]);
    else cqErr("combine mode " ~ toString(mode));
};

// a Workplane argument's shapes (_selectShapes(wp.vals())) or a shape value
const argShapes = function(c, st, a) {
    if (a is map && a.wp != undefined) return wpGet(st, a.wp).objs;
    if (a is map && a.shape != undefined) return [shapeGet(st, a.shape)];
    cqErr("not a shape argument: " ~ toString(a));
};
const solidsQ = function(c, objs) {
    var qs = [];
    for (var o in objs) if (o.t == "solid") qs = append(qs, o.q);
    return size(qs) == 0 ? undefined : qUnion(qs);
};

// ================================================================================================
// Workplane methods. Each: (c, st, out, selfI, a) with a = the bound arguments.

const cqWorkplaneNew = function(c, st, out, a) {      // Workplane.__init__
    var s = st[];
    const ci = size(s.ctx);
    s.ctx = append(s.ctx, {"pw": [], "pe": [], "fp": undefined, "tags": {}});
    st[] = s;
    var objs = [];
    if (a.obj != undefined) objs = argShapes(c, st, a.obj);
    wpPut(st, out, {"plane": plFromTape(a.plane), "objs": objs, "parent": undefined, "ctx": ci});
};

const cqWorkplane = function(c, st, out, selfI, a) {   // Workplane.workplane
    const w = wpGet(st, selfI);
    const computeXdir = function(normal) {
        var xd = cross(vector(0, 0, 1), normal);
        if (norm(xd) < CQTOL) xd = vector(1, 0, 0);
        return xd;
    };
    var center; var normal; var xDir;
    if (size(w.objs) > 1) {
        var fqs = [];
        for (var o in w.objs) {
            if (o.t != "face" || (faceGeom(c, o.q) != "PLANE")) cqErr("If multiple objects selected, they all must be planar faces.");
            fqs = append(fqs, o.q);
        }
        const n0 = faceNormal0(c, fqs[0]);
        const p0 = centroid(c, fqs[0]);
        for (var j = 1; j < size(fqs); j += 1) {
            const n1 = faceNormal0(c, fqs[j]);
            const p1 = centroid(c, fqs[j]);
            const dn = n0 - n1;
            if (!(abs(dn[0]) < CQTOL || abs(dn[1]) < CQTOL || abs(dn[2]) < CQTOL)) cqErr("Selected faces must be co-planar.");
            if (abs(dot(n0, p0 - p1)) >= CQTOL) cqErr("Selected faces must be co-planar.");
        }
        if (a.centerOption == "CenterOfBoundBox") {
            const bb = evBox3d(c, {"topology": qUnion(fqs), "tight": true});
            center = unlen((bb.minCorner + bb.maxCorner) / 2);
            warn(st, "CenterOfBoundBox: Onshape's tight box, CadQuery's is OCCT's (enlarged)");
        } else center = centroid(c, qUnion(fqs));
        normal = n0;
        xDir = computeXdir(normal);
    } else {
        const obj = (size(w.objs) == 0) ? {"t": "vec", "p": w.plane.o} : w.objs[0];
        if (obj.t == "face") {
            if (a.centerOption == "CenterOfBoundBox") {
                const bb = evBox3d(c, {"topology": obj.q, "tight": true});
                center = unlen((bb.minCorner + bb.maxCorner) / 2);
                warn(st, "CenterOfBoundBox: Onshape's tight box, CadQuery's is OCCT's (enlarged)");
            } else center = objCenter(c, obj);
            normal = faceNormalAt(c, obj.q, center);
            xDir = computeXdir(normal);
        } else if (obj.t == "vec" || isShapeObj(obj)) {
            if (a.centerOption == "CenterOfBoundBox" && obj.t != "vec") {
                const bb = evBox3d(c, {"topology": bodiesOf(c, obj), "tight": true});
                center = unlen((bb.minCorner + bb.maxCorner) / 2);
                warn(st, "CenterOfBoundBox: Onshape's tight box, CadQuery's is OCCT's (enlarged)");
            } else center = objCenter(c, obj);
            var pv = undefined;
            if (w.parent != undefined) {
                const pw = wpGet(st, w.parent);
                if (size(pw.objs) > 0) pv = pw.objs[0];
            }
            if (pv != undefined && pv.t == "face") {
                normal = faceNormalAt(c, pv.q, center);
                xDir = computeXdir(normal);
            } else {
                normal = w.plane.z;
                xDir = w.plane.x;
            }
        } else cqErr("Needs a face or a vertex or point on a work plane");
    }
    if (a.centerOption == "ProjectedOrigin") {
        const orig = (a.origin == undefined) ? w.plane.o : v3(a.origin);
        center = orig - normal * (dot(orig - center, normal) / dot(normal, normal));
    } else if (a.centerOption != "CenterOfMass" && a.centerOption != "CenterOfBoundBox") cqErr("Undefined centerOption value provided.");
    if (a.invert == true) normal = -normal;
    const offsetCenter = center + normalize(normal) * a.offset;
    wpPut(st, out, {"plane": plMake(offsetCenter, xDir, normal), "objs": [], "parent": selfI, "ctx": w.ctx});
};

const cqTransformed = function(c, st, out, selfI, a) {   // Workplane.transformed
    const w = wpGet(st, selfI);
    var p = plRotated(w.plane, a.rotate);
    p.o = plW(w.plane, a.offset);
    wpPut(st, out, {"plane": p, "objs": [{"t": "vec", "p": p.o}], "parent": selfI, "ctx": w.ctx});
};

const cqCenter = function(c, st, out, selfI, a) {   // Workplane.center
    const w = wpGet(st, selfI);
    const o = plW(w.plane, [a.x, a.y]);
    var p = w.plane;
    p.o = o;
    wpPut(st, out, {"plane": p, "objs": [{"t": "vec", "p": o}], "parent": selfI, "ctx": w.ctx});
};

const cqMoveTo = function(c, st, out, selfI, a) {   // Workplane.moveTo
    const w = wpGet(st, selfI);
    newObject(st, out, selfI, [{"t": "vec", "p": plW(w.plane, [a.x, a.y, 0])}]);
};

const cqMove = function(c, st, out, selfI, a) {     // Workplane.move
    const w = wpGet(st, selfI);
    const p = fromPoint(c, w, true);
    newObject(st, out, selfI, [{"t": "vec", "p": plW(w.plane, p + vector(a.xDist, a.yDist, 0))}]);
};

const pushPts = function(c, st, out, selfI, pts) {   // Workplane.pushPoints
    const w = wpGet(st, selfI);
    var objs = [];
    for (var p in pts) {
        if (p is map && p.loc != undefined) objs = append(objs, {"t": "loc", "T": locFromTape(p.loc)});
        else objs = append(objs, {"t": "vec", "p": plW(w.plane, p)});
    }
    newObject(st, out, selfI, objs);
};
const cqPushPoints = function(c, st, out, selfI, a) { pushPts(c, st, out, selfI, a.pntList); };

const cqRarray = function(c, st, out, selfI, a) {   // Workplane.rarray
    if ((a.xSpacing <= 0 && a.ySpacing <= 0) || a.xCount < 1 || a.yCount < 1) cqErr("Spacing and count must be > 0 in at least one direction");
    const cen = (a.center is boolean) ? [a.center, a.center] : a.center;
    var pts = [];
    for (var x = 0; x < a.xCount; x += 1)
        for (var y = 0; y < a.yCount; y += 1)
            pts = append(pts, [a.xSpacing * x - (cen[0] ? a.xSpacing * (a.xCount - 1) * 0.5 : 0),
                               a.ySpacing * y - (cen[1] ? a.ySpacing * (a.yCount - 1) * 0.5 : 0)]);
    pushPts(c, st, out, selfI, pts);
};

const pyRemainder = function(x, y) {   // math.remainder (IEEE): x - n*y, n the nearest integer (ties to even)
    const q = x / y;
    var n = round(q);
    if (abs(q - floor(q) - 0.5) < 1e-15) n = (floor(q) % 2 == 0) ? floor(q) : floor(q) + 1;
    return x - n * y;
};
const cqPolarArray = function(c, st, out, selfI, a) {   // Workplane.polarArray
    if (a.count < 1) cqErr("At least 1 element required");
    var angle = a.angle;
    if (a.fill) {
        if (abs(pyRemainder(angle, 360)) < TOL6) angle = angle / a.count;
        else angle = (a.count > 1) ? angle / (a.count - 1) : a.startAngle;
    }
    var locs = [];
    for (var i = 0; i < a.count; i += 1) {
        const phi = a.startAngle + angle * i;
        const x = a.radius * cos(phi * degree);
        const y = a.radius * sin(phi * degree);
        const R = a.rotate ? rotationMatrix3d(vector(0, 0, 1), phi * degree) : identityMatrix(3);
        locs = append(locs, {"loc": {"R": [[R[0][0], R[0][1], R[0][2]], [R[1][0], R[1][1], R[1][2]], [R[2][0], R[2][1], R[2][2]]], "t": [x, y, 0]}});
    }
    pushPts(c, st, out, selfI, locs);
};

// 2D: wires made at every placement (eachpoint), added to pending wires
const placeWire = function(c, st, out, selfI, localWire) {
    const w = wpGet(st, selfI);
    var objs = [];
    for (var T in placements(c, w)) objs = append(objs, wXform(T, localWire));
    addPendingWires(st, w, objs);
    newObject(st, out, selfI, objs);
};
const polyWire = function(pts, constr) {
    var es = [];
    for (var i = 0; i + 1 < size(pts); i += 1) es = append(es, eLine(pts[i], pts[i + 1]));
    return {"t": "wire", "edges": es, "closed": norm(pts[0] - pts[size(pts) - 1]) < 1e-9, "constr": constr};
};
const cqRect = function(c, st, out, selfI, a) {   // Workplane.rect
    const cen = (a.centered is boolean) ? [a.centered, a.centered] : a.centered;
    const off = vector(cen[0] ? 0 : a.xLen / 2, cen[1] ? 0 : a.yLen / 2, 0);
    const pts = [vector(-a.xLen / 2, -a.yLen / 2, 0) + off, vector(a.xLen / 2, -a.yLen / 2, 0) + off,
                 vector(a.xLen / 2, a.yLen / 2, 0) + off, vector(-a.xLen / 2, a.yLen / 2, 0) + off];
    placeWire(c, st, out, selfI, polyWire(append(pts, pts[0]), a.forConstruction));
};
const cqCircle = function(c, st, out, selfI, a) {   // Workplane.circle
    if (a.radius <= 0) cqErr("circle radius must be positive");
    placeWire(c, st, out, selfI, {"t": "wire", "closed": true, "constr": a.forConstruction,
        "edges": [{"t": "edge", "k": "circle", "c": vector(0, 0, 0), "n": vector(0, 0, 1), "r": a.radius, "s": vector(a.radius, 0, 0)}]});
};
const cqPolygon = function(c, st, out, selfI, a) {   // Workplane.polygon
    const angle = 2 * PI / a.nSides;
    var radius = a.diameter / 2;
    if (a.circumscribed) radius /= cos(angle / 2 * radian);
    var pts = [];
    for (var i = 0; i <= a.nSides; i += 1) {
        var o = angle * i;
        if (a.circumscribed) o += angle / 2;
        pts = append(pts, vector(radius * cos(o * radian), radius * sin(o * radian), 0));
    }
    placeWire(c, st, out, selfI, polyWire(pts, a.forConstruction));
};
const cqEllipse = function(c, st, out, selfI, a) {   // Workplane.ellipse (closed)
    const xa = rotationMatrix3d(vector(0, 0, 1), a.rotation_angle * degree) * vector(1, 0, 0);
    placeWire(c, st, out, selfI, {"t": "wire", "closed": true, "constr": a.forConstruction,
        "edges": [{"t": "edge", "k": "ellipse", "c": vector(0, 0, 0), "n": vector(0, 0, 1), "xa": xa, "r1": a.x_radius, "r2": a.y_radius, "s": xa * a.x_radius}]});
};

// edges drawn one at a time (pending edges)
const drawEdge = function(c, st, out, selfI, e, constr) {
    const w = wpGet(st, selfI);
    if (constr != true) addPendingEdge(st, w, e);
    newObject(st, out, selfI, [e]);
};
const cqLineTo = function(c, st, out, selfI, a) {    // Workplane.lineTo
    const w = wpGet(st, selfI);
    drawEdge(c, st, out, selfI, eLine(fromPoint(c, w, false), plW(w.plane, [a.x, a.y])), a.forConstruction);
};
const cqLine = function(c, st, out, selfI, a) {      // Workplane.line
    const w = wpGet(st, selfI);
    const p = fromPoint(c, w, true);
    cqLineTo(c, st, out, selfI, {"x": a.xDist + p[0], "y": a.yDist + p[1], "forConstruction": a.forConstruction});
};
const cqHLine = function(c, st, out, selfI, a) { cqLine(c, st, out, selfI, {"xDist": a.distance, "yDist": 0, "forConstruction": a.forConstruction}); };
const cqVLine = function(c, st, out, selfI, a) { cqLine(c, st, out, selfI, {"xDist": 0, "yDist": a.distance, "forConstruction": a.forConstruction}); };
const cqHLineTo = function(c, st, out, selfI, a) {
    const p = fromPoint(c, wpGet(st, selfI), true);
    cqLineTo(c, st, out, selfI, {"x": a.xCoord, "y": p[1], "forConstruction": a.forConstruction});
};
const cqVLineTo = function(c, st, out, selfI, a) {
    const p = fromPoint(c, wpGet(st, selfI), true);
    cqLineTo(c, st, out, selfI, {"x": p[0], "y": a.yCoord, "forConstruction": a.forConstruction});
};
const cqPolarLine = function(c, st, out, selfI, a) {
    cqLine(c, st, out, selfI, {"xDist": cos(a.angle * degree) * a.distance, "yDist": sin(a.angle * degree) * a.distance, "forConstruction": a.forConstruction});
};
const cqPolarLineTo = function(c, st, out, selfI, a) {
    cqLineTo(c, st, out, selfI, {"x": cos(a.angle * degree) * a.distance, "y": sin(a.angle * degree) * a.distance, "forConstruction": a.forConstruction});
};
const cqThreePointArc = function(c, st, out, selfI, a) {   // Workplane.threePointArc
    const w = wpGet(st, selfI);
    drawEdge(c, st, out, selfI, eArc(fromPoint(c, w, false), plW(w.plane, a.point1), plW(w.plane, a.point2)), a.forConstruction);
};
const cqPolyline = function(c, st, out, selfI, a) {   // Workplane.polyline
    const w = wpGet(st, selfI);
    var start; var pts;
    if (a.includeCurrent) { start = fromPoint(c, w, false); pts = a.listOfXYTuple; }
    else {
        start = plW(w.plane, a.listOfXYTuple[0]);
        pts = subArray(a.listOfXYTuple, 1, size(a.listOfXYTuple));
    }
    var edges = [];
    for (var t in pts) {
        const e = plW(w.plane, t);
        const ed = eLine(start, e);
        edges = append(edges, ed);
        start = e;
        if (!a.forConstruction) addPendingEdge(st, w, ed);
    }
    newObject(st, out, selfI, edges);
};
const cqWire = function(c, st, out, selfI, a) {       // Workplane.wire
    const w = wpGet(st, selfI);
    var cx = ctxGet(st, w);
    if (size(cx.pe) == 0) { wpPut(st, out, w); return; }
    const edges = cx.pe;
    cx.pe = [];
    ctxPut(st, w, cx);
    // Wire.assembleEdges: the edges in order, connected end to start
    for (var i = 0; i + 1 < size(edges); i += 1)
        if (norm(eEnd(edges[i]) - eStart(edges[i + 1])) > 1e-6) cqErr("assembleEdges: edges are not connected");
    const closed = norm(eEnd(edges[size(edges) - 1]) - eStart(edges[0])) < 1e-6;
    const wire = {"t": "wire", "edges": edges, "closed": closed, "constr": a.forConstruction};
    if (!a.forConstruction) addPendingWires(st, w, [wire]);
    var others = [];
    for (var o in w.objs) if (o.t != "edge") others = append(others, o);
    newObject(st, out, selfI, append(others, wire));
};
const cqClose = function(c, st, out, selfI, a) {      // Workplane.close
    const w = wpGet(st, selfI);
    const endPoint = fromPoint(c, w, true);
    const cx = ctxGet(st, w);
    if (cx.fp == undefined) cqErr("No start point specified - cannot close");
    const startPoint = cx.fp;
    if (norm(endPoint - startPoint) > 1e-6) {
        // self.polyline([endPoint, startPoint]): its result is discarded, its edge stays pending
        addPendingEdge(st, w, eLine(plW(w.plane, endPoint), plW(w.plane, startPoint)));
    }
    var cx2 = ctxGet(st, w);
    cx2.fp = undefined;
    ctxPut(st, w, cx2);
    cqWire(c, st, out, selfI, {"forConstruction": false});
};

// 3D
// Solid.extrudeLinear's taper (LocOpe_DPrism, its height divided by cos(taper) so the solid is
// `dist` tall): every side face leans in by `taper` degrees from the extrusion's direction,
// about the face it starts from; out for a negative taper. Onshape's extrude does the same with
// opDraft about its start cap (std extrude.fs, extrudeWithDraft).
const taperDraft = function(c, st, id, dir, taper) {
    for (var b in ents(c, qCreatedBy(id, EntityType.BODY))) {
        const start = qOwnedByBody(qCapEntity(id, CapType.START, EntityType.FACE), b);
        const end = qOwnedByBody(qCapEntity(id, CapType.END, EntityType.FACE), b);
        opDraft(c, nid(st, "dr"), {"draftType": DraftType.NEUTRAL_PLANE, "neutralPlane": start,
                                   "draftFaces": qSubtraction(qOwnedByBody(b, EntityType.FACE), qUnion([start, end])),
                                   "pullVec": dir * (taper > 0 ? 1 : -1), "angle": abs(taper) * degree,
                                   "tangentPropagation": false, "reFillet": false});
    }
};
const extrudeRaw = function(c, st, selfI, dist, both, taper) {   // Workplane._extrude (distance)
    const w = wpGet(st, selfI);
    const faces = getFaces(c, st, selfI);
    if (abs(dist) < 1e-12) cqErr("extrude distance is zero");
    const t = (taper == undefined) ? 0 : taper;
    const dir = w.plane.z * (dist > 0 ? 1 : -1);
    if (t == 0) {
        const id = nid(st, "ex");
        var def = {"entities": faces.q, "direction": dir, "endBound": BoundingType.BLIND, "endDepth": abs(dist) * MM};
        if (both) { def.startBound = BoundingType.BLIND; def.startDepth = abs(dist) * MM; }
        opExtrude(c, id, def);
        return qCreatedBy(id, EntityType.BODY);
    }
    if (faces.inner) cqErr("Inner wires not allowed with tapered extrusion");
    // each way its own tapered extrusion (both: s1 fused with s2)
    var qs = [];
    for (var d in (both ? [dir, -dir] : [dir])) {
        const id = nid(st, "ex");
        opExtrude(c, id, {"entities": faces.q, "direction": d, "endBound": BoundingType.BLIND, "endDepth": abs(dist) * MM});
        taperDraft(c, st, id, d, t);
        qs = append(qs, qCreatedBy(id, EntityType.BODY));
    }
    if (size(qs) > 1) opBoolean(c, nid(st, "bo"), {"tools": qUnion(qs), "operationType": BooleanOperationType.UNION});
    return qUnion(qs);
};
const cqCutBlind = function(c, st, out, selfI, a) {   // Workplane.cutBlind
    if (!(a.until is number)) cqUnsup("cutBlind until " ~ toString(a.until));
    const toCut = extrudeRaw(c, st, selfI, a.until, a.both == true, a.taper);
    const base = findSolid(st, selfI);
    newObject(st, out, selfI, [solidObj(boolOp(c, st, "cut", base, toCut))]);
};
const cqExtrude = function(c, st, out, selfI, a) {    // Workplane.extrude
    if (a.combine == "cut" || a.combine == "s") return cqCutBlind(c, st, out, selfI, {"until": a.until, "both": a.both, "taper": a.taper});
    if (!(a.until is number)) cqUnsup("extrude until " ~ toString(a.until));
    const r = extrudeRaw(c, st, selfI, a.until, a.both == true, a.taper);
    combineWithBase(c, st, out, selfI, r, a.combine);
};
const cqCutThruAll = function(c, st, out, selfI, a) {   // Workplane.cutThruAll: both directions
    if (a.taper != 0) cqUnsup("cutThruAll with a taper");
    const w = wpGet(st, selfI);
    const base = findSolid(st, selfI);
    const faces = getFaces(c, st, selfI).q;
    const bb = evBox3d(c, {"topology": qUnion([base, faces]), "tight": false});
    const depth = norm(bb.maxCorner - bb.minCorner) / MM * 2 + 1;
    const id = nid(st, "ex");
    opExtrude(c, id, {"entities": faces, "direction": w.plane.z, "endBound": BoundingType.BLIND, "endDepth": depth * MM,
                      "startBound": BoundingType.BLIND, "startDepth": depth * MM});
    newObject(st, out, selfI, [solidObj(boolOp(c, st, "cut", base, qCreatedBy(id, EntityType.BODY)))]);
};
const largestDimension = function(c, st, selfI) {   // findSolid().BoundingBox().DiagonalLength
    const bb = evBox3d(c, {"topology": findSolid(st, selfI), "tight": false});
    return norm(bb.maxCorner - bb.minCorner) / MM;
};
// a cylinder (radius r, along local -Z from local z0 to z0 - h) placed by T
const cylAlongNegZ = function(c, st, T, r, z0, h) {
    const id = nid(st, "cy");
    fCylinder(c, id, {"bottomCenter": T * (vector(0, 0, z0 - h) * MM), "topCenter": T * (vector(0, 0, z0) * MM), "radius": r * MM});
    return qCreatedBy(id, EntityType.BODY);
};
const cutEach = function(c, st, out, selfI, toolsAt) {   // Workplane.cutEach
    const w = wpGet(st, selfI);
    const base = findSolid(st, selfI);
    var tools = [];
    for (var T in placements(c, w)) tools = append(tools, toolsAt(T));
    newObject(st, out, selfI, [solidObj(boolOp(c, st, "cut", base, qUnion(tools)))]);
};
const cqHole = function(c, st, out, selfI, a) {     // Workplane.hole
    const depth = (a.depth == undefined) ? largestDimension(c, st, selfI) : a.depth;
    cutEach(c, st, out, selfI, function(T) { return cylAlongNegZ(c, st, T, a.diameter / 2, 0, depth); });
};
const cqCboreHole = function(c, st, out, selfI, a) {   // Workplane.cboreHole
    const depth = (a.depth == undefined) ? largestDimension(c, st, selfI) : a.depth;
    cutEach(c, st, out, selfI, function(T) {
        return qUnion([cylAlongNegZ(c, st, T, a.diameter / 2, 0, depth), cylAlongNegZ(c, st, T, a.cboreDiameter / 2, 0, a.cboreDepth)]);
    });
};
const cqCskHole = function(c, st, out, selfI, a) {     // Workplane.cskHole
    const depth = (a.depth == undefined) ? largestDimension(c, st, selfI) : a.depth;
    const r = a.cskDiameter / 2;
    const h = r / tan(a.cskAngle / 2 * degree);
    cutEach(c, st, out, selfI, function(T) {
        const id = nid(st, "co");
        fCone(c, id, {"bottomCenter": T * (vector(0, 0, 0) * MM), "topCenter": T * (vector(0, 0, -h) * MM), "bottomRadius": r * MM, "topRadius": 0 * MM});
        return qUnion([cylAlongNegZ(c, st, T, a.diameter / 2, 0, depth), qCreatedBy(id, EntityType.BODY)]);
    });
};
const solidsAtPlacements = function(c, st, out, selfI, a, make) {   // eachpoint(..., True, combine, clean)
    const w = wpGet(st, selfI);
    var qs = [];
    for (var T in placements(c, w)) qs = append(qs, make(T));
    combineWithBase(c, st, out, selfI, qUnion(qs), a.combine);
};
const cqBox = function(c, st, out, selfI, a) {      // Workplane.box
    const cen = (a.centered is boolean) ? [a.centered, a.centered, a.centered] : a.centered;
    const off = vector(cen[0] ? -a.length / 2 : 0, cen[1] ? -a.width / 2 : 0, cen[2] ? -a.height / 2 : 0);
    if (a.length <= 0 || a.width <= 0 || a.height <= 0) cqErr("box with a non-positive dimension");
    solidsAtPlacements(c, st, out, selfI, a, function(T) {
        const id = nid(st, "bx");
        fCuboid(c, id, {"corner1": off * MM, "corner2": (off + vector(a.length, a.width, a.height)) * MM});
        opTransform(c, nid(st, "tr"), {"bodies": qCreatedBy(id, EntityType.BODY), "transform": T});
        return qCreatedBy(id, EntityType.BODY);
    });
};
const cqSphere = function(c, st, out, selfI, a) {   // Workplane.sphere (whole spheres)
    if (a.angle1 != -90 || a.angle2 != 90 || a.angle3 != 360) cqUnsup("partial sphere");
    const cen = (a.centered is boolean) ? [a.centered, a.centered, a.centered] : a.centered;
    const off = vector(cen[0] ? 0 : a.radius, cen[1] ? 0 : a.radius, cen[2] ? 0 : a.radius);
    solidsAtPlacements(c, st, out, selfI, a, function(T) {
        const id = nid(st, "sp");
        opSphere(c, id, {"center": T * (off * MM), "radius": a.radius * MM});
        return qCreatedBy(id, EntityType.BODY);
    });
};
const cqCylinder = function(c, st, out, selfI, a) {   // Workplane.cylinder (whole cylinders)
    if (a.angle != 360) cqUnsup("partial cylinder");
    const cen = (a.centered is boolean) ? [a.centered, a.centered, a.centered] : a.centered;
    const off = vector(cen[0] ? 0 : a.radius, cen[1] ? 0 : a.radius, cen[2] ? -a.height / 2 : 0);
    const d = normalize(v3(a.direct));
    const R = toWorld(coordSystem(vector(0, 0, 0) * MM, defaultXDir(d), d));   // Plane(Vector(), normal=direct).location
    solidsAtPlacements(c, st, out, selfI, a, function(T) {
        const id = nid(st, "cy");
        fCylinder(c, id, {"bottomCenter": (T * R) * (off * MM), "topCenter": (T * R) * ((off + vector(0, 0, a.height)) * MM), "radius": a.radius * MM});
        return qCreatedBy(id, EntityType.BODY);
    });
};
const cqRevolve = function(c, st, out, selfI, a) {   // Workplane.revolve
    const w = wpGet(st, selfI);
    var angle = a.angleDegrees % 360;
    if (angle < 0) angle += 360;
    if (angle == 0) angle = 360;
    const s = (a.axisStart == undefined) ? plW(w.plane, [0, 0]) : plW(w.plane, a.axisStart);
    var e;
    if (a.axisEnd == undefined) e = (s[1] != 0) ? plW(w.plane, [0, s[1]]) : plW(w.plane, [0, 1]);
    else e = plW(w.plane, a.axisEnd);
    if (norm(e - s) < 1e-12) cqErr("revolve axis of zero length");
    const faces = getFaces(c, st, selfI).q;
    const id = nid(st, "rv");
    opRevolve(c, id, {"entities": faces, "axis": line(s * MM, normalize(e - s)), "angleForward": (angle == 360 ? 2 * PI : angle * PI / 180) * radian});
    combineWithBase(c, st, out, selfI, qCreatedBy(id, EntityType.BODY), a.combine);
};

// Workplane.loft: each pending wire one section, in order (or the outer wires of the faces on
// the stack); OCCT's ThruSections in CadQuery. Two sections make a ruled loft whatever the
// kernel; so does `ruled` between each pair. A smooth loft through three or more is each
// kernel's own surface: the result carries a warning saying so.
const cqLoft = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    var cx = ctxGet(st, w);
    var profiles = [];
    if (size(cx.pw) > 0) {
        const wires = cx.pw;
        cx.pw = [];
        ctxPut(st, w, cx);
        for (var x in wires) {
            const f = sketchRegions(c, st, [x]);
            if (size(ents(c, f.q)) != 1) cqErr("loft: a section that isn't one closed wire");
            profiles = append(profiles, f.q);
        }
    } else {
        for (var o in w.objs) {
            if (o.t == "face") profiles = append(profiles, o.q);
            else if (o.t == "vertex") cqUnsup("loft to a vertex");
        }
    }
    if (size(profiles) == 0) cqErr("Nothing to loft");
    if (size(profiles) == 1) cqErr("More than one wire or face is required");
    var q;
    if (a.ruled && size(profiles) > 2) {
        var parts = [];
        for (var j = 0; j + 1 < size(profiles); j += 1) {
            const id = nid(st, "lo");
            opLoft(c, id, {"profileSubqueries": [profiles[j], profiles[j + 1]], "bodyType": ToolBodyType.SOLID});
            parts = append(parts, qCreatedBy(id, EntityType.BODY));
        }
        opBoolean(c, nid(st, "bo"), {"tools": qUnion(parts), "operationType": BooleanOperationType.UNION});
        q = qUnion(parts);
    } else {
        if (size(profiles) > 2) warn(st, "smooth loft through " ~ size(profiles) ~ " sections: Parasolid's surface, not OCCT's ThruSections");
        const id = nid(st, "lo");
        opLoft(c, id, {"profileSubqueries": profiles, "bodyType": ToolBodyType.SOLID});
        q = qCreatedBy(id, EntityType.BODY);
    }
    combineWithBase(c, st, out, selfI, q, a.combine);
};

const cqNewObject = function(c, st, out, selfI, a) {   // Workplane.newObject(objlist)
    var objs = [];
    for (var x in a.objlist) objs = concatenateArrays([objs, argShapes(c, st, x)]);
    newObject(st, out, selfI, objs);
};

const cqSlot2D = function(c, st, out, selfI, a) {   // Workplane.slot2D
    const r = a.diameter / 2;
    const p1 = vector(-a.length / 2 + r, a.diameter / 2, 0);
    const p2 = p1 + vector(a.length - a.diameter, 0, 0);
    const p3 = p1 + vector(a.length - a.diameter, -a.diameter, 0);
    const p4 = p1 + vector(0, -a.diameter, 0);
    const arc1 = p2 + vector(r, -r, 0);
    const arc2 = p4 + vector(-r, r, 0);
    var w = {"t": "wire", "closed": true, "constr": false, "edges": [eLine(p1, p2), eArc(p2, arc1, p3), eLine(p3, p4), eArc(p4, arc2, p1)]};
    w = wXform(transform(rotationMatrix3d(vector(0, 0, 1), a.angle * degree), vector(0, 0, 0) * MM), w);
    placeWire(c, st, out, selfI, w);
};

const cqSpline = function(c, st, out, selfI, a) {    // Workplane.spline
    const w = wpGet(st, selfI);
    var pts = [];
    if (a.includeCurrent) pts = append(pts, fromPoint(c, w, false));
    for (var p in a.listOfXYTuple) pts = append(pts, plW(w.plane, p));
    if (a.parameters != undefined) cqUnsup("spline with parameters");
    var tans = undefined;
    if (a.tangents != undefined) {
        tans = [];
        for (var t in a.tangents) tans = append(tans, t == undefined ? undefined : plW(w.plane, t) - w.plane.o);
    }
    const e = {"t": "edge", "k": "spline", "pts": pts, "tans": tans, "periodic": a.periodic};
    if (a.makeWire) {
        const wire = {"t": "wire", "edges": [e], "closed": a.periodic == true || norm(pts[0] - pts[size(pts) - 1]) < 1e-9, "constr": a.forConstruction};
        if (!a.forConstruction) addPendingWires(st, w, [wire]);
        newObject(st, out, selfI, [wire]);
    } else {
        if (!a.forConstruction) addPendingEdge(st, w, e);
        newObject(st, out, selfI, [e]);
    }
};

// A sweep's path as edges Parasolid can follow: one sketch when the path lies in a plane, else
// (straight segments only) a 3D polyline.
const pathEdges = function(c, st, p) {
    var edges = (p.t == "wire") ? p.edges : [p];
    var pts = [];
    for (var e in edges) pts = concatenateArrays([pts, (e.k == "line") ? [e.a, e.b] : (e.k == "arc" ? [e.a, e.m, e.b] : (e.k == "spline" ? e.pts : [e.c, e.s]))]);
    var planar = undefined;
    try silent { planar = wPlane({"edges": edges}); }
    if (planar == undefined && size(edges) == 1 && edges[0].k == "line") {
        const d = unitOf(edges[0].b - edges[0].a);
        planar = plMake(edges[0].a, d, defaultXDir(d));
    }
    var flat = planar != undefined;
    if (flat) for (var q in pts) if (abs(dot(q - planar.o, planar.z)) > 1e-7 * max(1, norm(q))) flat = false;
    if (flat) {
        const id = nid(st, "pk");
        var sk = newSketchOnPlane(c, id, {"sketchPlane": plane(planar.o * MM, planar.z, planar.x)});
        const to2 = function(q) { const l = plL(planar, q); return vector(l[0], l[1]) * MM; };
        var j = 0;
        for (var e in edges) {
            j += 1;
            if (e.k == "line") skLineSegment(sk, "p" ~ j, {"start": to2(e.a), "end": to2(e.b)});
            else if (e.k == "arc") skArc(sk, "p" ~ j, {"start": to2(e.a), "mid": to2(e.m), "end": to2(e.b)});
            else if (e.k == "circle") skCircle(sk, "p" ~ j, {"center": to2(e.c), "radius": e.r * MM});
            else if (e.k == "spline") cqSkSpline(sk, "p" ~ j, e, to2, planar, st);
            else cqUnsup("sweep path edge " ~ e.k);
        }
        skSolve(sk);
        return qCreatedBy(id, EntityType.EDGE);
    }
    for (var e in edges) if (e.k != "line") cqUnsup("a sweep path off one plane with a " ~ e.k);
    var chain = [edges[0].a * MM];
    for (var e in edges) chain = append(chain, e.b * MM);
    const id = nid(st, "pl");
    opPolyline(c, id, {"points": chain});
    return qCreatedBy(id, EntityType.EDGE);
};
const cqSweep = function(c, st, out, selfI, a) {     // Workplane.sweep / _sweep
    if (a.multisection == true || a.sweepAlongWires == true) cqUnsup("multisection sweep");
    if (a.normal != undefined || a.auxSpine != undefined) cqUnsup("sweep with a normal or an auxiliary spine");
    if (a.makeSolid == false) cqUnsup("sweep as a shell");
    var p;
    if (a.path.wp != undefined) {
        // path.wire(): its pending edges made a wire (and a pending wire of its context)
        const tmp = toString(a.path.wp) ~ "_wire";
        cqWire(c, st, tmp, a.path.wp, {"forConstruction": false});
        const objs = wpGet(st, tmp).objs;
        if (size(objs) == 0) cqErr("Wire or Edge instance required");
        p = objs[0];
    } else p = shapeGet(st, a.path.shape);
    if (p.t != "wire" && p.t != "edge") cqErr("Wire or Edge instance required");
    var corners = false;
    if (p.t == "wire") for (var j = 0; j + 1 < size(p.edges); j += 1) corners = true;
    if (corners) warn(st, "sweep along several edges: Parasolid's joins where they meet at an angle, not OCCT's '" ~ a.transition ~ "' transition");
    const path = pathEdges(c, st, p);
    const faces = getFaces(c, st, selfI).q;
    const id = nid(st, "sw");
    opSweep(c, id, {"profiles": faces, "path": path});
    combineWithBase(c, st, out, selfI, qCreatedBy(id, EntityType.BODY), a.combine);
};


const unionLike = function(c, st, out, selfI, kind, a, argName) {
    const arg = a[argName];
    if (kind == "union") {      // Workplane.union
        var newS = [];
        if (arg != undefined) {
            if (arg.wp != undefined) {
                const sq = select(c, st, wpGet(st, arg.wp), "Solids", undefined, undefined);
                if (size(sq) < 1) cqErr("Workplane object must have at least one solid on the stack to union!");
                for (var o in sq) newS = append(newS, o.q);
            } else {
                for (var o in argShapes(c, st, arg)) if (o.t == "solid") newS = append(newS, o.q);
            }
        }
        const solidRef = findSolidQ(st, selfI, true, true);
        var r;
        if (solidRef != undefined) r = boolOp(c, st, "union", solidRef, size(newS) == 0 ? undefined : qUnion(newS));
        else if (size(newS) > 1) r = boolOp(c, st, "union", newS[0], qUnion(subArray(newS, 1, size(newS))));
        else if (size(newS) == 1) r = newS[0];
        else cqErr("union of nothing");
        newObject(st, out, selfI, [solidObj(r)]);
        return;
    }
    const solidRef = findSolid(st, selfI);   // Workplane.cut / intersect
    const tools = solidsQ(c, argShapes(c, st, arg));
    if (tools == undefined) cqErr("Cannot " ~ kind ~ " a non-solid");
    newObject(st, out, selfI, [solidObj(boolOp(c, st, kind, solidRef, tools))]);
};
const cqUnion = function(c, st, out, selfI, a) { unionLike(c, st, out, selfI, "union", a, "toUnion"); };
const cqCut = function(c, st, out, selfI, a) { unionLike(c, st, out, selfI, "cut", a, "toCut"); };
const cqIntersect = function(c, st, out, selfI, a) { unionLike(c, st, out, selfI, "intersect", a, "toIntersect"); };

const cqCombine = function(c, st, out, selfI, a) {   // Workplane.combine
    const w = wpGet(st, selfI);
    var qs = [];
    for (var o in w.objs) {
        if (o.t == "solid") qs = append(qs, o.q);
        else if (isShapeObj(o)) cqUnsup("combine of " ~ o.t);
    }
    if (size(qs) == 0) cqErr("pop from empty list");
    var s = qs[0];
    if (size(qs) > 1) s = boolOp(c, st, "union", s, qUnion(subArray(qs, 1, size(qs))));
    newObject(st, out, selfI, [solidObj(s)]);
};

const edgesOfStack = function(c, st, selfI) {   // self.edges().vals()
    return select(c, st, wpGet(st, selfI), "Edges", undefined, undefined);
};
const cqFillet = function(c, st, out, selfI, a) {   // Workplane.fillet
    const solid = findSolid(st, selfI);
    const edges = edgesOfStack(c, st, selfI);
    if (size(edges) < 1) cqErr("Fillets requires that edges be selected");
    const cw = copyWithEntities(c, st, solid, edges);
    const eq = taggedIn(cw.base, EntityType.EDGE, cw.tag);
    if (size(ents(c, eq)) != size(edges)) cqErr("fillet: selected edges do not belong to the solid");
    opFillet(c, nid(st, "fi"), {"entities": eq, "radius": a.radius * MM});
    newObject(st, out, selfI, [solidObj(cw.base)]);
};
const cqChamfer = function(c, st, out, selfI, a) {   // Workplane.chamfer
    if (a.length2 != undefined && a.length2 != a.length) cqUnsup("chamfer with two lengths (CadQuery's side depends on OCCT's edge-face map order)");
    const solid = findSolid(st, selfI);
    const edges = edgesOfStack(c, st, selfI);
    if (size(edges) < 1) cqErr("Chamfer requires that edges be selected");
    const cw = copyWithEntities(c, st, solid, edges);
    const eq = taggedIn(cw.base, EntityType.EDGE, cw.tag);
    if (size(ents(c, eq)) != size(edges)) cqErr("chamfer: selected edges do not belong to the solid");
    opChamfer(c, nid(st, "ch"), {"entities": eq, "chamferType": ChamferType.EQUAL_OFFSETS, "width": a.length * MM});
    newObject(st, out, selfI, [solidObj(cw.base)]);
};
const cqShell = function(c, st, out, selfI, a) {     // Workplane.shell
    const solid = findSolid(st, selfI);
    const w = wpGet(st, selfI);
    var faces = [];
    for (var o in w.objs) if (o.t == "face") faces = append(faces, o);
    if (a.kind != "arc") warn(st, "shell kind " ~ a.kind ~ ": Onshape's shell has one join type");
    const cw = copyWithEntities(c, st, solid, faces);
    var entsQ = cw.base;
    if (size(faces) > 0) {
        entsQ = taggedIn(cw.base, EntityType.FACE, cw.tag);
        if (size(ents(c, entsQ)) != size(faces)) cqErr("shell: selected faces do not belong to the solid");
    }
    opShell(c, nid(st, "sh"), {"entities": entsQ, "thickness": a.thickness * MM});
    newObject(st, out, selfI, [solidObj(cw.base)]);
};

const xformObjs = function(c, st, objs, T) {   // Shape.rotate / translate / mirror over the stack's shapes
    var out = [];
    for (var o in objs) {
        if (o.t == "solid") out = append(out, solidObj(copyBodies(c, st, o.q, T)));
        else if (o.t == "wire") out = append(out, wXform(T, o));
        else if (o.t == "edge") out = append(out, eXform(T, o));
        else if (o.t == "face" || o.t == "edgeq" || o.t == "vertex") cqUnsup("transforming a selected " ~ o.t);
        else out = append(out, o);   // points and locations are not Shapes: kept as they are
    }
    return out;
};
const rotT = function(s, e, deg) {
    const sv = v3(s); const ev = v3(e);
    if (norm(ev - sv) < 1e-12) cqErr("rotation axis of zero length");
    return rotationAround(line(sv * MM, normalize(ev - sv)), deg * degree);
};
const cqRotate = function(c, st, out, selfI, a) {
    newObject(st, out, selfI, xformObjs(c, st, wpGet(st, selfI).objs, rotT(a.axisStartPoint, a.axisEndPoint, a.angleDegrees)));
};
const cqTranslate = function(c, st, out, selfI, a) {
    newObject(st, out, selfI, xformObjs(c, st, wpGet(st, selfI).objs, transform(v3(a.vec) * MM)));
};
const mirrorT = function(c, st, mp, bp) {   // Shape.mirror(mirrorPlane, basePointVector)
    var n;
    if (mp is string) {
        if (mp == "XY" || mp == "YX") n = vector(0, 0, 1);
        else if (mp == "XZ" || mp == "ZX") n = vector(0, 1, 0);
        else if (mp == "YZ" || mp == "ZY") n = vector(1, 0, 0);
        else cqErr("mirror plane " ~ mp);
    } else n = v3(mp);
    return mirrorAcross(plane(v3(bp) * MM, normalize(n)));
};
const cqMirror = function(c, st, out, selfI, a) {   // Workplane.mirror
    var mp = a.mirrorPlane; var bp = a.basePointVector;
    if (mp is map && mp.wp != undefined) {
        const fo = wpGet(st, mp.wp).objs;
        if (size(fo) == 0 || fo[0].t != "face") cqErr("Face required");
        mp = faceNormal0(c, fo[0].q);
        if (bp == undefined) bp = centroid(c, fo[0].q);
    }
    if (bp == undefined) bp = [0, 0, 0];
    const w = wpGet(st, selfI);
    var shapes = [];
    for (var o in w.objs) if (isShapeObj(o)) shapes = append(shapes, o);
    const mirrored = xformObjs(c, st, shapes, mirrorT(c, st, mp, bp));
    if (a.union == true) {
        const tmp = out ~ "_m";
        newObject(st, tmp, selfI, mirrored);
        cqUnion(c, st, out, selfI, {"toUnion": {"wp": tmp}});
    } else newObject(st, out, selfI, mirrored);
};

const cqSplit = function(c, st, out, selfI, a) {   // Workplane.split(keepTop, keepBottom)
    if (!a.keepTop && !a.keepBottom) cqErr("You have to keep at least one half");
    const w = wpGet(st, selfI);
    const solid = findSolid(st, selfI);
    const bb = evBox3d(c, {"topology": solid, "tight": false});
    const maxDim = norm(bb.maxCorner - bb.minCorner) / MM * 10;
    const mk = function(sign) {
        var qs = [];
        for (var T in placements(c, w)) {
            const id = nid(st, "bx");
            fCuboid(c, id, {"corner1": vector(-maxDim / 2, -maxDim / 2, sign > 0 ? 0 : -maxDim) * MM, "corner2": vector(maxDim / 2, maxDim / 2, sign > 0 ? maxDim : 0) * MM});
            opTransform(c, nid(st, "tr"), {"bodies": qCreatedBy(id, EntityType.BODY), "transform": T});
            qs = append(qs, qCreatedBy(id, EntityType.BODY));
        }
        return qUnion(qs);
    };
    var rv = [];
    if (a.keepTop) rv = append(rv, solidObj(boolOp(c, st, "cut", solid, mk(-1))));
    if (a.keepBottom) rv = append(rv, solidObj(boolOp(c, st, "cut", solid, mk(1))));
    newObject(st, out, selfI, rv);
};

const cqSelect = function(kind) {
    return function(c, st, out, selfI, a) {
        const w = wpGet(st, selfI);
        var tagW = undefined;
        if (a.tag != undefined) {
            const cx = ctxGet(st, w);
            if (cx.tags[a.tag] == undefined) cqErr("No Workplane object named " ~ a.tag ~ " in chain");
            tagW = wpGet(st, cx.tags[a.tag]);
        }
        newObject(st, out, selfI, select(c, st, w, kind, a.selector, tagW));
    };
};

const cqAdd = function(c, st, out, selfI, a) {      // Workplane.add: mutates self
    var w = wpGet(st, selfI);
    const o = a.obj;
    if (o is array) { for (var x in o) w.objs = concatenateArrays([w.objs, argShapes(c, st, x)]); }
    else w.objs = concatenateArrays([w.objs, argShapes(c, st, o)]);
    wpPut(st, selfI, w);
};
const cqTag = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    var cx = ctxGet(st, w);
    cx.tags[a.name] = selfI;
    ctxPut(st, w, cx);
};
const cqEnd = function(c, st, out, selfI, a) {
    var cur = selfI;
    for (var i = 0; i < a.n; i += 1) {
        const p = wpGet(st, cur).parent;
        if (p == undefined) cqErr("Cannot End the chain-- no parents!");
        cur = p;
    }
    wpPut(st, out, wpGet(st, cur));
};
const cqWorkplaneFromTagged = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    const cx = ctxGet(st, w);
    if (cx.tags[a.name] == undefined) cqErr("No Workplane object named " ~ a.name ~ " in chain");
    wpPut(st, out, {"plane": wpGet(st, cx.tags[a.name]).plane, "objs": [], "parent": selfI, "ctx": w.ctx});
};
const cqCopyWorkplane = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    wpPut(st, out, {"plane": wpGet(st, a.obj.wp).plane, "objs": [], "parent": selfI, "ctx": w.ctx});
};
const cqToPending = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    var cx = ctxGet(st, w);
    for (var o in w.objs) {
        if (o.t == "wire") cx.pw = append(cx.pw, o);
        if (o.t == "edge") cx.pe = append(cx.pe, o);
    }
    ctxPut(st, w, cx);
};
const cqClean = function(c, st, out, selfI, a) { newObject(st, out, selfI, wpGet(st, selfI).objs); };
const cqSlice = function(which) {
    return function(c, st, out, selfI, a) {
        const objs = wpGet(st, selfI).objs;
        if (size(objs) > 1) warn(st, which ~ "() of " ~ size(objs) ~ " objects: Parasolid's order may not be OCCT's");
        if (size(objs) == 0) { newObject(st, out, selfI, []); return; }
        var i = (which == "first") ? 0 : ((which == "last") ? size(objs) - 1 : a.i);
        if (i < 0) i += size(objs);
        newObject(st, out, selfI, [objs[i]]);
    };
};
const cqVal = function(c, st, outShape, selfI) {   // Workplane.val
    const w = wpGet(st, selfI);
    var s = st[];
    if (size(w.objs) > 1) {
        var sameKind = true;
        for (var o in w.objs) if (o.t != w.objs[0].t || (o.t != "solid" && o.t != "vec")) sameKind = false;
        if (w.objs[0].t != "solid" || size(w.objs) > 1) warn(st, "val() of " ~ size(w.objs) ~ " objects: Parasolid's order may not be OCCT's");
    }
    s = st[];
    s.shapes[outShape] = (size(w.objs) == 0) ? {"t": "vec", "p": w.plane.o} : w.objs[0];
    st[] = s;
};

// ---- shape-level calls (cq.Solid.make*, Shape methods)
const putShape = function(st, i, o) { var s = st[]; s.shapes[i] = o; st[] = s; };
const shCompoundMake = function(c, st, out, a) {    // Compound.makeCompound(listOfShapes)
    var qs = [];
    for (var x in a.listOfShapes) for (var o in argShapes(c, st, x)) {
        if (o.t == "solid") qs = append(qs, o.q);
        else cqUnsup("a compound of " ~ o.t);
    }
    putShape(st, out, solidObj(qUnion(qs)));
};

const shSolidMakeBox = function(c, st, out, a) {   // Solid.makeBox(length, width, height, pnt, dir)
    const T = toWorld(coordSystem(v3(a.pnt) * MM, defaultXDir(normalize(v3(a.dir))), normalize(v3(a.dir))));
    const id = nid(st, "bx");
    fCuboid(c, id, {"corner1": vector(0, 0, 0) * MM, "corner2": vector(a.length, a.width, a.height) * MM});
    opTransform(c, nid(st, "tr"), {"bodies": qCreatedBy(id, EntityType.BODY), "transform": T});
    putShape(st, out, solidObj(qCreatedBy(id, EntityType.BODY)));
};
const shSolidMakeCylinder = function(c, st, out, a) {   // Solid.makeCylinder(radius, height, pnt, dir, angleDegrees)
    if (a.angleDegrees != 360) cqUnsup("partial cylinder");
    const p = v3(a.pnt); const d = normalize(v3(a.dir));
    const id = nid(st, "cy");
    fCylinder(c, id, {"bottomCenter": p * MM, "topCenter": (p + d * a.height) * MM, "radius": a.radius * MM});
    putShape(st, out, solidObj(qCreatedBy(id, EntityType.BODY)));
};
const shSolidMakeCone = function(c, st, out, a) {   // Solid.makeCone(radius1, radius2, height, pnt, dir, angleDegrees)
    if (a.angleDegrees != 360) cqUnsup("partial cone");
    const p = v3(a.pnt); const d = normalize(v3(a.dir));
    const id = nid(st, "co");
    fCone(c, id, {"bottomCenter": p * MM, "topCenter": (p + d * a.height) * MM, "bottomRadius": a.radius1 * MM, "topRadius": a.radius2 * MM});
    putShape(st, out, solidObj(qCreatedBy(id, EntityType.BODY)));
};
const shSolidMakeSphere = function(c, st, out, a) {   // Solid.makeSphere(radius, pnt, dir, angleDegrees1, 2, 3)
    if (a.angleDegrees1 != 0 && a.angleDegrees1 != -90) cqUnsup("partial sphere");
    if (a.angleDegrees2 != 90 || a.angleDegrees3 != 360) cqUnsup("partial sphere");
    if (a.angleDegrees1 == 0) cqUnsup("hemisphere (makeSphere's default angleDegrees1=0)");
    const id = nid(st, "sp");
    opSphere(c, id, {"center": v3(a.pnt) * MM, "radius": a.radius * MM});
    putShape(st, out, solidObj(qCreatedBy(id, EntityType.BODY)));
};
const shapeXform = function(c, st, out, src, T) {
    if (src.t == "solid") putShape(st, out, solidObj(copyBodies(c, st, src.q, T)));
    else if (src.t == "wire") putShape(st, out, wXform(T, src));
    else if (src.t == "edge") putShape(st, out, eXform(T, src));
    else cqUnsup("transforming a " ~ src.t);
};
const shTranslate = function(c, st, out, selfS, a) { shapeXform(c, st, out, shapeGet(st, selfS), transform(v3(a.vector) * MM)); };
const shRotate = function(c, st, out, selfS, a) { shapeXform(c, st, out, shapeGet(st, selfS), rotT(a.startVector, a.endVector, a.angleDegrees)); };
const shMirror = function(c, st, out, selfS, a) { shapeXform(c, st, out, shapeGet(st, selfS), mirrorT(c, st, a.mirrorPlane, a.basePointVector)); };
const shMoved = function(c, st, out, selfS, a) { shapeXform(c, st, out, shapeGet(st, selfS), locFromTape(a.loc.loc)); };
const shBool = function(kind) {
    return function(c, st, out, selfS, a) {
        const s = shapeGet(st, selfS);
        if (s.t != "solid") cqUnsup(kind ~ " of a " ~ s.t);
        var qs = [];
        for (var x in a.toolShapes) for (var o in argShapes(c, st, x)) if (o.t == "solid") qs = append(qs, o.q);
        if (size(qs) == 0) { putShape(st, out, s); return; }
        putShape(st, out, solidObj(boolOp(c, st, kind, s.q, qUnion(qs))));
    };
};

// ---- measuring the result (worker.py's measure)
const cqMeasure = function(c, st, res) {
    var objs = [];
    if (res.wp != undefined) objs = wpGet(st, res.wp).objs;
    else if (res.shape != undefined) objs = [shapeGet(st, res.shape)];
    else if (res.list != undefined) for (var r in res.list) objs = concatenateArrays([objs, argShapes(c, st, r)]);
    var qs = [];
    var other = [];
    for (var o in objs) {
        if (o.t == "solid") qs = append(qs, o.q);
        else other = append(other, o.t);
    }
    if (size(qs) == 0) return {"status": "no-shape", "other": other, "warn": st[].warn, "log": st[].log};
    const B = qUnion(qs);
    const faces = qOwnedByBody(B, EntityType.FACE);
    const bb = evBox3d(c, {"topology": B, "tight": true});
    return {"status": "ok", "result_q": B,
            "vol": evVolume(c, {"entities": B, "accuracy": "HIGH"}) / (MM ^ 3),
            "area": evArea(c, {"entities": faces}) / (MM ^ 2),
            "solids": size(ents(c, qBodyType(qEntityFilter(B, EntityType.BODY), BodyType.SOLID))),
            "faces": size(ents(c, faces)),
            "edges": size(ents(c, qOwnedByBody(B, EntityType.EDGE))),
            "vertices": size(ents(c, qOwnedByBody(B, EntityType.VERTEX))),
            "bbox": [bb.minCorner[0] / MM, bb.minCorner[1] / MM, bb.minCorner[2] / MM, bb.maxCorner[0] / MM, bb.maxCorner[1] / MM, bb.maxCorner[2] / MM],
            "other": other, "warn": st[].warn, "log": st[].log};
};
const newState = function(base) { return new box({"base": base, "wps": {}, "ctx": [], "shapes": {}, "n": 0, "log": [], "warn": [], "cur": 0}); };
const atStep = function(st, i) { var s = st[]; s.cur = i; st[] = s; };
const stepVol = function(c, st, out, op) {   // after a solid-producing step: its solids and volume
    const w = st[].wps[out];
    if (w == undefined) return;
    const q = solidsQ(c, w.objs);
    if (q == undefined) return;
    logStep(c, st, out, op, {"solids": size(ents(c, q)), "vol": evVolume(c, {"entities": q}) / (MM ^ 3)});
};

const case_1024 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqSphere(c, st, 2, 1, {"radius": 12, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "sphere");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqRect(c, st, 4, 3, {"xLen": 24, "yLen": 24, "centered": true, "forConstruction": false});
        atStep(st, 5);
        cqExtrude(c, st, 5, 4, {"until": -12, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 5, "extrude");
        atStep(st, 6);
        cqCut(c, st, 6, 2, {"toCut": {"wp": 5}, "clean": true, "tol": undefined});
        stepVol(c, st, 6, "cut");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 8);
        cqSphere(c, st, 8, 7, {"radius": 11, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 8, "sphere");
        atStep(st, 9);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 10);
        cqRect(c, st, 10, 9, {"xLen": 22, "yLen": 22, "centered": true, "forConstruction": false});
        atStep(st, 11);
        cqExtrude(c, st, 11, 10, {"until": -11, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 11, "extrude");
        atStep(st, 12);
        cqCut(c, st, 12, 8, {"toCut": {"wp": 11}, "clean": true, "tol": undefined});
        stepVol(c, st, 12, "cut");
        atStep(st, 13);
        cqCut(c, st, 13, 6, {"toCut": {"wp": 12}, "clean": true, "tol": undefined});
        stepVol(c, st, 13, "cut");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 14, {"plane": {"origin": [11.61895003862225, 0, 3], "xDir": [0, 1, 0], "zDir": [-0.9682458365518544, 0, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 15);
        cqMoveTo(c, st, 15, 14, {"x": -0.5, "y": -0.75});
        atStep(st, 16);
        cqLineTo(c, st, 16, 15, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 17);
        cqThreePointArc(c, st, 17, 16, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 18);
        cqLineTo(c, st, 18, 17, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 19);
        cqClose(c, st, 19, 18, {});
        atStep(st, 20);
        cqExtrude(c, st, 20, 19, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 20, "extrude");
        atStep(st, 21);
        cqCut(c, st, 21, 13, {"toCut": {"wp": 20}, "clean": true, "tol": undefined});
        stepVol(c, st, 21, "cut");
        atStep(st, 22);
        cqWorkplaneNew(c, st, 22, {"plane": {"origin": [10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8385254915624212, -0.484122918275927, -0.25]}, "obj": undefined});
        atStep(st, 23);
        cqMoveTo(c, st, 23, 22, {"x": -0.5, "y": -0.75});
        atStep(st, 24);
        cqLineTo(c, st, 24, 23, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 25);
        cqThreePointArc(c, st, 25, 24, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 26);
        cqLineTo(c, st, 26, 25, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 27);
        cqClose(c, st, 27, 26, {});
        atStep(st, 28);
        cqExtrude(c, st, 28, 27, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 28, "extrude");
        atStep(st, 29);
        cqCut(c, st, 29, 21, {"toCut": {"wp": 28}, "clean": true, "tol": undefined});
        stepVol(c, st, 29, "cut");
        atStep(st, 30);
        cqWorkplaneNew(c, st, 30, {"plane": {"origin": [5.809475019311126, 10.062305898749052, 3], "xDir": [-0.8660254037844386, 0.5000000000000001, -1.3877787807814457e-17], "zDir": [-0.4841229182759272, -0.8385254915624211, -0.25]}, "obj": undefined});
        atStep(st, 31);
        cqMoveTo(c, st, 31, 30, {"x": -0.5, "y": -0.75});
        atStep(st, 32);
        cqLineTo(c, st, 32, 31, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 33);
        cqThreePointArc(c, st, 33, 32, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 34);
        cqLineTo(c, st, 34, 33, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 35);
        cqClose(c, st, 35, 34, {});
        atStep(st, 36);
        cqExtrude(c, st, 36, 35, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 36, "extrude");
        atStep(st, 37);
        cqCut(c, st, 37, 29, {"toCut": {"wp": 36}, "clean": true, "tol": undefined});
        stepVol(c, st, 37, "cut");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 38, {"plane": {"origin": [7.114554987125877e-16, 11.61895003862225, 3], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-5.928795822604899e-17, -0.9682458365518544, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 39);
        cqMoveTo(c, st, 39, 38, {"x": -0.5, "y": -0.75});
        atStep(st, 40);
        cqLineTo(c, st, 40, 39, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 41);
        cqThreePointArc(c, st, 41, 40, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 42);
        cqLineTo(c, st, 42, 41, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 43);
        cqClose(c, st, 43, 42, {});
        atStep(st, 44);
        cqExtrude(c, st, 44, 43, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 44, "extrude");
        atStep(st, 45);
        cqCut(c, st, 45, 37, {"toCut": {"wp": 44}, "clean": true, "tol": undefined});
        stepVol(c, st, 45, "cut");
        atStep(st, 46);
        cqWorkplaneNew(c, st, 46, {"plane": {"origin": [-5.809475019311122, 10.062305898749054, 3], "xDir": [-0.8660254037844387, -0.4999999999999998, 0], "zDir": [0.4841229182759269, -0.8385254915624213, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 47);
        cqMoveTo(c, st, 47, 46, {"x": -0.5, "y": -0.75});
        atStep(st, 48);
        cqLineTo(c, st, 48, 47, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 49);
        cqThreePointArc(c, st, 49, 48, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 50);
        cqLineTo(c, st, 50, 49, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 51);
        cqClose(c, st, 51, 50, {});
        atStep(st, 52);
        cqExtrude(c, st, 52, 51, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 52, "extrude");
        atStep(st, 53);
        cqCut(c, st, 53, 45, {"toCut": {"wp": 52}, "clean": true, "tol": undefined});
        stepVol(c, st, 53, "cut");
        atStep(st, 54);
        cqWorkplaneNew(c, st, 54, {"plane": {"origin": [-10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8385254915624212, -0.484122918275927, -0.25]}, "obj": undefined});
        atStep(st, 55);
        cqMoveTo(c, st, 55, 54, {"x": -0.5, "y": -0.75});
        atStep(st, 56);
        cqLineTo(c, st, 56, 55, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 57);
        cqThreePointArc(c, st, 57, 56, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 58);
        cqLineTo(c, st, 58, 57, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 59);
        cqClose(c, st, 59, 58, {});
        atStep(st, 60);
        cqExtrude(c, st, 60, 59, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 60, "extrude");
        atStep(st, 61);
        cqCut(c, st, 61, 53, {"toCut": {"wp": 60}, "clean": true, "tol": undefined});
        stepVol(c, st, 61, "cut");
        atStep(st, 62);
        cqWorkplaneNew(c, st, 62, {"plane": {"origin": [-11.61895003862225, 1.4229109974251757e-15, 3], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [0.9682458365518544, -1.1857591645209795e-16, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 63);
        cqMoveTo(c, st, 63, 62, {"x": -0.5, "y": -0.75});
        atStep(st, 64);
        cqLineTo(c, st, 64, 63, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 65);
        cqThreePointArc(c, st, 65, 64, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 66);
        cqLineTo(c, st, 66, 65, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 67);
        cqClose(c, st, 67, 66, {});
        atStep(st, 68);
        cqExtrude(c, st, 68, 67, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 68, "extrude");
        atStep(st, 69);
        cqCut(c, st, 69, 61, {"toCut": {"wp": 68}, "clean": true, "tol": undefined});
        stepVol(c, st, 69, "cut");
        atStep(st, 70);
        cqWorkplaneNew(c, st, 70, {"plane": {"origin": [-10.062305898749056, -5.8094750193111215, 3], "xDir": [0.4999999999999997, -0.8660254037844388, -1.3877787807814457e-17], "zDir": [0.8385254915624214, 0.4841229182759269, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 71);
        cqMoveTo(c, st, 71, 70, {"x": -0.5, "y": -0.75});
        atStep(st, 72);
        cqLineTo(c, st, 72, 71, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 73);
        cqThreePointArc(c, st, 73, 72, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 74);
        cqLineTo(c, st, 74, 73, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 75);
        cqClose(c, st, 75, 74, {});
        atStep(st, 76);
        cqExtrude(c, st, 76, 75, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 76, "extrude");
        atStep(st, 77);
        cqCut(c, st, 77, 69, {"toCut": {"wp": 76}, "clean": true, "tol": undefined});
        stepVol(c, st, 77, "cut");
        atStep(st, 78);
        cqWorkplaneNew(c, st, 78, {"plane": {"origin": [-5.80947501931113, -10.06230589874905, 3], "xDir": [0.8660254037844385, -0.5000000000000004, 0], "zDir": [0.4841229182759275, 0.8385254915624208, -0.25]}, "obj": undefined});
        atStep(st, 79);
        cqMoveTo(c, st, 79, 78, {"x": -0.5, "y": -0.75});
        atStep(st, 80);
        cqLineTo(c, st, 80, 79, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 81);
        cqThreePointArc(c, st, 81, 80, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 82);
        cqLineTo(c, st, 82, 81, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 83);
        cqClose(c, st, 83, 82, {});
        atStep(st, 84);
        cqExtrude(c, st, 84, 83, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 84, "extrude");
        atStep(st, 85);
        cqCut(c, st, 85, 77, {"toCut": {"wp": 84}, "clean": true, "tol": undefined});
        stepVol(c, st, 85, "cut");
        atStep(st, 86);
        cqWorkplaneNew(c, st, 86, {"plane": {"origin": [-2.134366496137763e-15, -11.61895003862225, 3], "xDir": [1, -1.8369701987210294e-16, 6.162975822039154e-33], "zDir": [1.778638746781469e-16, 0.9682458365518544, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 87);
        cqMoveTo(c, st, 87, 86, {"x": -0.5, "y": -0.75});
        atStep(st, 88);
        cqLineTo(c, st, 88, 87, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 89);
        cqThreePointArc(c, st, 89, 88, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 90);
        cqLineTo(c, st, 90, 89, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 91);
        cqClose(c, st, 91, 90, {});
        atStep(st, 92);
        cqExtrude(c, st, 92, 91, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 92, "extrude");
        atStep(st, 93);
        cqCut(c, st, 93, 85, {"toCut": {"wp": 92}, "clean": true, "tol": undefined});
        stepVol(c, st, 93, "cut");
        atStep(st, 94);
        cqWorkplaneNew(c, st, 94, {"plane": {"origin": [5.809475019311126, -10.062305898749052, 3], "xDir": [0.8660254037844386, 0.5000000000000001, 1.3877787807814457e-17], "zDir": [-0.4841229182759272, 0.8385254915624211, -0.25]}, "obj": undefined});
        atStep(st, 95);
        cqMoveTo(c, st, 95, 94, {"x": -0.5, "y": -0.75});
        atStep(st, 96);
        cqLineTo(c, st, 96, 95, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 97);
        cqThreePointArc(c, st, 97, 96, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 98);
        cqLineTo(c, st, 98, 97, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 99);
        cqClose(c, st, 99, 98, {});
        atStep(st, 100);
        cqExtrude(c, st, 100, 99, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 100, "extrude");
        atStep(st, 101);
        cqCut(c, st, 101, 93, {"toCut": {"wp": 100}, "clean": true, "tol": undefined});
        stepVol(c, st, 101, "cut");
        atStep(st, 102);
        cqWorkplaneNew(c, st, 102, {"plane": {"origin": [10.06230589874905, -5.80947501931113, 3], "xDir": [0.5000000000000004, 0.8660254037844385, 0], "zDir": [-0.8385254915624208, 0.4841229182759275, -0.25]}, "obj": undefined});
        atStep(st, 103);
        cqMoveTo(c, st, 103, 102, {"x": -0.5, "y": -0.75});
        atStep(st, 104);
        cqLineTo(c, st, 104, 103, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 105);
        cqThreePointArc(c, st, 105, 104, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 106);
        cqLineTo(c, st, 106, 105, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 107);
        cqClose(c, st, 107, 106, {});
        atStep(st, 108);
        cqExtrude(c, st, 108, 107, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 108, "extrude");
        atStep(st, 109);
        cqCut(c, st, 109, 101, {"toCut": {"wp": 108}, "clean": true, "tol": undefined});
        stepVol(c, st, 109, "cut");
        atStep(st, 110);
        cqWorkplaneNew(c, st, 110, {"plane": {"origin": [10.392304845413264, 0, 6], "xDir": [0, 1, 0], "zDir": [-0.8660254037844387, 0, -0.5]}, "obj": undefined});
        atStep(st, 111);
        cqMoveTo(c, st, 111, 110, {"x": -0.5, "y": -0.75});
        atStep(st, 112);
        cqLineTo(c, st, 112, 111, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 113);
        cqThreePointArc(c, st, 113, 112, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 114);
        cqLineTo(c, st, 114, 113, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 115);
        cqClose(c, st, 115, 114, {});
        atStep(st, 116);
        cqExtrude(c, st, 116, 115, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 116, "extrude");
        atStep(st, 117);
        cqCut(c, st, 117, 109, {"toCut": {"wp": 116}, "clean": true, "tol": undefined});
        stepVol(c, st, 117, "cut");
        atStep(st, 118);
        cqWorkplaneNew(c, st, 118, {"plane": {"origin": [9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, 0.8660254037844387, -2.7755575615628907e-17], "zDir": [-0.7500000000000001, -0.4330127018922193, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 119);
        cqMoveTo(c, st, 119, 118, {"x": -0.5, "y": -0.75});
        atStep(st, 120);
        cqLineTo(c, st, 120, 119, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 121);
        cqThreePointArc(c, st, 121, 120, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 122);
        cqLineTo(c, st, 122, 121, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 123);
        cqClose(c, st, 123, 122, {});
        atStep(st, 124);
        cqExtrude(c, st, 124, 123, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 124, "extrude");
        atStep(st, 125);
        cqCut(c, st, 125, 117, {"toCut": {"wp": 124}, "clean": true, "tol": undefined});
        stepVol(c, st, 125, "cut");
        atStep(st, 126);
        cqWorkplaneNew(c, st, 126, {"plane": {"origin": [5.196152422706633, 9, 6], "xDir": [-0.8660254037844386, 0.5000000000000001, -2.7755575615628907e-17], "zDir": [-0.4330127018922194, -0.75, -0.5]}, "obj": undefined});
        atStep(st, 127);
        cqMoveTo(c, st, 127, 126, {"x": -0.5, "y": -0.75});
        atStep(st, 128);
        cqLineTo(c, st, 128, 127, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 129);
        cqThreePointArc(c, st, 129, 128, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 130);
        cqLineTo(c, st, 130, 129, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 131);
        cqClose(c, st, 131, 130, {});
        atStep(st, 132);
        cqExtrude(c, st, 132, 131, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 132, "extrude");
        atStep(st, 133);
        cqCut(c, st, 133, 125, {"toCut": {"wp": 132}, "clean": true, "tol": undefined});
        stepVol(c, st, 133, "cut");
        atStep(st, 134);
        cqWorkplaneNew(c, st, 134, {"plane": {"origin": [6.363451432349442e-16, 10.392304845413264, 6], "xDir": [-1, 6.123233995736767e-17, 3.081487911019578e-33], "zDir": [-5.3028761936245346e-17, -0.8660254037844387, -0.5]}, "obj": undefined});
        atStep(st, 135);
        cqMoveTo(c, st, 135, 134, {"x": -0.5, "y": -0.75});
        atStep(st, 136);
        cqLineTo(c, st, 136, 135, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 137);
        cqThreePointArc(c, st, 137, 136, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 138);
        cqLineTo(c, st, 138, 137, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 139);
        cqClose(c, st, 139, 138, {});
        atStep(st, 140);
        cqExtrude(c, st, 140, 139, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 140, "extrude");
        atStep(st, 141);
        cqCut(c, st, 141, 133, {"toCut": {"wp": 140}, "clean": true, "tol": undefined});
        stepVol(c, st, 141, "cut");
        atStep(st, 142);
        cqWorkplaneNew(c, st, 142, {"plane": {"origin": [-5.196152422706629, 9, 6], "xDir": [-0.8660254037844387, -0.4999999999999998, -2.7755575615628907e-17], "zDir": [0.4330127018922192, -0.7500000000000001, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 143);
        cqMoveTo(c, st, 143, 142, {"x": -0.5, "y": -0.75});
        atStep(st, 144);
        cqLineTo(c, st, 144, 143, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 145);
        cqThreePointArc(c, st, 145, 144, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 146);
        cqLineTo(c, st, 146, 145, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 147);
        cqClose(c, st, 147, 146, {});
        atStep(st, 148);
        cqExtrude(c, st, 148, 147, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 148, "extrude");
        atStep(st, 149);
        cqCut(c, st, 149, 141, {"toCut": {"wp": 148}, "clean": true, "tol": undefined});
        stepVol(c, st, 149, "cut");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [-9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, -0.8660254037844387, 2.7755575615628907e-17], "zDir": [0.7500000000000001, -0.4330127018922193, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 151);
        cqMoveTo(c, st, 151, 150, {"x": -0.5, "y": -0.75});
        atStep(st, 152);
        cqLineTo(c, st, 152, 151, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 153);
        cqThreePointArc(c, st, 153, 152, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 154);
        cqLineTo(c, st, 154, 153, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 155);
        cqClose(c, st, 155, 154, {});
        atStep(st, 156);
        cqExtrude(c, st, 156, 155, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 156, "extrude");
        atStep(st, 157);
        cqCut(c, st, 157, 149, {"toCut": {"wp": 156}, "clean": true, "tol": undefined});
        stepVol(c, st, 157, "cut");
        atStep(st, 158);
        cqWorkplaneNew(c, st, 158, {"plane": {"origin": [-10.392304845413264, 1.2726902864698884e-15, 6], "xDir": [-1.2246467991473535e-16, -1, 6.162975822039155e-33], "zDir": [0.8660254037844387, -1.060575238724907e-16, -0.5]}, "obj": undefined});
        atStep(st, 159);
        cqMoveTo(c, st, 159, 158, {"x": -0.5, "y": -0.75});
        atStep(st, 160);
        cqLineTo(c, st, 160, 159, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 161);
        cqThreePointArc(c, st, 161, 160, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 162);
        cqLineTo(c, st, 162, 161, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 163);
        cqClose(c, st, 163, 162, {});
        atStep(st, 164);
        cqExtrude(c, st, 164, 163, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 164, "extrude");
        atStep(st, 165);
        cqCut(c, st, 165, 157, {"toCut": {"wp": 164}, "clean": true, "tol": undefined});
        stepVol(c, st, 165, "cut");
        atStep(st, 166);
        cqWorkplaneNew(c, st, 166, {"plane": {"origin": [-9.000000000000002, -5.196152422706629, 6], "xDir": [0.4999999999999998, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [0.7500000000000001, 0.43301270189221913, -0.5]}, "obj": undefined});
        atStep(st, 167);
        cqMoveTo(c, st, 167, 166, {"x": -0.5, "y": -0.75});
        atStep(st, 168);
        cqLineTo(c, st, 168, 167, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 169);
        cqThreePointArc(c, st, 169, 168, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 170);
        cqLineTo(c, st, 170, 169, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 171);
        cqClose(c, st, 171, 170, {});
        atStep(st, 172);
        cqExtrude(c, st, 172, 171, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 172, "extrude");
        atStep(st, 173);
        cqCut(c, st, 173, 165, {"toCut": {"wp": 172}, "clean": true, "tol": undefined});
        stepVol(c, st, 173, "cut");
        atStep(st, 174);
        cqWorkplaneNew(c, st, 174, {"plane": {"origin": [-5.1961524227066365, -8.999999999999998, 6], "xDir": [0.8660254037844384, -0.5000000000000003, -2.7755575615628907e-17], "zDir": [0.4330127018922197, 0.7499999999999999, -0.5]}, "obj": undefined});
        atStep(st, 175);
        cqMoveTo(c, st, 175, 174, {"x": -0.5, "y": -0.75});
        atStep(st, 176);
        cqLineTo(c, st, 176, 175, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 177);
        cqThreePointArc(c, st, 177, 176, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 178);
        cqLineTo(c, st, 178, 177, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 179);
        cqClose(c, st, 179, 178, {});
        atStep(st, 180);
        cqExtrude(c, st, 180, 179, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 180, "extrude");
        atStep(st, 181);
        cqCut(c, st, 181, 173, {"toCut": {"wp": 180}, "clean": true, "tol": undefined});
        stepVol(c, st, 181, "cut");
        atStep(st, 182);
        cqWorkplaneNew(c, st, 182, {"plane": {"origin": [-1.9090354297048323e-15, -10.392304845413264, 6], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.59086285808736e-16, 0.8660254037844387, -0.5]}, "obj": undefined});
        atStep(st, 183);
        cqMoveTo(c, st, 183, 182, {"x": -0.5, "y": -0.75});
        atStep(st, 184);
        cqLineTo(c, st, 184, 183, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 185);
        cqThreePointArc(c, st, 185, 184, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 186);
        cqLineTo(c, st, 186, 185, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 187);
        cqClose(c, st, 187, 186, {});
        atStep(st, 188);
        cqExtrude(c, st, 188, 187, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 188, "extrude");
        atStep(st, 189);
        cqCut(c, st, 189, 181, {"toCut": {"wp": 188}, "clean": true, "tol": undefined});
        stepVol(c, st, 189, "cut");
        atStep(st, 190);
        cqWorkplaneNew(c, st, 190, {"plane": {"origin": [5.196152422706633, -9, 6], "xDir": [0.8660254037844386, 0.5000000000000001, 2.7755575615628907e-17], "zDir": [-0.4330127018922194, 0.75, -0.5]}, "obj": undefined});
        atStep(st, 191);
        cqMoveTo(c, st, 191, 190, {"x": -0.5, "y": -0.75});
        atStep(st, 192);
        cqLineTo(c, st, 192, 191, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 193);
        cqThreePointArc(c, st, 193, 192, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 194);
        cqLineTo(c, st, 194, 193, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 195);
        cqClose(c, st, 195, 194, {});
        atStep(st, 196);
        cqExtrude(c, st, 196, 195, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 196, "extrude");
        atStep(st, 197);
        cqCut(c, st, 197, 189, {"toCut": {"wp": 196}, "clean": true, "tol": undefined});
        stepVol(c, st, 197, "cut");
        atStep(st, 198);
        cqWorkplaneNew(c, st, 198, {"plane": {"origin": [8.999999999999998, -5.1961524227066365, 6], "xDir": [0.5000000000000003, 0.8660254037844384, -2.7755575615628907e-17], "zDir": [-0.7499999999999999, 0.4330127018922197, -0.5]}, "obj": undefined});
        atStep(st, 199);
        cqMoveTo(c, st, 199, 198, {"x": -0.5, "y": -0.75});
        atStep(st, 200);
        cqLineTo(c, st, 200, 199, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 201);
        cqThreePointArc(c, st, 201, 200, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 202);
        cqLineTo(c, st, 202, 201, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 203);
        cqClose(c, st, 203, 202, {});
        atStep(st, 204);
        cqExtrude(c, st, 204, 203, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 204, "extrude");
        atStep(st, 205);
        cqCut(c, st, 205, 197, {"toCut": {"wp": 204}, "clean": true, "tol": undefined});
        stepVol(c, st, 205, "cut");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 206, {"plane": {"origin": [7.937253933193772, 0, 9], "xDir": [0, 1, 0], "zDir": [-0.6614378277661477, 0, -0.75]}, "obj": undefined});
        atStep(st, 207);
        cqMoveTo(c, st, 207, 206, {"x": -0.5, "y": -0.75});
        atStep(st, 208);
        cqLineTo(c, st, 208, 207, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 209);
        cqThreePointArc(c, st, 209, 208, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 210);
        cqLineTo(c, st, 210, 209, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 211);
        cqClose(c, st, 211, 210, {});
        atStep(st, 212);
        cqExtrude(c, st, 212, 211, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 212, "extrude");
        atStep(st, 213);
        cqCut(c, st, 213, 205, {"toCut": {"wp": 212}, "clean": true, "tol": undefined});
        stepVol(c, st, 213, "cut");
        atStep(st, 214);
        cqWorkplaneNew(c, st, 214, {"plane": {"origin": [6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, 0.8660254037844388, 2.7755575615628914e-17], "zDir": [-0.5728219618694801, -0.3307189138830738, -0.75]}, "obj": undefined});
        atStep(st, 215);
        cqMoveTo(c, st, 215, 214, {"x": -0.5, "y": -0.75});
        atStep(st, 216);
        cqLineTo(c, st, 216, 215, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 217);
        cqThreePointArc(c, st, 217, 216, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 218);
        cqLineTo(c, st, 218, 217, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 219);
        cqClose(c, st, 219, 218, {});
        atStep(st, 220);
        cqExtrude(c, st, 220, 219, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 220, "extrude");
        atStep(st, 221);
        cqCut(c, st, 221, 213, {"toCut": {"wp": 220}, "clean": true, "tol": undefined});
        stepVol(c, st, 221, "cut");
        atStep(st, 222);
        cqWorkplaneNew(c, st, 222, {"plane": {"origin": [3.968626966596887, 6.87386354243376, 9], "xDir": [-0.8660254037844386, 0.5000000000000002, 0], "zDir": [-0.3307189138830739, -0.57282196186948, -0.75]}, "obj": undefined});
        atStep(st, 223);
        cqMoveTo(c, st, 223, 222, {"x": -0.5, "y": -0.75});
        atStep(st, 224);
        cqLineTo(c, st, 224, 223, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 225);
        cqThreePointArc(c, st, 225, 224, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 226);
        cqLineTo(c, st, 226, 225, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 227);
        cqClose(c, st, 227, 226, {});
        atStep(st, 228);
        cqExtrude(c, st, 228, 227, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 228, "extrude");
        atStep(st, 229);
        cqCut(c, st, 229, 221, {"toCut": {"wp": 228}, "clean": true, "tol": undefined});
        stepVol(c, st, 229, "cut");
        atStep(st, 230);
        cqWorkplaneNew(c, st, 230, {"plane": {"origin": [4.860166311652746e-16, 7.937253933193772, 9], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-4.0501385930439555e-17, -0.6614378277661477, -0.75]}, "obj": undefined});
        atStep(st, 231);
        cqMoveTo(c, st, 231, 230, {"x": -0.5, "y": -0.75});
        atStep(st, 232);
        cqLineTo(c, st, 232, 231, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 233);
        cqThreePointArc(c, st, 233, 232, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 234);
        cqLineTo(c, st, 234, 233, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 235);
        cqClose(c, st, 235, 234, {});
        atStep(st, 236);
        cqExtrude(c, st, 236, 235, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 236, "extrude");
        atStep(st, 237);
        cqCut(c, st, 237, 229, {"toCut": {"wp": 236}, "clean": true, "tol": undefined});
        stepVol(c, st, 237, "cut");
        atStep(st, 238);
        cqWorkplaneNew(c, st, 238, {"plane": {"origin": [-3.968626966596884, 6.873863542433761, 9], "xDir": [-0.8660254037844388, -0.4999999999999998, 2.7755575615628914e-17], "zDir": [0.3307189138830737, -0.5728219618694801, -0.75]}, "obj": undefined});
        atStep(st, 239);
        cqMoveTo(c, st, 239, 238, {"x": -0.5, "y": -0.75});
        atStep(st, 240);
        cqLineTo(c, st, 240, 239, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 241);
        cqThreePointArc(c, st, 241, 240, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 242);
        cqLineTo(c, st, 242, 241, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 243);
        cqClose(c, st, 243, 242, {});
        atStep(st, 244);
        cqExtrude(c, st, 244, 243, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 244, "extrude");
        atStep(st, 245);
        cqCut(c, st, 245, 237, {"toCut": {"wp": 244}, "clean": true, "tol": undefined});
        stepVol(c, st, 245, "cut");
        atStep(st, 246);
        cqWorkplaneNew(c, st, 246, {"plane": {"origin": [-6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [0.5728219618694801, -0.3307189138830738, -0.75]}, "obj": undefined});
        atStep(st, 247);
        cqMoveTo(c, st, 247, 246, {"x": -0.5, "y": -0.75});
        atStep(st, 248);
        cqLineTo(c, st, 248, 247, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 249);
        cqThreePointArc(c, st, 249, 248, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 250);
        cqLineTo(c, st, 250, 249, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 251);
        cqClose(c, st, 251, 250, {});
        atStep(st, 252);
        cqExtrude(c, st, 252, 251, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 252, "extrude");
        atStep(st, 253);
        cqCut(c, st, 253, 245, {"toCut": {"wp": 252}, "clean": true, "tol": undefined});
        stepVol(c, st, 253, "cut");
        atStep(st, 254);
        cqWorkplaneNew(c, st, 254, {"plane": {"origin": [-7.937253933193772, 9.720332623305493e-16, 9], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [0.6614378277661477, -8.100277186087911e-17, -0.75]}, "obj": undefined});
        atStep(st, 255);
        cqMoveTo(c, st, 255, 254, {"x": -0.5, "y": -0.75});
        atStep(st, 256);
        cqLineTo(c, st, 256, 255, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 257);
        cqThreePointArc(c, st, 257, 256, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 258);
        cqLineTo(c, st, 258, 257, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 259);
        cqClose(c, st, 259, 258, {});
        atStep(st, 260);
        cqExtrude(c, st, 260, 259, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 260, "extrude");
        atStep(st, 261);
        cqCut(c, st, 261, 253, {"toCut": {"wp": 260}, "clean": true, "tol": undefined});
        stepVol(c, st, 261, "cut");
        atStep(st, 262);
        cqWorkplaneNew(c, st, 262, {"plane": {"origin": [-6.873863542433762, -3.968626966596884, 9], "xDir": [0.4999999999999998, -0.8660254037844389, 0], "zDir": [0.5728219618694802, 0.3307189138830737, -0.75]}, "obj": undefined});
        atStep(st, 263);
        cqMoveTo(c, st, 263, 262, {"x": -0.5, "y": -0.75});
        atStep(st, 264);
        cqLineTo(c, st, 264, 263, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 265);
        cqThreePointArc(c, st, 265, 264, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 266);
        cqLineTo(c, st, 266, 265, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 267);
        cqClose(c, st, 267, 266, {});
        atStep(st, 268);
        cqExtrude(c, st, 268, 267, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 268, "extrude");
        atStep(st, 269);
        cqCut(c, st, 269, 261, {"toCut": {"wp": 268}, "clean": true, "tol": undefined});
        stepVol(c, st, 269, "cut");
        atStep(st, 270);
        cqWorkplaneNew(c, st, 270, {"plane": {"origin": [-3.9686269665968896, -6.873863542433758, 9], "xDir": [0.8660254037844384, -0.5000000000000004, -2.7755575615628914e-17], "zDir": [0.3307189138830741, 0.5728219618694799, -0.75]}, "obj": undefined});
        atStep(st, 271);
        cqMoveTo(c, st, 271, 270, {"x": -0.5, "y": -0.75});
        atStep(st, 272);
        cqLineTo(c, st, 272, 271, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 273);
        cqThreePointArc(c, st, 273, 272, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 274);
        cqLineTo(c, st, 274, 273, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 275);
        cqClose(c, st, 275, 274, {});
        atStep(st, 276);
        cqExtrude(c, st, 276, 275, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 276, "extrude");
        atStep(st, 277);
        cqCut(c, st, 277, 269, {"toCut": {"wp": 276}, "clean": true, "tol": undefined});
        stepVol(c, st, 277, "cut");
        atStep(st, 278);
        cqWorkplaneNew(c, st, 278, {"plane": {"origin": [-1.4580498934958238e-15, -7.937253933193772, 9], "xDir": [1, -1.83697019872103e-16, -1.232595164407831e-32], "zDir": [1.2150415779131866e-16, 0.6614378277661477, -0.75]}, "obj": undefined});
        atStep(st, 279);
        cqMoveTo(c, st, 279, 278, {"x": -0.5, "y": -0.75});
        atStep(st, 280);
        cqLineTo(c, st, 280, 279, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 281);
        cqThreePointArc(c, st, 281, 280, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 282);
        cqLineTo(c, st, 282, 281, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 283);
        cqClose(c, st, 283, 282, {});
        atStep(st, 284);
        cqExtrude(c, st, 284, 283, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 284, "extrude");
        atStep(st, 285);
        cqCut(c, st, 285, 277, {"toCut": {"wp": 284}, "clean": true, "tol": undefined});
        stepVol(c, st, 285, "cut");
        atStep(st, 286);
        cqWorkplaneNew(c, st, 286, {"plane": {"origin": [3.968626966596887, -6.87386354243376, 9], "xDir": [0.8660254037844386, 0.5000000000000002, 0], "zDir": [-0.3307189138830739, 0.57282196186948, -0.75]}, "obj": undefined});
        atStep(st, 287);
        cqMoveTo(c, st, 287, 286, {"x": -0.5, "y": -0.75});
        atStep(st, 288);
        cqLineTo(c, st, 288, 287, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 289);
        cqThreePointArc(c, st, 289, 288, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 290);
        cqLineTo(c, st, 290, 289, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 291);
        cqClose(c, st, 291, 290, {});
        atStep(st, 292);
        cqExtrude(c, st, 292, 291, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 292, "extrude");
        atStep(st, 293);
        cqCut(c, st, 293, 285, {"toCut": {"wp": 292}, "clean": true, "tol": undefined});
        stepVol(c, st, 293, "cut");
        atStep(st, 294);
        cqWorkplaneNew(c, st, 294, {"plane": {"origin": [6.873863542433758, -3.9686269665968896, 9], "xDir": [0.5000000000000004, 0.8660254037844384, -2.7755575615628914e-17], "zDir": [-0.5728219618694799, 0.3307189138830741, -0.75]}, "obj": undefined});
        atStep(st, 295);
        cqMoveTo(c, st, 295, 294, {"x": -0.5, "y": -0.75});
        atStep(st, 296);
        cqLineTo(c, st, 296, 295, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 297);
        cqThreePointArc(c, st, 297, 296, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 298);
        cqLineTo(c, st, 298, 297, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 299);
        cqClose(c, st, 299, 298, {});
        atStep(st, 300);
        cqExtrude(c, st, 300, 299, {"until": 13, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 300, "extrude");
        atStep(st, 301);
        cqCut(c, st, 301, 293, {"toCut": {"wp": 300}, "clean": true, "tol": undefined});
        stepVol(c, st, 301, "cut");
        atStep(st, 302);
        cqWorkplaneNew(c, st, 302, {"plane": {"origin": [11.61895003862225, 0, 3], "xDir": [0, 1, 0], "zDir": [-0.9682458365518544, 0, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 303);
        cqMoveTo(c, st, 303, 302, {"x": -0.5, "y": -0.75});
        atStep(st, 304);
        cqLineTo(c, st, 304, 303, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 305);
        cqThreePointArc(c, st, 305, 304, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 306);
        cqLineTo(c, st, 306, 305, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 307);
        cqClose(c, st, 307, 306, {});
        atStep(st, 308);
        cqWorkplaneNew(c, st, 308, {"plane": {"origin": [11.61895003862225, 0, 3], "xDir": [0, 1, 0], "zDir": [-0.9682458365518544, 0, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 309);
        cqMoveTo(c, st, 309, 308, {"x": -0.7, "y": -0.95});
        atStep(st, 310);
        cqLineTo(c, st, 310, 309, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 311);
        cqThreePointArc(c, st, 311, 310, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 312);
        cqLineTo(c, st, 312, 311, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 313);
        cqClose(c, st, 313, 312, {});
        atStep(st, 314);
        cqExtrude(c, st, 314, 313, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 314, "extrude");
        atStep(st, 315);
        cqExtrude(c, st, 315, 307, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 315, "extrude");
        atStep(st, 316);
        cqCut(c, st, 316, 314, {"toCut": {"wp": 315}, "clean": true, "tol": undefined});
        stepVol(c, st, 316, "cut");
        atStep(st, 317);
        cqUnion(c, st, 317, 301, {"toUnion": {"wp": 316}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 317, "union");
        atStep(st, 318);
        cqWorkplaneNew(c, st, 318, {"plane": {"origin": [10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8385254915624212, -0.484122918275927, -0.25]}, "obj": undefined});
        atStep(st, 319);
        cqMoveTo(c, st, 319, 318, {"x": -0.5, "y": -0.75});
        atStep(st, 320);
        cqLineTo(c, st, 320, 319, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 321);
        cqThreePointArc(c, st, 321, 320, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 322);
        cqLineTo(c, st, 322, 321, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 323);
        cqClose(c, st, 323, 322, {});
        atStep(st, 324);
        cqWorkplaneNew(c, st, 324, {"plane": {"origin": [10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8385254915624212, -0.484122918275927, -0.25]}, "obj": undefined});
        atStep(st, 325);
        cqMoveTo(c, st, 325, 324, {"x": -0.7, "y": -0.95});
        atStep(st, 326);
        cqLineTo(c, st, 326, 325, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 327);
        cqThreePointArc(c, st, 327, 326, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 328);
        cqLineTo(c, st, 328, 327, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 329);
        cqClose(c, st, 329, 328, {});
        atStep(st, 330);
        cqExtrude(c, st, 330, 329, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 330, "extrude");
        atStep(st, 331);
        cqExtrude(c, st, 331, 323, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 331, "extrude");
        atStep(st, 332);
        cqCut(c, st, 332, 330, {"toCut": {"wp": 331}, "clean": true, "tol": undefined});
        stepVol(c, st, 332, "cut");
        atStep(st, 333);
        cqUnion(c, st, 333, 317, {"toUnion": {"wp": 332}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 333, "union");
        atStep(st, 334);
        cqWorkplaneNew(c, st, 334, {"plane": {"origin": [5.809475019311126, 10.062305898749052, 3], "xDir": [-0.8660254037844386, 0.5000000000000001, -1.3877787807814457e-17], "zDir": [-0.4841229182759272, -0.8385254915624211, -0.25]}, "obj": undefined});
        atStep(st, 335);
        cqMoveTo(c, st, 335, 334, {"x": -0.5, "y": -0.75});
        atStep(st, 336);
        cqLineTo(c, st, 336, 335, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 337);
        cqThreePointArc(c, st, 337, 336, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 338);
        cqLineTo(c, st, 338, 337, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 339);
        cqClose(c, st, 339, 338, {});
        atStep(st, 340);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [5.809475019311126, 10.062305898749052, 3], "xDir": [-0.8660254037844386, 0.5000000000000001, -1.3877787807814457e-17], "zDir": [-0.4841229182759272, -0.8385254915624211, -0.25]}, "obj": undefined});
        atStep(st, 341);
        cqMoveTo(c, st, 341, 340, {"x": -0.7, "y": -0.95});
        atStep(st, 342);
        cqLineTo(c, st, 342, 341, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 343);
        cqThreePointArc(c, st, 343, 342, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 344);
        cqLineTo(c, st, 344, 343, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 345);
        cqClose(c, st, 345, 344, {});
        atStep(st, 346);
        cqExtrude(c, st, 346, 345, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 346, "extrude");
        atStep(st, 347);
        cqExtrude(c, st, 347, 339, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 347, "extrude");
        atStep(st, 348);
        cqCut(c, st, 348, 346, {"toCut": {"wp": 347}, "clean": true, "tol": undefined});
        stepVol(c, st, 348, "cut");
        atStep(st, 349);
        cqUnion(c, st, 349, 333, {"toUnion": {"wp": 348}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 349, "union");
        atStep(st, 350);
        cqWorkplaneNew(c, st, 350, {"plane": {"origin": [7.114554987125877e-16, 11.61895003862225, 3], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-5.928795822604899e-17, -0.9682458365518544, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 351);
        cqMoveTo(c, st, 351, 350, {"x": -0.5, "y": -0.75});
        atStep(st, 352);
        cqLineTo(c, st, 352, 351, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 353);
        cqThreePointArc(c, st, 353, 352, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 354);
        cqLineTo(c, st, 354, 353, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 355);
        cqClose(c, st, 355, 354, {});
        atStep(st, 356);
        cqWorkplaneNew(c, st, 356, {"plane": {"origin": [7.114554987125877e-16, 11.61895003862225, 3], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-5.928795822604899e-17, -0.9682458365518544, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 357);
        cqMoveTo(c, st, 357, 356, {"x": -0.7, "y": -0.95});
        atStep(st, 358);
        cqLineTo(c, st, 358, 357, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 359);
        cqThreePointArc(c, st, 359, 358, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 360);
        cqLineTo(c, st, 360, 359, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 361);
        cqClose(c, st, 361, 360, {});
        atStep(st, 362);
        cqExtrude(c, st, 362, 361, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        atStep(st, 363);
        cqExtrude(c, st, 363, 355, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 363, "extrude");
        atStep(st, 364);
        cqCut(c, st, 364, 362, {"toCut": {"wp": 363}, "clean": true, "tol": undefined});
        stepVol(c, st, 364, "cut");
        atStep(st, 365);
        cqUnion(c, st, 365, 349, {"toUnion": {"wp": 364}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 365, "union");
        atStep(st, 366);
        cqWorkplaneNew(c, st, 366, {"plane": {"origin": [-5.809475019311122, 10.062305898749054, 3], "xDir": [-0.8660254037844387, -0.4999999999999998, 0], "zDir": [0.4841229182759269, -0.8385254915624213, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 367);
        cqMoveTo(c, st, 367, 366, {"x": -0.5, "y": -0.75});
        atStep(st, 368);
        cqLineTo(c, st, 368, 367, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 369);
        cqThreePointArc(c, st, 369, 368, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 370);
        cqLineTo(c, st, 370, 369, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 371);
        cqClose(c, st, 371, 370, {});
        atStep(st, 372);
        cqWorkplaneNew(c, st, 372, {"plane": {"origin": [-5.809475019311122, 10.062305898749054, 3], "xDir": [-0.8660254037844387, -0.4999999999999998, 0], "zDir": [0.4841229182759269, -0.8385254915624213, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 373);
        cqMoveTo(c, st, 373, 372, {"x": -0.7, "y": -0.95});
        atStep(st, 374);
        cqLineTo(c, st, 374, 373, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 375);
        cqThreePointArc(c, st, 375, 374, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 376);
        cqLineTo(c, st, 376, 375, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 377);
        cqClose(c, st, 377, 376, {});
        atStep(st, 378);
        cqExtrude(c, st, 378, 377, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 378, "extrude");
        atStep(st, 379);
        cqExtrude(c, st, 379, 371, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 379, "extrude");
        atStep(st, 380);
        cqCut(c, st, 380, 378, {"toCut": {"wp": 379}, "clean": true, "tol": undefined});
        stepVol(c, st, 380, "cut");
        atStep(st, 381);
        cqUnion(c, st, 381, 365, {"toUnion": {"wp": 380}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 381, "union");
        atStep(st, 382);
        cqWorkplaneNew(c, st, 382, {"plane": {"origin": [-10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8385254915624212, -0.484122918275927, -0.25]}, "obj": undefined});
        atStep(st, 383);
        cqMoveTo(c, st, 383, 382, {"x": -0.5, "y": -0.75});
        atStep(st, 384);
        cqLineTo(c, st, 384, 383, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 385);
        cqThreePointArc(c, st, 385, 384, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 386);
        cqLineTo(c, st, 386, 385, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 387);
        cqClose(c, st, 387, 386, {});
        atStep(st, 388);
        cqWorkplaneNew(c, st, 388, {"plane": {"origin": [-10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8385254915624212, -0.484122918275927, -0.25]}, "obj": undefined});
        atStep(st, 389);
        cqMoveTo(c, st, 389, 388, {"x": -0.7, "y": -0.95});
        atStep(st, 390);
        cqLineTo(c, st, 390, 389, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 391);
        cqThreePointArc(c, st, 391, 390, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 392);
        cqLineTo(c, st, 392, 391, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 393);
        cqClose(c, st, 393, 392, {});
        atStep(st, 394);
        cqExtrude(c, st, 394, 393, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 394, "extrude");
        atStep(st, 395);
        cqExtrude(c, st, 395, 387, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 395, "extrude");
        atStep(st, 396);
        cqCut(c, st, 396, 394, {"toCut": {"wp": 395}, "clean": true, "tol": undefined});
        stepVol(c, st, 396, "cut");
        atStep(st, 397);
        cqUnion(c, st, 397, 381, {"toUnion": {"wp": 396}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 397, "union");
        atStep(st, 398);
        cqWorkplaneNew(c, st, 398, {"plane": {"origin": [-11.61895003862225, 1.4229109974251757e-15, 3], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [0.9682458365518544, -1.1857591645209795e-16, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 399);
        cqMoveTo(c, st, 399, 398, {"x": -0.5, "y": -0.75});
        atStep(st, 400);
        cqLineTo(c, st, 400, 399, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 401);
        cqThreePointArc(c, st, 401, 400, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 402);
        cqLineTo(c, st, 402, 401, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 403);
        cqClose(c, st, 403, 402, {});
        atStep(st, 404);
        cqWorkplaneNew(c, st, 404, {"plane": {"origin": [-11.61895003862225, 1.4229109974251757e-15, 3], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [0.9682458365518544, -1.1857591645209795e-16, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 405);
        cqMoveTo(c, st, 405, 404, {"x": -0.7, "y": -0.95});
        atStep(st, 406);
        cqLineTo(c, st, 406, 405, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 407);
        cqThreePointArc(c, st, 407, 406, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 408);
        cqLineTo(c, st, 408, 407, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 409);
        cqClose(c, st, 409, 408, {});
        atStep(st, 410);
        cqExtrude(c, st, 410, 409, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 410, "extrude");
        atStep(st, 411);
        cqExtrude(c, st, 411, 403, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 411, "extrude");
        atStep(st, 412);
        cqCut(c, st, 412, 410, {"toCut": {"wp": 411}, "clean": true, "tol": undefined});
        stepVol(c, st, 412, "cut");
        atStep(st, 413);
        cqUnion(c, st, 413, 397, {"toUnion": {"wp": 412}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 413, "union");
        atStep(st, 414);
        cqWorkplaneNew(c, st, 414, {"plane": {"origin": [-10.062305898749056, -5.8094750193111215, 3], "xDir": [0.4999999999999997, -0.8660254037844388, -1.3877787807814457e-17], "zDir": [0.8385254915624214, 0.4841229182759269, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 415);
        cqMoveTo(c, st, 415, 414, {"x": -0.5, "y": -0.75});
        atStep(st, 416);
        cqLineTo(c, st, 416, 415, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 417);
        cqThreePointArc(c, st, 417, 416, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 418);
        cqLineTo(c, st, 418, 417, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 419);
        cqClose(c, st, 419, 418, {});
        atStep(st, 420);
        cqWorkplaneNew(c, st, 420, {"plane": {"origin": [-10.062305898749056, -5.8094750193111215, 3], "xDir": [0.4999999999999997, -0.8660254037844388, -1.3877787807814457e-17], "zDir": [0.8385254915624214, 0.4841229182759269, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 421);
        cqMoveTo(c, st, 421, 420, {"x": -0.7, "y": -0.95});
        atStep(st, 422);
        cqLineTo(c, st, 422, 421, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 423);
        cqThreePointArc(c, st, 423, 422, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 424);
        cqLineTo(c, st, 424, 423, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 425);
        cqClose(c, st, 425, 424, {});
        atStep(st, 426);
        cqExtrude(c, st, 426, 425, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 427);
        cqExtrude(c, st, 427, 419, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 427, "extrude");
        atStep(st, 428);
        cqCut(c, st, 428, 426, {"toCut": {"wp": 427}, "clean": true, "tol": undefined});
        stepVol(c, st, 428, "cut");
        atStep(st, 429);
        cqUnion(c, st, 429, 413, {"toUnion": {"wp": 428}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 429, "union");
        atStep(st, 430);
        cqWorkplaneNew(c, st, 430, {"plane": {"origin": [-5.80947501931113, -10.06230589874905, 3], "xDir": [0.8660254037844385, -0.5000000000000004, 0], "zDir": [0.4841229182759275, 0.8385254915624208, -0.25]}, "obj": undefined});
        atStep(st, 431);
        cqMoveTo(c, st, 431, 430, {"x": -0.5, "y": -0.75});
        atStep(st, 432);
        cqLineTo(c, st, 432, 431, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 433);
        cqThreePointArc(c, st, 433, 432, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 434);
        cqLineTo(c, st, 434, 433, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 435);
        cqClose(c, st, 435, 434, {});
        atStep(st, 436);
        cqWorkplaneNew(c, st, 436, {"plane": {"origin": [-5.80947501931113, -10.06230589874905, 3], "xDir": [0.8660254037844385, -0.5000000000000004, 0], "zDir": [0.4841229182759275, 0.8385254915624208, -0.25]}, "obj": undefined});
        atStep(st, 437);
        cqMoveTo(c, st, 437, 436, {"x": -0.7, "y": -0.95});
        atStep(st, 438);
        cqLineTo(c, st, 438, 437, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 439);
        cqThreePointArc(c, st, 439, 438, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 440);
        cqLineTo(c, st, 440, 439, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 441);
        cqClose(c, st, 441, 440, {});
        atStep(st, 442);
        cqExtrude(c, st, 442, 441, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 442, "extrude");
        atStep(st, 443);
        cqExtrude(c, st, 443, 435, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 443, "extrude");
        atStep(st, 444);
        cqCut(c, st, 444, 442, {"toCut": {"wp": 443}, "clean": true, "tol": undefined});
        stepVol(c, st, 444, "cut");
        atStep(st, 445);
        cqUnion(c, st, 445, 429, {"toUnion": {"wp": 444}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 445, "union");
        atStep(st, 446);
        cqWorkplaneNew(c, st, 446, {"plane": {"origin": [-2.134366496137763e-15, -11.61895003862225, 3], "xDir": [1, -1.8369701987210294e-16, 6.162975822039154e-33], "zDir": [1.778638746781469e-16, 0.9682458365518544, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 447);
        cqMoveTo(c, st, 447, 446, {"x": -0.5, "y": -0.75});
        atStep(st, 448);
        cqLineTo(c, st, 448, 447, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 449);
        cqThreePointArc(c, st, 449, 448, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 450);
        cqLineTo(c, st, 450, 449, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 451);
        cqClose(c, st, 451, 450, {});
        atStep(st, 452);
        cqWorkplaneNew(c, st, 452, {"plane": {"origin": [-2.134366496137763e-15, -11.61895003862225, 3], "xDir": [1, -1.8369701987210294e-16, 6.162975822039154e-33], "zDir": [1.778638746781469e-16, 0.9682458365518544, -0.25000000000000006]}, "obj": undefined});
        atStep(st, 453);
        cqMoveTo(c, st, 453, 452, {"x": -0.7, "y": -0.95});
        atStep(st, 454);
        cqLineTo(c, st, 454, 453, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 455);
        cqThreePointArc(c, st, 455, 454, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 456);
        cqLineTo(c, st, 456, 455, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 457);
        cqClose(c, st, 457, 456, {});
        atStep(st, 458);
        cqExtrude(c, st, 458, 457, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 458, "extrude");
        atStep(st, 459);
        cqExtrude(c, st, 459, 451, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 459, "extrude");
        atStep(st, 460);
        cqCut(c, st, 460, 458, {"toCut": {"wp": 459}, "clean": true, "tol": undefined});
        stepVol(c, st, 460, "cut");
        atStep(st, 461);
        cqUnion(c, st, 461, 445, {"toUnion": {"wp": 460}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 461, "union");
        atStep(st, 462);
        cqWorkplaneNew(c, st, 462, {"plane": {"origin": [5.809475019311126, -10.062305898749052, 3], "xDir": [0.8660254037844386, 0.5000000000000001, 1.3877787807814457e-17], "zDir": [-0.4841229182759272, 0.8385254915624211, -0.25]}, "obj": undefined});
        atStep(st, 463);
        cqMoveTo(c, st, 463, 462, {"x": -0.5, "y": -0.75});
        atStep(st, 464);
        cqLineTo(c, st, 464, 463, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 465);
        cqThreePointArc(c, st, 465, 464, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 466);
        cqLineTo(c, st, 466, 465, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 467);
        cqClose(c, st, 467, 466, {});
        atStep(st, 468);
        cqWorkplaneNew(c, st, 468, {"plane": {"origin": [5.809475019311126, -10.062305898749052, 3], "xDir": [0.8660254037844386, 0.5000000000000001, 1.3877787807814457e-17], "zDir": [-0.4841229182759272, 0.8385254915624211, -0.25]}, "obj": undefined});
        atStep(st, 469);
        cqMoveTo(c, st, 469, 468, {"x": -0.7, "y": -0.95});
        atStep(st, 470);
        cqLineTo(c, st, 470, 469, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 471);
        cqThreePointArc(c, st, 471, 470, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 472);
        cqLineTo(c, st, 472, 471, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 473);
        cqClose(c, st, 473, 472, {});
        atStep(st, 474);
        cqExtrude(c, st, 474, 473, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 474, "extrude");
        atStep(st, 475);
        cqExtrude(c, st, 475, 467, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 475, "extrude");
        atStep(st, 476);
        cqCut(c, st, 476, 474, {"toCut": {"wp": 475}, "clean": true, "tol": undefined});
        stepVol(c, st, 476, "cut");
        atStep(st, 477);
        cqUnion(c, st, 477, 461, {"toUnion": {"wp": 476}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 477, "union");
        atStep(st, 478);
        cqWorkplaneNew(c, st, 478, {"plane": {"origin": [10.06230589874905, -5.80947501931113, 3], "xDir": [0.5000000000000004, 0.8660254037844385, 0], "zDir": [-0.8385254915624208, 0.4841229182759275, -0.25]}, "obj": undefined});
        atStep(st, 479);
        cqMoveTo(c, st, 479, 478, {"x": -0.5, "y": -0.75});
        atStep(st, 480);
        cqLineTo(c, st, 480, 479, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 481);
        cqThreePointArc(c, st, 481, 480, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 482);
        cqLineTo(c, st, 482, 481, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 483);
        cqClose(c, st, 483, 482, {});
        atStep(st, 484);
        cqWorkplaneNew(c, st, 484, {"plane": {"origin": [10.06230589874905, -5.80947501931113, 3], "xDir": [0.5000000000000004, 0.8660254037844385, 0], "zDir": [-0.8385254915624208, 0.4841229182759275, -0.25]}, "obj": undefined});
        atStep(st, 485);
        cqMoveTo(c, st, 485, 484, {"x": -0.7, "y": -0.95});
        atStep(st, 486);
        cqLineTo(c, st, 486, 485, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 487);
        cqThreePointArc(c, st, 487, 486, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 488);
        cqLineTo(c, st, 488, 487, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 489);
        cqClose(c, st, 489, 488, {});
        atStep(st, 490);
        cqExtrude(c, st, 490, 489, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 490, "extrude");
        atStep(st, 491);
        cqExtrude(c, st, 491, 483, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 491, "extrude");
        atStep(st, 492);
        cqCut(c, st, 492, 490, {"toCut": {"wp": 491}, "clean": true, "tol": undefined});
        stepVol(c, st, 492, "cut");
        atStep(st, 493);
        cqUnion(c, st, 493, 477, {"toUnion": {"wp": 492}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 493, "union");
        atStep(st, 494);
        cqWorkplaneNew(c, st, 494, {"plane": {"origin": [10.392304845413264, 0, 6], "xDir": [0, 1, 0], "zDir": [-0.8660254037844387, 0, -0.5]}, "obj": undefined});
        atStep(st, 495);
        cqMoveTo(c, st, 495, 494, {"x": -0.5, "y": -0.75});
        atStep(st, 496);
        cqLineTo(c, st, 496, 495, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 497);
        cqThreePointArc(c, st, 497, 496, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 498);
        cqLineTo(c, st, 498, 497, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 499);
        cqClose(c, st, 499, 498, {});
        atStep(st, 500);
        cqWorkplaneNew(c, st, 500, {"plane": {"origin": [10.392304845413264, 0, 6], "xDir": [0, 1, 0], "zDir": [-0.8660254037844387, 0, -0.5]}, "obj": undefined});
        atStep(st, 501);
        cqMoveTo(c, st, 501, 500, {"x": -0.7, "y": -0.95});
        atStep(st, 502);
        cqLineTo(c, st, 502, 501, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 503);
        cqThreePointArc(c, st, 503, 502, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 504);
        cqLineTo(c, st, 504, 503, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 505);
        cqClose(c, st, 505, 504, {});
        atStep(st, 506);
        cqExtrude(c, st, 506, 505, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 506, "extrude");
        atStep(st, 507);
        cqExtrude(c, st, 507, 499, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 507, "extrude");
        atStep(st, 508);
        cqCut(c, st, 508, 506, {"toCut": {"wp": 507}, "clean": true, "tol": undefined});
        stepVol(c, st, 508, "cut");
        atStep(st, 509);
        cqUnion(c, st, 509, 493, {"toUnion": {"wp": 508}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 509, "union");
        atStep(st, 510);
        cqWorkplaneNew(c, st, 510, {"plane": {"origin": [9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, 0.8660254037844387, -2.7755575615628907e-17], "zDir": [-0.7500000000000001, -0.4330127018922193, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 511);
        cqMoveTo(c, st, 511, 510, {"x": -0.5, "y": -0.75});
        atStep(st, 512);
        cqLineTo(c, st, 512, 511, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 513);
        cqThreePointArc(c, st, 513, 512, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 514);
        cqLineTo(c, st, 514, 513, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 515);
        cqClose(c, st, 515, 514, {});
        atStep(st, 516);
        cqWorkplaneNew(c, st, 516, {"plane": {"origin": [9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, 0.8660254037844387, -2.7755575615628907e-17], "zDir": [-0.7500000000000001, -0.4330127018922193, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 517);
        cqMoveTo(c, st, 517, 516, {"x": -0.7, "y": -0.95});
        atStep(st, 518);
        cqLineTo(c, st, 518, 517, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 519);
        cqThreePointArc(c, st, 519, 518, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 520);
        cqLineTo(c, st, 520, 519, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 521);
        cqClose(c, st, 521, 520, {});
        atStep(st, 522);
        cqExtrude(c, st, 522, 521, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 522, "extrude");
        atStep(st, 523);
        cqExtrude(c, st, 523, 515, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 523, "extrude");
        atStep(st, 524);
        cqCut(c, st, 524, 522, {"toCut": {"wp": 523}, "clean": true, "tol": undefined});
        stepVol(c, st, 524, "cut");
        atStep(st, 525);
        cqUnion(c, st, 525, 509, {"toUnion": {"wp": 524}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 525, "union");
        atStep(st, 526);
        cqWorkplaneNew(c, st, 526, {"plane": {"origin": [5.196152422706633, 9, 6], "xDir": [-0.8660254037844386, 0.5000000000000001, -2.7755575615628907e-17], "zDir": [-0.4330127018922194, -0.75, -0.5]}, "obj": undefined});
        atStep(st, 527);
        cqMoveTo(c, st, 527, 526, {"x": -0.5, "y": -0.75});
        atStep(st, 528);
        cqLineTo(c, st, 528, 527, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 529);
        cqThreePointArc(c, st, 529, 528, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 530);
        cqLineTo(c, st, 530, 529, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 531);
        cqClose(c, st, 531, 530, {});
        atStep(st, 532);
        cqWorkplaneNew(c, st, 532, {"plane": {"origin": [5.196152422706633, 9, 6], "xDir": [-0.8660254037844386, 0.5000000000000001, -2.7755575615628907e-17], "zDir": [-0.4330127018922194, -0.75, -0.5]}, "obj": undefined});
        atStep(st, 533);
        cqMoveTo(c, st, 533, 532, {"x": -0.7, "y": -0.95});
        atStep(st, 534);
        cqLineTo(c, st, 534, 533, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 535);
        cqThreePointArc(c, st, 535, 534, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 536);
        cqLineTo(c, st, 536, 535, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 537);
        cqClose(c, st, 537, 536, {});
        atStep(st, 538);
        cqExtrude(c, st, 538, 537, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 538, "extrude");
        atStep(st, 539);
        cqExtrude(c, st, 539, 531, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 539, "extrude");
        atStep(st, 540);
        cqCut(c, st, 540, 538, {"toCut": {"wp": 539}, "clean": true, "tol": undefined});
        stepVol(c, st, 540, "cut");
        atStep(st, 541);
        cqUnion(c, st, 541, 525, {"toUnion": {"wp": 540}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 541, "union");
        atStep(st, 542);
        cqWorkplaneNew(c, st, 542, {"plane": {"origin": [6.363451432349442e-16, 10.392304845413264, 6], "xDir": [-1, 6.123233995736767e-17, 3.081487911019578e-33], "zDir": [-5.3028761936245346e-17, -0.8660254037844387, -0.5]}, "obj": undefined});
        atStep(st, 543);
        cqMoveTo(c, st, 543, 542, {"x": -0.5, "y": -0.75});
        atStep(st, 544);
        cqLineTo(c, st, 544, 543, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 545);
        cqThreePointArc(c, st, 545, 544, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 546);
        cqLineTo(c, st, 546, 545, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 547);
        cqClose(c, st, 547, 546, {});
        atStep(st, 548);
        cqWorkplaneNew(c, st, 548, {"plane": {"origin": [6.363451432349442e-16, 10.392304845413264, 6], "xDir": [-1, 6.123233995736767e-17, 3.081487911019578e-33], "zDir": [-5.3028761936245346e-17, -0.8660254037844387, -0.5]}, "obj": undefined});
        atStep(st, 549);
        cqMoveTo(c, st, 549, 548, {"x": -0.7, "y": -0.95});
        atStep(st, 550);
        cqLineTo(c, st, 550, 549, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 551);
        cqThreePointArc(c, st, 551, 550, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 552);
        cqLineTo(c, st, 552, 551, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 553);
        cqClose(c, st, 553, 552, {});
        atStep(st, 554);
        cqExtrude(c, st, 554, 553, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 554, "extrude");
        atStep(st, 555);
        cqExtrude(c, st, 555, 547, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 555, "extrude");
        atStep(st, 556);
        cqCut(c, st, 556, 554, {"toCut": {"wp": 555}, "clean": true, "tol": undefined});
        stepVol(c, st, 556, "cut");
        atStep(st, 557);
        cqUnion(c, st, 557, 541, {"toUnion": {"wp": 556}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 557, "union");
        atStep(st, 558);
        cqWorkplaneNew(c, st, 558, {"plane": {"origin": [-5.196152422706629, 9, 6], "xDir": [-0.8660254037844387, -0.4999999999999998, -2.7755575615628907e-17], "zDir": [0.4330127018922192, -0.7500000000000001, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 559);
        cqMoveTo(c, st, 559, 558, {"x": -0.5, "y": -0.75});
        atStep(st, 560);
        cqLineTo(c, st, 560, 559, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 561);
        cqThreePointArc(c, st, 561, 560, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 562);
        cqLineTo(c, st, 562, 561, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 563);
        cqClose(c, st, 563, 562, {});
        atStep(st, 564);
        cqWorkplaneNew(c, st, 564, {"plane": {"origin": [-5.196152422706629, 9, 6], "xDir": [-0.8660254037844387, -0.4999999999999998, -2.7755575615628907e-17], "zDir": [0.4330127018922192, -0.7500000000000001, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 565);
        cqMoveTo(c, st, 565, 564, {"x": -0.7, "y": -0.95});
        atStep(st, 566);
        cqLineTo(c, st, 566, 565, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 567);
        cqThreePointArc(c, st, 567, 566, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 568);
        cqLineTo(c, st, 568, 567, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 569);
        cqClose(c, st, 569, 568, {});
        atStep(st, 570);
        cqExtrude(c, st, 570, 569, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 570, "extrude");
        atStep(st, 571);
        cqExtrude(c, st, 571, 563, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 571, "extrude");
        atStep(st, 572);
        cqCut(c, st, 572, 570, {"toCut": {"wp": 571}, "clean": true, "tol": undefined});
        stepVol(c, st, 572, "cut");
        atStep(st, 573);
        cqUnion(c, st, 573, 557, {"toUnion": {"wp": 572}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 573, "union");
        atStep(st, 574);
        cqWorkplaneNew(c, st, 574, {"plane": {"origin": [-9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, -0.8660254037844387, 2.7755575615628907e-17], "zDir": [0.7500000000000001, -0.4330127018922193, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 575);
        cqMoveTo(c, st, 575, 574, {"x": -0.5, "y": -0.75});
        atStep(st, 576);
        cqLineTo(c, st, 576, 575, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 577);
        cqThreePointArc(c, st, 577, 576, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 578);
        cqLineTo(c, st, 578, 577, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 579);
        cqClose(c, st, 579, 578, {});
        atStep(st, 580);
        cqWorkplaneNew(c, st, 580, {"plane": {"origin": [-9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, -0.8660254037844387, 2.7755575615628907e-17], "zDir": [0.7500000000000001, -0.4330127018922193, -0.5000000000000001]}, "obj": undefined});
        atStep(st, 581);
        cqMoveTo(c, st, 581, 580, {"x": -0.7, "y": -0.95});
        atStep(st, 582);
        cqLineTo(c, st, 582, 581, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 583);
        cqThreePointArc(c, st, 583, 582, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 584);
        cqLineTo(c, st, 584, 583, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 585);
        cqClose(c, st, 585, 584, {});
        atStep(st, 586);
        cqExtrude(c, st, 586, 585, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 586, "extrude");
        atStep(st, 587);
        cqExtrude(c, st, 587, 579, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 587, "extrude");
        atStep(st, 588);
        cqCut(c, st, 588, 586, {"toCut": {"wp": 587}, "clean": true, "tol": undefined});
        stepVol(c, st, 588, "cut");
        atStep(st, 589);
        cqUnion(c, st, 589, 573, {"toUnion": {"wp": 588}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 589, "union");
        atStep(st, 590);
        cqWorkplaneNew(c, st, 590, {"plane": {"origin": [-10.392304845413264, 1.2726902864698884e-15, 6], "xDir": [-1.2246467991473535e-16, -1, 6.162975822039155e-33], "zDir": [0.8660254037844387, -1.060575238724907e-16, -0.5]}, "obj": undefined});
        atStep(st, 591);
        cqMoveTo(c, st, 591, 590, {"x": -0.5, "y": -0.75});
        atStep(st, 592);
        cqLineTo(c, st, 592, 591, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 593);
        cqThreePointArc(c, st, 593, 592, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 594);
        cqLineTo(c, st, 594, 593, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 595);
        cqClose(c, st, 595, 594, {});
        atStep(st, 596);
        cqWorkplaneNew(c, st, 596, {"plane": {"origin": [-10.392304845413264, 1.2726902864698884e-15, 6], "xDir": [-1.2246467991473535e-16, -1, 6.162975822039155e-33], "zDir": [0.8660254037844387, -1.060575238724907e-16, -0.5]}, "obj": undefined});
        atStep(st, 597);
        cqMoveTo(c, st, 597, 596, {"x": -0.7, "y": -0.95});
        atStep(st, 598);
        cqLineTo(c, st, 598, 597, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 599);
        cqThreePointArc(c, st, 599, 598, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 600);
        cqLineTo(c, st, 600, 599, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 601);
        cqClose(c, st, 601, 600, {});
        atStep(st, 602);
        cqExtrude(c, st, 602, 601, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 602, "extrude");
        atStep(st, 603);
        cqExtrude(c, st, 603, 595, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 603, "extrude");
        atStep(st, 604);
        cqCut(c, st, 604, 602, {"toCut": {"wp": 603}, "clean": true, "tol": undefined});
        stepVol(c, st, 604, "cut");
        atStep(st, 605);
        cqUnion(c, st, 605, 589, {"toUnion": {"wp": 604}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 605, "union");
        atStep(st, 606);
        cqWorkplaneNew(c, st, 606, {"plane": {"origin": [-9.000000000000002, -5.196152422706629, 6], "xDir": [0.4999999999999998, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [0.7500000000000001, 0.43301270189221913, -0.5]}, "obj": undefined});
        atStep(st, 607);
        cqMoveTo(c, st, 607, 606, {"x": -0.5, "y": -0.75});
        atStep(st, 608);
        cqLineTo(c, st, 608, 607, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 609);
        cqThreePointArc(c, st, 609, 608, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 610);
        cqLineTo(c, st, 610, 609, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 611);
        cqClose(c, st, 611, 610, {});
        atStep(st, 612);
        cqWorkplaneNew(c, st, 612, {"plane": {"origin": [-9.000000000000002, -5.196152422706629, 6], "xDir": [0.4999999999999998, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [0.7500000000000001, 0.43301270189221913, -0.5]}, "obj": undefined});
        atStep(st, 613);
        cqMoveTo(c, st, 613, 612, {"x": -0.7, "y": -0.95});
        atStep(st, 614);
        cqLineTo(c, st, 614, 613, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 615);
        cqThreePointArc(c, st, 615, 614, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 616);
        cqLineTo(c, st, 616, 615, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 617);
        cqClose(c, st, 617, 616, {});
        atStep(st, 618);
        cqExtrude(c, st, 618, 617, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 618, "extrude");
        atStep(st, 619);
        cqExtrude(c, st, 619, 611, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 619, "extrude");
        atStep(st, 620);
        cqCut(c, st, 620, 618, {"toCut": {"wp": 619}, "clean": true, "tol": undefined});
        stepVol(c, st, 620, "cut");
        atStep(st, 621);
        cqUnion(c, st, 621, 605, {"toUnion": {"wp": 620}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 621, "union");
        atStep(st, 622);
        cqWorkplaneNew(c, st, 622, {"plane": {"origin": [-5.1961524227066365, -8.999999999999998, 6], "xDir": [0.8660254037844384, -0.5000000000000003, -2.7755575615628907e-17], "zDir": [0.4330127018922197, 0.7499999999999999, -0.5]}, "obj": undefined});
        atStep(st, 623);
        cqMoveTo(c, st, 623, 622, {"x": -0.5, "y": -0.75});
        atStep(st, 624);
        cqLineTo(c, st, 624, 623, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 625);
        cqThreePointArc(c, st, 625, 624, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 626);
        cqLineTo(c, st, 626, 625, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 627);
        cqClose(c, st, 627, 626, {});
        atStep(st, 628);
        cqWorkplaneNew(c, st, 628, {"plane": {"origin": [-5.1961524227066365, -8.999999999999998, 6], "xDir": [0.8660254037844384, -0.5000000000000003, -2.7755575615628907e-17], "zDir": [0.4330127018922197, 0.7499999999999999, -0.5]}, "obj": undefined});
        atStep(st, 629);
        cqMoveTo(c, st, 629, 628, {"x": -0.7, "y": -0.95});
        atStep(st, 630);
        cqLineTo(c, st, 630, 629, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 631);
        cqThreePointArc(c, st, 631, 630, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 632);
        cqLineTo(c, st, 632, 631, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 633);
        cqClose(c, st, 633, 632, {});
        atStep(st, 634);
        cqExtrude(c, st, 634, 633, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 634, "extrude");
        atStep(st, 635);
        cqExtrude(c, st, 635, 627, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 635, "extrude");
        atStep(st, 636);
        cqCut(c, st, 636, 634, {"toCut": {"wp": 635}, "clean": true, "tol": undefined});
        stepVol(c, st, 636, "cut");
        atStep(st, 637);
        cqUnion(c, st, 637, 621, {"toUnion": {"wp": 636}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 637, "union");
        atStep(st, 638);
        cqWorkplaneNew(c, st, 638, {"plane": {"origin": [-1.9090354297048323e-15, -10.392304845413264, 6], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.59086285808736e-16, 0.8660254037844387, -0.5]}, "obj": undefined});
        atStep(st, 639);
        cqMoveTo(c, st, 639, 638, {"x": -0.5, "y": -0.75});
        atStep(st, 640);
        cqLineTo(c, st, 640, 639, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 641);
        cqThreePointArc(c, st, 641, 640, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 642);
        cqLineTo(c, st, 642, 641, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 643);
        cqClose(c, st, 643, 642, {});
        atStep(st, 644);
        cqWorkplaneNew(c, st, 644, {"plane": {"origin": [-1.9090354297048323e-15, -10.392304845413264, 6], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.59086285808736e-16, 0.8660254037844387, -0.5]}, "obj": undefined});
        atStep(st, 645);
        cqMoveTo(c, st, 645, 644, {"x": -0.7, "y": -0.95});
        atStep(st, 646);
        cqLineTo(c, st, 646, 645, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 647);
        cqThreePointArc(c, st, 647, 646, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 648);
        cqLineTo(c, st, 648, 647, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 649);
        cqClose(c, st, 649, 648, {});
        atStep(st, 650);
        cqExtrude(c, st, 650, 649, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 650, "extrude");
        atStep(st, 651);
        cqExtrude(c, st, 651, 643, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 651, "extrude");
        atStep(st, 652);
        cqCut(c, st, 652, 650, {"toCut": {"wp": 651}, "clean": true, "tol": undefined});
        stepVol(c, st, 652, "cut");
        atStep(st, 653);
        cqUnion(c, st, 653, 637, {"toUnion": {"wp": 652}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 653, "union");
        atStep(st, 654);
        cqWorkplaneNew(c, st, 654, {"plane": {"origin": [5.196152422706633, -9, 6], "xDir": [0.8660254037844386, 0.5000000000000001, 2.7755575615628907e-17], "zDir": [-0.4330127018922194, 0.75, -0.5]}, "obj": undefined});
        atStep(st, 655);
        cqMoveTo(c, st, 655, 654, {"x": -0.5, "y": -0.75});
        atStep(st, 656);
        cqLineTo(c, st, 656, 655, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 657);
        cqThreePointArc(c, st, 657, 656, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 658);
        cqLineTo(c, st, 658, 657, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 659);
        cqClose(c, st, 659, 658, {});
        atStep(st, 660);
        cqWorkplaneNew(c, st, 660, {"plane": {"origin": [5.196152422706633, -9, 6], "xDir": [0.8660254037844386, 0.5000000000000001, 2.7755575615628907e-17], "zDir": [-0.4330127018922194, 0.75, -0.5]}, "obj": undefined});
        atStep(st, 661);
        cqMoveTo(c, st, 661, 660, {"x": -0.7, "y": -0.95});
        atStep(st, 662);
        cqLineTo(c, st, 662, 661, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 663);
        cqThreePointArc(c, st, 663, 662, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 664);
        cqLineTo(c, st, 664, 663, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 665);
        cqClose(c, st, 665, 664, {});
        atStep(st, 666);
        cqExtrude(c, st, 666, 665, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 666, "extrude");
        atStep(st, 667);
        cqExtrude(c, st, 667, 659, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 667, "extrude");
        atStep(st, 668);
        cqCut(c, st, 668, 666, {"toCut": {"wp": 667}, "clean": true, "tol": undefined});
        stepVol(c, st, 668, "cut");
        atStep(st, 669);
        cqUnion(c, st, 669, 653, {"toUnion": {"wp": 668}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 669, "union");
        atStep(st, 670);
        cqWorkplaneNew(c, st, 670, {"plane": {"origin": [8.999999999999998, -5.1961524227066365, 6], "xDir": [0.5000000000000003, 0.8660254037844384, -2.7755575615628907e-17], "zDir": [-0.7499999999999999, 0.4330127018922197, -0.5]}, "obj": undefined});
        atStep(st, 671);
        cqMoveTo(c, st, 671, 670, {"x": -0.5, "y": -0.75});
        atStep(st, 672);
        cqLineTo(c, st, 672, 671, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 673);
        cqThreePointArc(c, st, 673, 672, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 674);
        cqLineTo(c, st, 674, 673, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 675);
        cqClose(c, st, 675, 674, {});
        atStep(st, 676);
        cqWorkplaneNew(c, st, 676, {"plane": {"origin": [8.999999999999998, -5.1961524227066365, 6], "xDir": [0.5000000000000003, 0.8660254037844384, -2.7755575615628907e-17], "zDir": [-0.7499999999999999, 0.4330127018922197, -0.5]}, "obj": undefined});
        atStep(st, 677);
        cqMoveTo(c, st, 677, 676, {"x": -0.7, "y": -0.95});
        atStep(st, 678);
        cqLineTo(c, st, 678, 677, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 679);
        cqThreePointArc(c, st, 679, 678, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 680);
        cqLineTo(c, st, 680, 679, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 681);
        cqClose(c, st, 681, 680, {});
        atStep(st, 682);
        cqExtrude(c, st, 682, 681, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 682, "extrude");
        atStep(st, 683);
        cqExtrude(c, st, 683, 675, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 683, "extrude");
        atStep(st, 684);
        cqCut(c, st, 684, 682, {"toCut": {"wp": 683}, "clean": true, "tol": undefined});
        stepVol(c, st, 684, "cut");
        atStep(st, 685);
        cqUnion(c, st, 685, 669, {"toUnion": {"wp": 684}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 685, "union");
        atStep(st, 686);
        cqWorkplaneNew(c, st, 686, {"plane": {"origin": [7.937253933193772, 0, 9], "xDir": [0, 1, 0], "zDir": [-0.6614378277661477, 0, -0.75]}, "obj": undefined});
        atStep(st, 687);
        cqMoveTo(c, st, 687, 686, {"x": -0.5, "y": -0.75});
        atStep(st, 688);
        cqLineTo(c, st, 688, 687, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 689);
        cqThreePointArc(c, st, 689, 688, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 690);
        cqLineTo(c, st, 690, 689, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 691);
        cqClose(c, st, 691, 690, {});
        atStep(st, 692);
        cqWorkplaneNew(c, st, 692, {"plane": {"origin": [7.937253933193772, 0, 9], "xDir": [0, 1, 0], "zDir": [-0.6614378277661477, 0, -0.75]}, "obj": undefined});
        atStep(st, 693);
        cqMoveTo(c, st, 693, 692, {"x": -0.7, "y": -0.95});
        atStep(st, 694);
        cqLineTo(c, st, 694, 693, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 695);
        cqThreePointArc(c, st, 695, 694, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 696);
        cqLineTo(c, st, 696, 695, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 697);
        cqClose(c, st, 697, 696, {});
        atStep(st, 698);
        cqExtrude(c, st, 698, 697, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 698, "extrude");
        atStep(st, 699);
        cqExtrude(c, st, 699, 691, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 699, "extrude");
        atStep(st, 700);
        cqCut(c, st, 700, 698, {"toCut": {"wp": 699}, "clean": true, "tol": undefined});
        stepVol(c, st, 700, "cut");
        atStep(st, 701);
        cqUnion(c, st, 701, 685, {"toUnion": {"wp": 700}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 701, "union");
        atStep(st, 702);
        cqWorkplaneNew(c, st, 702, {"plane": {"origin": [6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, 0.8660254037844388, 2.7755575615628914e-17], "zDir": [-0.5728219618694801, -0.3307189138830738, -0.75]}, "obj": undefined});
        atStep(st, 703);
        cqMoveTo(c, st, 703, 702, {"x": -0.5, "y": -0.75});
        atStep(st, 704);
        cqLineTo(c, st, 704, 703, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 705);
        cqThreePointArc(c, st, 705, 704, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 706);
        cqLineTo(c, st, 706, 705, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 707);
        cqClose(c, st, 707, 706, {});
        atStep(st, 708);
        cqWorkplaneNew(c, st, 708, {"plane": {"origin": [6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, 0.8660254037844388, 2.7755575615628914e-17], "zDir": [-0.5728219618694801, -0.3307189138830738, -0.75]}, "obj": undefined});
        atStep(st, 709);
        cqMoveTo(c, st, 709, 708, {"x": -0.7, "y": -0.95});
        atStep(st, 710);
        cqLineTo(c, st, 710, 709, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 711);
        cqThreePointArc(c, st, 711, 710, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 712);
        cqLineTo(c, st, 712, 711, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 713);
        cqClose(c, st, 713, 712, {});
        atStep(st, 714);
        cqExtrude(c, st, 714, 713, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 714, "extrude");
        atStep(st, 715);
        cqExtrude(c, st, 715, 707, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 715, "extrude");
        atStep(st, 716);
        cqCut(c, st, 716, 714, {"toCut": {"wp": 715}, "clean": true, "tol": undefined});
        stepVol(c, st, 716, "cut");
        atStep(st, 717);
        cqUnion(c, st, 717, 701, {"toUnion": {"wp": 716}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 717, "union");
        atStep(st, 718);
        cqWorkplaneNew(c, st, 718, {"plane": {"origin": [3.968626966596887, 6.87386354243376, 9], "xDir": [-0.8660254037844386, 0.5000000000000002, 0], "zDir": [-0.3307189138830739, -0.57282196186948, -0.75]}, "obj": undefined});
        atStep(st, 719);
        cqMoveTo(c, st, 719, 718, {"x": -0.5, "y": -0.75});
        atStep(st, 720);
        cqLineTo(c, st, 720, 719, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 721);
        cqThreePointArc(c, st, 721, 720, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 722);
        cqLineTo(c, st, 722, 721, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 723);
        cqClose(c, st, 723, 722, {});
        atStep(st, 724);
        cqWorkplaneNew(c, st, 724, {"plane": {"origin": [3.968626966596887, 6.87386354243376, 9], "xDir": [-0.8660254037844386, 0.5000000000000002, 0], "zDir": [-0.3307189138830739, -0.57282196186948, -0.75]}, "obj": undefined});
        atStep(st, 725);
        cqMoveTo(c, st, 725, 724, {"x": -0.7, "y": -0.95});
        atStep(st, 726);
        cqLineTo(c, st, 726, 725, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 727);
        cqThreePointArc(c, st, 727, 726, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 728);
        cqLineTo(c, st, 728, 727, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 729);
        cqClose(c, st, 729, 728, {});
        atStep(st, 730);
        cqExtrude(c, st, 730, 729, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 730, "extrude");
        atStep(st, 731);
        cqExtrude(c, st, 731, 723, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 731, "extrude");
        atStep(st, 732);
        cqCut(c, st, 732, 730, {"toCut": {"wp": 731}, "clean": true, "tol": undefined});
        stepVol(c, st, 732, "cut");
        atStep(st, 733);
        cqUnion(c, st, 733, 717, {"toUnion": {"wp": 732}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 733, "union");
        atStep(st, 734);
        cqWorkplaneNew(c, st, 734, {"plane": {"origin": [4.860166311652746e-16, 7.937253933193772, 9], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-4.0501385930439555e-17, -0.6614378277661477, -0.75]}, "obj": undefined});
        atStep(st, 735);
        cqMoveTo(c, st, 735, 734, {"x": -0.5, "y": -0.75});
        atStep(st, 736);
        cqLineTo(c, st, 736, 735, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 737);
        cqThreePointArc(c, st, 737, 736, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 738);
        cqLineTo(c, st, 738, 737, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 739);
        cqClose(c, st, 739, 738, {});
        atStep(st, 740);
        cqWorkplaneNew(c, st, 740, {"plane": {"origin": [4.860166311652746e-16, 7.937253933193772, 9], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-4.0501385930439555e-17, -0.6614378277661477, -0.75]}, "obj": undefined});
        atStep(st, 741);
        cqMoveTo(c, st, 741, 740, {"x": -0.7, "y": -0.95});
        atStep(st, 742);
        cqLineTo(c, st, 742, 741, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 743);
        cqThreePointArc(c, st, 743, 742, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 744);
        cqLineTo(c, st, 744, 743, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 745);
        cqClose(c, st, 745, 744, {});
        atStep(st, 746);
        cqExtrude(c, st, 746, 745, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 746, "extrude");
        atStep(st, 747);
        cqExtrude(c, st, 747, 739, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 747, "extrude");
        atStep(st, 748);
        cqCut(c, st, 748, 746, {"toCut": {"wp": 747}, "clean": true, "tol": undefined});
        stepVol(c, st, 748, "cut");
        atStep(st, 749);
        cqUnion(c, st, 749, 733, {"toUnion": {"wp": 748}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 749, "union");
        atStep(st, 750);
        cqWorkplaneNew(c, st, 750, {"plane": {"origin": [-3.968626966596884, 6.873863542433761, 9], "xDir": [-0.8660254037844388, -0.4999999999999998, 2.7755575615628914e-17], "zDir": [0.3307189138830737, -0.5728219618694801, -0.75]}, "obj": undefined});
        atStep(st, 751);
        cqMoveTo(c, st, 751, 750, {"x": -0.5, "y": -0.75});
        atStep(st, 752);
        cqLineTo(c, st, 752, 751, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 753);
        cqThreePointArc(c, st, 753, 752, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 754);
        cqLineTo(c, st, 754, 753, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 755);
        cqClose(c, st, 755, 754, {});
        atStep(st, 756);
        cqWorkplaneNew(c, st, 756, {"plane": {"origin": [-3.968626966596884, 6.873863542433761, 9], "xDir": [-0.8660254037844388, -0.4999999999999998, 2.7755575615628914e-17], "zDir": [0.3307189138830737, -0.5728219618694801, -0.75]}, "obj": undefined});
        atStep(st, 757);
        cqMoveTo(c, st, 757, 756, {"x": -0.7, "y": -0.95});
        atStep(st, 758);
        cqLineTo(c, st, 758, 757, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 759);
        cqThreePointArc(c, st, 759, 758, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 760);
        cqLineTo(c, st, 760, 759, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 761);
        cqClose(c, st, 761, 760, {});
        atStep(st, 762);
        cqExtrude(c, st, 762, 761, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 762, "extrude");
        atStep(st, 763);
        cqExtrude(c, st, 763, 755, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 763, "extrude");
        atStep(st, 764);
        cqCut(c, st, 764, 762, {"toCut": {"wp": 763}, "clean": true, "tol": undefined});
        stepVol(c, st, 764, "cut");
        atStep(st, 765);
        cqUnion(c, st, 765, 749, {"toUnion": {"wp": 764}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 765, "union");
        atStep(st, 766);
        cqWorkplaneNew(c, st, 766, {"plane": {"origin": [-6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [0.5728219618694801, -0.3307189138830738, -0.75]}, "obj": undefined});
        atStep(st, 767);
        cqMoveTo(c, st, 767, 766, {"x": -0.5, "y": -0.75});
        atStep(st, 768);
        cqLineTo(c, st, 768, 767, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 769);
        cqThreePointArc(c, st, 769, 768, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 770);
        cqLineTo(c, st, 770, 769, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 771);
        cqClose(c, st, 771, 770, {});
        atStep(st, 772);
        cqWorkplaneNew(c, st, 772, {"plane": {"origin": [-6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [0.5728219618694801, -0.3307189138830738, -0.75]}, "obj": undefined});
        atStep(st, 773);
        cqMoveTo(c, st, 773, 772, {"x": -0.7, "y": -0.95});
        atStep(st, 774);
        cqLineTo(c, st, 774, 773, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 775);
        cqThreePointArc(c, st, 775, 774, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 776);
        cqLineTo(c, st, 776, 775, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 777);
        cqClose(c, st, 777, 776, {});
        atStep(st, 778);
        cqExtrude(c, st, 778, 777, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 778, "extrude");
        atStep(st, 779);
        cqExtrude(c, st, 779, 771, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 779, "extrude");
        atStep(st, 780);
        cqCut(c, st, 780, 778, {"toCut": {"wp": 779}, "clean": true, "tol": undefined});
        stepVol(c, st, 780, "cut");
        atStep(st, 781);
        cqUnion(c, st, 781, 765, {"toUnion": {"wp": 780}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 781, "union");
        atStep(st, 782);
        cqWorkplaneNew(c, st, 782, {"plane": {"origin": [-7.937253933193772, 9.720332623305493e-16, 9], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [0.6614378277661477, -8.100277186087911e-17, -0.75]}, "obj": undefined});
        atStep(st, 783);
        cqMoveTo(c, st, 783, 782, {"x": -0.5, "y": -0.75});
        atStep(st, 784);
        cqLineTo(c, st, 784, 783, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 785);
        cqThreePointArc(c, st, 785, 784, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 786);
        cqLineTo(c, st, 786, 785, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 787);
        cqClose(c, st, 787, 786, {});
        atStep(st, 788);
        cqWorkplaneNew(c, st, 788, {"plane": {"origin": [-7.937253933193772, 9.720332623305493e-16, 9], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [0.6614378277661477, -8.100277186087911e-17, -0.75]}, "obj": undefined});
        atStep(st, 789);
        cqMoveTo(c, st, 789, 788, {"x": -0.7, "y": -0.95});
        atStep(st, 790);
        cqLineTo(c, st, 790, 789, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 791);
        cqThreePointArc(c, st, 791, 790, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 792);
        cqLineTo(c, st, 792, 791, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 793);
        cqClose(c, st, 793, 792, {});
        atStep(st, 794);
        cqExtrude(c, st, 794, 793, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 794, "extrude");
        atStep(st, 795);
        cqExtrude(c, st, 795, 787, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 795, "extrude");
        atStep(st, 796);
        cqCut(c, st, 796, 794, {"toCut": {"wp": 795}, "clean": true, "tol": undefined});
        stepVol(c, st, 796, "cut");
        atStep(st, 797);
        cqUnion(c, st, 797, 781, {"toUnion": {"wp": 796}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 797, "union");
        atStep(st, 798);
        cqWorkplaneNew(c, st, 798, {"plane": {"origin": [-6.873863542433762, -3.968626966596884, 9], "xDir": [0.4999999999999998, -0.8660254037844389, 0], "zDir": [0.5728219618694802, 0.3307189138830737, -0.75]}, "obj": undefined});
        atStep(st, 799);
        cqMoveTo(c, st, 799, 798, {"x": -0.5, "y": -0.75});
        atStep(st, 800);
        cqLineTo(c, st, 800, 799, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 801);
        cqThreePointArc(c, st, 801, 800, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 802);
        cqLineTo(c, st, 802, 801, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 803);
        cqClose(c, st, 803, 802, {});
        atStep(st, 804);
        cqWorkplaneNew(c, st, 804, {"plane": {"origin": [-6.873863542433762, -3.968626966596884, 9], "xDir": [0.4999999999999998, -0.8660254037844389, 0], "zDir": [0.5728219618694802, 0.3307189138830737, -0.75]}, "obj": undefined});
        atStep(st, 805);
        cqMoveTo(c, st, 805, 804, {"x": -0.7, "y": -0.95});
        atStep(st, 806);
        cqLineTo(c, st, 806, 805, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 807);
        cqThreePointArc(c, st, 807, 806, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 808);
        cqLineTo(c, st, 808, 807, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 809);
        cqClose(c, st, 809, 808, {});
        atStep(st, 810);
        cqExtrude(c, st, 810, 809, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 810, "extrude");
        atStep(st, 811);
        cqExtrude(c, st, 811, 803, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 811, "extrude");
        atStep(st, 812);
        cqCut(c, st, 812, 810, {"toCut": {"wp": 811}, "clean": true, "tol": undefined});
        stepVol(c, st, 812, "cut");
        atStep(st, 813);
        cqUnion(c, st, 813, 797, {"toUnion": {"wp": 812}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 813, "union");
        atStep(st, 814);
        cqWorkplaneNew(c, st, 814, {"plane": {"origin": [-3.9686269665968896, -6.873863542433758, 9], "xDir": [0.8660254037844384, -0.5000000000000004, -2.7755575615628914e-17], "zDir": [0.3307189138830741, 0.5728219618694799, -0.75]}, "obj": undefined});
        atStep(st, 815);
        cqMoveTo(c, st, 815, 814, {"x": -0.5, "y": -0.75});
        atStep(st, 816);
        cqLineTo(c, st, 816, 815, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 817);
        cqThreePointArc(c, st, 817, 816, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 818);
        cqLineTo(c, st, 818, 817, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 819);
        cqClose(c, st, 819, 818, {});
        atStep(st, 820);
        cqWorkplaneNew(c, st, 820, {"plane": {"origin": [-3.9686269665968896, -6.873863542433758, 9], "xDir": [0.8660254037844384, -0.5000000000000004, -2.7755575615628914e-17], "zDir": [0.3307189138830741, 0.5728219618694799, -0.75]}, "obj": undefined});
        atStep(st, 821);
        cqMoveTo(c, st, 821, 820, {"x": -0.7, "y": -0.95});
        atStep(st, 822);
        cqLineTo(c, st, 822, 821, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 823);
        cqThreePointArc(c, st, 823, 822, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 824);
        cqLineTo(c, st, 824, 823, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 825);
        cqClose(c, st, 825, 824, {});
        atStep(st, 826);
        cqExtrude(c, st, 826, 825, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 826, "extrude");
        atStep(st, 827);
        cqExtrude(c, st, 827, 819, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 827, "extrude");
        atStep(st, 828);
        cqCut(c, st, 828, 826, {"toCut": {"wp": 827}, "clean": true, "tol": undefined});
        stepVol(c, st, 828, "cut");
        atStep(st, 829);
        cqUnion(c, st, 829, 813, {"toUnion": {"wp": 828}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 829, "union");
        atStep(st, 830);
        cqWorkplaneNew(c, st, 830, {"plane": {"origin": [-1.4580498934958238e-15, -7.937253933193772, 9], "xDir": [1, -1.83697019872103e-16, -1.232595164407831e-32], "zDir": [1.2150415779131866e-16, 0.6614378277661477, -0.75]}, "obj": undefined});
        atStep(st, 831);
        cqMoveTo(c, st, 831, 830, {"x": -0.5, "y": -0.75});
        atStep(st, 832);
        cqLineTo(c, st, 832, 831, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 833);
        cqThreePointArc(c, st, 833, 832, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 834);
        cqLineTo(c, st, 834, 833, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 835);
        cqClose(c, st, 835, 834, {});
        atStep(st, 836);
        cqWorkplaneNew(c, st, 836, {"plane": {"origin": [-1.4580498934958238e-15, -7.937253933193772, 9], "xDir": [1, -1.83697019872103e-16, -1.232595164407831e-32], "zDir": [1.2150415779131866e-16, 0.6614378277661477, -0.75]}, "obj": undefined});
        atStep(st, 837);
        cqMoveTo(c, st, 837, 836, {"x": -0.7, "y": -0.95});
        atStep(st, 838);
        cqLineTo(c, st, 838, 837, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 839);
        cqThreePointArc(c, st, 839, 838, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 840);
        cqLineTo(c, st, 840, 839, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 841);
        cqClose(c, st, 841, 840, {});
        atStep(st, 842);
        cqExtrude(c, st, 842, 841, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 842, "extrude");
        atStep(st, 843);
        cqExtrude(c, st, 843, 835, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 843, "extrude");
        atStep(st, 844);
        cqCut(c, st, 844, 842, {"toCut": {"wp": 843}, "clean": true, "tol": undefined});
        stepVol(c, st, 844, "cut");
        atStep(st, 845);
        cqUnion(c, st, 845, 829, {"toUnion": {"wp": 844}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 845, "union");
        atStep(st, 846);
        cqWorkplaneNew(c, st, 846, {"plane": {"origin": [3.968626966596887, -6.87386354243376, 9], "xDir": [0.8660254037844386, 0.5000000000000002, 0], "zDir": [-0.3307189138830739, 0.57282196186948, -0.75]}, "obj": undefined});
        atStep(st, 847);
        cqMoveTo(c, st, 847, 846, {"x": -0.5, "y": -0.75});
        atStep(st, 848);
        cqLineTo(c, st, 848, 847, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 849);
        cqThreePointArc(c, st, 849, 848, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 850);
        cqLineTo(c, st, 850, 849, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 851);
        cqClose(c, st, 851, 850, {});
        atStep(st, 852);
        cqWorkplaneNew(c, st, 852, {"plane": {"origin": [3.968626966596887, -6.87386354243376, 9], "xDir": [0.8660254037844386, 0.5000000000000002, 0], "zDir": [-0.3307189138830739, 0.57282196186948, -0.75]}, "obj": undefined});
        atStep(st, 853);
        cqMoveTo(c, st, 853, 852, {"x": -0.7, "y": -0.95});
        atStep(st, 854);
        cqLineTo(c, st, 854, 853, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 855);
        cqThreePointArc(c, st, 855, 854, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 856);
        cqLineTo(c, st, 856, 855, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 857);
        cqClose(c, st, 857, 856, {});
        atStep(st, 858);
        cqExtrude(c, st, 858, 857, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 858, "extrude");
        atStep(st, 859);
        cqExtrude(c, st, 859, 851, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 859, "extrude");
        atStep(st, 860);
        cqCut(c, st, 860, 858, {"toCut": {"wp": 859}, "clean": true, "tol": undefined});
        stepVol(c, st, 860, "cut");
        atStep(st, 861);
        cqUnion(c, st, 861, 845, {"toUnion": {"wp": 860}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 861, "union");
        atStep(st, 862);
        cqWorkplaneNew(c, st, 862, {"plane": {"origin": [6.873863542433758, -3.9686269665968896, 9], "xDir": [0.5000000000000004, 0.8660254037844384, -2.7755575615628914e-17], "zDir": [-0.5728219618694799, 0.3307189138830741, -0.75]}, "obj": undefined});
        atStep(st, 863);
        cqMoveTo(c, st, 863, 862, {"x": -0.5, "y": -0.75});
        atStep(st, 864);
        cqLineTo(c, st, 864, 863, {"x": -0.5, "y": 0.75, "forConstruction": false});
        atStep(st, 865);
        cqThreePointArc(c, st, 865, 864, {"point1": [0, 1.25], "point2": [0.5, 0.75], "forConstruction": false});
        atStep(st, 866);
        cqLineTo(c, st, 866, 865, {"x": 0.5, "y": -0.75, "forConstruction": false});
        atStep(st, 867);
        cqClose(c, st, 867, 866, {});
        atStep(st, 868);
        cqWorkplaneNew(c, st, 868, {"plane": {"origin": [6.873863542433758, -3.9686269665968896, 9], "xDir": [0.5000000000000004, 0.8660254037844384, -2.7755575615628914e-17], "zDir": [-0.5728219618694799, 0.3307189138830741, -0.75]}, "obj": undefined});
        atStep(st, 869);
        cqMoveTo(c, st, 869, 868, {"x": -0.7, "y": -0.95});
        atStep(st, 870);
        cqLineTo(c, st, 870, 869, {"x": -0.7, "y": 0.75, "forConstruction": false});
        atStep(st, 871);
        cqThreePointArc(c, st, 871, 870, {"point1": [0, 1.45], "point2": [0.7, 0.75], "forConstruction": false});
        atStep(st, 872);
        cqLineTo(c, st, 872, 871, {"x": 0.7, "y": -0.95, "forConstruction": false});
        atStep(st, 873);
        cqClose(c, st, 873, 872, {});
        atStep(st, 874);
        cqExtrude(c, st, 874, 873, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 874, "extrude");
        atStep(st, 875);
        cqExtrude(c, st, 875, 867, {"until": -0.1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 875, "extrude");
        atStep(st, 876);
        cqCut(c, st, 876, 874, {"toCut": {"wp": 875}, "clean": true, "tol": undefined});
        stepVol(c, st, 876, "cut");
        atStep(st, 877);
        cqUnion(c, st, 877, 861, {"toUnion": {"wp": 876}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 877, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 877}), {"i": 1024});
    } catch (e) {
        return {"i": 1024, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_1024(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c1024"] = r; }
    return out;
}
