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

const case_1516 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqBox(c, st, 2, 1, {"length": 100, "width": 60, "height": 5, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "box");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqTransformed(c, st, 4, 3, {"rotate": [0, 0, 0], "offset": [-40, -25.980762113533157, -2.5]});
        atStep(st, 5);
        cqRect(c, st, 5, 4, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 6);
        cqExtrude(c, st, 6, 5, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 6, "extrude");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 8);
        cqTransformed(c, st, 8, 7, {"rotate": [0, 0, 59.99999999999999], "offset": [-42.5, -21.65063509461096, -2.5]});
        atStep(st, 9);
        cqRect(c, st, 9, 8, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 10);
        cqExtrude(c, st, 10, 9, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 10, "extrude");
        atStep(st, 11);
        cqWorkplaneNew(c, st, 11, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 12);
        cqTransformed(c, st, 12, 11, {"rotate": [0, 0, 120], "offset": [-47.5, -21.65063509461096, -2.5]});
        atStep(st, 13);
        cqRect(c, st, 13, 12, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 14);
        cqExtrude(c, st, 14, 13, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 14, "extrude");
        atStep(st, 15);
        cqWorkplaneNew(c, st, 15, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 16);
        cqTransformed(c, st, 16, 15, {"rotate": [0, 0, 0], "offset": [-30, -25.980762113533157, -2.5]});
        atStep(st, 17);
        cqRect(c, st, 17, 16, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 18);
        cqExtrude(c, st, 18, 17, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 18, "extrude");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 20);
        cqTransformed(c, st, 20, 19, {"rotate": [0, 0, 59.99999999999999], "offset": [-32.5, -21.65063509461096, -2.5]});
        atStep(st, 21);
        cqRect(c, st, 21, 20, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 22);
        cqExtrude(c, st, 22, 21, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 22, "extrude");
        atStep(st, 23);
        cqWorkplaneNew(c, st, 23, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 24);
        cqTransformed(c, st, 24, 23, {"rotate": [0, 0, 120], "offset": [-37.5, -21.65063509461096, -2.5]});
        atStep(st, 25);
        cqRect(c, st, 25, 24, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 26);
        cqExtrude(c, st, 26, 25, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 26, "extrude");
        atStep(st, 27);
        cqWorkplaneNew(c, st, 27, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 28);
        cqTransformed(c, st, 28, 27, {"rotate": [0, 0, 0], "offset": [-20, -25.980762113533157, -2.5]});
        atStep(st, 29);
        cqRect(c, st, 29, 28, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 30);
        cqExtrude(c, st, 30, 29, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 30, "extrude");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 31, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 32);
        cqTransformed(c, st, 32, 31, {"rotate": [0, 0, 59.99999999999999], "offset": [-22.5, -21.65063509461096, -2.5]});
        atStep(st, 33);
        cqRect(c, st, 33, 32, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 34);
        cqExtrude(c, st, 34, 33, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 34, "extrude");
        atStep(st, 35);
        cqWorkplaneNew(c, st, 35, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 36);
        cqTransformed(c, st, 36, 35, {"rotate": [0, 0, 120], "offset": [-27.5, -21.65063509461096, -2.5]});
        atStep(st, 37);
        cqRect(c, st, 37, 36, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 38);
        cqExtrude(c, st, 38, 37, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 38, "extrude");
        atStep(st, 39);
        cqWorkplaneNew(c, st, 39, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 40);
        cqTransformed(c, st, 40, 39, {"rotate": [0, 0, 0], "offset": [-10, -25.980762113533157, -2.5]});
        atStep(st, 41);
        cqRect(c, st, 41, 40, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 42);
        cqExtrude(c, st, 42, 41, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 42, "extrude");
        atStep(st, 43);
        cqWorkplaneNew(c, st, 43, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 44);
        cqTransformed(c, st, 44, 43, {"rotate": [0, 0, 59.99999999999999], "offset": [-12.5, -21.65063509461096, -2.5]});
        atStep(st, 45);
        cqRect(c, st, 45, 44, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 46);
        cqExtrude(c, st, 46, 45, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 46, "extrude");
        atStep(st, 47);
        cqWorkplaneNew(c, st, 47, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 48);
        cqTransformed(c, st, 48, 47, {"rotate": [0, 0, 120], "offset": [-17.5, -21.65063509461096, -2.5]});
        atStep(st, 49);
        cqRect(c, st, 49, 48, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 50);
        cqExtrude(c, st, 50, 49, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 50, "extrude");
        atStep(st, 51);
        cqWorkplaneNew(c, st, 51, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 52);
        cqTransformed(c, st, 52, 51, {"rotate": [0, 0, 0], "offset": [0, -25.980762113533157, -2.5]});
        atStep(st, 53);
        cqRect(c, st, 53, 52, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 54);
        cqExtrude(c, st, 54, 53, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 54, "extrude");
        atStep(st, 55);
        cqWorkplaneNew(c, st, 55, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 56);
        cqTransformed(c, st, 56, 55, {"rotate": [0, 0, 59.99999999999999], "offset": [-2.5, -21.65063509461096, -2.5]});
        atStep(st, 57);
        cqRect(c, st, 57, 56, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 58);
        cqExtrude(c, st, 58, 57, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 58, "extrude");
        atStep(st, 59);
        cqWorkplaneNew(c, st, 59, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 60);
        cqTransformed(c, st, 60, 59, {"rotate": [0, 0, 120], "offset": [-7.5, -21.65063509461096, -2.5]});
        atStep(st, 61);
        cqRect(c, st, 61, 60, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 62);
        cqExtrude(c, st, 62, 61, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 62, "extrude");
        atStep(st, 63);
        cqWorkplaneNew(c, st, 63, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 64);
        cqTransformed(c, st, 64, 63, {"rotate": [0, 0, 0], "offset": [10, -25.980762113533157, -2.5]});
        atStep(st, 65);
        cqRect(c, st, 65, 64, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 66);
        cqExtrude(c, st, 66, 65, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 66, "extrude");
        atStep(st, 67);
        cqWorkplaneNew(c, st, 67, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 68);
        cqTransformed(c, st, 68, 67, {"rotate": [0, 0, 59.99999999999999], "offset": [7.5, -21.65063509461096, -2.5]});
        atStep(st, 69);
        cqRect(c, st, 69, 68, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 70);
        cqExtrude(c, st, 70, 69, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 70, "extrude");
        atStep(st, 71);
        cqWorkplaneNew(c, st, 71, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 72);
        cqTransformed(c, st, 72, 71, {"rotate": [0, 0, 120], "offset": [2.5, -21.65063509461096, -2.5]});
        atStep(st, 73);
        cqRect(c, st, 73, 72, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 74);
        cqExtrude(c, st, 74, 73, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 74, "extrude");
        atStep(st, 75);
        cqWorkplaneNew(c, st, 75, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 76);
        cqTransformed(c, st, 76, 75, {"rotate": [0, 0, 0], "offset": [20, -25.980762113533157, -2.5]});
        atStep(st, 77);
        cqRect(c, st, 77, 76, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 78);
        cqExtrude(c, st, 78, 77, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 78, "extrude");
        atStep(st, 79);
        cqWorkplaneNew(c, st, 79, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 80);
        cqTransformed(c, st, 80, 79, {"rotate": [0, 0, 59.99999999999999], "offset": [17.5, -21.65063509461096, -2.5]});
        atStep(st, 81);
        cqRect(c, st, 81, 80, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 82);
        cqExtrude(c, st, 82, 81, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 82, "extrude");
        atStep(st, 83);
        cqWorkplaneNew(c, st, 83, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 84);
        cqTransformed(c, st, 84, 83, {"rotate": [0, 0, 120], "offset": [12.5, -21.65063509461096, -2.5]});
        atStep(st, 85);
        cqRect(c, st, 85, 84, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 86);
        cqExtrude(c, st, 86, 85, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 86, "extrude");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 88);
        cqTransformed(c, st, 88, 87, {"rotate": [0, 0, 0], "offset": [30, -25.980762113533157, -2.5]});
        atStep(st, 89);
        cqRect(c, st, 89, 88, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 90);
        cqExtrude(c, st, 90, 89, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 90, "extrude");
        atStep(st, 91);
        cqWorkplaneNew(c, st, 91, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 92);
        cqTransformed(c, st, 92, 91, {"rotate": [0, 0, 59.99999999999999], "offset": [27.5, -21.65063509461096, -2.5]});
        atStep(st, 93);
        cqRect(c, st, 93, 92, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 94);
        cqExtrude(c, st, 94, 93, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 94, "extrude");
        atStep(st, 95);
        cqWorkplaneNew(c, st, 95, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 96);
        cqTransformed(c, st, 96, 95, {"rotate": [0, 0, 120], "offset": [22.5, -21.65063509461096, -2.5]});
        atStep(st, 97);
        cqRect(c, st, 97, 96, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 98);
        cqExtrude(c, st, 98, 97, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 98, "extrude");
        atStep(st, 99);
        cqWorkplaneNew(c, st, 99, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 100);
        cqTransformed(c, st, 100, 99, {"rotate": [0, 0, 0], "offset": [40, -25.980762113533157, -2.5]});
        atStep(st, 101);
        cqRect(c, st, 101, 100, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 102);
        cqExtrude(c, st, 102, 101, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 102, "extrude");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 104);
        cqTransformed(c, st, 104, 103, {"rotate": [0, 0, 59.99999999999999], "offset": [37.5, -21.65063509461096, -2.5]});
        atStep(st, 105);
        cqRect(c, st, 105, 104, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 106);
        cqExtrude(c, st, 106, 105, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 106, "extrude");
        atStep(st, 107);
        cqWorkplaneNew(c, st, 107, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 108);
        cqTransformed(c, st, 108, 107, {"rotate": [0, 0, 120], "offset": [32.5, -21.65063509461096, -2.5]});
        atStep(st, 109);
        cqRect(c, st, 109, 108, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 110);
        cqExtrude(c, st, 110, 109, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 110, "extrude");
        atStep(st, 111);
        cqWorkplaneNew(c, st, 111, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 112);
        cqTransformed(c, st, 112, 111, {"rotate": [0, 0, 59.99999999999999], "offset": [47.5, -21.65063509461096, -2.5]});
        atStep(st, 113);
        cqRect(c, st, 113, 112, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 114);
        cqExtrude(c, st, 114, 113, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 114, "extrude");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 115, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 116);
        cqTransformed(c, st, 116, 115, {"rotate": [0, 0, 120], "offset": [42.5, -21.65063509461096, -2.5]});
        atStep(st, 117);
        cqRect(c, st, 117, 116, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 118);
        cqExtrude(c, st, 118, 117, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 118, "extrude");
        atStep(st, 119);
        cqWorkplaneNew(c, st, 119, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 120);
        cqTransformed(c, st, 120, 119, {"rotate": [0, 0, 0], "offset": [-45, -17.32050807568877, -2.5]});
        atStep(st, 121);
        cqRect(c, st, 121, 120, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 122);
        cqExtrude(c, st, 122, 121, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 122, "extrude");
        atStep(st, 123);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 124);
        cqTransformed(c, st, 124, 123, {"rotate": [0, 0, 59.99999999999999], "offset": [-47.5, -12.990381056766578, -2.5]});
        atStep(st, 125);
        cqRect(c, st, 125, 124, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 126);
        cqExtrude(c, st, 126, 125, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 126, "extrude");
        atStep(st, 127);
        cqWorkplaneNew(c, st, 127, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 128);
        cqTransformed(c, st, 128, 127, {"rotate": [0, 0, 0], "offset": [-35, -17.32050807568877, -2.5]});
        atStep(st, 129);
        cqRect(c, st, 129, 128, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 130);
        cqExtrude(c, st, 130, 129, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 130, "extrude");
        atStep(st, 131);
        cqWorkplaneNew(c, st, 131, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 132);
        cqTransformed(c, st, 132, 131, {"rotate": [0, 0, 59.99999999999999], "offset": [-37.5, -12.990381056766578, -2.5]});
        atStep(st, 133);
        cqRect(c, st, 133, 132, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 134);
        cqExtrude(c, st, 134, 133, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 134, "extrude");
        atStep(st, 135);
        cqWorkplaneNew(c, st, 135, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 136);
        cqTransformed(c, st, 136, 135, {"rotate": [0, 0, 120], "offset": [-42.5, -12.990381056766578, -2.5]});
        atStep(st, 137);
        cqRect(c, st, 137, 136, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 138);
        cqExtrude(c, st, 138, 137, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 138, "extrude");
        atStep(st, 139);
        cqWorkplaneNew(c, st, 139, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 140);
        cqTransformed(c, st, 140, 139, {"rotate": [0, 0, 0], "offset": [-25, -17.32050807568877, -2.5]});
        atStep(st, 141);
        cqRect(c, st, 141, 140, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 142);
        cqExtrude(c, st, 142, 141, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 142, "extrude");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 143, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 144);
        cqTransformed(c, st, 144, 143, {"rotate": [0, 0, 59.99999999999999], "offset": [-27.5, -12.990381056766578, -2.5]});
        atStep(st, 145);
        cqRect(c, st, 145, 144, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 146);
        cqExtrude(c, st, 146, 145, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 146, "extrude");
        atStep(st, 147);
        cqWorkplaneNew(c, st, 147, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 148);
        cqTransformed(c, st, 148, 147, {"rotate": [0, 0, 120], "offset": [-32.5, -12.990381056766578, -2.5]});
        atStep(st, 149);
        cqRect(c, st, 149, 148, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 150);
        cqExtrude(c, st, 150, 149, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 150, "extrude");
        atStep(st, 151);
        cqWorkplaneNew(c, st, 151, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 152);
        cqTransformed(c, st, 152, 151, {"rotate": [0, 0, 0], "offset": [-15, -17.32050807568877, -2.5]});
        atStep(st, 153);
        cqRect(c, st, 153, 152, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 154);
        cqExtrude(c, st, 154, 153, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 154, "extrude");
        atStep(st, 155);
        cqWorkplaneNew(c, st, 155, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 156);
        cqTransformed(c, st, 156, 155, {"rotate": [0, 0, 59.99999999999999], "offset": [-17.5, -12.990381056766578, -2.5]});
        atStep(st, 157);
        cqRect(c, st, 157, 156, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 158);
        cqExtrude(c, st, 158, 157, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 158, "extrude");
        atStep(st, 159);
        cqWorkplaneNew(c, st, 159, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 160);
        cqTransformed(c, st, 160, 159, {"rotate": [0, 0, 120], "offset": [-22.5, -12.990381056766578, -2.5]});
        atStep(st, 161);
        cqRect(c, st, 161, 160, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 162);
        cqExtrude(c, st, 162, 161, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 162, "extrude");
        atStep(st, 163);
        cqWorkplaneNew(c, st, 163, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 164);
        cqTransformed(c, st, 164, 163, {"rotate": [0, 0, 0], "offset": [-5, -17.32050807568877, -2.5]});
        atStep(st, 165);
        cqRect(c, st, 165, 164, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 166);
        cqExtrude(c, st, 166, 165, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 166, "extrude");
        atStep(st, 167);
        cqWorkplaneNew(c, st, 167, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 168);
        cqTransformed(c, st, 168, 167, {"rotate": [0, 0, 59.99999999999999], "offset": [-7.5, -12.990381056766578, -2.5]});
        atStep(st, 169);
        cqRect(c, st, 169, 168, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 170);
        cqExtrude(c, st, 170, 169, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 170, "extrude");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 172);
        cqTransformed(c, st, 172, 171, {"rotate": [0, 0, 120], "offset": [-12.5, -12.990381056766578, -2.5]});
        atStep(st, 173);
        cqRect(c, st, 173, 172, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 174);
        cqExtrude(c, st, 174, 173, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 174, "extrude");
        atStep(st, 175);
        cqWorkplaneNew(c, st, 175, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 176);
        cqTransformed(c, st, 176, 175, {"rotate": [0, 0, 0], "offset": [5, -17.32050807568877, -2.5]});
        atStep(st, 177);
        cqRect(c, st, 177, 176, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 178);
        cqExtrude(c, st, 178, 177, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 178, "extrude");
        atStep(st, 179);
        cqWorkplaneNew(c, st, 179, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 180);
        cqTransformed(c, st, 180, 179, {"rotate": [0, 0, 59.99999999999999], "offset": [2.5, -12.990381056766578, -2.5]});
        atStep(st, 181);
        cqRect(c, st, 181, 180, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 182);
        cqExtrude(c, st, 182, 181, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 182, "extrude");
        atStep(st, 183);
        cqWorkplaneNew(c, st, 183, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 184);
        cqTransformed(c, st, 184, 183, {"rotate": [0, 0, 120], "offset": [-2.5, -12.990381056766578, -2.5]});
        atStep(st, 185);
        cqRect(c, st, 185, 184, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 186);
        cqExtrude(c, st, 186, 185, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 186, "extrude");
        atStep(st, 187);
        cqWorkplaneNew(c, st, 187, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 188);
        cqTransformed(c, st, 188, 187, {"rotate": [0, 0, 0], "offset": [15, -17.32050807568877, -2.5]});
        atStep(st, 189);
        cqRect(c, st, 189, 188, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 190);
        cqExtrude(c, st, 190, 189, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 190, "extrude");
        atStep(st, 191);
        cqWorkplaneNew(c, st, 191, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 192);
        cqTransformed(c, st, 192, 191, {"rotate": [0, 0, 59.99999999999999], "offset": [12.5, -12.990381056766578, -2.5]});
        atStep(st, 193);
        cqRect(c, st, 193, 192, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 194);
        cqExtrude(c, st, 194, 193, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 194, "extrude");
        atStep(st, 195);
        cqWorkplaneNew(c, st, 195, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 196);
        cqTransformed(c, st, 196, 195, {"rotate": [0, 0, 120], "offset": [7.5, -12.990381056766578, -2.5]});
        atStep(st, 197);
        cqRect(c, st, 197, 196, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 198);
        cqExtrude(c, st, 198, 197, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 198, "extrude");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 199, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 200);
        cqTransformed(c, st, 200, 199, {"rotate": [0, 0, 0], "offset": [25, -17.32050807568877, -2.5]});
        atStep(st, 201);
        cqRect(c, st, 201, 200, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 202);
        cqExtrude(c, st, 202, 201, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 202, "extrude");
        atStep(st, 203);
        cqWorkplaneNew(c, st, 203, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 204);
        cqTransformed(c, st, 204, 203, {"rotate": [0, 0, 59.99999999999999], "offset": [22.5, -12.990381056766578, -2.5]});
        atStep(st, 205);
        cqRect(c, st, 205, 204, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 206);
        cqExtrude(c, st, 206, 205, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 206, "extrude");
        atStep(st, 207);
        cqWorkplaneNew(c, st, 207, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 208);
        cqTransformed(c, st, 208, 207, {"rotate": [0, 0, 120], "offset": [17.5, -12.990381056766578, -2.5]});
        atStep(st, 209);
        cqRect(c, st, 209, 208, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 210);
        cqExtrude(c, st, 210, 209, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 210, "extrude");
        atStep(st, 211);
        cqWorkplaneNew(c, st, 211, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 212);
        cqTransformed(c, st, 212, 211, {"rotate": [0, 0, 0], "offset": [35, -17.32050807568877, -2.5]});
        atStep(st, 213);
        cqRect(c, st, 213, 212, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 214);
        cqExtrude(c, st, 214, 213, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 214, "extrude");
        atStep(st, 215);
        cqWorkplaneNew(c, st, 215, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 216);
        cqTransformed(c, st, 216, 215, {"rotate": [0, 0, 59.99999999999999], "offset": [32.5, -12.990381056766578, -2.5]});
        atStep(st, 217);
        cqRect(c, st, 217, 216, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 218);
        cqExtrude(c, st, 218, 217, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 218, "extrude");
        atStep(st, 219);
        cqWorkplaneNew(c, st, 219, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 220);
        cqTransformed(c, st, 220, 219, {"rotate": [0, 0, 120], "offset": [27.5, -12.990381056766578, -2.5]});
        atStep(st, 221);
        cqRect(c, st, 221, 220, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 222);
        cqExtrude(c, st, 222, 221, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 222, "extrude");
        atStep(st, 223);
        cqWorkplaneNew(c, st, 223, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 224);
        cqTransformed(c, st, 224, 223, {"rotate": [0, 0, 0], "offset": [45, -17.32050807568877, -2.5]});
        atStep(st, 225);
        cqRect(c, st, 225, 224, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 226);
        cqExtrude(c, st, 226, 225, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 226, "extrude");
        atStep(st, 227);
        cqWorkplaneNew(c, st, 227, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 228);
        cqTransformed(c, st, 228, 227, {"rotate": [0, 0, 59.99999999999999], "offset": [42.5, -12.990381056766578, -2.5]});
        atStep(st, 229);
        cqRect(c, st, 229, 228, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 230);
        cqExtrude(c, st, 230, 229, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 230, "extrude");
        atStep(st, 231);
        cqWorkplaneNew(c, st, 231, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 232);
        cqTransformed(c, st, 232, 231, {"rotate": [0, 0, 120], "offset": [37.5, -12.990381056766578, -2.5]});
        atStep(st, 233);
        cqRect(c, st, 233, 232, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 234);
        cqExtrude(c, st, 234, 233, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 234, "extrude");
        atStep(st, 235);
        cqWorkplaneNew(c, st, 235, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 236);
        cqTransformed(c, st, 236, 235, {"rotate": [0, 0, 120], "offset": [47.5, -12.990381056766578, -2.5]});
        atStep(st, 237);
        cqRect(c, st, 237, 236, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 238);
        cqExtrude(c, st, 238, 237, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 238, "extrude");
        atStep(st, 239);
        cqWorkplaneNew(c, st, 239, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 240);
        cqTransformed(c, st, 240, 239, {"rotate": [0, 0, 0], "offset": [-40, -8.660254037844386, -2.5]});
        atStep(st, 241);
        cqRect(c, st, 241, 240, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 242);
        cqExtrude(c, st, 242, 241, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 242, "extrude");
        atStep(st, 243);
        cqWorkplaneNew(c, st, 243, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 244);
        cqTransformed(c, st, 244, 243, {"rotate": [0, 0, 59.99999999999999], "offset": [-42.5, -4.330127018922193, -2.5]});
        atStep(st, 245);
        cqRect(c, st, 245, 244, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 246);
        cqExtrude(c, st, 246, 245, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 246, "extrude");
        atStep(st, 247);
        cqWorkplaneNew(c, st, 247, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 248);
        cqTransformed(c, st, 248, 247, {"rotate": [0, 0, 120], "offset": [-47.5, -4.330127018922193, -2.5]});
        atStep(st, 249);
        cqRect(c, st, 249, 248, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 250);
        cqExtrude(c, st, 250, 249, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 250, "extrude");
        atStep(st, 251);
        cqWorkplaneNew(c, st, 251, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 252);
        cqTransformed(c, st, 252, 251, {"rotate": [0, 0, 0], "offset": [-30, -8.660254037844386, -2.5]});
        atStep(st, 253);
        cqRect(c, st, 253, 252, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 254);
        cqExtrude(c, st, 254, 253, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        atStep(st, 255);
        cqWorkplaneNew(c, st, 255, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 256);
        cqTransformed(c, st, 256, 255, {"rotate": [0, 0, 59.99999999999999], "offset": [-32.5, -4.330127018922193, -2.5]});
        atStep(st, 257);
        cqRect(c, st, 257, 256, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 258);
        cqExtrude(c, st, 258, 257, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 258, "extrude");
        atStep(st, 259);
        cqWorkplaneNew(c, st, 259, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 260);
        cqTransformed(c, st, 260, 259, {"rotate": [0, 0, 120], "offset": [-37.5, -4.330127018922193, -2.5]});
        atStep(st, 261);
        cqRect(c, st, 261, 260, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 262);
        cqExtrude(c, st, 262, 261, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 262, "extrude");
        atStep(st, 263);
        cqWorkplaneNew(c, st, 263, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 264);
        cqTransformed(c, st, 264, 263, {"rotate": [0, 0, 0], "offset": [-20, -8.660254037844386, -2.5]});
        atStep(st, 265);
        cqRect(c, st, 265, 264, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 266);
        cqExtrude(c, st, 266, 265, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 266, "extrude");
        atStep(st, 267);
        cqWorkplaneNew(c, st, 267, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 268);
        cqTransformed(c, st, 268, 267, {"rotate": [0, 0, 59.99999999999999], "offset": [-22.5, -4.330127018922193, -2.5]});
        atStep(st, 269);
        cqRect(c, st, 269, 268, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 270);
        cqExtrude(c, st, 270, 269, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 270, "extrude");
        atStep(st, 271);
        cqWorkplaneNew(c, st, 271, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 272);
        cqTransformed(c, st, 272, 271, {"rotate": [0, 0, 120], "offset": [-27.5, -4.330127018922193, -2.5]});
        atStep(st, 273);
        cqRect(c, st, 273, 272, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 274);
        cqExtrude(c, st, 274, 273, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 274, "extrude");
        atStep(st, 275);
        cqWorkplaneNew(c, st, 275, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 276);
        cqTransformed(c, st, 276, 275, {"rotate": [0, 0, 0], "offset": [-10, -8.660254037844386, -2.5]});
        atStep(st, 277);
        cqRect(c, st, 277, 276, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 278);
        cqExtrude(c, st, 278, 277, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 278, "extrude");
        atStep(st, 279);
        cqWorkplaneNew(c, st, 279, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 280);
        cqTransformed(c, st, 280, 279, {"rotate": [0, 0, 59.99999999999999], "offset": [-12.5, -4.330127018922193, -2.5]});
        atStep(st, 281);
        cqRect(c, st, 281, 280, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 282);
        cqExtrude(c, st, 282, 281, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 282, "extrude");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 284);
        cqTransformed(c, st, 284, 283, {"rotate": [0, 0, 120], "offset": [-17.5, -4.330127018922193, -2.5]});
        atStep(st, 285);
        cqRect(c, st, 285, 284, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 286);
        cqExtrude(c, st, 286, 285, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 286, "extrude");
        atStep(st, 287);
        cqWorkplaneNew(c, st, 287, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 288);
        cqTransformed(c, st, 288, 287, {"rotate": [0, 0, 0], "offset": [0, -8.660254037844386, -2.5]});
        atStep(st, 289);
        cqRect(c, st, 289, 288, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 290);
        cqExtrude(c, st, 290, 289, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 290, "extrude");
        atStep(st, 291);
        cqWorkplaneNew(c, st, 291, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 292);
        cqTransformed(c, st, 292, 291, {"rotate": [0, 0, 59.99999999999999], "offset": [-2.5, -4.330127018922193, -2.5]});
        atStep(st, 293);
        cqRect(c, st, 293, 292, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 294);
        cqExtrude(c, st, 294, 293, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 294, "extrude");
        atStep(st, 295);
        cqWorkplaneNew(c, st, 295, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 296);
        cqTransformed(c, st, 296, 295, {"rotate": [0, 0, 120], "offset": [-7.5, -4.330127018922193, -2.5]});
        atStep(st, 297);
        cqRect(c, st, 297, 296, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 298);
        cqExtrude(c, st, 298, 297, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 298, "extrude");
        atStep(st, 299);
        cqWorkplaneNew(c, st, 299, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 300);
        cqTransformed(c, st, 300, 299, {"rotate": [0, 0, 0], "offset": [10, -8.660254037844386, -2.5]});
        atStep(st, 301);
        cqRect(c, st, 301, 300, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 302);
        cqExtrude(c, st, 302, 301, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 302, "extrude");
        atStep(st, 303);
        cqWorkplaneNew(c, st, 303, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 304);
        cqTransformed(c, st, 304, 303, {"rotate": [0, 0, 59.99999999999999], "offset": [7.5, -4.330127018922193, -2.5]});
        atStep(st, 305);
        cqRect(c, st, 305, 304, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 306);
        cqExtrude(c, st, 306, 305, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 306, "extrude");
        atStep(st, 307);
        cqWorkplaneNew(c, st, 307, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 308);
        cqTransformed(c, st, 308, 307, {"rotate": [0, 0, 120], "offset": [2.5, -4.330127018922193, -2.5]});
        atStep(st, 309);
        cqRect(c, st, 309, 308, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 310);
        cqExtrude(c, st, 310, 309, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 310, "extrude");
        atStep(st, 311);
        cqWorkplaneNew(c, st, 311, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 312);
        cqTransformed(c, st, 312, 311, {"rotate": [0, 0, 0], "offset": [20, -8.660254037844386, -2.5]});
        atStep(st, 313);
        cqRect(c, st, 313, 312, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 314);
        cqExtrude(c, st, 314, 313, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 314, "extrude");
        atStep(st, 315);
        cqWorkplaneNew(c, st, 315, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 316);
        cqTransformed(c, st, 316, 315, {"rotate": [0, 0, 59.99999999999999], "offset": [17.5, -4.330127018922193, -2.5]});
        atStep(st, 317);
        cqRect(c, st, 317, 316, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 318);
        cqExtrude(c, st, 318, 317, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 318, "extrude");
        atStep(st, 319);
        cqWorkplaneNew(c, st, 319, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 320);
        cqTransformed(c, st, 320, 319, {"rotate": [0, 0, 120], "offset": [12.5, -4.330127018922193, -2.5]});
        atStep(st, 321);
        cqRect(c, st, 321, 320, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 322);
        cqExtrude(c, st, 322, 321, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 322, "extrude");
        atStep(st, 323);
        cqWorkplaneNew(c, st, 323, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 324);
        cqTransformed(c, st, 324, 323, {"rotate": [0, 0, 0], "offset": [30, -8.660254037844386, -2.5]});
        atStep(st, 325);
        cqRect(c, st, 325, 324, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 326);
        cqExtrude(c, st, 326, 325, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 326, "extrude");
        atStep(st, 327);
        cqWorkplaneNew(c, st, 327, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 328);
        cqTransformed(c, st, 328, 327, {"rotate": [0, 0, 59.99999999999999], "offset": [27.5, -4.330127018922193, -2.5]});
        atStep(st, 329);
        cqRect(c, st, 329, 328, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 330);
        cqExtrude(c, st, 330, 329, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 330, "extrude");
        atStep(st, 331);
        cqWorkplaneNew(c, st, 331, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 332);
        cqTransformed(c, st, 332, 331, {"rotate": [0, 0, 120], "offset": [22.5, -4.330127018922193, -2.5]});
        atStep(st, 333);
        cqRect(c, st, 333, 332, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 334);
        cqExtrude(c, st, 334, 333, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 334, "extrude");
        atStep(st, 335);
        cqWorkplaneNew(c, st, 335, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 336);
        cqTransformed(c, st, 336, 335, {"rotate": [0, 0, 0], "offset": [40, -8.660254037844386, -2.5]});
        atStep(st, 337);
        cqRect(c, st, 337, 336, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 338);
        cqExtrude(c, st, 338, 337, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 338, "extrude");
        atStep(st, 339);
        cqWorkplaneNew(c, st, 339, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 340);
        cqTransformed(c, st, 340, 339, {"rotate": [0, 0, 59.99999999999999], "offset": [37.5, -4.330127018922193, -2.5]});
        atStep(st, 341);
        cqRect(c, st, 341, 340, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 342);
        cqExtrude(c, st, 342, 341, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 342, "extrude");
        atStep(st, 343);
        cqWorkplaneNew(c, st, 343, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 344);
        cqTransformed(c, st, 344, 343, {"rotate": [0, 0, 120], "offset": [32.5, -4.330127018922193, -2.5]});
        atStep(st, 345);
        cqRect(c, st, 345, 344, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 346);
        cqExtrude(c, st, 346, 345, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 346, "extrude");
        atStep(st, 347);
        cqWorkplaneNew(c, st, 347, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 348);
        cqTransformed(c, st, 348, 347, {"rotate": [0, 0, 59.99999999999999], "offset": [47.5, -4.330127018922193, -2.5]});
        atStep(st, 349);
        cqRect(c, st, 349, 348, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 350);
        cqExtrude(c, st, 350, 349, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 350, "extrude");
        atStep(st, 351);
        cqWorkplaneNew(c, st, 351, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 352);
        cqTransformed(c, st, 352, 351, {"rotate": [0, 0, 120], "offset": [42.5, -4.330127018922193, -2.5]});
        atStep(st, 353);
        cqRect(c, st, 353, 352, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 354);
        cqExtrude(c, st, 354, 353, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 354, "extrude");
        atStep(st, 355);
        cqWorkplaneNew(c, st, 355, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 356);
        cqTransformed(c, st, 356, 355, {"rotate": [0, 0, 0], "offset": [-45, 0, -2.5]});
        atStep(st, 357);
        cqRect(c, st, 357, 356, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 358);
        cqExtrude(c, st, 358, 357, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 358, "extrude");
        atStep(st, 359);
        cqWorkplaneNew(c, st, 359, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 360);
        cqTransformed(c, st, 360, 359, {"rotate": [0, 0, 59.99999999999999], "offset": [-47.5, 4.330127018922193, -2.5]});
        atStep(st, 361);
        cqRect(c, st, 361, 360, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 362);
        cqExtrude(c, st, 362, 361, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        atStep(st, 363);
        cqWorkplaneNew(c, st, 363, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 364);
        cqTransformed(c, st, 364, 363, {"rotate": [0, 0, 0], "offset": [-35, 0, -2.5]});
        atStep(st, 365);
        cqRect(c, st, 365, 364, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 366);
        cqExtrude(c, st, 366, 365, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 366, "extrude");
        atStep(st, 367);
        cqWorkplaneNew(c, st, 367, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 368);
        cqTransformed(c, st, 368, 367, {"rotate": [0, 0, 59.99999999999999], "offset": [-37.5, 4.330127018922193, -2.5]});
        atStep(st, 369);
        cqRect(c, st, 369, 368, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 370);
        cqExtrude(c, st, 370, 369, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 370, "extrude");
        atStep(st, 371);
        cqWorkplaneNew(c, st, 371, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 372);
        cqTransformed(c, st, 372, 371, {"rotate": [0, 0, 120], "offset": [-42.5, 4.330127018922193, -2.5]});
        atStep(st, 373);
        cqRect(c, st, 373, 372, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 374);
        cqExtrude(c, st, 374, 373, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 374, "extrude");
        atStep(st, 375);
        cqWorkplaneNew(c, st, 375, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 376);
        cqTransformed(c, st, 376, 375, {"rotate": [0, 0, 0], "offset": [-25, 0, -2.5]});
        atStep(st, 377);
        cqRect(c, st, 377, 376, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 378);
        cqExtrude(c, st, 378, 377, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 378, "extrude");
        atStep(st, 379);
        cqWorkplaneNew(c, st, 379, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 380);
        cqTransformed(c, st, 380, 379, {"rotate": [0, 0, 59.99999999999999], "offset": [-27.5, 4.330127018922193, -2.5]});
        atStep(st, 381);
        cqRect(c, st, 381, 380, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 382);
        cqExtrude(c, st, 382, 381, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 382, "extrude");
        atStep(st, 383);
        cqWorkplaneNew(c, st, 383, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 384);
        cqTransformed(c, st, 384, 383, {"rotate": [0, 0, 120], "offset": [-32.5, 4.330127018922193, -2.5]});
        atStep(st, 385);
        cqRect(c, st, 385, 384, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 386);
        cqExtrude(c, st, 386, 385, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 386, "extrude");
        atStep(st, 387);
        cqWorkplaneNew(c, st, 387, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 388);
        cqTransformed(c, st, 388, 387, {"rotate": [0, 0, 0], "offset": [-15, 0, -2.5]});
        atStep(st, 389);
        cqRect(c, st, 389, 388, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 390);
        cqExtrude(c, st, 390, 389, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 390, "extrude");
        atStep(st, 391);
        cqWorkplaneNew(c, st, 391, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 392);
        cqTransformed(c, st, 392, 391, {"rotate": [0, 0, 59.99999999999999], "offset": [-17.5, 4.330127018922193, -2.5]});
        atStep(st, 393);
        cqRect(c, st, 393, 392, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 394);
        cqExtrude(c, st, 394, 393, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 394, "extrude");
        atStep(st, 395);
        cqWorkplaneNew(c, st, 395, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 396);
        cqTransformed(c, st, 396, 395, {"rotate": [0, 0, 120], "offset": [-22.5, 4.330127018922193, -2.5]});
        atStep(st, 397);
        cqRect(c, st, 397, 396, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 398);
        cqExtrude(c, st, 398, 397, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 398, "extrude");
        atStep(st, 399);
        cqWorkplaneNew(c, st, 399, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 400);
        cqTransformed(c, st, 400, 399, {"rotate": [0, 0, 0], "offset": [-5, 0, -2.5]});
        atStep(st, 401);
        cqRect(c, st, 401, 400, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 402);
        cqExtrude(c, st, 402, 401, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 402, "extrude");
        atStep(st, 403);
        cqWorkplaneNew(c, st, 403, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 404);
        cqTransformed(c, st, 404, 403, {"rotate": [0, 0, 59.99999999999999], "offset": [-7.5, 4.330127018922193, -2.5]});
        atStep(st, 405);
        cqRect(c, st, 405, 404, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 406);
        cqExtrude(c, st, 406, 405, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 406, "extrude");
        atStep(st, 407);
        cqWorkplaneNew(c, st, 407, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 408);
        cqTransformed(c, st, 408, 407, {"rotate": [0, 0, 120], "offset": [-12.5, 4.330127018922193, -2.5]});
        atStep(st, 409);
        cqRect(c, st, 409, 408, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 410);
        cqExtrude(c, st, 410, 409, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 410, "extrude");
        atStep(st, 411);
        cqWorkplaneNew(c, st, 411, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 412);
        cqTransformed(c, st, 412, 411, {"rotate": [0, 0, 0], "offset": [5, 0, -2.5]});
        atStep(st, 413);
        cqRect(c, st, 413, 412, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 414);
        cqExtrude(c, st, 414, 413, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 414, "extrude");
        atStep(st, 415);
        cqWorkplaneNew(c, st, 415, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 416);
        cqTransformed(c, st, 416, 415, {"rotate": [0, 0, 59.99999999999999], "offset": [2.5, 4.330127018922193, -2.5]});
        atStep(st, 417);
        cqRect(c, st, 417, 416, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 418);
        cqExtrude(c, st, 418, 417, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 418, "extrude");
        atStep(st, 419);
        cqWorkplaneNew(c, st, 419, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 420);
        cqTransformed(c, st, 420, 419, {"rotate": [0, 0, 120], "offset": [-2.5, 4.330127018922193, -2.5]});
        atStep(st, 421);
        cqRect(c, st, 421, 420, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 422);
        cqExtrude(c, st, 422, 421, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 422, "extrude");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 424);
        cqTransformed(c, st, 424, 423, {"rotate": [0, 0, 0], "offset": [15, 0, -2.5]});
        atStep(st, 425);
        cqRect(c, st, 425, 424, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 426);
        cqExtrude(c, st, 426, 425, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 427);
        cqWorkplaneNew(c, st, 427, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 428);
        cqTransformed(c, st, 428, 427, {"rotate": [0, 0, 59.99999999999999], "offset": [12.5, 4.330127018922193, -2.5]});
        atStep(st, 429);
        cqRect(c, st, 429, 428, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 430);
        cqExtrude(c, st, 430, 429, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 430, "extrude");
        atStep(st, 431);
        cqWorkplaneNew(c, st, 431, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 432);
        cqTransformed(c, st, 432, 431, {"rotate": [0, 0, 120], "offset": [7.5, 4.330127018922193, -2.5]});
        atStep(st, 433);
        cqRect(c, st, 433, 432, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 434);
        cqExtrude(c, st, 434, 433, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 434, "extrude");
        atStep(st, 435);
        cqWorkplaneNew(c, st, 435, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 436);
        cqTransformed(c, st, 436, 435, {"rotate": [0, 0, 0], "offset": [25, 0, -2.5]});
        atStep(st, 437);
        cqRect(c, st, 437, 436, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 438);
        cqExtrude(c, st, 438, 437, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 438, "extrude");
        atStep(st, 439);
        cqWorkplaneNew(c, st, 439, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 440);
        cqTransformed(c, st, 440, 439, {"rotate": [0, 0, 59.99999999999999], "offset": [22.5, 4.330127018922193, -2.5]});
        atStep(st, 441);
        cqRect(c, st, 441, 440, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 442);
        cqExtrude(c, st, 442, 441, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 442, "extrude");
        atStep(st, 443);
        cqWorkplaneNew(c, st, 443, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 444);
        cqTransformed(c, st, 444, 443, {"rotate": [0, 0, 120], "offset": [17.5, 4.330127018922193, -2.5]});
        atStep(st, 445);
        cqRect(c, st, 445, 444, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 446);
        cqExtrude(c, st, 446, 445, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 446, "extrude");
        atStep(st, 447);
        cqWorkplaneNew(c, st, 447, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 448);
        cqTransformed(c, st, 448, 447, {"rotate": [0, 0, 0], "offset": [35, 0, -2.5]});
        atStep(st, 449);
        cqRect(c, st, 449, 448, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 450);
        cqExtrude(c, st, 450, 449, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 450, "extrude");
        atStep(st, 451);
        cqWorkplaneNew(c, st, 451, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 452);
        cqTransformed(c, st, 452, 451, {"rotate": [0, 0, 59.99999999999999], "offset": [32.5, 4.330127018922193, -2.5]});
        atStep(st, 453);
        cqRect(c, st, 453, 452, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 454);
        cqExtrude(c, st, 454, 453, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 454, "extrude");
        atStep(st, 455);
        cqWorkplaneNew(c, st, 455, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 456);
        cqTransformed(c, st, 456, 455, {"rotate": [0, 0, 120], "offset": [27.5, 4.330127018922193, -2.5]});
        atStep(st, 457);
        cqRect(c, st, 457, 456, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 458);
        cqExtrude(c, st, 458, 457, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 458, "extrude");
        atStep(st, 459);
        cqWorkplaneNew(c, st, 459, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 460);
        cqTransformed(c, st, 460, 459, {"rotate": [0, 0, 0], "offset": [45, 0, -2.5]});
        atStep(st, 461);
        cqRect(c, st, 461, 460, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 462);
        cqExtrude(c, st, 462, 461, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 462, "extrude");
        atStep(st, 463);
        cqWorkplaneNew(c, st, 463, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 464);
        cqTransformed(c, st, 464, 463, {"rotate": [0, 0, 59.99999999999999], "offset": [42.5, 4.330127018922193, -2.5]});
        atStep(st, 465);
        cqRect(c, st, 465, 464, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 466);
        cqExtrude(c, st, 466, 465, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 466, "extrude");
        atStep(st, 467);
        cqWorkplaneNew(c, st, 467, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 468);
        cqTransformed(c, st, 468, 467, {"rotate": [0, 0, 120], "offset": [37.5, 4.330127018922193, -2.5]});
        atStep(st, 469);
        cqRect(c, st, 469, 468, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 470);
        cqExtrude(c, st, 470, 469, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 470, "extrude");
        atStep(st, 471);
        cqWorkplaneNew(c, st, 471, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 472);
        cqTransformed(c, st, 472, 471, {"rotate": [0, 0, 120], "offset": [47.5, 4.330127018922193, -2.5]});
        atStep(st, 473);
        cqRect(c, st, 473, 472, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 474);
        cqExtrude(c, st, 474, 473, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 474, "extrude");
        atStep(st, 475);
        cqWorkplaneNew(c, st, 475, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 476);
        cqTransformed(c, st, 476, 475, {"rotate": [0, 0, 0], "offset": [-40, 8.660254037844386, -2.5]});
        atStep(st, 477);
        cqRect(c, st, 477, 476, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 478);
        cqExtrude(c, st, 478, 477, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 478, "extrude");
        atStep(st, 479);
        cqWorkplaneNew(c, st, 479, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 480);
        cqTransformed(c, st, 480, 479, {"rotate": [0, 0, 59.99999999999999], "offset": [-42.5, 12.990381056766578, -2.5]});
        atStep(st, 481);
        cqRect(c, st, 481, 480, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 482);
        cqExtrude(c, st, 482, 481, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 482, "extrude");
        atStep(st, 483);
        cqWorkplaneNew(c, st, 483, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 484);
        cqTransformed(c, st, 484, 483, {"rotate": [0, 0, 120], "offset": [-47.5, 12.990381056766578, -2.5]});
        atStep(st, 485);
        cqRect(c, st, 485, 484, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 486);
        cqExtrude(c, st, 486, 485, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 486, "extrude");
        atStep(st, 487);
        cqWorkplaneNew(c, st, 487, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 488);
        cqTransformed(c, st, 488, 487, {"rotate": [0, 0, 0], "offset": [-30, 8.660254037844386, -2.5]});
        atStep(st, 489);
        cqRect(c, st, 489, 488, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 490);
        cqExtrude(c, st, 490, 489, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 490, "extrude");
        atStep(st, 491);
        cqWorkplaneNew(c, st, 491, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 492);
        cqTransformed(c, st, 492, 491, {"rotate": [0, 0, 59.99999999999999], "offset": [-32.5, 12.990381056766578, -2.5]});
        atStep(st, 493);
        cqRect(c, st, 493, 492, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 494);
        cqExtrude(c, st, 494, 493, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 494, "extrude");
        atStep(st, 495);
        cqWorkplaneNew(c, st, 495, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 496);
        cqTransformed(c, st, 496, 495, {"rotate": [0, 0, 120], "offset": [-37.5, 12.990381056766578, -2.5]});
        atStep(st, 497);
        cqRect(c, st, 497, 496, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 498);
        cqExtrude(c, st, 498, 497, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 498, "extrude");
        atStep(st, 499);
        cqWorkplaneNew(c, st, 499, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 500);
        cqTransformed(c, st, 500, 499, {"rotate": [0, 0, 0], "offset": [-20, 8.660254037844386, -2.5]});
        atStep(st, 501);
        cqRect(c, st, 501, 500, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 502);
        cqExtrude(c, st, 502, 501, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 502, "extrude");
        atStep(st, 503);
        cqWorkplaneNew(c, st, 503, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 504);
        cqTransformed(c, st, 504, 503, {"rotate": [0, 0, 59.99999999999999], "offset": [-22.5, 12.990381056766578, -2.5]});
        atStep(st, 505);
        cqRect(c, st, 505, 504, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 506);
        cqExtrude(c, st, 506, 505, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 506, "extrude");
        atStep(st, 507);
        cqWorkplaneNew(c, st, 507, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 508);
        cqTransformed(c, st, 508, 507, {"rotate": [0, 0, 120], "offset": [-27.5, 12.990381056766578, -2.5]});
        atStep(st, 509);
        cqRect(c, st, 509, 508, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 510);
        cqExtrude(c, st, 510, 509, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 510, "extrude");
        atStep(st, 511);
        cqWorkplaneNew(c, st, 511, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 512);
        cqTransformed(c, st, 512, 511, {"rotate": [0, 0, 0], "offset": [-10, 8.660254037844386, -2.5]});
        atStep(st, 513);
        cqRect(c, st, 513, 512, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 514);
        cqExtrude(c, st, 514, 513, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 514, "extrude");
        atStep(st, 515);
        cqWorkplaneNew(c, st, 515, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 516);
        cqTransformed(c, st, 516, 515, {"rotate": [0, 0, 59.99999999999999], "offset": [-12.5, 12.990381056766578, -2.5]});
        atStep(st, 517);
        cqRect(c, st, 517, 516, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 518);
        cqExtrude(c, st, 518, 517, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 518, "extrude");
        atStep(st, 519);
        cqWorkplaneNew(c, st, 519, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 520);
        cqTransformed(c, st, 520, 519, {"rotate": [0, 0, 120], "offset": [-17.5, 12.990381056766578, -2.5]});
        atStep(st, 521);
        cqRect(c, st, 521, 520, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 522);
        cqExtrude(c, st, 522, 521, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 522, "extrude");
        atStep(st, 523);
        cqWorkplaneNew(c, st, 523, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 524);
        cqTransformed(c, st, 524, 523, {"rotate": [0, 0, 0], "offset": [0, 8.660254037844386, -2.5]});
        atStep(st, 525);
        cqRect(c, st, 525, 524, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 526);
        cqExtrude(c, st, 526, 525, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 526, "extrude");
        atStep(st, 527);
        cqWorkplaneNew(c, st, 527, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 528);
        cqTransformed(c, st, 528, 527, {"rotate": [0, 0, 59.99999999999999], "offset": [-2.5, 12.990381056766578, -2.5]});
        atStep(st, 529);
        cqRect(c, st, 529, 528, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 530);
        cqExtrude(c, st, 530, 529, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 530, "extrude");
        atStep(st, 531);
        cqWorkplaneNew(c, st, 531, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 532);
        cqTransformed(c, st, 532, 531, {"rotate": [0, 0, 120], "offset": [-7.5, 12.990381056766578, -2.5]});
        atStep(st, 533);
        cqRect(c, st, 533, 532, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 534);
        cqExtrude(c, st, 534, 533, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 534, "extrude");
        atStep(st, 535);
        cqWorkplaneNew(c, st, 535, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 536);
        cqTransformed(c, st, 536, 535, {"rotate": [0, 0, 0], "offset": [10, 8.660254037844386, -2.5]});
        atStep(st, 537);
        cqRect(c, st, 537, 536, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 538);
        cqExtrude(c, st, 538, 537, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 538, "extrude");
        atStep(st, 539);
        cqWorkplaneNew(c, st, 539, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 540);
        cqTransformed(c, st, 540, 539, {"rotate": [0, 0, 59.99999999999999], "offset": [7.5, 12.990381056766578, -2.5]});
        atStep(st, 541);
        cqRect(c, st, 541, 540, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 542);
        cqExtrude(c, st, 542, 541, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 542, "extrude");
        atStep(st, 543);
        cqWorkplaneNew(c, st, 543, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 544);
        cqTransformed(c, st, 544, 543, {"rotate": [0, 0, 120], "offset": [2.5, 12.990381056766578, -2.5]});
        atStep(st, 545);
        cqRect(c, st, 545, 544, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 546);
        cqExtrude(c, st, 546, 545, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 546, "extrude");
        atStep(st, 547);
        cqWorkplaneNew(c, st, 547, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 548);
        cqTransformed(c, st, 548, 547, {"rotate": [0, 0, 0], "offset": [20, 8.660254037844386, -2.5]});
        atStep(st, 549);
        cqRect(c, st, 549, 548, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 550);
        cqExtrude(c, st, 550, 549, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 550, "extrude");
        atStep(st, 551);
        cqWorkplaneNew(c, st, 551, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 552);
        cqTransformed(c, st, 552, 551, {"rotate": [0, 0, 59.99999999999999], "offset": [17.5, 12.990381056766578, -2.5]});
        atStep(st, 553);
        cqRect(c, st, 553, 552, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 554);
        cqExtrude(c, st, 554, 553, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 554, "extrude");
        atStep(st, 555);
        cqWorkplaneNew(c, st, 555, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 556);
        cqTransformed(c, st, 556, 555, {"rotate": [0, 0, 120], "offset": [12.5, 12.990381056766578, -2.5]});
        atStep(st, 557);
        cqRect(c, st, 557, 556, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 558);
        cqExtrude(c, st, 558, 557, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 558, "extrude");
        atStep(st, 559);
        cqWorkplaneNew(c, st, 559, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 560);
        cqTransformed(c, st, 560, 559, {"rotate": [0, 0, 0], "offset": [30, 8.660254037844386, -2.5]});
        atStep(st, 561);
        cqRect(c, st, 561, 560, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 562);
        cqExtrude(c, st, 562, 561, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 562, "extrude");
        atStep(st, 563);
        cqWorkplaneNew(c, st, 563, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 564);
        cqTransformed(c, st, 564, 563, {"rotate": [0, 0, 59.99999999999999], "offset": [27.5, 12.990381056766578, -2.5]});
        atStep(st, 565);
        cqRect(c, st, 565, 564, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 566);
        cqExtrude(c, st, 566, 565, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 566, "extrude");
        atStep(st, 567);
        cqWorkplaneNew(c, st, 567, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 568);
        cqTransformed(c, st, 568, 567, {"rotate": [0, 0, 120], "offset": [22.5, 12.990381056766578, -2.5]});
        atStep(st, 569);
        cqRect(c, st, 569, 568, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 570);
        cqExtrude(c, st, 570, 569, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 570, "extrude");
        atStep(st, 571);
        cqWorkplaneNew(c, st, 571, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 572);
        cqTransformed(c, st, 572, 571, {"rotate": [0, 0, 0], "offset": [40, 8.660254037844386, -2.5]});
        atStep(st, 573);
        cqRect(c, st, 573, 572, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 574);
        cqExtrude(c, st, 574, 573, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 574, "extrude");
        atStep(st, 575);
        cqWorkplaneNew(c, st, 575, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 576);
        cqTransformed(c, st, 576, 575, {"rotate": [0, 0, 59.99999999999999], "offset": [37.5, 12.990381056766578, -2.5]});
        atStep(st, 577);
        cqRect(c, st, 577, 576, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 578);
        cqExtrude(c, st, 578, 577, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 578, "extrude");
        atStep(st, 579);
        cqWorkplaneNew(c, st, 579, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 580);
        cqTransformed(c, st, 580, 579, {"rotate": [0, 0, 120], "offset": [32.5, 12.990381056766578, -2.5]});
        atStep(st, 581);
        cqRect(c, st, 581, 580, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 582);
        cqExtrude(c, st, 582, 581, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 582, "extrude");
        atStep(st, 583);
        cqWorkplaneNew(c, st, 583, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 584);
        cqTransformed(c, st, 584, 583, {"rotate": [0, 0, 59.99999999999999], "offset": [47.5, 12.990381056766578, -2.5]});
        atStep(st, 585);
        cqRect(c, st, 585, 584, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 586);
        cqExtrude(c, st, 586, 585, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 586, "extrude");
        atStep(st, 587);
        cqWorkplaneNew(c, st, 587, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 588);
        cqTransformed(c, st, 588, 587, {"rotate": [0, 0, 120], "offset": [42.5, 12.990381056766578, -2.5]});
        atStep(st, 589);
        cqRect(c, st, 589, 588, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 590);
        cqExtrude(c, st, 590, 589, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 590, "extrude");
        atStep(st, 591);
        cqWorkplaneNew(c, st, 591, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 592);
        cqTransformed(c, st, 592, 591, {"rotate": [0, 0, 0], "offset": [-45, 17.32050807568877, -2.5]});
        atStep(st, 593);
        cqRect(c, st, 593, 592, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 594);
        cqExtrude(c, st, 594, 593, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 594, "extrude");
        atStep(st, 595);
        cqWorkplaneNew(c, st, 595, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 596);
        cqTransformed(c, st, 596, 595, {"rotate": [0, 0, 59.99999999999999], "offset": [-47.5, 21.65063509461096, -2.5]});
        atStep(st, 597);
        cqRect(c, st, 597, 596, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 598);
        cqExtrude(c, st, 598, 597, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 598, "extrude");
        atStep(st, 599);
        cqWorkplaneNew(c, st, 599, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 600);
        cqTransformed(c, st, 600, 599, {"rotate": [0, 0, 0], "offset": [-35, 17.32050807568877, -2.5]});
        atStep(st, 601);
        cqRect(c, st, 601, 600, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 602);
        cqExtrude(c, st, 602, 601, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 602, "extrude");
        atStep(st, 603);
        cqWorkplaneNew(c, st, 603, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 604);
        cqTransformed(c, st, 604, 603, {"rotate": [0, 0, 59.99999999999999], "offset": [-37.5, 21.65063509461096, -2.5]});
        atStep(st, 605);
        cqRect(c, st, 605, 604, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 606);
        cqExtrude(c, st, 606, 605, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 606, "extrude");
        atStep(st, 607);
        cqWorkplaneNew(c, st, 607, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 608);
        cqTransformed(c, st, 608, 607, {"rotate": [0, 0, 120], "offset": [-42.5, 21.65063509461096, -2.5]});
        atStep(st, 609);
        cqRect(c, st, 609, 608, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 610);
        cqExtrude(c, st, 610, 609, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 610, "extrude");
        atStep(st, 611);
        cqWorkplaneNew(c, st, 611, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 612);
        cqTransformed(c, st, 612, 611, {"rotate": [0, 0, 0], "offset": [-25, 17.32050807568877, -2.5]});
        atStep(st, 613);
        cqRect(c, st, 613, 612, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 614);
        cqExtrude(c, st, 614, 613, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 614, "extrude");
        atStep(st, 615);
        cqWorkplaneNew(c, st, 615, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 616);
        cqTransformed(c, st, 616, 615, {"rotate": [0, 0, 59.99999999999999], "offset": [-27.5, 21.65063509461096, -2.5]});
        atStep(st, 617);
        cqRect(c, st, 617, 616, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 618);
        cqExtrude(c, st, 618, 617, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 618, "extrude");
        atStep(st, 619);
        cqWorkplaneNew(c, st, 619, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 620);
        cqTransformed(c, st, 620, 619, {"rotate": [0, 0, 120], "offset": [-32.5, 21.65063509461096, -2.5]});
        atStep(st, 621);
        cqRect(c, st, 621, 620, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 622);
        cqExtrude(c, st, 622, 621, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 622, "extrude");
        atStep(st, 623);
        cqWorkplaneNew(c, st, 623, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 624);
        cqTransformed(c, st, 624, 623, {"rotate": [0, 0, 0], "offset": [-15, 17.32050807568877, -2.5]});
        atStep(st, 625);
        cqRect(c, st, 625, 624, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 626);
        cqExtrude(c, st, 626, 625, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 626, "extrude");
        atStep(st, 627);
        cqWorkplaneNew(c, st, 627, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 628);
        cqTransformed(c, st, 628, 627, {"rotate": [0, 0, 59.99999999999999], "offset": [-17.5, 21.65063509461096, -2.5]});
        atStep(st, 629);
        cqRect(c, st, 629, 628, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 630);
        cqExtrude(c, st, 630, 629, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 630, "extrude");
        atStep(st, 631);
        cqWorkplaneNew(c, st, 631, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 632);
        cqTransformed(c, st, 632, 631, {"rotate": [0, 0, 120], "offset": [-22.5, 21.65063509461096, -2.5]});
        atStep(st, 633);
        cqRect(c, st, 633, 632, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 634);
        cqExtrude(c, st, 634, 633, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 634, "extrude");
        atStep(st, 635);
        cqWorkplaneNew(c, st, 635, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 636);
        cqTransformed(c, st, 636, 635, {"rotate": [0, 0, 0], "offset": [-5, 17.32050807568877, -2.5]});
        atStep(st, 637);
        cqRect(c, st, 637, 636, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 638);
        cqExtrude(c, st, 638, 637, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 638, "extrude");
        atStep(st, 639);
        cqWorkplaneNew(c, st, 639, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 640);
        cqTransformed(c, st, 640, 639, {"rotate": [0, 0, 59.99999999999999], "offset": [-7.5, 21.65063509461096, -2.5]});
        atStep(st, 641);
        cqRect(c, st, 641, 640, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 642);
        cqExtrude(c, st, 642, 641, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 642, "extrude");
        atStep(st, 643);
        cqWorkplaneNew(c, st, 643, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 644);
        cqTransformed(c, st, 644, 643, {"rotate": [0, 0, 120], "offset": [-12.5, 21.65063509461096, -2.5]});
        atStep(st, 645);
        cqRect(c, st, 645, 644, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 646);
        cqExtrude(c, st, 646, 645, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 646, "extrude");
        atStep(st, 647);
        cqWorkplaneNew(c, st, 647, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 648);
        cqTransformed(c, st, 648, 647, {"rotate": [0, 0, 0], "offset": [5, 17.32050807568877, -2.5]});
        atStep(st, 649);
        cqRect(c, st, 649, 648, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 650);
        cqExtrude(c, st, 650, 649, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 650, "extrude");
        atStep(st, 651);
        cqWorkplaneNew(c, st, 651, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 652);
        cqTransformed(c, st, 652, 651, {"rotate": [0, 0, 59.99999999999999], "offset": [2.5, 21.65063509461096, -2.5]});
        atStep(st, 653);
        cqRect(c, st, 653, 652, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 654);
        cqExtrude(c, st, 654, 653, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 654, "extrude");
        atStep(st, 655);
        cqWorkplaneNew(c, st, 655, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 656);
        cqTransformed(c, st, 656, 655, {"rotate": [0, 0, 120], "offset": [-2.5, 21.65063509461096, -2.5]});
        atStep(st, 657);
        cqRect(c, st, 657, 656, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 658);
        cqExtrude(c, st, 658, 657, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 658, "extrude");
        atStep(st, 659);
        cqWorkplaneNew(c, st, 659, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 660);
        cqTransformed(c, st, 660, 659, {"rotate": [0, 0, 0], "offset": [15, 17.32050807568877, -2.5]});
        atStep(st, 661);
        cqRect(c, st, 661, 660, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 662);
        cqExtrude(c, st, 662, 661, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 662, "extrude");
        atStep(st, 663);
        cqWorkplaneNew(c, st, 663, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 664);
        cqTransformed(c, st, 664, 663, {"rotate": [0, 0, 59.99999999999999], "offset": [12.5, 21.65063509461096, -2.5]});
        atStep(st, 665);
        cqRect(c, st, 665, 664, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 666);
        cqExtrude(c, st, 666, 665, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 666, "extrude");
        atStep(st, 667);
        cqWorkplaneNew(c, st, 667, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 668);
        cqTransformed(c, st, 668, 667, {"rotate": [0, 0, 120], "offset": [7.5, 21.65063509461096, -2.5]});
        atStep(st, 669);
        cqRect(c, st, 669, 668, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 670);
        cqExtrude(c, st, 670, 669, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 670, "extrude");
        atStep(st, 671);
        cqWorkplaneNew(c, st, 671, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 672);
        cqTransformed(c, st, 672, 671, {"rotate": [0, 0, 0], "offset": [25, 17.32050807568877, -2.5]});
        atStep(st, 673);
        cqRect(c, st, 673, 672, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 674);
        cqExtrude(c, st, 674, 673, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 674, "extrude");
        atStep(st, 675);
        cqWorkplaneNew(c, st, 675, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 676);
        cqTransformed(c, st, 676, 675, {"rotate": [0, 0, 59.99999999999999], "offset": [22.5, 21.65063509461096, -2.5]});
        atStep(st, 677);
        cqRect(c, st, 677, 676, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 678);
        cqExtrude(c, st, 678, 677, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 678, "extrude");
        atStep(st, 679);
        cqWorkplaneNew(c, st, 679, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 680);
        cqTransformed(c, st, 680, 679, {"rotate": [0, 0, 120], "offset": [17.5, 21.65063509461096, -2.5]});
        atStep(st, 681);
        cqRect(c, st, 681, 680, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 682);
        cqExtrude(c, st, 682, 681, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 682, "extrude");
        atStep(st, 683);
        cqWorkplaneNew(c, st, 683, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 684);
        cqTransformed(c, st, 684, 683, {"rotate": [0, 0, 0], "offset": [35, 17.32050807568877, -2.5]});
        atStep(st, 685);
        cqRect(c, st, 685, 684, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 686);
        cqExtrude(c, st, 686, 685, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 686, "extrude");
        atStep(st, 687);
        cqWorkplaneNew(c, st, 687, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 688);
        cqTransformed(c, st, 688, 687, {"rotate": [0, 0, 59.99999999999999], "offset": [32.5, 21.65063509461096, -2.5]});
        atStep(st, 689);
        cqRect(c, st, 689, 688, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 690);
        cqExtrude(c, st, 690, 689, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 690, "extrude");
        atStep(st, 691);
        cqWorkplaneNew(c, st, 691, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 692);
        cqTransformed(c, st, 692, 691, {"rotate": [0, 0, 120], "offset": [27.5, 21.65063509461096, -2.5]});
        atStep(st, 693);
        cqRect(c, st, 693, 692, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 694);
        cqExtrude(c, st, 694, 693, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 694, "extrude");
        atStep(st, 695);
        cqWorkplaneNew(c, st, 695, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 696);
        cqTransformed(c, st, 696, 695, {"rotate": [0, 0, 0], "offset": [45, 17.32050807568877, -2.5]});
        atStep(st, 697);
        cqRect(c, st, 697, 696, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 698);
        cqExtrude(c, st, 698, 697, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 698, "extrude");
        atStep(st, 699);
        cqWorkplaneNew(c, st, 699, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 700);
        cqTransformed(c, st, 700, 699, {"rotate": [0, 0, 59.99999999999999], "offset": [42.5, 21.65063509461096, -2.5]});
        atStep(st, 701);
        cqRect(c, st, 701, 700, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 702);
        cqExtrude(c, st, 702, 701, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 702, "extrude");
        atStep(st, 703);
        cqWorkplaneNew(c, st, 703, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 704);
        cqTransformed(c, st, 704, 703, {"rotate": [0, 0, 120], "offset": [37.5, 21.65063509461096, -2.5]});
        atStep(st, 705);
        cqRect(c, st, 705, 704, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 706);
        cqExtrude(c, st, 706, 705, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 706, "extrude");
        atStep(st, 707);
        cqWorkplaneNew(c, st, 707, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 708);
        cqTransformed(c, st, 708, 707, {"rotate": [0, 0, 120], "offset": [47.5, 21.65063509461096, -2.5]});
        atStep(st, 709);
        cqRect(c, st, 709, 708, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 710);
        cqExtrude(c, st, 710, 709, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 710, "extrude");
        atStep(st, 711);
        cqWorkplaneNew(c, st, 711, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 712);
        cqTransformed(c, st, 712, 711, {"rotate": [0, 0, 0], "offset": [-40, 25.980762113533157, -2.5]});
        atStep(st, 713);
        cqRect(c, st, 713, 712, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 714);
        cqExtrude(c, st, 714, 713, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 714, "extrude");
        atStep(st, 715);
        cqWorkplaneNew(c, st, 715, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 716);
        cqTransformed(c, st, 716, 715, {"rotate": [0, 0, 0], "offset": [-30, 25.980762113533157, -2.5]});
        atStep(st, 717);
        cqRect(c, st, 717, 716, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 718);
        cqExtrude(c, st, 718, 717, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 718, "extrude");
        atStep(st, 719);
        cqWorkplaneNew(c, st, 719, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 720);
        cqTransformed(c, st, 720, 719, {"rotate": [0, 0, 0], "offset": [-20, 25.980762113533157, -2.5]});
        atStep(st, 721);
        cqRect(c, st, 721, 720, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 722);
        cqExtrude(c, st, 722, 721, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 722, "extrude");
        atStep(st, 723);
        cqWorkplaneNew(c, st, 723, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 724);
        cqTransformed(c, st, 724, 723, {"rotate": [0, 0, 0], "offset": [-10, 25.980762113533157, -2.5]});
        atStep(st, 725);
        cqRect(c, st, 725, 724, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 726);
        cqExtrude(c, st, 726, 725, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 726, "extrude");
        atStep(st, 727);
        cqWorkplaneNew(c, st, 727, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 728);
        cqTransformed(c, st, 728, 727, {"rotate": [0, 0, 0], "offset": [0, 25.980762113533157, -2.5]});
        atStep(st, 729);
        cqRect(c, st, 729, 728, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 730);
        cqExtrude(c, st, 730, 729, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 730, "extrude");
        atStep(st, 731);
        cqWorkplaneNew(c, st, 731, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 732);
        cqTransformed(c, st, 732, 731, {"rotate": [0, 0, 0], "offset": [10, 25.980762113533157, -2.5]});
        atStep(st, 733);
        cqRect(c, st, 733, 732, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 734);
        cqExtrude(c, st, 734, 733, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 734, "extrude");
        atStep(st, 735);
        cqWorkplaneNew(c, st, 735, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 736);
        cqTransformed(c, st, 736, 735, {"rotate": [0, 0, 0], "offset": [20, 25.980762113533157, -2.5]});
        atStep(st, 737);
        cqRect(c, st, 737, 736, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 738);
        cqExtrude(c, st, 738, 737, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 738, "extrude");
        atStep(st, 739);
        cqWorkplaneNew(c, st, 739, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 740);
        cqTransformed(c, st, 740, 739, {"rotate": [0, 0, 0], "offset": [30, 25.980762113533157, -2.5]});
        atStep(st, 741);
        cqRect(c, st, 741, 740, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 742);
        cqExtrude(c, st, 742, 741, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 742, "extrude");
        atStep(st, 743);
        cqWorkplaneNew(c, st, 743, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 744);
        cqTransformed(c, st, 744, 743, {"rotate": [0, 0, 0], "offset": [40, 25.980762113533157, -2.5]});
        atStep(st, 745);
        cqRect(c, st, 745, 744, {"xLen": 10, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 746);
        cqExtrude(c, st, 746, 745, {"until": 5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 746, "extrude");
        atStep(st, 747);
        cqUnion(c, st, 747, 2, {"toUnion": {"wp": 6}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 747, "union");
        atStep(st, 748);
        cqUnion(c, st, 748, 747, {"toUnion": {"wp": 10}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 748, "union");
        atStep(st, 749);
        cqUnion(c, st, 749, 748, {"toUnion": {"wp": 14}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 749, "union");
        atStep(st, 750);
        cqUnion(c, st, 750, 749, {"toUnion": {"wp": 18}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 750, "union");
        atStep(st, 751);
        cqUnion(c, st, 751, 750, {"toUnion": {"wp": 22}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 751, "union");
        atStep(st, 752);
        cqUnion(c, st, 752, 751, {"toUnion": {"wp": 26}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 752, "union");
        atStep(st, 753);
        cqUnion(c, st, 753, 752, {"toUnion": {"wp": 30}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 753, "union");
        atStep(st, 754);
        cqUnion(c, st, 754, 753, {"toUnion": {"wp": 34}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 754, "union");
        atStep(st, 755);
        cqUnion(c, st, 755, 754, {"toUnion": {"wp": 38}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 755, "union");
        atStep(st, 756);
        cqUnion(c, st, 756, 755, {"toUnion": {"wp": 42}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 756, "union");
        atStep(st, 757);
        cqUnion(c, st, 757, 756, {"toUnion": {"wp": 46}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 757, "union");
        atStep(st, 758);
        cqUnion(c, st, 758, 757, {"toUnion": {"wp": 50}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 758, "union");
        atStep(st, 759);
        cqUnion(c, st, 759, 758, {"toUnion": {"wp": 54}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 759, "union");
        atStep(st, 760);
        cqUnion(c, st, 760, 759, {"toUnion": {"wp": 58}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 760, "union");
        atStep(st, 761);
        cqUnion(c, st, 761, 760, {"toUnion": {"wp": 62}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 761, "union");
        atStep(st, 762);
        cqUnion(c, st, 762, 761, {"toUnion": {"wp": 66}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 762, "union");
        atStep(st, 763);
        cqUnion(c, st, 763, 762, {"toUnion": {"wp": 70}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 763, "union");
        atStep(st, 764);
        cqUnion(c, st, 764, 763, {"toUnion": {"wp": 74}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 764, "union");
        atStep(st, 765);
        cqUnion(c, st, 765, 764, {"toUnion": {"wp": 78}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 765, "union");
        atStep(st, 766);
        cqUnion(c, st, 766, 765, {"toUnion": {"wp": 82}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 766, "union");
        atStep(st, 767);
        cqUnion(c, st, 767, 766, {"toUnion": {"wp": 86}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 767, "union");
        atStep(st, 768);
        cqUnion(c, st, 768, 767, {"toUnion": {"wp": 90}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 768, "union");
        atStep(st, 769);
        cqUnion(c, st, 769, 768, {"toUnion": {"wp": 94}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 769, "union");
        atStep(st, 770);
        cqUnion(c, st, 770, 769, {"toUnion": {"wp": 98}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 770, "union");
        atStep(st, 771);
        cqUnion(c, st, 771, 770, {"toUnion": {"wp": 102}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 771, "union");
        atStep(st, 772);
        cqUnion(c, st, 772, 771, {"toUnion": {"wp": 106}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 772, "union");
        atStep(st, 773);
        cqUnion(c, st, 773, 772, {"toUnion": {"wp": 110}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 773, "union");
        atStep(st, 774);
        cqUnion(c, st, 774, 773, {"toUnion": {"wp": 114}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 774, "union");
        atStep(st, 775);
        cqUnion(c, st, 775, 774, {"toUnion": {"wp": 118}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 775, "union");
        atStep(st, 776);
        cqUnion(c, st, 776, 775, {"toUnion": {"wp": 122}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 776, "union");
        atStep(st, 777);
        cqUnion(c, st, 777, 776, {"toUnion": {"wp": 126}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 777, "union");
        atStep(st, 778);
        cqUnion(c, st, 778, 777, {"toUnion": {"wp": 130}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 778, "union");
        atStep(st, 779);
        cqUnion(c, st, 779, 778, {"toUnion": {"wp": 134}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 779, "union");
        atStep(st, 780);
        cqUnion(c, st, 780, 779, {"toUnion": {"wp": 138}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 780, "union");
        atStep(st, 781);
        cqUnion(c, st, 781, 780, {"toUnion": {"wp": 142}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 781, "union");
        atStep(st, 782);
        cqUnion(c, st, 782, 781, {"toUnion": {"wp": 146}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 782, "union");
        atStep(st, 783);
        cqUnion(c, st, 783, 782, {"toUnion": {"wp": 150}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 783, "union");
        atStep(st, 784);
        cqUnion(c, st, 784, 783, {"toUnion": {"wp": 154}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 784, "union");
        atStep(st, 785);
        cqUnion(c, st, 785, 784, {"toUnion": {"wp": 158}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 785, "union");
        atStep(st, 786);
        cqUnion(c, st, 786, 785, {"toUnion": {"wp": 162}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 786, "union");
        atStep(st, 787);
        cqUnion(c, st, 787, 786, {"toUnion": {"wp": 166}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 787, "union");
        atStep(st, 788);
        cqUnion(c, st, 788, 787, {"toUnion": {"wp": 170}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 788, "union");
        atStep(st, 789);
        cqUnion(c, st, 789, 788, {"toUnion": {"wp": 174}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 789, "union");
        atStep(st, 790);
        cqUnion(c, st, 790, 789, {"toUnion": {"wp": 178}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 790, "union");
        atStep(st, 791);
        cqUnion(c, st, 791, 790, {"toUnion": {"wp": 182}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 791, "union");
        atStep(st, 792);
        cqUnion(c, st, 792, 791, {"toUnion": {"wp": 186}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 792, "union");
        atStep(st, 793);
        cqUnion(c, st, 793, 792, {"toUnion": {"wp": 190}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 793, "union");
        atStep(st, 794);
        cqUnion(c, st, 794, 793, {"toUnion": {"wp": 194}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 794, "union");
        atStep(st, 795);
        cqUnion(c, st, 795, 794, {"toUnion": {"wp": 198}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 795, "union");
        atStep(st, 796);
        cqUnion(c, st, 796, 795, {"toUnion": {"wp": 202}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 796, "union");
        atStep(st, 797);
        cqUnion(c, st, 797, 796, {"toUnion": {"wp": 206}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 797, "union");
        atStep(st, 798);
        cqUnion(c, st, 798, 797, {"toUnion": {"wp": 210}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 798, "union");
        atStep(st, 799);
        cqUnion(c, st, 799, 798, {"toUnion": {"wp": 214}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 799, "union");
        atStep(st, 800);
        cqUnion(c, st, 800, 799, {"toUnion": {"wp": 218}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 800, "union");
        atStep(st, 801);
        cqUnion(c, st, 801, 800, {"toUnion": {"wp": 222}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 801, "union");
        atStep(st, 802);
        cqUnion(c, st, 802, 801, {"toUnion": {"wp": 226}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 802, "union");
        atStep(st, 803);
        cqUnion(c, st, 803, 802, {"toUnion": {"wp": 230}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 803, "union");
        atStep(st, 804);
        cqUnion(c, st, 804, 803, {"toUnion": {"wp": 234}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 804, "union");
        atStep(st, 805);
        cqUnion(c, st, 805, 804, {"toUnion": {"wp": 238}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 805, "union");
        atStep(st, 806);
        cqUnion(c, st, 806, 805, {"toUnion": {"wp": 242}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 806, "union");
        atStep(st, 807);
        cqUnion(c, st, 807, 806, {"toUnion": {"wp": 246}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 807, "union");
        atStep(st, 808);
        cqUnion(c, st, 808, 807, {"toUnion": {"wp": 250}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 808, "union");
        atStep(st, 809);
        cqUnion(c, st, 809, 808, {"toUnion": {"wp": 254}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 809, "union");
        atStep(st, 810);
        cqUnion(c, st, 810, 809, {"toUnion": {"wp": 258}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 810, "union");
        atStep(st, 811);
        cqUnion(c, st, 811, 810, {"toUnion": {"wp": 262}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 811, "union");
        atStep(st, 812);
        cqUnion(c, st, 812, 811, {"toUnion": {"wp": 266}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 812, "union");
        atStep(st, 813);
        cqUnion(c, st, 813, 812, {"toUnion": {"wp": 270}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 813, "union");
        atStep(st, 814);
        cqUnion(c, st, 814, 813, {"toUnion": {"wp": 274}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 814, "union");
        atStep(st, 815);
        cqUnion(c, st, 815, 814, {"toUnion": {"wp": 278}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 815, "union");
        atStep(st, 816);
        cqUnion(c, st, 816, 815, {"toUnion": {"wp": 282}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 816, "union");
        atStep(st, 817);
        cqUnion(c, st, 817, 816, {"toUnion": {"wp": 286}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 817, "union");
        atStep(st, 818);
        cqUnion(c, st, 818, 817, {"toUnion": {"wp": 290}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 818, "union");
        atStep(st, 819);
        cqUnion(c, st, 819, 818, {"toUnion": {"wp": 294}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 819, "union");
        atStep(st, 820);
        cqUnion(c, st, 820, 819, {"toUnion": {"wp": 298}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 820, "union");
        atStep(st, 821);
        cqUnion(c, st, 821, 820, {"toUnion": {"wp": 302}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 821, "union");
        atStep(st, 822);
        cqUnion(c, st, 822, 821, {"toUnion": {"wp": 306}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 822, "union");
        atStep(st, 823);
        cqUnion(c, st, 823, 822, {"toUnion": {"wp": 310}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 823, "union");
        atStep(st, 824);
        cqUnion(c, st, 824, 823, {"toUnion": {"wp": 314}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 824, "union");
        atStep(st, 825);
        cqUnion(c, st, 825, 824, {"toUnion": {"wp": 318}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 825, "union");
        atStep(st, 826);
        cqUnion(c, st, 826, 825, {"toUnion": {"wp": 322}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 826, "union");
        atStep(st, 827);
        cqUnion(c, st, 827, 826, {"toUnion": {"wp": 326}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 827, "union");
        atStep(st, 828);
        cqUnion(c, st, 828, 827, {"toUnion": {"wp": 330}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 828, "union");
        atStep(st, 829);
        cqUnion(c, st, 829, 828, {"toUnion": {"wp": 334}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 829, "union");
        atStep(st, 830);
        cqUnion(c, st, 830, 829, {"toUnion": {"wp": 338}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 830, "union");
        atStep(st, 831);
        cqUnion(c, st, 831, 830, {"toUnion": {"wp": 342}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 831, "union");
        atStep(st, 832);
        cqUnion(c, st, 832, 831, {"toUnion": {"wp": 346}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 832, "union");
        atStep(st, 833);
        cqUnion(c, st, 833, 832, {"toUnion": {"wp": 350}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 833, "union");
        atStep(st, 834);
        cqUnion(c, st, 834, 833, {"toUnion": {"wp": 354}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 834, "union");
        atStep(st, 835);
        cqUnion(c, st, 835, 834, {"toUnion": {"wp": 358}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 835, "union");
        atStep(st, 836);
        cqUnion(c, st, 836, 835, {"toUnion": {"wp": 362}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 836, "union");
        atStep(st, 837);
        cqUnion(c, st, 837, 836, {"toUnion": {"wp": 366}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 837, "union");
        atStep(st, 838);
        cqUnion(c, st, 838, 837, {"toUnion": {"wp": 370}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 838, "union");
        atStep(st, 839);
        cqUnion(c, st, 839, 838, {"toUnion": {"wp": 374}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 839, "union");
        atStep(st, 840);
        cqUnion(c, st, 840, 839, {"toUnion": {"wp": 378}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 840, "union");
        atStep(st, 841);
        cqUnion(c, st, 841, 840, {"toUnion": {"wp": 382}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 841, "union");
        atStep(st, 842);
        cqUnion(c, st, 842, 841, {"toUnion": {"wp": 386}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 842, "union");
        atStep(st, 843);
        cqUnion(c, st, 843, 842, {"toUnion": {"wp": 390}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 843, "union");
        atStep(st, 844);
        cqUnion(c, st, 844, 843, {"toUnion": {"wp": 394}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 844, "union");
        atStep(st, 845);
        cqUnion(c, st, 845, 844, {"toUnion": {"wp": 398}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 845, "union");
        atStep(st, 846);
        cqUnion(c, st, 846, 845, {"toUnion": {"wp": 402}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 846, "union");
        atStep(st, 847);
        cqUnion(c, st, 847, 846, {"toUnion": {"wp": 406}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 847, "union");
        atStep(st, 848);
        cqUnion(c, st, 848, 847, {"toUnion": {"wp": 410}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 848, "union");
        atStep(st, 849);
        cqUnion(c, st, 849, 848, {"toUnion": {"wp": 414}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 849, "union");
        atStep(st, 850);
        cqUnion(c, st, 850, 849, {"toUnion": {"wp": 418}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 850, "union");
        atStep(st, 851);
        cqUnion(c, st, 851, 850, {"toUnion": {"wp": 422}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 851, "union");
        atStep(st, 852);
        cqUnion(c, st, 852, 851, {"toUnion": {"wp": 426}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 852, "union");
        atStep(st, 853);
        cqUnion(c, st, 853, 852, {"toUnion": {"wp": 430}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 853, "union");
        atStep(st, 854);
        cqUnion(c, st, 854, 853, {"toUnion": {"wp": 434}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 854, "union");
        atStep(st, 855);
        cqUnion(c, st, 855, 854, {"toUnion": {"wp": 438}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 855, "union");
        atStep(st, 856);
        cqUnion(c, st, 856, 855, {"toUnion": {"wp": 442}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 856, "union");
        atStep(st, 857);
        cqUnion(c, st, 857, 856, {"toUnion": {"wp": 446}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 857, "union");
        atStep(st, 858);
        cqUnion(c, st, 858, 857, {"toUnion": {"wp": 450}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 858, "union");
        atStep(st, 859);
        cqUnion(c, st, 859, 858, {"toUnion": {"wp": 454}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 859, "union");
        atStep(st, 860);
        cqUnion(c, st, 860, 859, {"toUnion": {"wp": 458}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 860, "union");
        atStep(st, 861);
        cqUnion(c, st, 861, 860, {"toUnion": {"wp": 462}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 861, "union");
        atStep(st, 862);
        cqUnion(c, st, 862, 861, {"toUnion": {"wp": 466}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 862, "union");
        atStep(st, 863);
        cqUnion(c, st, 863, 862, {"toUnion": {"wp": 470}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 863, "union");
        atStep(st, 864);
        cqUnion(c, st, 864, 863, {"toUnion": {"wp": 474}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 864, "union");
        atStep(st, 865);
        cqUnion(c, st, 865, 864, {"toUnion": {"wp": 478}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 865, "union");
        atStep(st, 866);
        cqUnion(c, st, 866, 865, {"toUnion": {"wp": 482}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 866, "union");
        atStep(st, 867);
        cqUnion(c, st, 867, 866, {"toUnion": {"wp": 486}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 867, "union");
        atStep(st, 868);
        cqUnion(c, st, 868, 867, {"toUnion": {"wp": 490}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 868, "union");
        atStep(st, 869);
        cqUnion(c, st, 869, 868, {"toUnion": {"wp": 494}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 869, "union");
        atStep(st, 870);
        cqUnion(c, st, 870, 869, {"toUnion": {"wp": 498}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 870, "union");
        atStep(st, 871);
        cqUnion(c, st, 871, 870, {"toUnion": {"wp": 502}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 871, "union");
        atStep(st, 872);
        cqUnion(c, st, 872, 871, {"toUnion": {"wp": 506}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 872, "union");
        atStep(st, 873);
        cqUnion(c, st, 873, 872, {"toUnion": {"wp": 510}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 873, "union");
        atStep(st, 874);
        cqUnion(c, st, 874, 873, {"toUnion": {"wp": 514}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 874, "union");
        atStep(st, 875);
        cqUnion(c, st, 875, 874, {"toUnion": {"wp": 518}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 875, "union");
        atStep(st, 876);
        cqUnion(c, st, 876, 875, {"toUnion": {"wp": 522}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 876, "union");
        atStep(st, 877);
        cqUnion(c, st, 877, 876, {"toUnion": {"wp": 526}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 877, "union");
        atStep(st, 878);
        cqUnion(c, st, 878, 877, {"toUnion": {"wp": 530}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 878, "union");
        atStep(st, 879);
        cqUnion(c, st, 879, 878, {"toUnion": {"wp": 534}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 879, "union");
        atStep(st, 880);
        cqUnion(c, st, 880, 879, {"toUnion": {"wp": 538}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 880, "union");
        atStep(st, 881);
        cqUnion(c, st, 881, 880, {"toUnion": {"wp": 542}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 881, "union");
        atStep(st, 882);
        cqUnion(c, st, 882, 881, {"toUnion": {"wp": 546}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 882, "union");
        atStep(st, 883);
        cqUnion(c, st, 883, 882, {"toUnion": {"wp": 550}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 883, "union");
        atStep(st, 884);
        cqUnion(c, st, 884, 883, {"toUnion": {"wp": 554}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 884, "union");
        atStep(st, 885);
        cqUnion(c, st, 885, 884, {"toUnion": {"wp": 558}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 885, "union");
        atStep(st, 886);
        cqUnion(c, st, 886, 885, {"toUnion": {"wp": 562}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 886, "union");
        atStep(st, 887);
        cqUnion(c, st, 887, 886, {"toUnion": {"wp": 566}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 887, "union");
        atStep(st, 888);
        cqUnion(c, st, 888, 887, {"toUnion": {"wp": 570}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 888, "union");
        atStep(st, 889);
        cqUnion(c, st, 889, 888, {"toUnion": {"wp": 574}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 889, "union");
        atStep(st, 890);
        cqUnion(c, st, 890, 889, {"toUnion": {"wp": 578}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 890, "union");
        atStep(st, 891);
        cqUnion(c, st, 891, 890, {"toUnion": {"wp": 582}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 891, "union");
        atStep(st, 892);
        cqUnion(c, st, 892, 891, {"toUnion": {"wp": 586}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 892, "union");
        atStep(st, 893);
        cqUnion(c, st, 893, 892, {"toUnion": {"wp": 590}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 893, "union");
        atStep(st, 894);
        cqUnion(c, st, 894, 893, {"toUnion": {"wp": 594}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 894, "union");
        atStep(st, 895);
        cqUnion(c, st, 895, 894, {"toUnion": {"wp": 598}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 895, "union");
        atStep(st, 896);
        cqUnion(c, st, 896, 895, {"toUnion": {"wp": 602}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 896, "union");
        atStep(st, 897);
        cqUnion(c, st, 897, 896, {"toUnion": {"wp": 606}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 897, "union");
        atStep(st, 898);
        cqUnion(c, st, 898, 897, {"toUnion": {"wp": 610}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 898, "union");
        atStep(st, 899);
        cqUnion(c, st, 899, 898, {"toUnion": {"wp": 614}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 899, "union");
        atStep(st, 900);
        cqUnion(c, st, 900, 899, {"toUnion": {"wp": 618}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 900, "union");
        atStep(st, 901);
        cqUnion(c, st, 901, 900, {"toUnion": {"wp": 622}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 901, "union");
        atStep(st, 902);
        cqUnion(c, st, 902, 901, {"toUnion": {"wp": 626}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 902, "union");
        atStep(st, 903);
        cqUnion(c, st, 903, 902, {"toUnion": {"wp": 630}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 903, "union");
        atStep(st, 904);
        cqUnion(c, st, 904, 903, {"toUnion": {"wp": 634}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 904, "union");
        atStep(st, 905);
        cqUnion(c, st, 905, 904, {"toUnion": {"wp": 638}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 905, "union");
        atStep(st, 906);
        cqUnion(c, st, 906, 905, {"toUnion": {"wp": 642}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 906, "union");
        atStep(st, 907);
        cqUnion(c, st, 907, 906, {"toUnion": {"wp": 646}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 907, "union");
        atStep(st, 908);
        cqUnion(c, st, 908, 907, {"toUnion": {"wp": 650}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 908, "union");
        atStep(st, 909);
        cqUnion(c, st, 909, 908, {"toUnion": {"wp": 654}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 909, "union");
        atStep(st, 910);
        cqUnion(c, st, 910, 909, {"toUnion": {"wp": 658}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 910, "union");
        atStep(st, 911);
        cqUnion(c, st, 911, 910, {"toUnion": {"wp": 662}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 911, "union");
        atStep(st, 912);
        cqUnion(c, st, 912, 911, {"toUnion": {"wp": 666}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 912, "union");
        atStep(st, 913);
        cqUnion(c, st, 913, 912, {"toUnion": {"wp": 670}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 913, "union");
        atStep(st, 914);
        cqUnion(c, st, 914, 913, {"toUnion": {"wp": 674}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 914, "union");
        atStep(st, 915);
        cqUnion(c, st, 915, 914, {"toUnion": {"wp": 678}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 915, "union");
        atStep(st, 916);
        cqUnion(c, st, 916, 915, {"toUnion": {"wp": 682}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 916, "union");
        atStep(st, 917);
        cqUnion(c, st, 917, 916, {"toUnion": {"wp": 686}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 917, "union");
        atStep(st, 918);
        cqUnion(c, st, 918, 917, {"toUnion": {"wp": 690}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 918, "union");
        atStep(st, 919);
        cqUnion(c, st, 919, 918, {"toUnion": {"wp": 694}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 919, "union");
        atStep(st, 920);
        cqUnion(c, st, 920, 919, {"toUnion": {"wp": 698}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 920, "union");
        atStep(st, 921);
        cqUnion(c, st, 921, 920, {"toUnion": {"wp": 702}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 921, "union");
        atStep(st, 922);
        cqUnion(c, st, 922, 921, {"toUnion": {"wp": 706}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 922, "union");
        atStep(st, 923);
        cqUnion(c, st, 923, 922, {"toUnion": {"wp": 710}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 923, "union");
        atStep(st, 924);
        cqUnion(c, st, 924, 923, {"toUnion": {"wp": 714}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 924, "union");
        atStep(st, 925);
        cqUnion(c, st, 925, 924, {"toUnion": {"wp": 718}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 925, "union");
        atStep(st, 926);
        cqUnion(c, st, 926, 925, {"toUnion": {"wp": 722}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 926, "union");
        atStep(st, 927);
        cqUnion(c, st, 927, 926, {"toUnion": {"wp": 726}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 927, "union");
        atStep(st, 928);
        cqUnion(c, st, 928, 927, {"toUnion": {"wp": 730}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 928, "union");
        atStep(st, 929);
        cqUnion(c, st, 929, 928, {"toUnion": {"wp": 734}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 929, "union");
        atStep(st, 930);
        cqUnion(c, st, 930, 929, {"toUnion": {"wp": 738}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 930, "union");
        atStep(st, 931);
        cqUnion(c, st, 931, 930, {"toUnion": {"wp": 742}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 931, "union");
        atStep(st, 932);
        cqUnion(c, st, 932, 931, {"toUnion": {"wp": 746}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 932, "union");
        atStep(st, 933);
        cqSelect("Faces")(c, st, 933, 932, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 934);
        cqWorkplane(c, st, 934, 933, {"offset": 0, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 935);
        cqPushPoints(c, st, 935, 934, {"pntList": [[-20, -22.5], [20, -22.5], [-40, -7.5], [0, -7.5], [40, -7.5], [-20, 7.5], [20, 7.5], [-40, 22.5], [0, 22.5], [40, 22.5]]});
        atStep(st, 936);
        cqCircle(c, st, 936, 935, {"radius": 2.5, "forConstruction": false});
        atStep(st, 937);
        cqCutThruAll(c, st, 937, 936, {"clean": true, "taper": 0});
        stepVol(c, st, 937, "cutThruAll");
        atStep(st, 938);
        cqSelect("Faces")(c, st, 938, 937, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 939);
        cqWorkplane(c, st, 939, 938, {"offset": 0, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 940);
        cqPushPoints(c, st, 940, 939, {"pntList": [[-40, -22.5], [0, -22.5], [40, -22.5], [-20, -7.5], [20, -7.5], [-40, 7.5], [0, 7.5], [40, 7.5], [-20, 22.5], [20, 22.5]]});
        atStep(st, 941);
        cqRect(c, st, 941, 940, {"xLen": 8, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 942);
        cqCutThruAll(c, st, 942, 941, {"clean": true, "taper": 0});
        stepVol(c, st, 942, "cutThruAll");
        return mergeMaps(cqMeasure(c, st, {"wp": 942}), {"i": 1516});
    } catch (e) {
        return {"i": 1516, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_1516(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c1516"] = r; }
    return out;
}
