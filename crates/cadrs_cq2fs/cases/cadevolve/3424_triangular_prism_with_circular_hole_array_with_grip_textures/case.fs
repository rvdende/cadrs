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

const case_3424 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqPolyline(c, st, 2, 1, {"listOfXYTuple": [[-5, -2.8867513459481287], [5, -2.8867513459481287], [0, 5.773502691896256]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 3);
        cqClose(c, st, 3, 2, {});
        atStep(st, 4);
        cqExtrude(c, st, 4, 3, {"until": 25, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 4, "extrude");
        atStep(st, 5);
        cqWorkplaneNew(c, st, 5, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 6);
        cqPushPoints(c, st, 6, 5, {"pntList": [[0.5, 0.5]]});
        atStep(st, 7);
        cqSphere(c, st, 7, 6, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 7, "sphere");
        atStep(st, 8);
        cqUnion(c, st, 8, 4, {"toUnion": {"wp": 7}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 8, "union");
        atStep(st, 9);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 10);
        cqPushPoints(c, st, 10, 9, {"pntList": [[2.75, 0.5]]});
        atStep(st, 11);
        cqSphere(c, st, 11, 10, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 11, "sphere");
        atStep(st, 12);
        cqUnion(c, st, 12, 8, {"toUnion": {"wp": 11}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 12, "union");
        atStep(st, 13);
        cqWorkplaneNew(c, st, 13, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 14);
        cqPushPoints(c, st, 14, 13, {"pntList": [[5, 0.5]]});
        atStep(st, 15);
        cqSphere(c, st, 15, 14, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 15, "sphere");
        atStep(st, 16);
        cqUnion(c, st, 16, 12, {"toUnion": {"wp": 15}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 16, "union");
        atStep(st, 17);
        cqWorkplaneNew(c, st, 17, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 18);
        cqPushPoints(c, st, 18, 17, {"pntList": [[7.25, 0.5]]});
        atStep(st, 19);
        cqSphere(c, st, 19, 18, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 19, "sphere");
        atStep(st, 20);
        cqUnion(c, st, 20, 16, {"toUnion": {"wp": 19}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 20, "union");
        atStep(st, 21);
        cqWorkplaneNew(c, st, 21, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 22);
        cqPushPoints(c, st, 22, 21, {"pntList": [[9.5, 0.5]]});
        atStep(st, 23);
        cqSphere(c, st, 23, 22, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 23, "sphere");
        atStep(st, 24);
        cqUnion(c, st, 24, 20, {"toUnion": {"wp": 23}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 24, "union");
        atStep(st, 25);
        cqWorkplaneNew(c, st, 25, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 26);
        cqPushPoints(c, st, 26, 25, {"pntList": [[0.5, 2.6818181818181817]]});
        atStep(st, 27);
        cqSphere(c, st, 27, 26, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 27, "sphere");
        atStep(st, 28);
        cqUnion(c, st, 28, 24, {"toUnion": {"wp": 27}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 28, "union");
        atStep(st, 29);
        cqWorkplaneNew(c, st, 29, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 30);
        cqPushPoints(c, st, 30, 29, {"pntList": [[2.75, 2.6818181818181817]]});
        atStep(st, 31);
        cqSphere(c, st, 31, 30, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 31, "sphere");
        atStep(st, 32);
        cqUnion(c, st, 32, 28, {"toUnion": {"wp": 31}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 32, "union");
        atStep(st, 33);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 34);
        cqPushPoints(c, st, 34, 33, {"pntList": [[5, 2.6818181818181817]]});
        atStep(st, 35);
        cqSphere(c, st, 35, 34, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 35, "sphere");
        atStep(st, 36);
        cqUnion(c, st, 36, 32, {"toUnion": {"wp": 35}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 36, "union");
        atStep(st, 37);
        cqWorkplaneNew(c, st, 37, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 38);
        cqPushPoints(c, st, 38, 37, {"pntList": [[7.25, 2.6818181818181817]]});
        atStep(st, 39);
        cqSphere(c, st, 39, 38, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 39, "sphere");
        atStep(st, 40);
        cqUnion(c, st, 40, 36, {"toUnion": {"wp": 39}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 40, "union");
        atStep(st, 41);
        cqWorkplaneNew(c, st, 41, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 42);
        cqPushPoints(c, st, 42, 41, {"pntList": [[9.5, 2.6818181818181817]]});
        atStep(st, 43);
        cqSphere(c, st, 43, 42, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 43, "sphere");
        atStep(st, 44);
        cqUnion(c, st, 44, 40, {"toUnion": {"wp": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 44, "union");
        atStep(st, 45);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 46);
        cqPushPoints(c, st, 46, 45, {"pntList": [[0.5, 4.863636363636363]]});
        atStep(st, 47);
        cqSphere(c, st, 47, 46, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 47, "sphere");
        atStep(st, 48);
        cqUnion(c, st, 48, 44, {"toUnion": {"wp": 47}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 48, "union");
        atStep(st, 49);
        cqWorkplaneNew(c, st, 49, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 50);
        cqPushPoints(c, st, 50, 49, {"pntList": [[2.75, 4.863636363636363]]});
        atStep(st, 51);
        cqSphere(c, st, 51, 50, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 51, "sphere");
        atStep(st, 52);
        cqUnion(c, st, 52, 48, {"toUnion": {"wp": 51}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 52, "union");
        atStep(st, 53);
        cqWorkplaneNew(c, st, 53, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 54);
        cqPushPoints(c, st, 54, 53, {"pntList": [[5, 4.863636363636363]]});
        atStep(st, 55);
        cqSphere(c, st, 55, 54, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 55, "sphere");
        atStep(st, 56);
        cqUnion(c, st, 56, 52, {"toUnion": {"wp": 55}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 56, "union");
        atStep(st, 57);
        cqWorkplaneNew(c, st, 57, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 58);
        cqPushPoints(c, st, 58, 57, {"pntList": [[7.25, 4.863636363636363]]});
        atStep(st, 59);
        cqSphere(c, st, 59, 58, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 59, "sphere");
        atStep(st, 60);
        cqUnion(c, st, 60, 56, {"toUnion": {"wp": 59}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 60, "union");
        atStep(st, 61);
        cqWorkplaneNew(c, st, 61, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 62);
        cqPushPoints(c, st, 62, 61, {"pntList": [[9.5, 4.863636363636363]]});
        atStep(st, 63);
        cqSphere(c, st, 63, 62, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 63, "sphere");
        atStep(st, 64);
        cqUnion(c, st, 64, 60, {"toUnion": {"wp": 63}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 64, "union");
        atStep(st, 65);
        cqWorkplaneNew(c, st, 65, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 66);
        cqPushPoints(c, st, 66, 65, {"pntList": [[0.5, 7.045454545454545]]});
        atStep(st, 67);
        cqSphere(c, st, 67, 66, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 67, "sphere");
        atStep(st, 68);
        cqUnion(c, st, 68, 64, {"toUnion": {"wp": 67}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 68, "union");
        atStep(st, 69);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 70);
        cqPushPoints(c, st, 70, 69, {"pntList": [[2.75, 7.045454545454545]]});
        atStep(st, 71);
        cqSphere(c, st, 71, 70, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 71, "sphere");
        atStep(st, 72);
        cqUnion(c, st, 72, 68, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 74);
        cqPushPoints(c, st, 74, 73, {"pntList": [[5, 7.045454545454545]]});
        atStep(st, 75);
        cqSphere(c, st, 75, 74, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 75, "sphere");
        atStep(st, 76);
        cqUnion(c, st, 76, 72, {"toUnion": {"wp": 75}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 76, "union");
        atStep(st, 77);
        cqWorkplaneNew(c, st, 77, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 78);
        cqPushPoints(c, st, 78, 77, {"pntList": [[7.25, 7.045454545454545]]});
        atStep(st, 79);
        cqSphere(c, st, 79, 78, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 79, "sphere");
        atStep(st, 80);
        cqUnion(c, st, 80, 76, {"toUnion": {"wp": 79}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 80, "union");
        atStep(st, 81);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 82);
        cqPushPoints(c, st, 82, 81, {"pntList": [[9.5, 7.045454545454545]]});
        atStep(st, 83);
        cqSphere(c, st, 83, 82, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 83, "sphere");
        atStep(st, 84);
        cqUnion(c, st, 84, 80, {"toUnion": {"wp": 83}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 84, "union");
        atStep(st, 85);
        cqWorkplaneNew(c, st, 85, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 86);
        cqPushPoints(c, st, 86, 85, {"pntList": [[0.5, 9.227272727272728]]});
        atStep(st, 87);
        cqSphere(c, st, 87, 86, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 87, "sphere");
        atStep(st, 88);
        cqUnion(c, st, 88, 84, {"toUnion": {"wp": 87}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 88, "union");
        atStep(st, 89);
        cqWorkplaneNew(c, st, 89, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 90);
        cqPushPoints(c, st, 90, 89, {"pntList": [[2.75, 9.227272727272728]]});
        atStep(st, 91);
        cqSphere(c, st, 91, 90, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 91, "sphere");
        atStep(st, 92);
        cqUnion(c, st, 92, 88, {"toUnion": {"wp": 91}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 92, "union");
        atStep(st, 93);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 94);
        cqPushPoints(c, st, 94, 93, {"pntList": [[5, 9.227272727272728]]});
        atStep(st, 95);
        cqSphere(c, st, 95, 94, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 95, "sphere");
        atStep(st, 96);
        cqUnion(c, st, 96, 92, {"toUnion": {"wp": 95}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 96, "union");
        atStep(st, 97);
        cqWorkplaneNew(c, st, 97, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 98);
        cqPushPoints(c, st, 98, 97, {"pntList": [[7.25, 9.227272727272728]]});
        atStep(st, 99);
        cqSphere(c, st, 99, 98, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 99, "sphere");
        atStep(st, 100);
        cqUnion(c, st, 100, 96, {"toUnion": {"wp": 99}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 100, "union");
        atStep(st, 101);
        cqWorkplaneNew(c, st, 101, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 102);
        cqPushPoints(c, st, 102, 101, {"pntList": [[9.5, 9.227272727272728]]});
        atStep(st, 103);
        cqSphere(c, st, 103, 102, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 103, "sphere");
        atStep(st, 104);
        cqUnion(c, st, 104, 100, {"toUnion": {"wp": 103}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 104, "union");
        atStep(st, 105);
        cqWorkplaneNew(c, st, 105, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 106);
        cqPushPoints(c, st, 106, 105, {"pntList": [[0.5, 11.409090909090908]]});
        atStep(st, 107);
        cqSphere(c, st, 107, 106, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 107, "sphere");
        atStep(st, 108);
        cqUnion(c, st, 108, 104, {"toUnion": {"wp": 107}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 108, "union");
        atStep(st, 109);
        cqWorkplaneNew(c, st, 109, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 110);
        cqPushPoints(c, st, 110, 109, {"pntList": [[2.75, 11.409090909090908]]});
        atStep(st, 111);
        cqSphere(c, st, 111, 110, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 111, "sphere");
        atStep(st, 112);
        cqUnion(c, st, 112, 108, {"toUnion": {"wp": 111}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 112, "union");
        atStep(st, 113);
        cqWorkplaneNew(c, st, 113, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 114);
        cqPushPoints(c, st, 114, 113, {"pntList": [[5, 11.409090909090908]]});
        atStep(st, 115);
        cqSphere(c, st, 115, 114, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 115, "sphere");
        atStep(st, 116);
        cqUnion(c, st, 116, 112, {"toUnion": {"wp": 115}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 116, "union");
        atStep(st, 117);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 118);
        cqPushPoints(c, st, 118, 117, {"pntList": [[7.25, 11.409090909090908]]});
        atStep(st, 119);
        cqSphere(c, st, 119, 118, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 119, "sphere");
        atStep(st, 120);
        cqUnion(c, st, 120, 116, {"toUnion": {"wp": 119}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 120, "union");
        atStep(st, 121);
        cqWorkplaneNew(c, st, 121, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 122);
        cqPushPoints(c, st, 122, 121, {"pntList": [[9.5, 11.409090909090908]]});
        atStep(st, 123);
        cqSphere(c, st, 123, 122, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 123, "sphere");
        atStep(st, 124);
        cqUnion(c, st, 124, 120, {"toUnion": {"wp": 123}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 124, "union");
        atStep(st, 125);
        cqWorkplaneNew(c, st, 125, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 126);
        cqPushPoints(c, st, 126, 125, {"pntList": [[0.5, 13.59090909090909]]});
        atStep(st, 127);
        cqSphere(c, st, 127, 126, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 127, "sphere");
        atStep(st, 128);
        cqUnion(c, st, 128, 124, {"toUnion": {"wp": 127}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 128, "union");
        atStep(st, 129);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 130);
        cqPushPoints(c, st, 130, 129, {"pntList": [[2.75, 13.59090909090909]]});
        atStep(st, 131);
        cqSphere(c, st, 131, 130, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 131, "sphere");
        atStep(st, 132);
        cqUnion(c, st, 132, 128, {"toUnion": {"wp": 131}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 132, "union");
        atStep(st, 133);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 134);
        cqPushPoints(c, st, 134, 133, {"pntList": [[5, 13.59090909090909]]});
        atStep(st, 135);
        cqSphere(c, st, 135, 134, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 135, "sphere");
        atStep(st, 136);
        cqUnion(c, st, 136, 132, {"toUnion": {"wp": 135}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 136, "union");
        atStep(st, 137);
        cqWorkplaneNew(c, st, 137, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 138);
        cqPushPoints(c, st, 138, 137, {"pntList": [[7.25, 13.59090909090909]]});
        atStep(st, 139);
        cqSphere(c, st, 139, 138, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 139, "sphere");
        atStep(st, 140);
        cqUnion(c, st, 140, 136, {"toUnion": {"wp": 139}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 140, "union");
        atStep(st, 141);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 142);
        cqPushPoints(c, st, 142, 141, {"pntList": [[9.5, 13.59090909090909]]});
        atStep(st, 143);
        cqSphere(c, st, 143, 142, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 143, "sphere");
        atStep(st, 144);
        cqUnion(c, st, 144, 140, {"toUnion": {"wp": 143}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 144, "union");
        atStep(st, 145);
        cqWorkplaneNew(c, st, 145, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 146);
        cqPushPoints(c, st, 146, 145, {"pntList": [[0.5, 15.772727272727272]]});
        atStep(st, 147);
        cqSphere(c, st, 147, 146, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 147, "sphere");
        atStep(st, 148);
        cqUnion(c, st, 148, 144, {"toUnion": {"wp": 147}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 148, "union");
        atStep(st, 149);
        cqWorkplaneNew(c, st, 149, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 150);
        cqPushPoints(c, st, 150, 149, {"pntList": [[2.75, 15.772727272727272]]});
        atStep(st, 151);
        cqSphere(c, st, 151, 150, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 151, "sphere");
        atStep(st, 152);
        cqUnion(c, st, 152, 148, {"toUnion": {"wp": 151}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 152, "union");
        atStep(st, 153);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 154);
        cqPushPoints(c, st, 154, 153, {"pntList": [[5, 15.772727272727272]]});
        atStep(st, 155);
        cqSphere(c, st, 155, 154, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 155, "sphere");
        atStep(st, 156);
        cqUnion(c, st, 156, 152, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        atStep(st, 157);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 158);
        cqPushPoints(c, st, 158, 157, {"pntList": [[7.25, 15.772727272727272]]});
        atStep(st, 159);
        cqSphere(c, st, 159, 158, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 159, "sphere");
        atStep(st, 160);
        cqUnion(c, st, 160, 156, {"toUnion": {"wp": 159}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 160, "union");
        atStep(st, 161);
        cqWorkplaneNew(c, st, 161, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 162);
        cqPushPoints(c, st, 162, 161, {"pntList": [[9.5, 15.772727272727272]]});
        atStep(st, 163);
        cqSphere(c, st, 163, 162, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 163, "sphere");
        atStep(st, 164);
        cqUnion(c, st, 164, 160, {"toUnion": {"wp": 163}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 164, "union");
        atStep(st, 165);
        cqWorkplaneNew(c, st, 165, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 166);
        cqPushPoints(c, st, 166, 165, {"pntList": [[0.5, 17.954545454545453]]});
        atStep(st, 167);
        cqSphere(c, st, 167, 166, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 167, "sphere");
        atStep(st, 168);
        cqUnion(c, st, 168, 164, {"toUnion": {"wp": 167}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 168, "union");
        atStep(st, 169);
        cqWorkplaneNew(c, st, 169, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 170);
        cqPushPoints(c, st, 170, 169, {"pntList": [[2.75, 17.954545454545453]]});
        atStep(st, 171);
        cqSphere(c, st, 171, 170, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 171, "sphere");
        atStep(st, 172);
        cqUnion(c, st, 172, 168, {"toUnion": {"wp": 171}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 172, "union");
        atStep(st, 173);
        cqWorkplaneNew(c, st, 173, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 174);
        cqPushPoints(c, st, 174, 173, {"pntList": [[5, 17.954545454545453]]});
        atStep(st, 175);
        cqSphere(c, st, 175, 174, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 175, "sphere");
        atStep(st, 176);
        cqUnion(c, st, 176, 172, {"toUnion": {"wp": 175}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 176, "union");
        atStep(st, 177);
        cqWorkplaneNew(c, st, 177, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 178);
        cqPushPoints(c, st, 178, 177, {"pntList": [[7.25, 17.954545454545453]]});
        atStep(st, 179);
        cqSphere(c, st, 179, 178, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 179, "sphere");
        atStep(st, 180);
        cqUnion(c, st, 180, 176, {"toUnion": {"wp": 179}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 180, "union");
        atStep(st, 181);
        cqWorkplaneNew(c, st, 181, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 182);
        cqPushPoints(c, st, 182, 181, {"pntList": [[9.5, 17.954545454545453]]});
        atStep(st, 183);
        cqSphere(c, st, 183, 182, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 183, "sphere");
        atStep(st, 184);
        cqUnion(c, st, 184, 180, {"toUnion": {"wp": 183}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 184, "union");
        atStep(st, 185);
        cqWorkplaneNew(c, st, 185, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 186);
        cqPushPoints(c, st, 186, 185, {"pntList": [[0.5, 20.136363636363633]]});
        atStep(st, 187);
        cqSphere(c, st, 187, 186, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 187, "sphere");
        atStep(st, 188);
        cqUnion(c, st, 188, 184, {"toUnion": {"wp": 187}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 188, "union");
        atStep(st, 189);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 190);
        cqPushPoints(c, st, 190, 189, {"pntList": [[2.75, 20.136363636363633]]});
        atStep(st, 191);
        cqSphere(c, st, 191, 190, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 191, "sphere");
        atStep(st, 192);
        cqUnion(c, st, 192, 188, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        atStep(st, 193);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 194);
        cqPushPoints(c, st, 194, 193, {"pntList": [[5, 20.136363636363633]]});
        atStep(st, 195);
        cqSphere(c, st, 195, 194, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 195, "sphere");
        atStep(st, 196);
        cqUnion(c, st, 196, 192, {"toUnion": {"wp": 195}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 196, "union");
        atStep(st, 197);
        cqWorkplaneNew(c, st, 197, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 198);
        cqPushPoints(c, st, 198, 197, {"pntList": [[7.25, 20.136363636363633]]});
        atStep(st, 199);
        cqSphere(c, st, 199, 198, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 199, "sphere");
        atStep(st, 200);
        cqUnion(c, st, 200, 196, {"toUnion": {"wp": 199}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 200, "union");
        atStep(st, 201);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 202);
        cqPushPoints(c, st, 202, 201, {"pntList": [[9.5, 20.136363636363633]]});
        atStep(st, 203);
        cqSphere(c, st, 203, 202, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 203, "sphere");
        atStep(st, 204);
        cqUnion(c, st, 204, 200, {"toUnion": {"wp": 203}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 204, "union");
        atStep(st, 205);
        cqWorkplaneNew(c, st, 205, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 206);
        cqPushPoints(c, st, 206, 205, {"pntList": [[0.5, 22.318181818181817]]});
        atStep(st, 207);
        cqSphere(c, st, 207, 206, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 207, "sphere");
        atStep(st, 208);
        cqUnion(c, st, 208, 204, {"toUnion": {"wp": 207}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 208, "union");
        atStep(st, 209);
        cqWorkplaneNew(c, st, 209, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 210);
        cqPushPoints(c, st, 210, 209, {"pntList": [[2.75, 22.318181818181817]]});
        atStep(st, 211);
        cqSphere(c, st, 211, 210, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 211, "sphere");
        atStep(st, 212);
        cqUnion(c, st, 212, 208, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 214);
        cqPushPoints(c, st, 214, 213, {"pntList": [[5, 22.318181818181817]]});
        atStep(st, 215);
        cqSphere(c, st, 215, 214, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 215, "sphere");
        atStep(st, 216);
        cqUnion(c, st, 216, 212, {"toUnion": {"wp": 215}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 216, "union");
        atStep(st, 217);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 218);
        cqPushPoints(c, st, 218, 217, {"pntList": [[7.25, 22.318181818181817]]});
        atStep(st, 219);
        cqSphere(c, st, 219, 218, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 219, "sphere");
        atStep(st, 220);
        cqUnion(c, st, 220, 216, {"toUnion": {"wp": 219}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 220, "union");
        atStep(st, 221);
        cqWorkplaneNew(c, st, 221, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 222);
        cqPushPoints(c, st, 222, 221, {"pntList": [[9.5, 22.318181818181817]]});
        atStep(st, 223);
        cqSphere(c, st, 223, 222, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 223, "sphere");
        atStep(st, 224);
        cqUnion(c, st, 224, 220, {"toUnion": {"wp": 223}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 224, "union");
        atStep(st, 225);
        cqWorkplaneNew(c, st, 225, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 226);
        cqPushPoints(c, st, 226, 225, {"pntList": [[0.5, 24.5]]});
        atStep(st, 227);
        cqSphere(c, st, 227, 226, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 227, "sphere");
        atStep(st, 228);
        cqUnion(c, st, 228, 224, {"toUnion": {"wp": 227}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 228, "union");
        atStep(st, 229);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 230);
        cqPushPoints(c, st, 230, 229, {"pntList": [[2.75, 24.5]]});
        atStep(st, 231);
        cqSphere(c, st, 231, 230, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 231, "sphere");
        atStep(st, 232);
        cqUnion(c, st, 232, 228, {"toUnion": {"wp": 231}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 232, "union");
        atStep(st, 233);
        cqWorkplaneNew(c, st, 233, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 234);
        cqPushPoints(c, st, 234, 233, {"pntList": [[5, 24.5]]});
        atStep(st, 235);
        cqSphere(c, st, 235, 234, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 235, "sphere");
        atStep(st, 236);
        cqUnion(c, st, 236, 232, {"toUnion": {"wp": 235}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 236, "union");
        atStep(st, 237);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 238);
        cqPushPoints(c, st, 238, 237, {"pntList": [[7.25, 24.5]]});
        atStep(st, 239);
        cqSphere(c, st, 239, 238, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 239, "sphere");
        atStep(st, 240);
        cqUnion(c, st, 240, 236, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        atStep(st, 241);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 242);
        cqPushPoints(c, st, 242, 241, {"pntList": [[9.5, 24.5]]});
        atStep(st, 243);
        cqSphere(c, st, 243, 242, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 243, "sphere");
        atStep(st, 244);
        cqUnion(c, st, 244, 240, {"toUnion": {"wp": 243}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 244, "union");
        atStep(st, 245);
        cqWorkplaneNew(c, st, 245, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 246);
        cqPushPoints(c, st, 246, 245, {"pntList": [[0.5, 0.5]]});
        atStep(st, 247);
        cqSphere(c, st, 247, 246, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 247, "sphere");
        atStep(st, 248);
        cqUnion(c, st, 248, 244, {"toUnion": {"wp": 247}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 248, "union");
        atStep(st, 249);
        cqWorkplaneNew(c, st, 249, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 250);
        cqPushPoints(c, st, 250, 249, {"pntList": [[2.75, 0.5]]});
        atStep(st, 251);
        cqSphere(c, st, 251, 250, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 251, "sphere");
        atStep(st, 252);
        cqUnion(c, st, 252, 248, {"toUnion": {"wp": 251}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 252, "union");
        atStep(st, 253);
        cqWorkplaneNew(c, st, 253, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 254);
        cqPushPoints(c, st, 254, 253, {"pntList": [[5, 0.5]]});
        atStep(st, 255);
        cqSphere(c, st, 255, 254, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 255, "sphere");
        atStep(st, 256);
        cqUnion(c, st, 256, 252, {"toUnion": {"wp": 255}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 256, "union");
        atStep(st, 257);
        cqWorkplaneNew(c, st, 257, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 258);
        cqPushPoints(c, st, 258, 257, {"pntList": [[7.25, 0.5]]});
        atStep(st, 259);
        cqSphere(c, st, 259, 258, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 259, "sphere");
        atStep(st, 260);
        cqUnion(c, st, 260, 256, {"toUnion": {"wp": 259}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 260, "union");
        atStep(st, 261);
        cqWorkplaneNew(c, st, 261, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 262);
        cqPushPoints(c, st, 262, 261, {"pntList": [[9.5, 0.5]]});
        atStep(st, 263);
        cqSphere(c, st, 263, 262, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 263, "sphere");
        atStep(st, 264);
        cqUnion(c, st, 264, 260, {"toUnion": {"wp": 263}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 264, "union");
        atStep(st, 265);
        cqWorkplaneNew(c, st, 265, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 266);
        cqPushPoints(c, st, 266, 265, {"pntList": [[0.5, 2.6818181818181817]]});
        atStep(st, 267);
        cqSphere(c, st, 267, 266, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 267, "sphere");
        atStep(st, 268);
        cqUnion(c, st, 268, 264, {"toUnion": {"wp": 267}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 268, "union");
        atStep(st, 269);
        cqWorkplaneNew(c, st, 269, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 270);
        cqPushPoints(c, st, 270, 269, {"pntList": [[2.75, 2.6818181818181817]]});
        atStep(st, 271);
        cqSphere(c, st, 271, 270, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 271, "sphere");
        atStep(st, 272);
        cqUnion(c, st, 272, 268, {"toUnion": {"wp": 271}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 272, "union");
        atStep(st, 273);
        cqWorkplaneNew(c, st, 273, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 274);
        cqPushPoints(c, st, 274, 273, {"pntList": [[5, 2.6818181818181817]]});
        atStep(st, 275);
        cqSphere(c, st, 275, 274, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 275, "sphere");
        atStep(st, 276);
        cqUnion(c, st, 276, 272, {"toUnion": {"wp": 275}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 276, "union");
        atStep(st, 277);
        cqWorkplaneNew(c, st, 277, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 278);
        cqPushPoints(c, st, 278, 277, {"pntList": [[7.25, 2.6818181818181817]]});
        atStep(st, 279);
        cqSphere(c, st, 279, 278, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 279, "sphere");
        atStep(st, 280);
        cqUnion(c, st, 280, 276, {"toUnion": {"wp": 279}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 280, "union");
        atStep(st, 281);
        cqWorkplaneNew(c, st, 281, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 282);
        cqPushPoints(c, st, 282, 281, {"pntList": [[9.5, 2.6818181818181817]]});
        atStep(st, 283);
        cqSphere(c, st, 283, 282, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 283, "sphere");
        atStep(st, 284);
        cqUnion(c, st, 284, 280, {"toUnion": {"wp": 283}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 284, "union");
        atStep(st, 285);
        cqWorkplaneNew(c, st, 285, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 286);
        cqPushPoints(c, st, 286, 285, {"pntList": [[0.5, 4.863636363636363]]});
        atStep(st, 287);
        cqSphere(c, st, 287, 286, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 287, "sphere");
        atStep(st, 288);
        cqUnion(c, st, 288, 284, {"toUnion": {"wp": 287}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 288, "union");
        atStep(st, 289);
        cqWorkplaneNew(c, st, 289, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 290);
        cqPushPoints(c, st, 290, 289, {"pntList": [[2.75, 4.863636363636363]]});
        atStep(st, 291);
        cqSphere(c, st, 291, 290, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 291, "sphere");
        atStep(st, 292);
        cqUnion(c, st, 292, 288, {"toUnion": {"wp": 291}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 292, "union");
        atStep(st, 293);
        cqWorkplaneNew(c, st, 293, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 294);
        cqPushPoints(c, st, 294, 293, {"pntList": [[5, 4.863636363636363]]});
        atStep(st, 295);
        cqSphere(c, st, 295, 294, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 295, "sphere");
        atStep(st, 296);
        cqUnion(c, st, 296, 292, {"toUnion": {"wp": 295}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 296, "union");
        atStep(st, 297);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 298);
        cqPushPoints(c, st, 298, 297, {"pntList": [[7.25, 4.863636363636363]]});
        atStep(st, 299);
        cqSphere(c, st, 299, 298, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 299, "sphere");
        atStep(st, 300);
        cqUnion(c, st, 300, 296, {"toUnion": {"wp": 299}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 300, "union");
        atStep(st, 301);
        cqWorkplaneNew(c, st, 301, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 302);
        cqPushPoints(c, st, 302, 301, {"pntList": [[9.5, 4.863636363636363]]});
        atStep(st, 303);
        cqSphere(c, st, 303, 302, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 303, "sphere");
        atStep(st, 304);
        cqUnion(c, st, 304, 300, {"toUnion": {"wp": 303}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 304, "union");
        atStep(st, 305);
        cqWorkplaneNew(c, st, 305, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 306);
        cqPushPoints(c, st, 306, 305, {"pntList": [[0.5, 7.045454545454545]]});
        atStep(st, 307);
        cqSphere(c, st, 307, 306, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 307, "sphere");
        atStep(st, 308);
        cqUnion(c, st, 308, 304, {"toUnion": {"wp": 307}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 308, "union");
        atStep(st, 309);
        cqWorkplaneNew(c, st, 309, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 310);
        cqPushPoints(c, st, 310, 309, {"pntList": [[2.75, 7.045454545454545]]});
        atStep(st, 311);
        cqSphere(c, st, 311, 310, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 311, "sphere");
        atStep(st, 312);
        cqUnion(c, st, 312, 308, {"toUnion": {"wp": 311}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 312, "union");
        atStep(st, 313);
        cqWorkplaneNew(c, st, 313, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 314);
        cqPushPoints(c, st, 314, 313, {"pntList": [[5, 7.045454545454545]]});
        atStep(st, 315);
        cqSphere(c, st, 315, 314, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 315, "sphere");
        atStep(st, 316);
        cqUnion(c, st, 316, 312, {"toUnion": {"wp": 315}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 316, "union");
        atStep(st, 317);
        cqWorkplaneNew(c, st, 317, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 318);
        cqPushPoints(c, st, 318, 317, {"pntList": [[7.25, 7.045454545454545]]});
        atStep(st, 319);
        cqSphere(c, st, 319, 318, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 319, "sphere");
        atStep(st, 320);
        cqUnion(c, st, 320, 316, {"toUnion": {"wp": 319}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 320, "union");
        atStep(st, 321);
        cqWorkplaneNew(c, st, 321, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 322);
        cqPushPoints(c, st, 322, 321, {"pntList": [[9.5, 7.045454545454545]]});
        atStep(st, 323);
        cqSphere(c, st, 323, 322, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 323, "sphere");
        atStep(st, 324);
        cqUnion(c, st, 324, 320, {"toUnion": {"wp": 323}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 324, "union");
        atStep(st, 325);
        cqWorkplaneNew(c, st, 325, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 326);
        cqPushPoints(c, st, 326, 325, {"pntList": [[0.5, 9.227272727272728]]});
        atStep(st, 327);
        cqSphere(c, st, 327, 326, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 327, "sphere");
        atStep(st, 328);
        cqUnion(c, st, 328, 324, {"toUnion": {"wp": 327}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 328, "union");
        atStep(st, 329);
        cqWorkplaneNew(c, st, 329, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 330);
        cqPushPoints(c, st, 330, 329, {"pntList": [[2.75, 9.227272727272728]]});
        atStep(st, 331);
        cqSphere(c, st, 331, 330, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 331, "sphere");
        atStep(st, 332);
        cqUnion(c, st, 332, 328, {"toUnion": {"wp": 331}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 332, "union");
        atStep(st, 333);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 334);
        cqPushPoints(c, st, 334, 333, {"pntList": [[5, 9.227272727272728]]});
        atStep(st, 335);
        cqSphere(c, st, 335, 334, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 335, "sphere");
        atStep(st, 336);
        cqUnion(c, st, 336, 332, {"toUnion": {"wp": 335}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 336, "union");
        atStep(st, 337);
        cqWorkplaneNew(c, st, 337, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 338);
        cqPushPoints(c, st, 338, 337, {"pntList": [[7.25, 9.227272727272728]]});
        atStep(st, 339);
        cqSphere(c, st, 339, 338, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 339, "sphere");
        atStep(st, 340);
        cqUnion(c, st, 340, 336, {"toUnion": {"wp": 339}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 340, "union");
        atStep(st, 341);
        cqWorkplaneNew(c, st, 341, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 342);
        cqPushPoints(c, st, 342, 341, {"pntList": [[9.5, 9.227272727272728]]});
        atStep(st, 343);
        cqSphere(c, st, 343, 342, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 343, "sphere");
        atStep(st, 344);
        cqUnion(c, st, 344, 340, {"toUnion": {"wp": 343}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 344, "union");
        atStep(st, 345);
        cqWorkplaneNew(c, st, 345, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 346);
        cqPushPoints(c, st, 346, 345, {"pntList": [[0.5, 11.409090909090908]]});
        atStep(st, 347);
        cqSphere(c, st, 347, 346, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 347, "sphere");
        atStep(st, 348);
        cqUnion(c, st, 348, 344, {"toUnion": {"wp": 347}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 348, "union");
        atStep(st, 349);
        cqWorkplaneNew(c, st, 349, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 350);
        cqPushPoints(c, st, 350, 349, {"pntList": [[2.75, 11.409090909090908]]});
        atStep(st, 351);
        cqSphere(c, st, 351, 350, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 351, "sphere");
        atStep(st, 352);
        cqUnion(c, st, 352, 348, {"toUnion": {"wp": 351}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 352, "union");
        atStep(st, 353);
        cqWorkplaneNew(c, st, 353, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 354);
        cqPushPoints(c, st, 354, 353, {"pntList": [[5, 11.409090909090908]]});
        atStep(st, 355);
        cqSphere(c, st, 355, 354, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 355, "sphere");
        atStep(st, 356);
        cqUnion(c, st, 356, 352, {"toUnion": {"wp": 355}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 356, "union");
        atStep(st, 357);
        cqWorkplaneNew(c, st, 357, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 358);
        cqPushPoints(c, st, 358, 357, {"pntList": [[7.25, 11.409090909090908]]});
        atStep(st, 359);
        cqSphere(c, st, 359, 358, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 359, "sphere");
        atStep(st, 360);
        cqUnion(c, st, 360, 356, {"toUnion": {"wp": 359}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 360, "union");
        atStep(st, 361);
        cqWorkplaneNew(c, st, 361, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 362);
        cqPushPoints(c, st, 362, 361, {"pntList": [[9.5, 11.409090909090908]]});
        atStep(st, 363);
        cqSphere(c, st, 363, 362, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 363, "sphere");
        atStep(st, 364);
        cqUnion(c, st, 364, 360, {"toUnion": {"wp": 363}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 364, "union");
        atStep(st, 365);
        cqWorkplaneNew(c, st, 365, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 366);
        cqPushPoints(c, st, 366, 365, {"pntList": [[0.5, 13.59090909090909]]});
        atStep(st, 367);
        cqSphere(c, st, 367, 366, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 367, "sphere");
        atStep(st, 368);
        cqUnion(c, st, 368, 364, {"toUnion": {"wp": 367}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 368, "union");
        atStep(st, 369);
        cqWorkplaneNew(c, st, 369, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 370);
        cqPushPoints(c, st, 370, 369, {"pntList": [[2.75, 13.59090909090909]]});
        atStep(st, 371);
        cqSphere(c, st, 371, 370, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 371, "sphere");
        atStep(st, 372);
        cqUnion(c, st, 372, 368, {"toUnion": {"wp": 371}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 372, "union");
        atStep(st, 373);
        cqWorkplaneNew(c, st, 373, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 374);
        cqPushPoints(c, st, 374, 373, {"pntList": [[5, 13.59090909090909]]});
        atStep(st, 375);
        cqSphere(c, st, 375, 374, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 375, "sphere");
        atStep(st, 376);
        cqUnion(c, st, 376, 372, {"toUnion": {"wp": 375}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 376, "union");
        atStep(st, 377);
        cqWorkplaneNew(c, st, 377, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 378);
        cqPushPoints(c, st, 378, 377, {"pntList": [[7.25, 13.59090909090909]]});
        atStep(st, 379);
        cqSphere(c, st, 379, 378, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 379, "sphere");
        atStep(st, 380);
        cqUnion(c, st, 380, 376, {"toUnion": {"wp": 379}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 380, "union");
        atStep(st, 381);
        cqWorkplaneNew(c, st, 381, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 382);
        cqPushPoints(c, st, 382, 381, {"pntList": [[9.5, 13.59090909090909]]});
        atStep(st, 383);
        cqSphere(c, st, 383, 382, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 383, "sphere");
        atStep(st, 384);
        cqUnion(c, st, 384, 380, {"toUnion": {"wp": 383}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 384, "union");
        atStep(st, 385);
        cqWorkplaneNew(c, st, 385, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 386);
        cqPushPoints(c, st, 386, 385, {"pntList": [[0.5, 15.772727272727272]]});
        atStep(st, 387);
        cqSphere(c, st, 387, 386, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 387, "sphere");
        atStep(st, 388);
        cqUnion(c, st, 388, 384, {"toUnion": {"wp": 387}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 388, "union");
        atStep(st, 389);
        cqWorkplaneNew(c, st, 389, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 390);
        cqPushPoints(c, st, 390, 389, {"pntList": [[2.75, 15.772727272727272]]});
        atStep(st, 391);
        cqSphere(c, st, 391, 390, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 391, "sphere");
        atStep(st, 392);
        cqUnion(c, st, 392, 388, {"toUnion": {"wp": 391}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 392, "union");
        atStep(st, 393);
        cqWorkplaneNew(c, st, 393, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 394);
        cqPushPoints(c, st, 394, 393, {"pntList": [[5, 15.772727272727272]]});
        atStep(st, 395);
        cqSphere(c, st, 395, 394, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 395, "sphere");
        atStep(st, 396);
        cqUnion(c, st, 396, 392, {"toUnion": {"wp": 395}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 396, "union");
        atStep(st, 397);
        cqWorkplaneNew(c, st, 397, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 398);
        cqPushPoints(c, st, 398, 397, {"pntList": [[7.25, 15.772727272727272]]});
        atStep(st, 399);
        cqSphere(c, st, 399, 398, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 399, "sphere");
        atStep(st, 400);
        cqUnion(c, st, 400, 396, {"toUnion": {"wp": 399}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 400, "union");
        atStep(st, 401);
        cqWorkplaneNew(c, st, 401, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 402);
        cqPushPoints(c, st, 402, 401, {"pntList": [[9.5, 15.772727272727272]]});
        atStep(st, 403);
        cqSphere(c, st, 403, 402, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 403, "sphere");
        atStep(st, 404);
        cqUnion(c, st, 404, 400, {"toUnion": {"wp": 403}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 404, "union");
        atStep(st, 405);
        cqWorkplaneNew(c, st, 405, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 406);
        cqPushPoints(c, st, 406, 405, {"pntList": [[0.5, 17.954545454545453]]});
        atStep(st, 407);
        cqSphere(c, st, 407, 406, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 407, "sphere");
        atStep(st, 408);
        cqUnion(c, st, 408, 404, {"toUnion": {"wp": 407}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 408, "union");
        atStep(st, 409);
        cqWorkplaneNew(c, st, 409, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 410);
        cqPushPoints(c, st, 410, 409, {"pntList": [[2.75, 17.954545454545453]]});
        atStep(st, 411);
        cqSphere(c, st, 411, 410, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 411, "sphere");
        atStep(st, 412);
        cqUnion(c, st, 412, 408, {"toUnion": {"wp": 411}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 412, "union");
        atStep(st, 413);
        cqWorkplaneNew(c, st, 413, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 414);
        cqPushPoints(c, st, 414, 413, {"pntList": [[5, 17.954545454545453]]});
        atStep(st, 415);
        cqSphere(c, st, 415, 414, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 415, "sphere");
        atStep(st, 416);
        cqUnion(c, st, 416, 412, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        atStep(st, 417);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 418);
        cqPushPoints(c, st, 418, 417, {"pntList": [[7.25, 17.954545454545453]]});
        atStep(st, 419);
        cqSphere(c, st, 419, 418, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 419, "sphere");
        atStep(st, 420);
        cqUnion(c, st, 420, 416, {"toUnion": {"wp": 419}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 420, "union");
        atStep(st, 421);
        cqWorkplaneNew(c, st, 421, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 422);
        cqPushPoints(c, st, 422, 421, {"pntList": [[9.5, 17.954545454545453]]});
        atStep(st, 423);
        cqSphere(c, st, 423, 422, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 423, "sphere");
        atStep(st, 424);
        cqUnion(c, st, 424, 420, {"toUnion": {"wp": 423}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 424, "union");
        atStep(st, 425);
        cqWorkplaneNew(c, st, 425, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 426);
        cqPushPoints(c, st, 426, 425, {"pntList": [[0.5, 20.136363636363633]]});
        atStep(st, 427);
        cqSphere(c, st, 427, 426, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 427, "sphere");
        atStep(st, 428);
        cqUnion(c, st, 428, 424, {"toUnion": {"wp": 427}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 428, "union");
        atStep(st, 429);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 430);
        cqPushPoints(c, st, 430, 429, {"pntList": [[2.75, 20.136363636363633]]});
        atStep(st, 431);
        cqSphere(c, st, 431, 430, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 431, "sphere");
        atStep(st, 432);
        cqUnion(c, st, 432, 428, {"toUnion": {"wp": 431}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        atStep(st, 433);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 434);
        cqPushPoints(c, st, 434, 433, {"pntList": [[5, 20.136363636363633]]});
        atStep(st, 435);
        cqSphere(c, st, 435, 434, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 435, "sphere");
        atStep(st, 436);
        cqUnion(c, st, 436, 432, {"toUnion": {"wp": 435}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 436, "union");
        atStep(st, 437);
        cqWorkplaneNew(c, st, 437, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 438);
        cqPushPoints(c, st, 438, 437, {"pntList": [[7.25, 20.136363636363633]]});
        atStep(st, 439);
        cqSphere(c, st, 439, 438, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 439, "sphere");
        atStep(st, 440);
        cqUnion(c, st, 440, 436, {"toUnion": {"wp": 439}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 440, "union");
        atStep(st, 441);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 442);
        cqPushPoints(c, st, 442, 441, {"pntList": [[9.5, 20.136363636363633]]});
        atStep(st, 443);
        cqSphere(c, st, 443, 442, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 443, "sphere");
        atStep(st, 444);
        cqUnion(c, st, 444, 440, {"toUnion": {"wp": 443}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 444, "union");
        atStep(st, 445);
        cqWorkplaneNew(c, st, 445, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 446);
        cqPushPoints(c, st, 446, 445, {"pntList": [[0.5, 22.318181818181817]]});
        atStep(st, 447);
        cqSphere(c, st, 447, 446, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 447, "sphere");
        atStep(st, 448);
        cqUnion(c, st, 448, 444, {"toUnion": {"wp": 447}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 448, "union");
        atStep(st, 449);
        cqWorkplaneNew(c, st, 449, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 450);
        cqPushPoints(c, st, 450, 449, {"pntList": [[2.75, 22.318181818181817]]});
        atStep(st, 451);
        cqSphere(c, st, 451, 450, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 451, "sphere");
        atStep(st, 452);
        cqUnion(c, st, 452, 448, {"toUnion": {"wp": 451}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 452, "union");
        atStep(st, 453);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 454);
        cqPushPoints(c, st, 454, 453, {"pntList": [[5, 22.318181818181817]]});
        atStep(st, 455);
        cqSphere(c, st, 455, 454, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 455, "sphere");
        atStep(st, 456);
        cqUnion(c, st, 456, 452, {"toUnion": {"wp": 455}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 456, "union");
        atStep(st, 457);
        cqWorkplaneNew(c, st, 457, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 458);
        cqPushPoints(c, st, 458, 457, {"pntList": [[7.25, 22.318181818181817]]});
        atStep(st, 459);
        cqSphere(c, st, 459, 458, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 459, "sphere");
        atStep(st, 460);
        cqUnion(c, st, 460, 456, {"toUnion": {"wp": 459}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 460, "union");
        atStep(st, 461);
        cqWorkplaneNew(c, st, 461, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 462);
        cqPushPoints(c, st, 462, 461, {"pntList": [[9.5, 22.318181818181817]]});
        atStep(st, 463);
        cqSphere(c, st, 463, 462, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 463, "sphere");
        atStep(st, 464);
        cqUnion(c, st, 464, 460, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 464, "union");
        atStep(st, 465);
        cqWorkplaneNew(c, st, 465, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 466);
        cqPushPoints(c, st, 466, 465, {"pntList": [[0.5, 24.5]]});
        atStep(st, 467);
        cqSphere(c, st, 467, 466, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 467, "sphere");
        atStep(st, 468);
        cqUnion(c, st, 468, 464, {"toUnion": {"wp": 467}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 468, "union");
        atStep(st, 469);
        cqWorkplaneNew(c, st, 469, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 470);
        cqPushPoints(c, st, 470, 469, {"pntList": [[2.75, 24.5]]});
        atStep(st, 471);
        cqSphere(c, st, 471, 470, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 471, "sphere");
        atStep(st, 472);
        cqUnion(c, st, 472, 468, {"toUnion": {"wp": 471}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 472, "union");
        atStep(st, 473);
        cqWorkplaneNew(c, st, 473, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 474);
        cqPushPoints(c, st, 474, 473, {"pntList": [[5, 24.5]]});
        atStep(st, 475);
        cqSphere(c, st, 475, 474, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 475, "sphere");
        atStep(st, 476);
        cqUnion(c, st, 476, 472, {"toUnion": {"wp": 475}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 476, "union");
        atStep(st, 477);
        cqWorkplaneNew(c, st, 477, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 478);
        cqPushPoints(c, st, 478, 477, {"pntList": [[7.25, 24.5]]});
        atStep(st, 479);
        cqSphere(c, st, 479, 478, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 479, "sphere");
        atStep(st, 480);
        cqUnion(c, st, 480, 476, {"toUnion": {"wp": 479}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 480, "union");
        atStep(st, 481);
        cqWorkplaneNew(c, st, 481, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 482);
        cqPushPoints(c, st, 482, 481, {"pntList": [[9.5, 24.5]]});
        atStep(st, 483);
        cqSphere(c, st, 483, 482, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 483, "sphere");
        atStep(st, 484);
        cqUnion(c, st, 484, 480, {"toUnion": {"wp": 483}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 484, "union");
        atStep(st, 485);
        cqWorkplaneNew(c, st, 485, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 486);
        cqPushPoints(c, st, 486, 485, {"pntList": [[0.5, 0.5]]});
        atStep(st, 487);
        cqSphere(c, st, 487, 486, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 487, "sphere");
        atStep(st, 488);
        cqUnion(c, st, 488, 484, {"toUnion": {"wp": 487}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 488, "union");
        atStep(st, 489);
        cqWorkplaneNew(c, st, 489, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 490);
        cqPushPoints(c, st, 490, 489, {"pntList": [[2.75, 0.5]]});
        atStep(st, 491);
        cqSphere(c, st, 491, 490, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 491, "sphere");
        atStep(st, 492);
        cqUnion(c, st, 492, 488, {"toUnion": {"wp": 491}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 492, "union");
        atStep(st, 493);
        cqWorkplaneNew(c, st, 493, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 494);
        cqPushPoints(c, st, 494, 493, {"pntList": [[5, 0.5]]});
        atStep(st, 495);
        cqSphere(c, st, 495, 494, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 495, "sphere");
        atStep(st, 496);
        cqUnion(c, st, 496, 492, {"toUnion": {"wp": 495}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 496, "union");
        atStep(st, 497);
        cqWorkplaneNew(c, st, 497, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 498);
        cqPushPoints(c, st, 498, 497, {"pntList": [[7.25, 0.5]]});
        atStep(st, 499);
        cqSphere(c, st, 499, 498, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 499, "sphere");
        atStep(st, 500);
        cqUnion(c, st, 500, 496, {"toUnion": {"wp": 499}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 500, "union");
        atStep(st, 501);
        cqWorkplaneNew(c, st, 501, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 502);
        cqPushPoints(c, st, 502, 501, {"pntList": [[9.5, 0.5]]});
        atStep(st, 503);
        cqSphere(c, st, 503, 502, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 503, "sphere");
        atStep(st, 504);
        cqUnion(c, st, 504, 500, {"toUnion": {"wp": 503}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 504, "union");
        atStep(st, 505);
        cqWorkplaneNew(c, st, 505, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 506);
        cqPushPoints(c, st, 506, 505, {"pntList": [[0.5, 2.6818181818181817]]});
        atStep(st, 507);
        cqSphere(c, st, 507, 506, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 507, "sphere");
        atStep(st, 508);
        cqUnion(c, st, 508, 504, {"toUnion": {"wp": 507}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 508, "union");
        atStep(st, 509);
        cqWorkplaneNew(c, st, 509, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 510);
        cqPushPoints(c, st, 510, 509, {"pntList": [[2.75, 2.6818181818181817]]});
        atStep(st, 511);
        cqSphere(c, st, 511, 510, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 511, "sphere");
        atStep(st, 512);
        cqUnion(c, st, 512, 508, {"toUnion": {"wp": 511}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 512, "union");
        atStep(st, 513);
        cqWorkplaneNew(c, st, 513, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 514);
        cqPushPoints(c, st, 514, 513, {"pntList": [[5, 2.6818181818181817]]});
        atStep(st, 515);
        cqSphere(c, st, 515, 514, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 515, "sphere");
        atStep(st, 516);
        cqUnion(c, st, 516, 512, {"toUnion": {"wp": 515}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 516, "union");
        atStep(st, 517);
        cqWorkplaneNew(c, st, 517, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 518);
        cqPushPoints(c, st, 518, 517, {"pntList": [[7.25, 2.6818181818181817]]});
        atStep(st, 519);
        cqSphere(c, st, 519, 518, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 519, "sphere");
        atStep(st, 520);
        cqUnion(c, st, 520, 516, {"toUnion": {"wp": 519}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 520, "union");
        atStep(st, 521);
        cqWorkplaneNew(c, st, 521, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 522);
        cqPushPoints(c, st, 522, 521, {"pntList": [[9.5, 2.6818181818181817]]});
        atStep(st, 523);
        cqSphere(c, st, 523, 522, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 523, "sphere");
        atStep(st, 524);
        cqUnion(c, st, 524, 520, {"toUnion": {"wp": 523}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 524, "union");
        atStep(st, 525);
        cqWorkplaneNew(c, st, 525, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 526);
        cqPushPoints(c, st, 526, 525, {"pntList": [[0.5, 4.863636363636363]]});
        atStep(st, 527);
        cqSphere(c, st, 527, 526, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 527, "sphere");
        atStep(st, 528);
        cqUnion(c, st, 528, 524, {"toUnion": {"wp": 527}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 528, "union");
        atStep(st, 529);
        cqWorkplaneNew(c, st, 529, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 530);
        cqPushPoints(c, st, 530, 529, {"pntList": [[2.75, 4.863636363636363]]});
        atStep(st, 531);
        cqSphere(c, st, 531, 530, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 531, "sphere");
        atStep(st, 532);
        cqUnion(c, st, 532, 528, {"toUnion": {"wp": 531}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 532, "union");
        atStep(st, 533);
        cqWorkplaneNew(c, st, 533, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 534);
        cqPushPoints(c, st, 534, 533, {"pntList": [[5, 4.863636363636363]]});
        atStep(st, 535);
        cqSphere(c, st, 535, 534, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 535, "sphere");
        atStep(st, 536);
        cqUnion(c, st, 536, 532, {"toUnion": {"wp": 535}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 536, "union");
        atStep(st, 537);
        cqWorkplaneNew(c, st, 537, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 538);
        cqPushPoints(c, st, 538, 537, {"pntList": [[7.25, 4.863636363636363]]});
        atStep(st, 539);
        cqSphere(c, st, 539, 538, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 539, "sphere");
        atStep(st, 540);
        cqUnion(c, st, 540, 536, {"toUnion": {"wp": 539}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 540, "union");
        atStep(st, 541);
        cqWorkplaneNew(c, st, 541, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 542);
        cqPushPoints(c, st, 542, 541, {"pntList": [[9.5, 4.863636363636363]]});
        atStep(st, 543);
        cqSphere(c, st, 543, 542, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 543, "sphere");
        atStep(st, 544);
        cqUnion(c, st, 544, 540, {"toUnion": {"wp": 543}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 544, "union");
        atStep(st, 545);
        cqWorkplaneNew(c, st, 545, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 546);
        cqPushPoints(c, st, 546, 545, {"pntList": [[0.5, 7.045454545454545]]});
        atStep(st, 547);
        cqSphere(c, st, 547, 546, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 547, "sphere");
        atStep(st, 548);
        cqUnion(c, st, 548, 544, {"toUnion": {"wp": 547}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 548, "union");
        atStep(st, 549);
        cqWorkplaneNew(c, st, 549, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 550);
        cqPushPoints(c, st, 550, 549, {"pntList": [[2.75, 7.045454545454545]]});
        atStep(st, 551);
        cqSphere(c, st, 551, 550, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 551, "sphere");
        atStep(st, 552);
        cqUnion(c, st, 552, 548, {"toUnion": {"wp": 551}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 552, "union");
        atStep(st, 553);
        cqWorkplaneNew(c, st, 553, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 554);
        cqPushPoints(c, st, 554, 553, {"pntList": [[5, 7.045454545454545]]});
        atStep(st, 555);
        cqSphere(c, st, 555, 554, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 555, "sphere");
        atStep(st, 556);
        cqUnion(c, st, 556, 552, {"toUnion": {"wp": 555}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 556, "union");
        atStep(st, 557);
        cqWorkplaneNew(c, st, 557, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 558);
        cqPushPoints(c, st, 558, 557, {"pntList": [[7.25, 7.045454545454545]]});
        atStep(st, 559);
        cqSphere(c, st, 559, 558, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 559, "sphere");
        atStep(st, 560);
        cqUnion(c, st, 560, 556, {"toUnion": {"wp": 559}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 560, "union");
        atStep(st, 561);
        cqWorkplaneNew(c, st, 561, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 562);
        cqPushPoints(c, st, 562, 561, {"pntList": [[9.5, 7.045454545454545]]});
        atStep(st, 563);
        cqSphere(c, st, 563, 562, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 563, "sphere");
        atStep(st, 564);
        cqUnion(c, st, 564, 560, {"toUnion": {"wp": 563}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 564, "union");
        atStep(st, 565);
        cqWorkplaneNew(c, st, 565, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 566);
        cqPushPoints(c, st, 566, 565, {"pntList": [[0.5, 9.227272727272728]]});
        atStep(st, 567);
        cqSphere(c, st, 567, 566, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 567, "sphere");
        atStep(st, 568);
        cqUnion(c, st, 568, 564, {"toUnion": {"wp": 567}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 568, "union");
        atStep(st, 569);
        cqWorkplaneNew(c, st, 569, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 570);
        cqPushPoints(c, st, 570, 569, {"pntList": [[2.75, 9.227272727272728]]});
        atStep(st, 571);
        cqSphere(c, st, 571, 570, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 571, "sphere");
        atStep(st, 572);
        cqUnion(c, st, 572, 568, {"toUnion": {"wp": 571}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 572, "union");
        atStep(st, 573);
        cqWorkplaneNew(c, st, 573, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 574);
        cqPushPoints(c, st, 574, 573, {"pntList": [[5, 9.227272727272728]]});
        atStep(st, 575);
        cqSphere(c, st, 575, 574, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 575, "sphere");
        atStep(st, 576);
        cqUnion(c, st, 576, 572, {"toUnion": {"wp": 575}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 576, "union");
        atStep(st, 577);
        cqWorkplaneNew(c, st, 577, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 578);
        cqPushPoints(c, st, 578, 577, {"pntList": [[7.25, 9.227272727272728]]});
        atStep(st, 579);
        cqSphere(c, st, 579, 578, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 579, "sphere");
        atStep(st, 580);
        cqUnion(c, st, 580, 576, {"toUnion": {"wp": 579}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 580, "union");
        atStep(st, 581);
        cqWorkplaneNew(c, st, 581, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 582);
        cqPushPoints(c, st, 582, 581, {"pntList": [[9.5, 9.227272727272728]]});
        atStep(st, 583);
        cqSphere(c, st, 583, 582, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 583, "sphere");
        atStep(st, 584);
        cqUnion(c, st, 584, 580, {"toUnion": {"wp": 583}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 584, "union");
        atStep(st, 585);
        cqWorkplaneNew(c, st, 585, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 586);
        cqPushPoints(c, st, 586, 585, {"pntList": [[0.5, 11.409090909090908]]});
        atStep(st, 587);
        cqSphere(c, st, 587, 586, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 587, "sphere");
        atStep(st, 588);
        cqUnion(c, st, 588, 584, {"toUnion": {"wp": 587}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 588, "union");
        atStep(st, 589);
        cqWorkplaneNew(c, st, 589, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 590);
        cqPushPoints(c, st, 590, 589, {"pntList": [[2.75, 11.409090909090908]]});
        atStep(st, 591);
        cqSphere(c, st, 591, 590, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 591, "sphere");
        atStep(st, 592);
        cqUnion(c, st, 592, 588, {"toUnion": {"wp": 591}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 592, "union");
        atStep(st, 593);
        cqWorkplaneNew(c, st, 593, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 594);
        cqPushPoints(c, st, 594, 593, {"pntList": [[5, 11.409090909090908]]});
        atStep(st, 595);
        cqSphere(c, st, 595, 594, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 595, "sphere");
        atStep(st, 596);
        cqUnion(c, st, 596, 592, {"toUnion": {"wp": 595}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 596, "union");
        atStep(st, 597);
        cqWorkplaneNew(c, st, 597, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 598);
        cqPushPoints(c, st, 598, 597, {"pntList": [[7.25, 11.409090909090908]]});
        atStep(st, 599);
        cqSphere(c, st, 599, 598, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 599, "sphere");
        atStep(st, 600);
        cqUnion(c, st, 600, 596, {"toUnion": {"wp": 599}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 600, "union");
        atStep(st, 601);
        cqWorkplaneNew(c, st, 601, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 602);
        cqPushPoints(c, st, 602, 601, {"pntList": [[9.5, 11.409090909090908]]});
        atStep(st, 603);
        cqSphere(c, st, 603, 602, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 603, "sphere");
        atStep(st, 604);
        cqUnion(c, st, 604, 600, {"toUnion": {"wp": 603}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 604, "union");
        atStep(st, 605);
        cqWorkplaneNew(c, st, 605, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 606);
        cqPushPoints(c, st, 606, 605, {"pntList": [[0.5, 13.59090909090909]]});
        atStep(st, 607);
        cqSphere(c, st, 607, 606, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 607, "sphere");
        atStep(st, 608);
        cqUnion(c, st, 608, 604, {"toUnion": {"wp": 607}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 608, "union");
        atStep(st, 609);
        cqWorkplaneNew(c, st, 609, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 610);
        cqPushPoints(c, st, 610, 609, {"pntList": [[2.75, 13.59090909090909]]});
        atStep(st, 611);
        cqSphere(c, st, 611, 610, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 611, "sphere");
        atStep(st, 612);
        cqUnion(c, st, 612, 608, {"toUnion": {"wp": 611}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 612, "union");
        atStep(st, 613);
        cqWorkplaneNew(c, st, 613, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 614);
        cqPushPoints(c, st, 614, 613, {"pntList": [[5, 13.59090909090909]]});
        atStep(st, 615);
        cqSphere(c, st, 615, 614, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 615, "sphere");
        atStep(st, 616);
        cqUnion(c, st, 616, 612, {"toUnion": {"wp": 615}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 616, "union");
        atStep(st, 617);
        cqWorkplaneNew(c, st, 617, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 618);
        cqPushPoints(c, st, 618, 617, {"pntList": [[7.25, 13.59090909090909]]});
        atStep(st, 619);
        cqSphere(c, st, 619, 618, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 619, "sphere");
        atStep(st, 620);
        cqUnion(c, st, 620, 616, {"toUnion": {"wp": 619}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 620, "union");
        atStep(st, 621);
        cqWorkplaneNew(c, st, 621, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 622);
        cqPushPoints(c, st, 622, 621, {"pntList": [[9.5, 13.59090909090909]]});
        atStep(st, 623);
        cqSphere(c, st, 623, 622, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 623, "sphere");
        atStep(st, 624);
        cqUnion(c, st, 624, 620, {"toUnion": {"wp": 623}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 624, "union");
        atStep(st, 625);
        cqWorkplaneNew(c, st, 625, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 626);
        cqPushPoints(c, st, 626, 625, {"pntList": [[0.5, 15.772727272727272]]});
        atStep(st, 627);
        cqSphere(c, st, 627, 626, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 627, "sphere");
        atStep(st, 628);
        cqUnion(c, st, 628, 624, {"toUnion": {"wp": 627}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 628, "union");
        atStep(st, 629);
        cqWorkplaneNew(c, st, 629, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 630);
        cqPushPoints(c, st, 630, 629, {"pntList": [[2.75, 15.772727272727272]]});
        atStep(st, 631);
        cqSphere(c, st, 631, 630, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 631, "sphere");
        atStep(st, 632);
        cqUnion(c, st, 632, 628, {"toUnion": {"wp": 631}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 632, "union");
        atStep(st, 633);
        cqWorkplaneNew(c, st, 633, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 634);
        cqPushPoints(c, st, 634, 633, {"pntList": [[5, 15.772727272727272]]});
        atStep(st, 635);
        cqSphere(c, st, 635, 634, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 635, "sphere");
        atStep(st, 636);
        cqUnion(c, st, 636, 632, {"toUnion": {"wp": 635}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 636, "union");
        atStep(st, 637);
        cqWorkplaneNew(c, st, 637, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 638);
        cqPushPoints(c, st, 638, 637, {"pntList": [[7.25, 15.772727272727272]]});
        atStep(st, 639);
        cqSphere(c, st, 639, 638, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 639, "sphere");
        atStep(st, 640);
        cqUnion(c, st, 640, 636, {"toUnion": {"wp": 639}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 640, "union");
        atStep(st, 641);
        cqWorkplaneNew(c, st, 641, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 642);
        cqPushPoints(c, st, 642, 641, {"pntList": [[9.5, 15.772727272727272]]});
        atStep(st, 643);
        cqSphere(c, st, 643, 642, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 643, "sphere");
        atStep(st, 644);
        cqUnion(c, st, 644, 640, {"toUnion": {"wp": 643}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 644, "union");
        atStep(st, 645);
        cqWorkplaneNew(c, st, 645, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 646);
        cqPushPoints(c, st, 646, 645, {"pntList": [[0.5, 17.954545454545453]]});
        atStep(st, 647);
        cqSphere(c, st, 647, 646, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 647, "sphere");
        atStep(st, 648);
        cqUnion(c, st, 648, 644, {"toUnion": {"wp": 647}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 648, "union");
        atStep(st, 649);
        cqWorkplaneNew(c, st, 649, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 650);
        cqPushPoints(c, st, 650, 649, {"pntList": [[2.75, 17.954545454545453]]});
        atStep(st, 651);
        cqSphere(c, st, 651, 650, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 651, "sphere");
        atStep(st, 652);
        cqUnion(c, st, 652, 648, {"toUnion": {"wp": 651}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 652, "union");
        atStep(st, 653);
        cqWorkplaneNew(c, st, 653, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 654);
        cqPushPoints(c, st, 654, 653, {"pntList": [[5, 17.954545454545453]]});
        atStep(st, 655);
        cqSphere(c, st, 655, 654, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 655, "sphere");
        atStep(st, 656);
        cqUnion(c, st, 656, 652, {"toUnion": {"wp": 655}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 656, "union");
        atStep(st, 657);
        cqWorkplaneNew(c, st, 657, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 658);
        cqPushPoints(c, st, 658, 657, {"pntList": [[7.25, 17.954545454545453]]});
        atStep(st, 659);
        cqSphere(c, st, 659, 658, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 659, "sphere");
        atStep(st, 660);
        cqUnion(c, st, 660, 656, {"toUnion": {"wp": 659}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 660, "union");
        atStep(st, 661);
        cqWorkplaneNew(c, st, 661, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 662);
        cqPushPoints(c, st, 662, 661, {"pntList": [[9.5, 17.954545454545453]]});
        atStep(st, 663);
        cqSphere(c, st, 663, 662, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 663, "sphere");
        atStep(st, 664);
        cqUnion(c, st, 664, 660, {"toUnion": {"wp": 663}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 664, "union");
        atStep(st, 665);
        cqWorkplaneNew(c, st, 665, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 666);
        cqPushPoints(c, st, 666, 665, {"pntList": [[0.5, 20.136363636363633]]});
        atStep(st, 667);
        cqSphere(c, st, 667, 666, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 667, "sphere");
        atStep(st, 668);
        cqUnion(c, st, 668, 664, {"toUnion": {"wp": 667}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 668, "union");
        atStep(st, 669);
        cqWorkplaneNew(c, st, 669, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 670);
        cqPushPoints(c, st, 670, 669, {"pntList": [[2.75, 20.136363636363633]]});
        atStep(st, 671);
        cqSphere(c, st, 671, 670, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 671, "sphere");
        atStep(st, 672);
        cqUnion(c, st, 672, 668, {"toUnion": {"wp": 671}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 672, "union");
        atStep(st, 673);
        cqWorkplaneNew(c, st, 673, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 674);
        cqPushPoints(c, st, 674, 673, {"pntList": [[5, 20.136363636363633]]});
        atStep(st, 675);
        cqSphere(c, st, 675, 674, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 675, "sphere");
        atStep(st, 676);
        cqUnion(c, st, 676, 672, {"toUnion": {"wp": 675}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 676, "union");
        atStep(st, 677);
        cqWorkplaneNew(c, st, 677, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 678);
        cqPushPoints(c, st, 678, 677, {"pntList": [[7.25, 20.136363636363633]]});
        atStep(st, 679);
        cqSphere(c, st, 679, 678, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 679, "sphere");
        atStep(st, 680);
        cqUnion(c, st, 680, 676, {"toUnion": {"wp": 679}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 680, "union");
        atStep(st, 681);
        cqWorkplaneNew(c, st, 681, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 682);
        cqPushPoints(c, st, 682, 681, {"pntList": [[9.5, 20.136363636363633]]});
        atStep(st, 683);
        cqSphere(c, st, 683, 682, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 683, "sphere");
        atStep(st, 684);
        cqUnion(c, st, 684, 680, {"toUnion": {"wp": 683}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 684, "union");
        atStep(st, 685);
        cqWorkplaneNew(c, st, 685, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 686);
        cqPushPoints(c, st, 686, 685, {"pntList": [[0.5, 22.318181818181817]]});
        atStep(st, 687);
        cqSphere(c, st, 687, 686, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 687, "sphere");
        atStep(st, 688);
        cqUnion(c, st, 688, 684, {"toUnion": {"wp": 687}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 688, "union");
        atStep(st, 689);
        cqWorkplaneNew(c, st, 689, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 690);
        cqPushPoints(c, st, 690, 689, {"pntList": [[2.75, 22.318181818181817]]});
        atStep(st, 691);
        cqSphere(c, st, 691, 690, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 691, "sphere");
        atStep(st, 692);
        cqUnion(c, st, 692, 688, {"toUnion": {"wp": 691}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 692, "union");
        atStep(st, 693);
        cqWorkplaneNew(c, st, 693, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 694);
        cqPushPoints(c, st, 694, 693, {"pntList": [[5, 22.318181818181817]]});
        atStep(st, 695);
        cqSphere(c, st, 695, 694, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 695, "sphere");
        atStep(st, 696);
        cqUnion(c, st, 696, 692, {"toUnion": {"wp": 695}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 696, "union");
        atStep(st, 697);
        cqWorkplaneNew(c, st, 697, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 698);
        cqPushPoints(c, st, 698, 697, {"pntList": [[7.25, 22.318181818181817]]});
        atStep(st, 699);
        cqSphere(c, st, 699, 698, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 699, "sphere");
        atStep(st, 700);
        cqUnion(c, st, 700, 696, {"toUnion": {"wp": 699}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 700, "union");
        atStep(st, 701);
        cqWorkplaneNew(c, st, 701, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 702);
        cqPushPoints(c, st, 702, 701, {"pntList": [[9.5, 22.318181818181817]]});
        atStep(st, 703);
        cqSphere(c, st, 703, 702, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 703, "sphere");
        atStep(st, 704);
        cqUnion(c, st, 704, 700, {"toUnion": {"wp": 703}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 704, "union");
        atStep(st, 705);
        cqWorkplaneNew(c, st, 705, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 706);
        cqPushPoints(c, st, 706, 705, {"pntList": [[0.5, 24.5]]});
        atStep(st, 707);
        cqSphere(c, st, 707, 706, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 707, "sphere");
        atStep(st, 708);
        cqUnion(c, st, 708, 704, {"toUnion": {"wp": 707}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 708, "union");
        atStep(st, 709);
        cqWorkplaneNew(c, st, 709, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 710);
        cqPushPoints(c, st, 710, 709, {"pntList": [[2.75, 24.5]]});
        atStep(st, 711);
        cqSphere(c, st, 711, 710, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 711, "sphere");
        atStep(st, 712);
        cqUnion(c, st, 712, 708, {"toUnion": {"wp": 711}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 712, "union");
        atStep(st, 713);
        cqWorkplaneNew(c, st, 713, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 714);
        cqPushPoints(c, st, 714, 713, {"pntList": [[5, 24.5]]});
        atStep(st, 715);
        cqSphere(c, st, 715, 714, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 715, "sphere");
        atStep(st, 716);
        cqUnion(c, st, 716, 712, {"toUnion": {"wp": 715}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 716, "union");
        atStep(st, 717);
        cqWorkplaneNew(c, st, 717, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 718);
        cqPushPoints(c, st, 718, 717, {"pntList": [[7.25, 24.5]]});
        atStep(st, 719);
        cqSphere(c, st, 719, 718, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 719, "sphere");
        atStep(st, 720);
        cqUnion(c, st, 720, 716, {"toUnion": {"wp": 719}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 720, "union");
        atStep(st, 721);
        cqWorkplaneNew(c, st, 721, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 722);
        cqPushPoints(c, st, 722, 721, {"pntList": [[9.5, 24.5]]});
        atStep(st, 723);
        cqSphere(c, st, 723, 722, {"radius": 0.5, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 723, "sphere");
        atStep(st, 724);
        cqUnion(c, st, 724, 720, {"toUnion": {"wp": 723}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 724, "union");
        atStep(st, 725);
        cqWorkplaneNew(c, st, 725, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 726);
        cqPushPoints(c, st, 726, 725, {"pntList": [[1.5, 12.5]]});
        atStep(st, 727);
        cqCircle(c, st, 727, 726, {"radius": 1, "forConstruction": false});
        atStep(st, 728);
        cqExtrude(c, st, 728, 727, {"until": -9.526279441628825, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 728, "extrude");
        atStep(st, 729);
        cqCut(c, st, 729, 724, {"toCut": {"wp": 728}, "clean": true, "tol": undefined});
        stepVol(c, st, 729, "cut");
        atStep(st, 730);
        cqWorkplaneNew(c, st, 730, {"plane": {"origin": [-5, -2.8867513459481287, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 731);
        cqPushPoints(c, st, 731, 730, {"pntList": [[8.5, 12.5]]});
        atStep(st, 732);
        cqCircle(c, st, 732, 731, {"radius": 1, "forConstruction": false});
        atStep(st, 733);
        cqExtrude(c, st, 733, 732, {"until": -9.526279441628825, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 733, "extrude");
        atStep(st, 734);
        cqCut(c, st, 734, 729, {"toCut": {"wp": 733}, "clean": true, "tol": undefined});
        stepVol(c, st, 734, "cut");
        atStep(st, 735);
        cqWorkplaneNew(c, st, 735, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 736);
        cqPushPoints(c, st, 736, 735, {"pntList": [[1.5, 12.5]]});
        atStep(st, 737);
        cqCircle(c, st, 737, 736, {"radius": 1, "forConstruction": false});
        atStep(st, 738);
        cqExtrude(c, st, 738, 737, {"until": -9.526279441628825, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 738, "extrude");
        atStep(st, 739);
        cqCut(c, st, 739, 734, {"toCut": {"wp": 738}, "clean": true, "tol": undefined});
        stepVol(c, st, 739, "cut");
        atStep(st, 740);
        cqWorkplaneNew(c, st, 740, {"plane": {"origin": [5, -2.8867513459481287, 0], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 741);
        cqPushPoints(c, st, 741, 740, {"pntList": [[8.5, 12.5]]});
        atStep(st, 742);
        cqCircle(c, st, 742, 741, {"radius": 1, "forConstruction": false});
        atStep(st, 743);
        cqExtrude(c, st, 743, 742, {"until": -9.526279441628825, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 743, "extrude");
        atStep(st, 744);
        cqCut(c, st, 744, 739, {"toCut": {"wp": 743}, "clean": true, "tol": undefined});
        stepVol(c, st, 744, "cut");
        atStep(st, 745);
        cqWorkplaneNew(c, st, 745, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 746);
        cqPushPoints(c, st, 746, 745, {"pntList": [[1.5, 12.5]]});
        atStep(st, 747);
        cqCircle(c, st, 747, 746, {"radius": 1, "forConstruction": false});
        atStep(st, 748);
        cqExtrude(c, st, 748, 747, {"until": -9.526279441628825, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 748, "extrude");
        atStep(st, 749);
        cqCut(c, st, 749, 744, {"toCut": {"wp": 748}, "clean": true, "tol": undefined});
        stepVol(c, st, 749, "cut");
        atStep(st, 750);
        cqWorkplaneNew(c, st, 750, {"plane": {"origin": [0, 5.773502691896256, 0], "xDir": [-0.5000000000000001, -0.8660254037844387, 0], "zDir": [-0.8660254037844387, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 751);
        cqPushPoints(c, st, 751, 750, {"pntList": [[8.5, 12.5]]});
        atStep(st, 752);
        cqCircle(c, st, 752, 751, {"radius": 1, "forConstruction": false});
        atStep(st, 753);
        cqExtrude(c, st, 753, 752, {"until": -9.526279441628825, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 753, "extrude");
        atStep(st, 754);
        cqCut(c, st, 754, 749, {"toCut": {"wp": 753}, "clean": true, "tol": undefined});
        stepVol(c, st, 754, "cut");
        return mergeMaps(cqMeasure(c, st, {"wp": 754}), {"i": 3424});
    } catch (e) {
        return {"i": 3424, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_3424(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c3424"] = r; }
    return out;
}
