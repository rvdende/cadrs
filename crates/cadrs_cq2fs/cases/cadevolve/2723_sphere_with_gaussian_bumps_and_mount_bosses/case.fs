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

const case_2723 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqSphere(c, st, 2, 1, {"radius": 10, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "sphere");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [-0.45448334391080025, 8.103577089132969, -1.3059146161891286], "xDir": [-1.7347234759768069e-18, 0.15910016577182776, 0.98726244598454], "zDir": [-0.0552852278365468, 0.98575252903364, -0.15885683833831]}, "obj": undefined});
        atStep(st, 4);
        cqSphere(c, st, 4, 3, {"radius": 2.139640897414381, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 4, "sphere");
        atStep(st, 5);
        cqCut(c, st, 5, 2, {"toCut": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 5, "cut");
        atStep(st, 6);
        cqUnion(c, st, 6, 5, {"toUnion": {"wp": 4}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 6, "union");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [-2.5989397554919766, 7.466250941093557, 5.450252514954969], "xDir": [0, -0.5896036929829948, 0.8076926923160903], "zDir": [-0.2706573442500742, 0.7775461693372206, 0.5675971780695452]}, "obj": undefined});
        atStep(st, 8);
        cqSphere(c, st, 8, 7, {"radius": 0.8904630998215006, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 8, "sphere");
        atStep(st, 9);
        cqCut(c, st, 9, 6, {"toCut": {"wp": 8}, "clean": true, "tol": undefined});
        stepVol(c, st, 9, "cut");
        atStep(st, 10);
        cqUnion(c, st, 10, 9, {"toUnion": {"wp": 8}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 10, "union");
        atStep(st, 11);
        cqWorkplaneNew(c, st, 11, {"plane": {"origin": [-5.621855419360134, -0.16560242829449817, 7.94604630523092], "xDir": [0.8163438911951341, 0, 0.5775661445309855], "zDir": [-0.5774825736506752, -0.017010845950426963, 0.8162257703906703]}, "obj": undefined});
        atStep(st, 12);
        cqSphere(c, st, 12, 11, {"radius": 0.7842804334170421, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 12, "sphere");
        atStep(st, 13);
        cqCut(c, st, 13, 10, {"toCut": {"wp": 12}, "clean": true, "tol": undefined});
        stepVol(c, st, 13, "cut");
        atStep(st, 14);
        cqUnion(c, st, 14, 13, {"toUnion": {"wp": 12}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 14, "union");
        atStep(st, 15);
        cqWorkplaneNew(c, st, 15, {"plane": {"origin": [-0.029911491559483534, 9.401850270701212, 2.2909088857607114], "xDir": [1.0842021724855044e-19, -0.23673912428531033, 0.9715732535597223], "zDir": [-0.00309099437953687, 0.9715686122235768, 0.23673799335066328]}, "obj": undefined});
        atStep(st, 16);
        cqSphere(c, st, 16, 15, {"radius": 0.987586927402742, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 16, "sphere");
        atStep(st, 17);
        cqCut(c, st, 17, 14, {"toCut": {"wp": 16}, "clean": true, "tol": undefined});
        stepVol(c, st, 17, "cut");
        atStep(st, 18);
        cqUnion(c, st, 18, 17, {"toUnion": {"wp": 16}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 18, "union");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [4.493017636123383, -3.1718549704128565, 4.350951182765586], "xDir": [-0.6956583142662714, 2.7755575615628907e-17, 0.7183728208891325], "zDir": [0.6406927836299838, -0.4522983782493674, 0.6204344719931791]}, "obj": undefined});
        atStep(st, 20);
        cqSphere(c, st, 20, 19, {"radius": 3.334142805618942, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 20, "sphere");
        atStep(st, 21);
        cqCut(c, st, 21, 18, {"toCut": {"wp": 20}, "clean": true, "tol": undefined});
        stepVol(c, st, 21, "cut");
        atStep(st, 22);
        cqUnion(c, st, 22, 21, {"toUnion": {"wp": 20}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 22, "union");
        atStep(st, 23);
        cqWorkplaneNew(c, st, 23, {"plane": {"origin": [-2.364564444376565, -5.36986797770335, 7.76060072561432], "xDir": [0, 0.8223337769595547, 0.5690054123393148], "zDir": [-0.24304274066631248, -0.5519441152983219, 0.797676575935987]}, "obj": undefined});
        atStep(st, 24);
        cqSphere(c, st, 24, 23, {"radius": 0.9213614209681662, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 24, "sphere");
        atStep(st, 25);
        cqCut(c, st, 25, 22, {"toCut": {"wp": 24}, "clean": true, "tol": undefined});
        stepVol(c, st, 25, "cut");
        atStep(st, 26);
        cqUnion(c, st, 26, 25, {"toUnion": {"wp": 24}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 26, "union");
        atStep(st, 27);
        cqWorkplaneNew(c, st, 27, {"plane": {"origin": [-7.177215791919713, -6.005195094726803, -1.2428771554356632], "xDir": [0.6417081533827539, -0.7669489199953905, 0], "zDir": [-0.7602729522670834, -0.6361223538447603, -0.13165632909243263]}, "obj": undefined});
        atStep(st, 28);
        cqSphere(c, st, 28, 27, {"radius": 1.0677087823082034, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 28, "sphere");
        atStep(st, 29);
        cqCut(c, st, 29, 26, {"toCut": {"wp": 28}, "clean": true, "tol": undefined});
        stepVol(c, st, 29, "cut");
        atStep(st, 30);
        cqUnion(c, st, 30, 29, {"toUnion": {"wp": 28}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 30, "union");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 31, {"plane": {"origin": [4.843555904707477, -5.472416143478907, -0.3363824831235166], "xDir": [-0.7488218533642896, -0.6627713270231823, 3.469446951953613e-18], "zDir": [0.6620703402545689, -0.7480298543899003, -0.045980446894565936]}, "obj": undefined});
        atStep(st, 32);
        cqSphere(c, st, 32, 31, {"radius": 3.0487034769041776, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 32, "sphere");
        atStep(st, 33);
        cqCut(c, st, 33, 30, {"toCut": {"wp": 32}, "clean": true, "tol": undefined});
        stepVol(c, st, 33, "cut");
        atStep(st, 34);
        cqUnion(c, st, 34, 33, {"toUnion": {"wp": 32}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 34, "union");
        atStep(st, 35);
        cqWorkplaneNew(c, st, 35, {"plane": {"origin": [9.556837471172573, 0.8453618692209173, 0.9389213271191263], "xDir": [-0.09777528445368024, 0, 0.9952085177237996], "zDir": [0.991374502364304, 0.08769325678548116, 0.09739860767117858]}, "obj": undefined});
        atStep(st, 36);
        cqSphere(c, st, 36, 35, {"radius": 1.0404458355832895, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 36, "sphere");
        atStep(st, 37);
        cqCut(c, st, 37, 34, {"toCut": {"wp": 36}, "clean": true, "tol": undefined});
        stepVol(c, st, 37, "cut");
        atStep(st, 38);
        cqUnion(c, st, 38, 37, {"toUnion": {"wp": 36}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 38, "union");
        atStep(st, 39);
        cqWorkplaneNew(c, st, 39, {"plane": {"origin": [-3.3829043130499237, -5.987831855993792, 5.877611393899858], "xDir": [2.7755575615628907e-17, -0.7005081031744388, -0.7136444474575205], "zDir": [-0.3739340356553299, -0.661873326449083, 0.649689954296466]}, "obj": undefined});
        atStep(st, 40);
        cqSphere(c, st, 40, 39, {"radius": 1.3541450475310064, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 40, "sphere");
        atStep(st, 41);
        cqCut(c, st, 41, 38, {"toCut": {"wp": 40}, "clean": true, "tol": undefined});
        stepVol(c, st, 41, "cut");
        atStep(st, 42);
        cqUnion(c, st, 42, 41, {"toUnion": {"wp": 40}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 42, "union");
        atStep(st, 43);
        cqWorkplaneNew(c, st, 43, {"plane": {"origin": [0.22639625747049277, 5.914562793657963, 6.419769727042246], "xDir": [0, 0.7354527888301013, -0.677575970207051], "zDir": [0.025927376259856843, 0.6773481888661816, 0.7352055509855618]}, "obj": undefined});
        atStep(st, 44);
        cqSphere(c, st, 44, 43, {"radius": 1.7688990214088962, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 44, "sphere");
        atStep(st, 45);
        cqCut(c, st, 45, 42, {"toCut": {"wp": 44}, "clean": true, "tol": undefined});
        stepVol(c, st, 45, "cut");
        atStep(st, 46);
        cqUnion(c, st, 46, 45, {"toUnion": {"wp": 44}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 46, "union");
        atStep(st, 47);
        cqWorkplaneNew(c, st, 47, {"plane": {"origin": [-7.058848760091261, -3.187765927974352, -6.086274012070926], "xDir": [0.6530048356252944, 0, -0.7573537381237266], "zDir": [-0.7165995726694945, -0.3236153343689519, -0.6178658169952189]}, "obj": undefined});
        atStep(st, 48);
        cqSphere(c, st, 48, 47, {"radius": 0.829547729427798, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 48, "sphere");
        atStep(st, 49);
        cqCut(c, st, 49, 46, {"toCut": {"wp": 48}, "clean": true, "tol": undefined});
        stepVol(c, st, 49, "cut");
        atStep(st, 50);
        cqUnion(c, st, 50, 49, {"toUnion": {"wp": 48}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 50, "union");
        atStep(st, 51);
        cqWorkplaneNew(c, st, 51, {"plane": {"origin": [-7.284950119331998, 2.4702070162599505, 5.865674494265749], "xDir": [-0.6271507173255323, 0, -0.7788979251211743], "zDir": [-0.7530753072195066, 0.25535547631674393, 0.6063589385597401]}, "obj": undefined});
        atStep(st, 52);
        cqSphere(c, st, 52, 51, {"radius": 1.0875596540129384, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 52, "sphere");
        atStep(st, 53);
        cqCut(c, st, 53, 50, {"toCut": {"wp": 52}, "clean": true, "tol": undefined});
        stepVol(c, st, 53, "cut");
        atStep(st, 54);
        cqUnion(c, st, 54, 53, {"toUnion": {"wp": 52}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 54, "union");
        atStep(st, 55);
        cqWorkplaneNew(c, st, 55, {"plane": {"origin": [8.250006773497892, -3.7042085764724737, 0.1436395336373205], "xDir": [0.4096018051262153, 0.9122644141022634, 0], "zDir": [0.9121493633649512, -0.4095501479652478, 0.01588128504114783]}, "obj": undefined});
        atStep(st, 56);
        cqSphere(c, st, 56, 55, {"radius": 1.497951134417816, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 56, "sphere");
        atStep(st, 57);
        cqCut(c, st, 57, 54, {"toCut": {"wp": 56}, "clean": true, "tol": undefined});
        stepVol(c, st, 57, "cut");
        atStep(st, 58);
        cqUnion(c, st, 58, 57, {"toUnion": {"wp": 56}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 58, "union");
        atStep(st, 59);
        cqWorkplaneNew(c, st, 59, {"plane": {"origin": [-7.884360402958831, -2.422342113889569, 3.741697413886939], "xDir": [-0.42874137107503263, 0, -0.9034272725176616], "zDir": [-0.8705164638324038, -0.26745209292869354, 0.4131228197337793]}, "obj": undefined});
        atStep(st, 60);
        cqSphere(c, st, 60, 59, {"radius": 1.5230099120830856, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 60, "sphere");
        atStep(st, 61);
        cqCut(c, st, 61, 58, {"toCut": {"wp": 60}, "clean": true, "tol": undefined});
        stepVol(c, st, 61, "cut");
        atStep(st, 62);
        cqUnion(c, st, 62, 61, {"toUnion": {"wp": 60}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 62, "union");
        atStep(st, 63);
        cqWorkplaneNew(c, st, 63, {"plane": {"origin": [-2.567853695181815, -1.9453535394029453, 8.003948655266974], "xDir": [0.9521963055786172, 1.387778780781446e-17, 0.30548681746097084], "zDir": [-0.2976204809766752, -0.22547120077485097, 0.9276770919476018]}, "obj": undefined});
        atStep(st, 64);
        cqSphere(c, st, 64, 63, {"radius": 1.7574703425016094, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 64, "sphere");
        atStep(st, 65);
        cqCut(c, st, 65, 62, {"toCut": {"wp": 64}, "clean": true, "tol": undefined});
        stepVol(c, st, 65, "cut");
        atStep(st, 66);
        cqUnion(c, st, 66, 65, {"toUnion": {"wp": 64}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 66, "union");
        atStep(st, 67);
        cqWorkplaneNew(c, st, 67, {"plane": {"origin": [-6.9684319524674665, 6.151195718753525, 1.8241074724350073], "xDir": [-0.6617776453471471, -0.74970017214803, 0], "zDir": [-0.7356676654175994, 0.6493908277800988, 0.19257372316621257]}, "obj": undefined});
        atStep(st, 68);
        cqSphere(c, st, 68, 67, {"radius": 0.970978094846619, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 68, "sphere");
        atStep(st, 69);
        cqCut(c, st, 69, 66, {"toCut": {"wp": 68}, "clean": true, "tol": undefined});
        stepVol(c, st, 69, "cut");
        atStep(st, 70);
        cqUnion(c, st, 70, 69, {"toUnion": {"wp": 68}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 70, "union");
        atStep(st, 71);
        cqWorkplaneNew(c, st, 71, {"plane": {"origin": [2.8652671462413193, 6.823744470856134, -5.866946681332734], "xDir": [0, 0.6519451358975923, 0.7582661404674943], "zDir": [0.3033867086137216, 0.7225271745253746, -0.6212173428912877]}, "obj": undefined});
        atStep(st, 72);
        cqSphere(c, st, 72, 71, {"radius": 1.0178253383929008, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 72, "sphere");
        atStep(st, 73);
        cqCut(c, st, 73, 70, {"toCut": {"wp": 72}, "clean": true, "tol": undefined});
        stepVol(c, st, 73, "cut");
        atStep(st, 74);
        cqUnion(c, st, 74, 73, {"toUnion": {"wp": 72}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 74, "union");
        atStep(st, 75);
        cqWorkplaneNew(c, st, 75, {"plane": {"origin": [7.979874951561499, 5.0518382288868, -0.44379746401083753], "xDir": [-0.5348946929405319, 0.8449187342366449, -3.4694469519536138e-18], "zDir": [0.8439874681112003, 0.5343051341012837, -0.046938015981238486]}, "obj": undefined});
        atStep(st, 76);
        cqSphere(c, st, 76, 75, {"radius": 0.9498776750294988, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 76, "sphere");
        atStep(st, 77);
        cqCut(c, st, 77, 74, {"toCut": {"wp": 76}, "clean": true, "tol": undefined});
        stepVol(c, st, 77, "cut");
        atStep(st, 78);
        cqUnion(c, st, 78, 77, {"toUnion": {"wp": 76}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 78, "union");
        atStep(st, 79);
        cqWorkplaneNew(c, st, 79, {"plane": {"origin": [2.4493642326286795, -3.730832325446719, 7.103967660447193], "xDir": [-1.387778780781446e-17, 0.8853338162127052, 0.4649559483115018], "zDir": [0.29195355801142053, -0.4446989783096573, 0.8467620318925613]}, "obj": undefined});
        atStep(st, 80);
        cqSphere(c, st, 80, 79, {"radius": 1.9060338305010909, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 80, "sphere");
        atStep(st, 81);
        cqCut(c, st, 81, 78, {"toCut": {"wp": 80}, "clean": true, "tol": undefined});
        stepVol(c, st, 81, "cut");
        atStep(st, 82);
        cqUnion(c, st, 82, 81, {"toUnion": {"wp": 80}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 82, "union");
        atStep(st, 83);
        cqWorkplaneNew(c, st, 83, {"plane": {"origin": [-5.902313953752223, 4.802399982981759, 0.6199152566265245], "xDir": [-0.6311283854129038, -0.7756783876879008, 6.93889390390723e-18], "zDir": [-0.7731169735884231, 0.6290442985405136, 0.08119984989610884]}, "obj": undefined});
        atStep(st, 84);
        cqSphere(c, st, 84, 83, {"radius": 2.7303360427735703, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 84, "sphere");
        atStep(st, 85);
        cqCut(c, st, 85, 82, {"toCut": {"wp": 84}, "clean": true, "tol": undefined});
        stepVol(c, st, 85, "cut");
        atStep(st, 86);
        cqUnion(c, st, 86, 85, {"toUnion": {"wp": 84}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 86, "union");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [4.178879630016086, -5.790974581076243, 5.691539697094374], "xDir": [0, -0.7009571817665515, -0.7132033576266265], "zDir": [0.4576121511641635, -0.6341461276722846, 0.623257417015757]}, "obj": undefined});
        atStep(st, 88);
        cqSphere(c, st, 88, 87, {"radius": 1.2909176343072972, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 88, "sphere");
        atStep(st, 89);
        cqCut(c, st, 89, 86, {"toCut": {"wp": 88}, "clean": true, "tol": undefined});
        stepVol(c, st, 89, "cut");
        atStep(st, 90);
        cqUnion(c, st, 90, 89, {"toUnion": {"wp": 88}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 90, "union");
        atStep(st, 91);
        cqWorkplaneNew(c, st, 91, {"plane": {"origin": [-3.2404424186664382, -1.773370371723672, 7.6061594784781565], "xDir": [0.9199898796793724, 0, 0.39194211471534185], "zDir": [-0.383225510264768, -0.20972468502369077, 0.8995297464642411]}, "obj": undefined});
        atStep(st, 92);
        cqSphere(c, st, 92, 91, {"radius": 1.9624962510309565, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 92, "sphere");
        atStep(st, 93);
        cqCut(c, st, 93, 90, {"toCut": {"wp": 92}, "clean": true, "tol": undefined});
        stepVol(c, st, 93, "cut");
        atStep(st, 94);
        cqUnion(c, st, 94, 93, {"toUnion": {"wp": 92}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 94, "union");
        atStep(st, 95);
        cqWorkplaneNew(c, st, 95, {"plane": {"origin": [1.0370788868238567, -0.5963747858127428, 9.72321238577403], "xDir": [0.9943598883111342, -8.673617379884035e-19, -0.10605853345143332], "zDir": [0.10586182985520488, -0.06087610779445533, 0.9925156787071454]}, "obj": undefined});
        atStep(st, 96);
        cqSphere(c, st, 96, 95, {"radius": 0.748378685148474, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 96, "sphere");
        atStep(st, 97);
        cqCut(c, st, 97, 94, {"toCut": {"wp": 96}, "clean": true, "tol": undefined});
        stepVol(c, st, 97, "cut");
        atStep(st, 98);
        cqUnion(c, st, 98, 97, {"toUnion": {"wp": 96}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 98, "union");
        atStep(st, 99);
        cqWorkplaneNew(c, st, 99, {"plane": {"origin": [-8.827721996402977, 0.7537122771076193, 2.051348112944625], "xDir": [-0.2263449079089054, 3.469446951953614e-18, -0.9740472178820232], "zDir": [-0.9706961884312792, 0.08287819154934206, 0.2255662100814244]}, "obj": undefined});
        atStep(st, 100);
        cqSphere(c, st, 100, 99, {"radius": 1.3658453669732595, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 100, "sphere");
        atStep(st, 101);
        cqCut(c, st, 101, 98, {"toCut": {"wp": 100}, "clean": true, "tol": undefined});
        stepVol(c, st, 101, "cut");
        atStep(st, 102);
        cqUnion(c, st, 102, 101, {"toUnion": {"wp": 100}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 102, "union");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [-0.9382885423384494, -8.030958304268939, -4.8440838815796665], "xDir": [0, 0.5164942485988263, -0.8562906581087604], "zDir": [-0.0995468854390944, -0.8520373533377672, -0.5139287558762876]}, "obj": undefined});
        atStep(st, 104);
        cqSphere(c, st, 104, 103, {"radius": 0.9335026698789742, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 104, "sphere");
        atStep(st, 105);
        cqCut(c, st, 105, 102, {"toCut": {"wp": 104}, "clean": true, "tol": undefined});
        stepVol(c, st, 105, "cut");
        atStep(st, 106);
        cqUnion(c, st, 106, 105, {"toUnion": {"wp": 104}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 106, "union");
        atStep(st, 107);
        cqWorkplaneNew(c, st, 107, {"plane": {"origin": [-3.668190739758135, 6.427630289927948, 5.3962414976873045], "xDir": [0, -0.6429847204707801, 0.7658790043088483], "zDir": [-0.4004959857272065, 0.7017737929910587, 0.5891659434211518]}, "obj": undefined});
        atStep(st, 108);
        cqSphere(c, st, 108, 107, {"radius": 1.3882589136683918, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 108, "sphere");
        atStep(st, 109);
        cqCut(c, st, 109, 106, {"toCut": {"wp": 108}, "clean": true, "tol": undefined});
        stepVol(c, st, 109, "cut");
        atStep(st, 110);
        cqUnion(c, st, 110, 109, {"toUnion": {"wp": 108}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 110, "union");
        atStep(st, 111);
        cqWorkplaneNew(c, st, 111, {"plane": {"origin": [-2.0668446089575836, -6.057945776074803, -6.404016358993741], "xDir": [0, -0.7264631805511318, 0.6872053894604827], "zDir": [-0.22826988868281645, -0.6690617194722608, -0.7072830221753923]}, "obj": undefined});
        atStep(st, 112);
        cqSphere(c, st, 112, 111, {"radius": 1.403451847238005, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 112, "sphere");
        atStep(st, 113);
        cqCut(c, st, 113, 110, {"toCut": {"wp": 112}, "clean": true, "tol": undefined});
        stepVol(c, st, 113, "cut");
        atStep(st, 114);
        cqUnion(c, st, 114, 113, {"toUnion": {"wp": 112}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 114, "union");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 115, {"plane": {"origin": [-4.926564681867343, -1.075179844889106, 7.225063857399888], "xDir": [0.8262068624346736, 6.9388939039072276e-18, 0.5633668613486706], "zDir": [-0.559156419066786, -0.1220310197354602, 0.8200320293980794]}, "obj": undefined});
        atStep(st, 116);
        cqSphere(c, st, 116, 115, {"radius": 1.7259152485529647, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 116, "sphere");
        atStep(st, 117);
        cqCut(c, st, 117, 114, {"toCut": {"wp": 116}, "clean": true, "tol": undefined});
        stepVol(c, st, 117, "cut");
        atStep(st, 118);
        cqUnion(c, st, 118, 117, {"toUnion": {"wp": 116}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 118, "union");
        atStep(st, 119);
        cqWorkplaneNew(c, st, 119, {"plane": {"origin": [-6.980795360355882, -5.50950639646838, 2.4937356742962917], "xDir": [0.6195301327515468, -0.7849728750809488, 0], "zDir": [-0.7558192750515212, -0.5965210145134751, 0.26999981982291654]}, "obj": undefined});
        atStep(st, 120);
        cqSphere(c, st, 120, 119, {"radius": 1.2428497779251748, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 120, "sphere");
        atStep(st, 121);
        cqCut(c, st, 121, 118, {"toCut": {"wp": 120}, "clean": true, "tol": undefined});
        stepVol(c, st, 121, "cut");
        atStep(st, 122);
        cqUnion(c, st, 122, 121, {"toUnion": {"wp": 120}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 122, "union");
        atStep(st, 123);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [9.08765126464088, -1.118078275334147, -2.462547287945314], "xDir": [0.26154494121334887, 3.469446951953614e-18, 0.9651912990312884], "zDir": [0.95845707509149, -0.11792156216091904, -0.259720119329625]}, "obj": undefined});
        atStep(st, 124);
        cqSphere(c, st, 124, 123, {"radius": 0.972697151830154, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 124, "sphere");
        atStep(st, 125);
        cqCut(c, st, 125, 122, {"toCut": {"wp": 124}, "clean": true, "tol": undefined});
        stepVol(c, st, 125, "cut");
        atStep(st, 126);
        cqUnion(c, st, 126, 125, {"toUnion": {"wp": 124}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 126, "union");
        atStep(st, 127);
        cqWorkplaneNew(c, st, 127, {"plane": {"origin": [1.4115627186504358, 3.26035186194081, 8.46397929945144], "xDir": [0, 0.9331617009405936, -0.35945686792389736], "zDir": [0.15377508481385202, 0.3551814435651067, 0.9220625604792224]}, "obj": undefined});
        atStep(st, 128);
        cqSphere(c, st, 128, 127, {"radius": 1.325342965775843, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 128, "sphere");
        atStep(st, 129);
        cqCut(c, st, 129, 126, {"toCut": {"wp": 128}, "clean": true, "tol": undefined});
        stepVol(c, st, 129, "cut");
        atStep(st, 130);
        cqUnion(c, st, 130, 129, {"toUnion": {"wp": 128}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 130, "union");
        atStep(st, 131);
        cqWorkplaneNew(c, st, 131, {"plane": {"origin": [6.7622727927547155, -2.827486248835484, 5.518619591500576], "xDir": [-0.6322662654378091, -2.7755575615628907e-17, 0.77475116623941], "zDir": [0.7370431868860999, -0.3081773746174751, 0.6014931807083619]}, "obj": undefined});
        atStep(st, 132);
        cqSphere(c, st, 132, 131, {"radius": 1.373421209687698, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 132, "sphere");
        atStep(st, 133);
        cqCut(c, st, 133, 130, {"toCut": {"wp": 132}, "clean": true, "tol": undefined});
        stepVol(c, st, 133, "cut");
        atStep(st, 134);
        cqUnion(c, st, 134, 133, {"toUnion": {"wp": 132}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 134, "union");
        atStep(st, 135);
        cqWorkplaneNew(c, st, 135, {"plane": {"origin": [-0.332558083039559, 5.328658564092884, -7.045269840681472], "xDir": [0, -0.7975641670112277, -0.6032341166576096], "zDir": [-0.037620794130943366, 0.6028070795981231, -0.7969995612516605]}, "obj": undefined});
        atStep(st, 136);
        cqSphere(c, st, 136, 135, {"radius": 1.6752815699590482, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 136, "sphere");
        atStep(st, 137);
        cqCut(c, st, 137, 134, {"toCut": {"wp": 136}, "clean": true, "tol": undefined});
        stepVol(c, st, 137, "cut");
        atStep(st, 138);
        cqUnion(c, st, 138, 137, {"toUnion": {"wp": 136}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 138, "union");
        atStep(st, 139);
        cqWorkplaneNew(c, st, 139, {"plane": {"origin": [3.8259096041363105, 8.172805457223705, -2.8318025077827653], "xDir": [0.9056757130145237, -0.42397111087388295, -1.3877787807814457e-17], "zDir": [0.40452096949600463, 0.8641268427994413, -0.2994120652069354]}, "obj": undefined});
        atStep(st, 140);
        cqSphere(c, st, 140, 139, {"radius": 1.184002687013622, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 140, "sphere");
        atStep(st, 141);
        cqCut(c, st, 141, 138, {"toCut": {"wp": 140}, "clean": true, "tol": undefined});
        stepVol(c, st, 141, "cut");
        atStep(st, 142);
        cqUnion(c, st, 142, 141, {"toUnion": {"wp": 140}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 142, "union");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 143, {"plane": {"origin": [5.622719278618528, -0.41639428126398575, -7.469688453219614], "xDir": [-0.7989487166672292, -3.469446951953614e-18, -0.6013991587421681], "zDir": [0.6008035930566217, -0.04449291666808909, -0.7981575176220668]}, "obj": undefined});
        atStep(st, 144);
        cqSphere(c, st, 144, 143, {"radius": 1.1406878466955648, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 144, "sphere");
        atStep(st, 145);
        cqCut(c, st, 145, 142, {"toCut": {"wp": 144}, "clean": true, "tol": undefined});
        stepVol(c, st, 145, "cut");
        atStep(st, 146);
        cqUnion(c, st, 146, 145, {"toUnion": {"wp": 144}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 146, "union");
        atStep(st, 147);
        cqWorkplaneNew(c, st, 147, {"plane": {"origin": [4.363329644154527, -7.037998555868417, 4.327928074839017], "xDir": [-0.8499151012650882, -0.5269196529278016, 0], "zDir": [0.46698546630846327, -0.7532419747138656, 0.4631966124507213]}, "obj": undefined});
        atStep(st, 148);
        cqSphere(c, st, 148, 147, {"radius": 1.2458971960017735, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 148, "sphere");
        atStep(st, 149);
        cqCut(c, st, 149, 146, {"toCut": {"wp": 148}, "clean": true, "tol": undefined});
        stepVol(c, st, 149, "cut");
        atStep(st, 150);
        cqUnion(c, st, 150, 149, {"toUnion": {"wp": 148}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 150, "union");
        atStep(st, 151);
        cqWorkplaneNew(c, st, 151, {"plane": {"origin": [8.199674732718895, -1.749236974917177, 3.084604911729814], "xDir": [-0.3520966563213712, 0, 0.9359636449175311], "zDir": [0.9178462120638056, -0.1958041731854539, 0.34528112714610515]}, "obj": undefined});
        atStep(st, 152);
        cqSphere(c, st, 152, 151, {"radius": 1.5365336270422625, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 152, "sphere");
        atStep(st, 153);
        cqCut(c, st, 153, 150, {"toCut": {"wp": 152}, "clean": true, "tol": undefined});
        stepVol(c, st, 153, "cut");
        atStep(st, 154);
        cqUnion(c, st, 154, 153, {"toUnion": {"wp": 152}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 154, "union");
        atStep(st, 155);
        cqWorkplaneNew(c, st, 155, {"plane": {"origin": [-3.4850736868717105, 5.319160222798115, 6.076407875963751], "xDir": [0, 0.7524353298279648, -0.6586661327460841], "zDir": [-0.3962308610222848, 0.6047549131985882, 0.6908491874032328]}, "obj": undefined});
        atStep(st, 156);
        cqSphere(c, st, 156, 155, {"radius": 1.7580057890481164, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 156, "sphere");
        atStep(st, 157);
        cqCut(c, st, 157, 154, {"toCut": {"wp": 156}, "clean": true, "tol": undefined});
        stepVol(c, st, 157, "cut");
        atStep(st, 158);
        cqUnion(c, st, 158, 157, {"toUnion": {"wp": 156}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 158, "union");
        atStep(st, 159);
        cqWorkplaneNew(c, st, 159, {"plane": {"origin": [4.307902253192998, 8.431080397712174, -1.0996731881311046], "xDir": [0.8904910294220203, -0.45500079837172897, 0], "zDir": [0.4519624682814872, 0.8845446537245147, -0.1153719326018421]}, "obj": undefined});
        atStep(st, 160);
        cqSphere(c, st, 160, 159, {"radius": 1.1032314903945508, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 160, "sphere");
        atStep(st, 161);
        cqCut(c, st, 161, 158, {"toCut": {"wp": 160}, "clean": true, "tol": undefined});
        stepVol(c, st, 161, "cut");
        atStep(st, 162);
        cqUnion(c, st, 162, 161, {"toUnion": {"wp": 160}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 162, "union");
        atStep(st, 163);
        cqWorkplaneNew(c, st, 163, {"plane": {"origin": [-9.538413897658147, -0.5159219912703831, 1.33317650326825], "xDir": [-0.1384236592730021, 8.673617379884035e-19, -0.9903731067398144], "zDir": [-0.988955196206272, -0.053491465098741565, 0.13822547904856045]}, "obj": undefined});
        atStep(st, 164);
        cqSphere(c, st, 164, 163, {"radius": 0.8732742920442238, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 164, "sphere");
        atStep(st, 165);
        cqCut(c, st, 165, 162, {"toCut": {"wp": 164}, "clean": true, "tol": undefined});
        stepVol(c, st, 165, "cut");
        atStep(st, 166);
        cqUnion(c, st, 166, 165, {"toUnion": {"wp": 164}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 166, "union");
        atStep(st, 167);
        cqWorkplaneNew(c, st, 167, {"plane": {"origin": [-0.04130992822630995, 7.049267710929731, 6.454865460615738], "xDir": [0, -0.6753286560446187, 0.7375169193482745], "zDir": [-0.00432193636353916, 0.7375100312274521, 0.675322348737958]}, "obj": undefined});
        atStep(st, 168);
        cqSphere(c, st, 168, 167, {"radius": 1.0288864836112153, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 168, "sphere");
        atStep(st, 169);
        cqCut(c, st, 169, 166, {"toCut": {"wp": 168}, "clean": true, "tol": undefined});
        stepVol(c, st, 169, "cut");
        atStep(st, 170);
        cqUnion(c, st, 170, 169, {"toUnion": {"wp": 168}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 170, "union");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [-4.182347165899539, 6.9799076770076605, 4.490257581945381], "xDir": [-2.7755575615628907e-17, -0.5410281722792654, 0.8410044689537371], "zDir": [-0.4500177970991616, 0.7510334633081398, 0.48314875482132713]}, "obj": undefined});
        atStep(st, 172);
        cqSphere(c, st, 172, 171, {"radius": 1.200383425912834, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 172, "sphere");
        atStep(st, 173);
        cqCut(c, st, 173, 170, {"toCut": {"wp": 172}, "clean": true, "tol": undefined});
        stepVol(c, st, 173, "cut");
        atStep(st, 174);
        cqUnion(c, st, 174, 173, {"toUnion": {"wp": 172}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 174, "union");
        atStep(st, 175);
        cqWorkplaneNew(c, st, 175, {"plane": {"origin": [7.345183055893172, -2.2279182039295105, -4.669566058590514], "xDir": [0.5364958871913084, -1.3877787807814457e-17, 0.843902934600189], "zDir": [0.8175448001915161, -0.24797515991289457, -0.519739184347292]}, "obj": undefined});
        atStep(st, 176);
        cqSphere(c, st, 176, 175, {"radius": 1.5385561291246923, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 176, "sphere");
        atStep(st, 177);
        cqCut(c, st, 177, 174, {"toCut": {"wp": 176}, "clean": true, "tol": undefined});
        stepVol(c, st, 177, "cut");
        atStep(st, 178);
        cqUnion(c, st, 178, 177, {"toUnion": {"wp": 176}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 178, "union");
        atStep(st, 179);
        cqWorkplaneNew(c, st, 179, {"plane": {"origin": [8.672298097044793, -3.0054087106983443, -2.693590631708098], "xDir": [0.3274471208771635, 0.9448695058203838, 0], "zDir": [0.906633082806805, -0.3141961835241088, -0.2815976054925074]}, "obj": undefined});
        atStep(st, 180);
        cqSphere(c, st, 180, 179, {"radius": 1.000591992477552, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 180, "sphere");
        atStep(st, 181);
        cqCut(c, st, 181, 178, {"toCut": {"wp": 180}, "clean": true, "tol": undefined});
        stepVol(c, st, 181, "cut");
        atStep(st, 182);
        cqUnion(c, st, 182, 181, {"toUnion": {"wp": 180}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 182, "union");
        atStep(st, 183);
        cqWorkplaneNew(c, st, 183, {"plane": {"origin": [-6.440598191273656, 5.46469103771385, 4.037159131258839], "xDir": [-0.6469733908461278, -0.7625125779533499, -2.7755575615628914e-17], "zDir": [-0.6879677280009223, 0.5837238973449353, 0.43123870060291264]}, "obj": undefined});
        atStep(st, 184);
        cqSphere(c, st, 184, 183, {"radius": 1.0344532466505882, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 184, "sphere");
        atStep(st, 185);
        cqCut(c, st, 185, 182, {"toCut": {"wp": 184}, "clean": true, "tol": undefined});
        stepVol(c, st, 185, "cut");
        atStep(st, 186);
        cqUnion(c, st, 186, 185, {"toUnion": {"wp": 184}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 186, "union");
        atStep(st, 187);
        cqWorkplaneNew(c, st, 187, {"plane": {"origin": [0.27103106694425455, 0.7146752305133925, -9.766497928973628], "xDir": [0, -0.9973333264658918, -0.07298106549290034], "zDir": [0.02766650297344377, 0.0729531290009355, -0.9969515562865596]}, "obj": undefined});
        atStep(st, 188);
        cqSphere(c, st, 188, 187, {"radius": 0.7779390428530258, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 188, "sphere");
        atStep(st, 189);
        cqCut(c, st, 189, 186, {"toCut": {"wp": 188}, "clean": true, "tol": undefined});
        stepVol(c, st, 189, "cut");
        atStep(st, 190);
        cqUnion(c, st, 190, 189, {"toUnion": {"wp": 188}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 190, "union");
        atStep(st, 191);
        cqWorkplaneNew(c, st, 191, {"plane": {"origin": [-6.621144807573543, 6.32436236311527, 2.616874683074643], "xDir": [-0.6907145038785601, -0.7231275642179564, 2.7755575615628914e-17], "zDir": [-0.6952884196060157, 0.6641232053158568, 0.27479880225860054]}, "obj": undefined});
        atStep(st, 192);
        cqSphere(c, st, 192, 191, {"radius": 1.0409456146480778, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 192, "sphere");
        atStep(st, 193);
        cqCut(c, st, 193, 190, {"toCut": {"wp": 192}, "clean": true, "tol": undefined});
        stepVol(c, st, 193, "cut");
        atStep(st, 194);
        cqUnion(c, st, 194, 193, {"toUnion": {"wp": 192}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 194, "union");
        atStep(st, 195);
        cqWorkplaneNew(c, st, 195, {"plane": {"origin": [-6.836203118474967, 4.819526116382741, -1.453314124093971], "xDir": [-0.5762020062729523, -0.8173073154982922, 0], "zDir": [-0.8052426494892376, 0.5676964115870756, -0.17118720663271136]}, "obj": undefined});
        atStep(st, 196);
        cqSphere(c, st, 196, 195, {"radius": 1.9189912455223057, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 196, "sphere");
        atStep(st, 197);
        cqCut(c, st, 197, 194, {"toCut": {"wp": 196}, "clean": true, "tol": undefined});
        stepVol(c, st, 197, "cut");
        atStep(st, 198);
        cqUnion(c, st, 198, 197, {"toUnion": {"wp": 196}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 198, "union");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 199, {"plane": {"origin": [8.240917125704028, 2.494466431319557, 2.952745463381561], "xDir": [-0.3373048392117317, 0, 0.94139547770549], "zDir": [0.9053559785220112, 0.2740447528314018, 0.32439177794763485]}, "obj": undefined});
        atStep(st, 200);
        cqSphere(c, st, 200, 199, {"radius": 1.2982416369011862, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 200, "sphere");
        atStep(st, 201);
        cqCut(c, st, 201, 198, {"toCut": {"wp": 200}, "clean": true, "tol": undefined});
        stepVol(c, st, 201, "cut");
        atStep(st, 202);
        cqUnion(c, st, 202, 201, {"toUnion": {"wp": 200}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 202, "union");
        atStep(st, 203);
        cqWorkplaneNew(c, st, 203, {"plane": {"origin": [9.999999, 0, 6.123233383413369e-16], "xDir": [0, -1, 0], "zDir": [1, 0, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 204);
        cqCircle(c, st, 204, 203, {"radius": 1, "forConstruction": false});
        atStep(st, 205);
        cqExtrude(c, st, 205, 204, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 205, "extrude");
        atStep(st, 206);
        cqSelect("Edges")(c, st, 206, 205, {"selector": undefined, "tag": undefined});
        atStep(st, 207);
        cqFillet(c, st, 207, 206, {"radius": 0.2});
        stepVol(c, st, 207, "fillet");
        atStep(st, 208);
        cqWorkplaneNew(c, st, 208, {"plane": {"origin": [9.999999, 0, 6.123233383413369e-16], "xDir": [0, -1, 0], "zDir": [1, 0, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 209);
        cqTransformed(c, st, 209, 208, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 210);
        cqCenter(c, st, 210, 209, {"x": 1, "y": 0});
        atStep(st, 211);
        cqRect(c, st, 211, 210, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 212);
        cqExtrude(c, st, 212, 211, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 212, "extrude");
        atStep(st, 213);
        cqUnion(c, st, 213, 207, {"toUnion": {"wp": 212}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 213, "union");
        atStep(st, 214);
        cqWorkplaneNew(c, st, 214, {"plane": {"origin": [9.999999, 0, 6.123233383413369e-16], "xDir": [0, -1, 0], "zDir": [1, 0, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 215);
        cqTransformed(c, st, 215, 214, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 216);
        cqCenter(c, st, 216, 215, {"x": 1, "y": 0});
        atStep(st, 217);
        cqRect(c, st, 217, 216, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 218);
        cqExtrude(c, st, 218, 217, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 218, "extrude");
        atStep(st, 219);
        cqUnion(c, st, 219, 213, {"toUnion": {"wp": 218}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 219, "union");
        atStep(st, 220);
        cqWorkplaneNew(c, st, 220, {"plane": {"origin": [9.999999, 0, 6.123233383413369e-16], "xDir": [0, -1, 0], "zDir": [1, 0, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 221);
        cqTransformed(c, st, 221, 220, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 222);
        cqCenter(c, st, 222, 221, {"x": 1, "y": 0});
        atStep(st, 223);
        cqRect(c, st, 223, 222, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 224);
        cqExtrude(c, st, 224, 223, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 224, "extrude");
        atStep(st, 225);
        cqUnion(c, st, 225, 219, {"toUnion": {"wp": 224}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 225, "union");
        atStep(st, 226);
        cqWorkplaneNew(c, st, 226, {"plane": {"origin": [9.999999, 0, 6.123233383413369e-16], "xDir": [0, -1, 0], "zDir": [1, 0, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 227);
        cqTransformed(c, st, 227, 226, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 228);
        cqCenter(c, st, 228, 227, {"x": 1, "y": 0});
        atStep(st, 229);
        cqRect(c, st, 229, 228, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 230);
        cqExtrude(c, st, 230, 229, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 230, "extrude");
        atStep(st, 231);
        cqUnion(c, st, 231, 225, {"toUnion": {"wp": 230}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 231, "union");
        atStep(st, 232);
        cqUnion(c, st, 232, 202, {"toUnion": {"wp": 231}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 232, "union");
        atStep(st, 233);
        cqWorkplaneNew(c, st, 233, {"plane": {"origin": [4.999999500000001, 8.660253171818983, 6.123233383413369e-16], "xDir": [0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, 0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 234);
        cqCircle(c, st, 234, 233, {"radius": 1, "forConstruction": false});
        atStep(st, 235);
        cqExtrude(c, st, 235, 234, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 235, "extrude");
        atStep(st, 236);
        cqSelect("Edges")(c, st, 236, 235, {"selector": undefined, "tag": undefined});
        atStep(st, 237);
        cqFillet(c, st, 237, 236, {"radius": 0.2});
        stepVol(c, st, 237, "fillet");
        atStep(st, 238);
        cqWorkplaneNew(c, st, 238, {"plane": {"origin": [4.999999500000001, 8.660253171818983, 6.123233383413369e-16], "xDir": [0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, 0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 239);
        cqTransformed(c, st, 239, 238, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 240);
        cqCenter(c, st, 240, 239, {"x": 1, "y": 0});
        atStep(st, 241);
        cqRect(c, st, 241, 240, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 242);
        cqExtrude(c, st, 242, 241, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 242, "extrude");
        atStep(st, 243);
        cqUnion(c, st, 243, 237, {"toUnion": {"wp": 242}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 243, "union");
        atStep(st, 244);
        cqWorkplaneNew(c, st, 244, {"plane": {"origin": [4.999999500000001, 8.660253171818983, 6.123233383413369e-16], "xDir": [0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, 0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 245);
        cqTransformed(c, st, 245, 244, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 246);
        cqCenter(c, st, 246, 245, {"x": 1, "y": 0});
        atStep(st, 247);
        cqRect(c, st, 247, 246, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 248);
        cqExtrude(c, st, 248, 247, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 248, "extrude");
        atStep(st, 249);
        cqUnion(c, st, 249, 243, {"toUnion": {"wp": 248}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 249, "union");
        atStep(st, 250);
        cqWorkplaneNew(c, st, 250, {"plane": {"origin": [4.999999500000001, 8.660253171818983, 6.123233383413369e-16], "xDir": [0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, 0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 251);
        cqTransformed(c, st, 251, 250, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 252);
        cqCenter(c, st, 252, 251, {"x": 1, "y": 0});
        atStep(st, 253);
        cqRect(c, st, 253, 252, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 254);
        cqExtrude(c, st, 254, 253, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        atStep(st, 255);
        cqUnion(c, st, 255, 249, {"toUnion": {"wp": 254}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 255, "union");
        atStep(st, 256);
        cqWorkplaneNew(c, st, 256, {"plane": {"origin": [4.999999500000001, 8.660253171818983, 6.123233383413369e-16], "xDir": [0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, 0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 257);
        cqTransformed(c, st, 257, 256, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 258);
        cqCenter(c, st, 258, 257, {"x": 1, "y": 0});
        atStep(st, 259);
        cqRect(c, st, 259, 258, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 260);
        cqExtrude(c, st, 260, 259, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 260, "extrude");
        atStep(st, 261);
        cqUnion(c, st, 261, 255, {"toUnion": {"wp": 260}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 261, "union");
        atStep(st, 262);
        cqUnion(c, st, 262, 232, {"toUnion": {"wp": 261}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 262, "union");
        atStep(st, 263);
        cqWorkplaneNew(c, st, 263, {"plane": {"origin": [-4.999999499999999, 8.660253171818985, 6.123233383413369e-16], "xDir": [0.8660254037844388, 0.49999999999999983, 0], "zDir": [-0.49999999999999983, 0.8660254037844388, 6.123233995736767e-17]}, "obj": undefined});
        atStep(st, 264);
        cqCircle(c, st, 264, 263, {"radius": 1, "forConstruction": false});
        atStep(st, 265);
        cqExtrude(c, st, 265, 264, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 265, "extrude");
        atStep(st, 266);
        cqSelect("Edges")(c, st, 266, 265, {"selector": undefined, "tag": undefined});
        atStep(st, 267);
        cqFillet(c, st, 267, 266, {"radius": 0.2});
        stepVol(c, st, 267, "fillet");
        atStep(st, 268);
        cqWorkplaneNew(c, st, 268, {"plane": {"origin": [-4.999999499999999, 8.660253171818985, 6.123233383413369e-16], "xDir": [0.8660254037844388, 0.49999999999999983, 0], "zDir": [-0.49999999999999983, 0.8660254037844388, 6.123233995736767e-17]}, "obj": undefined});
        atStep(st, 269);
        cqTransformed(c, st, 269, 268, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 270);
        cqCenter(c, st, 270, 269, {"x": 1, "y": 0});
        atStep(st, 271);
        cqRect(c, st, 271, 270, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 272);
        cqExtrude(c, st, 272, 271, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 272, "extrude");
        atStep(st, 273);
        cqUnion(c, st, 273, 267, {"toUnion": {"wp": 272}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 273, "union");
        atStep(st, 274);
        cqWorkplaneNew(c, st, 274, {"plane": {"origin": [-4.999999499999999, 8.660253171818985, 6.123233383413369e-16], "xDir": [0.8660254037844388, 0.49999999999999983, 0], "zDir": [-0.49999999999999983, 0.8660254037844388, 6.123233995736767e-17]}, "obj": undefined});
        atStep(st, 275);
        cqTransformed(c, st, 275, 274, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 276);
        cqCenter(c, st, 276, 275, {"x": 1, "y": 0});
        atStep(st, 277);
        cqRect(c, st, 277, 276, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 278);
        cqExtrude(c, st, 278, 277, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 278, "extrude");
        atStep(st, 279);
        cqUnion(c, st, 279, 273, {"toUnion": {"wp": 278}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 279, "union");
        atStep(st, 280);
        cqWorkplaneNew(c, st, 280, {"plane": {"origin": [-4.999999499999999, 8.660253171818985, 6.123233383413369e-16], "xDir": [0.8660254037844388, 0.49999999999999983, 0], "zDir": [-0.49999999999999983, 0.8660254037844388, 6.123233995736767e-17]}, "obj": undefined});
        atStep(st, 281);
        cqTransformed(c, st, 281, 280, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 282);
        cqCenter(c, st, 282, 281, {"x": 1, "y": 0});
        atStep(st, 283);
        cqRect(c, st, 283, 282, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 284);
        cqExtrude(c, st, 284, 283, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 284, "extrude");
        atStep(st, 285);
        cqUnion(c, st, 285, 279, {"toUnion": {"wp": 284}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 285, "union");
        atStep(st, 286);
        cqWorkplaneNew(c, st, 286, {"plane": {"origin": [-4.999999499999999, 8.660253171818985, 6.123233383413369e-16], "xDir": [0.8660254037844388, 0.49999999999999983, 0], "zDir": [-0.49999999999999983, 0.8660254037844388, 6.123233995736767e-17]}, "obj": undefined});
        atStep(st, 287);
        cqTransformed(c, st, 287, 286, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 288);
        cqCenter(c, st, 288, 287, {"x": 1, "y": 0});
        atStep(st, 289);
        cqRect(c, st, 289, 288, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 290);
        cqExtrude(c, st, 290, 289, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 290, "extrude");
        atStep(st, 291);
        cqUnion(c, st, 291, 285, {"toUnion": {"wp": 290}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 291, "union");
        atStep(st, 292);
        cqUnion(c, st, 292, 262, {"toUnion": {"wp": 291}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 292, "union");
        atStep(st, 293);
        cqWorkplaneNew(c, st, 293, {"plane": {"origin": [-9.999999, 1.2246466766826733e-15, 6.123233383413369e-16], "xDir": [1.2246467991473532e-16, 1, 0], "zDir": [-1, 1.2246467991473532e-16, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 294);
        cqCircle(c, st, 294, 293, {"radius": 1, "forConstruction": false});
        atStep(st, 295);
        cqExtrude(c, st, 295, 294, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 295, "extrude");
        atStep(st, 296);
        cqSelect("Edges")(c, st, 296, 295, {"selector": undefined, "tag": undefined});
        atStep(st, 297);
        cqFillet(c, st, 297, 296, {"radius": 0.2});
        stepVol(c, st, 297, "fillet");
        atStep(st, 298);
        cqWorkplaneNew(c, st, 298, {"plane": {"origin": [-9.999999, 1.2246466766826733e-15, 6.123233383413369e-16], "xDir": [1.2246467991473532e-16, 1, 0], "zDir": [-1, 1.2246467991473532e-16, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 299);
        cqTransformed(c, st, 299, 298, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 300);
        cqCenter(c, st, 300, 299, {"x": 1, "y": 0});
        atStep(st, 301);
        cqRect(c, st, 301, 300, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 302);
        cqExtrude(c, st, 302, 301, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 302, "extrude");
        atStep(st, 303);
        cqUnion(c, st, 303, 297, {"toUnion": {"wp": 302}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 303, "union");
        atStep(st, 304);
        cqWorkplaneNew(c, st, 304, {"plane": {"origin": [-9.999999, 1.2246466766826733e-15, 6.123233383413369e-16], "xDir": [1.2246467991473532e-16, 1, 0], "zDir": [-1, 1.2246467991473532e-16, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 305);
        cqTransformed(c, st, 305, 304, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 306);
        cqCenter(c, st, 306, 305, {"x": 1, "y": 0});
        atStep(st, 307);
        cqRect(c, st, 307, 306, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 308);
        cqExtrude(c, st, 308, 307, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 308, "extrude");
        atStep(st, 309);
        cqUnion(c, st, 309, 303, {"toUnion": {"wp": 308}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 309, "union");
        atStep(st, 310);
        cqWorkplaneNew(c, st, 310, {"plane": {"origin": [-9.999999, 1.2246466766826733e-15, 6.123233383413369e-16], "xDir": [1.2246467991473532e-16, 1, 0], "zDir": [-1, 1.2246467991473532e-16, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 311);
        cqTransformed(c, st, 311, 310, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 312);
        cqCenter(c, st, 312, 311, {"x": 1, "y": 0});
        atStep(st, 313);
        cqRect(c, st, 313, 312, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 314);
        cqExtrude(c, st, 314, 313, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 314, "extrude");
        atStep(st, 315);
        cqUnion(c, st, 315, 309, {"toUnion": {"wp": 314}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 315, "union");
        atStep(st, 316);
        cqWorkplaneNew(c, st, 316, {"plane": {"origin": [-9.999999, 1.2246466766826733e-15, 6.123233383413369e-16], "xDir": [1.2246467991473532e-16, 1, 0], "zDir": [-1, 1.2246467991473532e-16, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 317);
        cqTransformed(c, st, 317, 316, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 318);
        cqCenter(c, st, 318, 317, {"x": 1, "y": 0});
        atStep(st, 319);
        cqRect(c, st, 319, 318, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 320);
        cqExtrude(c, st, 320, 319, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 320, "extrude");
        atStep(st, 321);
        cqUnion(c, st, 321, 315, {"toUnion": {"wp": 320}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 321, "union");
        atStep(st, 322);
        cqUnion(c, st, 322, 292, {"toUnion": {"wp": 321}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 322, "union");
        atStep(st, 323);
        cqWorkplaneNew(c, st, 323, {"plane": {"origin": [-4.999999500000005, -8.660253171818981, 6.123233383413369e-16], "xDir": [-0.8660254037844384, 0.5000000000000004, -3.081487911019578e-33], "zDir": [-0.5000000000000004, -0.8660254037844384, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 324);
        cqCircle(c, st, 324, 323, {"radius": 1, "forConstruction": false});
        atStep(st, 325);
        cqExtrude(c, st, 325, 324, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 325, "extrude");
        atStep(st, 326);
        cqSelect("Edges")(c, st, 326, 325, {"selector": undefined, "tag": undefined});
        atStep(st, 327);
        cqFillet(c, st, 327, 326, {"radius": 0.2});
        stepVol(c, st, 327, "fillet");
        atStep(st, 328);
        cqWorkplaneNew(c, st, 328, {"plane": {"origin": [-4.999999500000005, -8.660253171818981, 6.123233383413369e-16], "xDir": [-0.8660254037844384, 0.5000000000000004, -3.081487911019578e-33], "zDir": [-0.5000000000000004, -0.8660254037844384, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 329);
        cqTransformed(c, st, 329, 328, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 330);
        cqCenter(c, st, 330, 329, {"x": 1, "y": 0});
        atStep(st, 331);
        cqRect(c, st, 331, 330, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 332);
        cqExtrude(c, st, 332, 331, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 332, "extrude");
        atStep(st, 333);
        cqUnion(c, st, 333, 327, {"toUnion": {"wp": 332}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 333, "union");
        atStep(st, 334);
        cqWorkplaneNew(c, st, 334, {"plane": {"origin": [-4.999999500000005, -8.660253171818981, 6.123233383413369e-16], "xDir": [-0.8660254037844384, 0.5000000000000004, -3.081487911019578e-33], "zDir": [-0.5000000000000004, -0.8660254037844384, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 335);
        cqTransformed(c, st, 335, 334, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 336);
        cqCenter(c, st, 336, 335, {"x": 1, "y": 0});
        atStep(st, 337);
        cqRect(c, st, 337, 336, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 338);
        cqExtrude(c, st, 338, 337, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 338, "extrude");
        atStep(st, 339);
        cqUnion(c, st, 339, 333, {"toUnion": {"wp": 338}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 339, "union");
        atStep(st, 340);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [-4.999999500000005, -8.660253171818981, 6.123233383413369e-16], "xDir": [-0.8660254037844384, 0.5000000000000004, -3.081487911019578e-33], "zDir": [-0.5000000000000004, -0.8660254037844384, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 341);
        cqTransformed(c, st, 341, 340, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 342);
        cqCenter(c, st, 342, 341, {"x": 1, "y": 0});
        atStep(st, 343);
        cqRect(c, st, 343, 342, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 344);
        cqExtrude(c, st, 344, 343, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 344, "extrude");
        atStep(st, 345);
        cqUnion(c, st, 345, 339, {"toUnion": {"wp": 344}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 345, "union");
        atStep(st, 346);
        cqWorkplaneNew(c, st, 346, {"plane": {"origin": [-4.999999500000005, -8.660253171818981, 6.123233383413369e-16], "xDir": [-0.8660254037844384, 0.5000000000000004, -3.081487911019578e-33], "zDir": [-0.5000000000000004, -0.8660254037844384, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 347);
        cqTransformed(c, st, 347, 346, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 348);
        cqCenter(c, st, 348, 347, {"x": 1, "y": 0});
        atStep(st, 349);
        cqRect(c, st, 349, 348, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 350);
        cqExtrude(c, st, 350, 349, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 350, "extrude");
        atStep(st, 351);
        cqUnion(c, st, 351, 345, {"toUnion": {"wp": 350}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 351, "union");
        atStep(st, 352);
        cqUnion(c, st, 352, 322, {"toUnion": {"wp": 351}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 352, "union");
        atStep(st, 353);
        cqWorkplaneNew(c, st, 353, {"plane": {"origin": [4.999999500000001, -8.660253171818983, 6.123233383413369e-16], "xDir": [-0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, -0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 354);
        cqCircle(c, st, 354, 353, {"radius": 1, "forConstruction": false});
        atStep(st, 355);
        cqExtrude(c, st, 355, 354, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 355, "extrude");
        atStep(st, 356);
        cqSelect("Edges")(c, st, 356, 355, {"selector": undefined, "tag": undefined});
        atStep(st, 357);
        cqFillet(c, st, 357, 356, {"radius": 0.2});
        stepVol(c, st, 357, "fillet");
        atStep(st, 358);
        cqWorkplaneNew(c, st, 358, {"plane": {"origin": [4.999999500000001, -8.660253171818983, 6.123233383413369e-16], "xDir": [-0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, -0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 359);
        cqTransformed(c, st, 359, 358, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 360);
        cqCenter(c, st, 360, 359, {"x": 1, "y": 0});
        atStep(st, 361);
        cqRect(c, st, 361, 360, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 362);
        cqExtrude(c, st, 362, 361, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        atStep(st, 363);
        cqUnion(c, st, 363, 357, {"toUnion": {"wp": 362}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 363, "union");
        atStep(st, 364);
        cqWorkplaneNew(c, st, 364, {"plane": {"origin": [4.999999500000001, -8.660253171818983, 6.123233383413369e-16], "xDir": [-0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, -0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 365);
        cqTransformed(c, st, 365, 364, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 366);
        cqCenter(c, st, 366, 365, {"x": 1, "y": 0});
        atStep(st, 367);
        cqRect(c, st, 367, 366, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 368);
        cqExtrude(c, st, 368, 367, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 368, "extrude");
        atStep(st, 369);
        cqUnion(c, st, 369, 363, {"toUnion": {"wp": 368}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 369, "union");
        atStep(st, 370);
        cqWorkplaneNew(c, st, 370, {"plane": {"origin": [4.999999500000001, -8.660253171818983, 6.123233383413369e-16], "xDir": [-0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, -0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 371);
        cqTransformed(c, st, 371, 370, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 372);
        cqCenter(c, st, 372, 371, {"x": 1, "y": 0});
        atStep(st, 373);
        cqRect(c, st, 373, 372, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 374);
        cqExtrude(c, st, 374, 373, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 374, "extrude");
        atStep(st, 375);
        cqUnion(c, st, 375, 369, {"toUnion": {"wp": 374}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 375, "union");
        atStep(st, 376);
        cqWorkplaneNew(c, st, 376, {"plane": {"origin": [4.999999500000001, -8.660253171818983, 6.123233383413369e-16], "xDir": [-0.8660254037844386, -0.5000000000000001, 0], "zDir": [0.5000000000000001, -0.8660254037844386, 6.123233995736766e-17]}, "obj": undefined});
        atStep(st, 377);
        cqTransformed(c, st, 377, 376, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 378);
        cqCenter(c, st, 378, 377, {"x": 1, "y": 0});
        atStep(st, 379);
        cqRect(c, st, 379, 378, {"xLen": 1.5, "yLen": 0.3, "centered": true, "forConstruction": false});
        atStep(st, 380);
        cqExtrude(c, st, 380, 379, {"until": 2.000001, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 380, "extrude");
        atStep(st, 381);
        cqUnion(c, st, 381, 375, {"toUnion": {"wp": 380}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 381, "union");
        atStep(st, 382);
        cqUnion(c, st, 382, 352, {"toUnion": {"wp": 381}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 382, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 382}), {"i": 2723});
    } catch (e) {
        return {"i": 2723, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_2723(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c2723"] = r; }
    return out;
}
