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

const case_3521 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqPolygon(c, st, 2, 1, {"nSides": 6, "diameter": 20, "forConstruction": false, "circumscribed": false});
        atStep(st, 3);
        cqExtrude(c, st, 3, 2, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 3, "extrude");
        atStep(st, 4);
        cqWorkplaneNew(c, st, 4, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 5);
        cqWorkplane(c, st, 5, 4, {"offset": 2, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 6);
        cqPolygon(c, st, 6, 5, {"nSides": 6, "diameter": 15.381197846482992, "forConstruction": false, "circumscribed": false});
        atStep(st, 7);
        cqExtrude(c, st, 7, 6, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 7, "extrude");
        atStep(st, 8);
        cqCut(c, st, 8, 3, {"toCut": {"wp": 7}, "clean": true, "tol": undefined});
        stepVol(c, st, 8, "cut");
        atStep(st, 9);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 10);
        cqRect(c, st, 10, 9, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 11);
        cqExtrude(c, st, 11, 10, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 11, "extrude");
        atStep(st, 12);
        cqCut(c, st, 12, 8, {"toCut": {"wp": 11}, "clean": true, "tol": undefined});
        stepVol(c, st, 12, "cut");
        atStep(st, 13);
        cqWorkplaneNew(c, st, 13, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 14);
        cqRect(c, st, 14, 13, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 15);
        cqExtrude(c, st, 15, 14, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 15, "extrude");
        atStep(st, 16);
        cqCut(c, st, 16, 12, {"toCut": {"wp": 15}, "clean": true, "tol": undefined});
        stepVol(c, st, 16, "cut");
        atStep(st, 17);
        cqWorkplaneNew(c, st, 17, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 18);
        cqRect(c, st, 18, 17, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 19);
        cqExtrude(c, st, 19, 18, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 19, "extrude");
        atStep(st, 20);
        cqCut(c, st, 20, 16, {"toCut": {"wp": 19}, "clean": true, "tol": undefined});
        stepVol(c, st, 20, "cut");
        atStep(st, 21);
        cqWorkplaneNew(c, st, 21, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 22);
        cqRect(c, st, 22, 21, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 23);
        cqExtrude(c, st, 23, 22, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 23, "extrude");
        atStep(st, 24);
        cqCut(c, st, 24, 20, {"toCut": {"wp": 23}, "clean": true, "tol": undefined});
        stepVol(c, st, 24, "cut");
        atStep(st, 25);
        cqWorkplaneNew(c, st, 25, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 26);
        cqRect(c, st, 26, 25, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 27);
        cqExtrude(c, st, 27, 26, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 27, "extrude");
        atStep(st, 28);
        cqCut(c, st, 28, 24, {"toCut": {"wp": 27}, "clean": true, "tol": undefined});
        stepVol(c, st, 28, "cut");
        atStep(st, 29);
        cqWorkplaneNew(c, st, 29, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 30);
        cqRect(c, st, 30, 29, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 31);
        cqExtrude(c, st, 31, 30, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 31, "extrude");
        atStep(st, 32);
        cqCut(c, st, 32, 28, {"toCut": {"wp": 31}, "clean": true, "tol": undefined});
        stepVol(c, st, 32, "cut");
        atStep(st, 33);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 34);
        cqCenter(c, st, 34, 33, {"x": 0, "y": 4});
        atStep(st, 35);
        cqCircle(c, st, 35, 34, {"radius": 1.5, "forConstruction": false});
        atStep(st, 36);
        cqExtrude(c, st, 36, 35, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 36, "extrude");
        atStep(st, 37);
        cqCut(c, st, 37, 32, {"toCut": {"wp": 36}, "clean": true, "tol": undefined});
        stepVol(c, st, 37, "cut");
        atStep(st, 38);
        cqCenter(c, st, 38, 33, {"x": 0, "y": -4});
        atStep(st, 39);
        cqCircle(c, st, 39, 38, {"radius": 1.5, "forConstruction": false});
        atStep(st, 40);
        cqExtrude(c, st, 40, 39, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 40, "extrude");
        atStep(st, 41);
        cqCut(c, st, 41, 37, {"toCut": {"wp": 40}, "clean": true, "tol": undefined});
        stepVol(c, st, 41, "cut");
        atStep(st, 42);
        cqWorkplaneNew(c, st, 42, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 43);
        cqCenter(c, st, 43, 42, {"x": 0, "y": 4});
        atStep(st, 44);
        cqCircle(c, st, 44, 43, {"radius": 1.5, "forConstruction": false});
        atStep(st, 45);
        cqExtrude(c, st, 45, 44, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 45, "extrude");
        atStep(st, 46);
        cqCut(c, st, 46, 41, {"toCut": {"wp": 45}, "clean": true, "tol": undefined});
        stepVol(c, st, 46, "cut");
        atStep(st, 47);
        cqCenter(c, st, 47, 42, {"x": 0, "y": -4});
        atStep(st, 48);
        cqCircle(c, st, 48, 47, {"radius": 1.5, "forConstruction": false});
        atStep(st, 49);
        cqExtrude(c, st, 49, 48, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 49, "extrude");
        atStep(st, 50);
        cqCut(c, st, 50, 46, {"toCut": {"wp": 49}, "clean": true, "tol": undefined});
        stepVol(c, st, 50, "cut");
        atStep(st, 51);
        cqWorkplaneNew(c, st, 51, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 52);
        cqCenter(c, st, 52, 51, {"x": 0, "y": 4});
        atStep(st, 53);
        cqCircle(c, st, 53, 52, {"radius": 1.5, "forConstruction": false});
        atStep(st, 54);
        cqExtrude(c, st, 54, 53, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 54, "extrude");
        atStep(st, 55);
        cqCut(c, st, 55, 50, {"toCut": {"wp": 54}, "clean": true, "tol": undefined});
        stepVol(c, st, 55, "cut");
        atStep(st, 56);
        cqCenter(c, st, 56, 51, {"x": 0, "y": -4});
        atStep(st, 57);
        cqCircle(c, st, 57, 56, {"radius": 1.5, "forConstruction": false});
        atStep(st, 58);
        cqExtrude(c, st, 58, 57, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 58, "extrude");
        atStep(st, 59);
        cqCut(c, st, 59, 55, {"toCut": {"wp": 58}, "clean": true, "tol": undefined});
        stepVol(c, st, 59, "cut");
        atStep(st, 60);
        cqWorkplaneNew(c, st, 60, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 61);
        cqCenter(c, st, 61, 60, {"x": 0, "y": 4});
        atStep(st, 62);
        cqCircle(c, st, 62, 61, {"radius": 1.5, "forConstruction": false});
        atStep(st, 63);
        cqExtrude(c, st, 63, 62, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 63, "extrude");
        atStep(st, 64);
        cqCut(c, st, 64, 59, {"toCut": {"wp": 63}, "clean": true, "tol": undefined});
        stepVol(c, st, 64, "cut");
        atStep(st, 65);
        cqCenter(c, st, 65, 60, {"x": 0, "y": -4});
        atStep(st, 66);
        cqCircle(c, st, 66, 65, {"radius": 1.5, "forConstruction": false});
        atStep(st, 67);
        cqExtrude(c, st, 67, 66, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 67, "extrude");
        atStep(st, 68);
        cqCut(c, st, 68, 64, {"toCut": {"wp": 67}, "clean": true, "tol": undefined});
        stepVol(c, st, 68, "cut");
        atStep(st, 69);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 70);
        cqCenter(c, st, 70, 69, {"x": 0, "y": 4});
        atStep(st, 71);
        cqCircle(c, st, 71, 70, {"radius": 1.5, "forConstruction": false});
        atStep(st, 72);
        cqExtrude(c, st, 72, 71, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 72, "extrude");
        atStep(st, 73);
        cqCut(c, st, 73, 68, {"toCut": {"wp": 72}, "clean": true, "tol": undefined});
        stepVol(c, st, 73, "cut");
        atStep(st, 74);
        cqCenter(c, st, 74, 69, {"x": 0, "y": -4});
        atStep(st, 75);
        cqCircle(c, st, 75, 74, {"radius": 1.5, "forConstruction": false});
        atStep(st, 76);
        cqExtrude(c, st, 76, 75, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 76, "extrude");
        atStep(st, 77);
        cqCut(c, st, 77, 73, {"toCut": {"wp": 76}, "clean": true, "tol": undefined});
        stepVol(c, st, 77, "cut");
        atStep(st, 78);
        cqWorkplaneNew(c, st, 78, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 79);
        cqCenter(c, st, 79, 78, {"x": 0, "y": 4});
        atStep(st, 80);
        cqCircle(c, st, 80, 79, {"radius": 1.5, "forConstruction": false});
        atStep(st, 81);
        cqExtrude(c, st, 81, 80, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 81, "extrude");
        atStep(st, 82);
        cqCut(c, st, 82, 77, {"toCut": {"wp": 81}, "clean": true, "tol": undefined});
        stepVol(c, st, 82, "cut");
        atStep(st, 83);
        cqCenter(c, st, 83, 78, {"x": 0, "y": -4});
        atStep(st, 84);
        cqCircle(c, st, 84, 83, {"radius": 1.5, "forConstruction": false});
        atStep(st, 85);
        cqExtrude(c, st, 85, 84, {"until": 1.501, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 85, "extrude");
        atStep(st, 86);
        cqCut(c, st, 86, 82, {"toCut": {"wp": 85}, "clean": true, "tol": undefined});
        stepVol(c, st, 86, "cut");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 88);
        cqCenter(c, st, 88, 87, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 89);
        cqRect(c, st, 89, 88, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 90);
        cqExtrude(c, st, 90, 89, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 90, "extrude");
        atStep(st, 91);
        cqCut(c, st, 91, 86, {"toCut": {"wp": 90}, "clean": true, "tol": undefined});
        stepVol(c, st, 91, "cut");
        atStep(st, 92);
        cqCenter(c, st, 92, 87, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 93);
        cqRect(c, st, 93, 92, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 94);
        cqExtrude(c, st, 94, 93, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 94, "extrude");
        atStep(st, 95);
        cqCut(c, st, 95, 91, {"toCut": {"wp": 94}, "clean": true, "tol": undefined});
        stepVol(c, st, 95, "cut");
        atStep(st, 96);
        cqWorkplaneNew(c, st, 96, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 97);
        cqCenter(c, st, 97, 96, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 98);
        cqRect(c, st, 98, 97, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 99);
        cqExtrude(c, st, 99, 98, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 99, "extrude");
        atStep(st, 100);
        cqCut(c, st, 100, 95, {"toCut": {"wp": 99}, "clean": true, "tol": undefined});
        stepVol(c, st, 100, "cut");
        atStep(st, 101);
        cqCenter(c, st, 101, 96, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 102);
        cqRect(c, st, 102, 101, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 103);
        cqExtrude(c, st, 103, 102, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 103, "extrude");
        atStep(st, 104);
        cqCut(c, st, 104, 100, {"toCut": {"wp": 103}, "clean": true, "tol": undefined});
        stepVol(c, st, 104, "cut");
        atStep(st, 105);
        cqWorkplaneNew(c, st, 105, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 106);
        cqCenter(c, st, 106, 105, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 107);
        cqRect(c, st, 107, 106, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 108);
        cqExtrude(c, st, 108, 107, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 108, "extrude");
        atStep(st, 109);
        cqCut(c, st, 109, 104, {"toCut": {"wp": 108}, "clean": true, "tol": undefined});
        stepVol(c, st, 109, "cut");
        atStep(st, 110);
        cqCenter(c, st, 110, 105, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 111);
        cqRect(c, st, 111, 110, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 112);
        cqExtrude(c, st, 112, 111, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 112, "extrude");
        atStep(st, 113);
        cqCut(c, st, 113, 109, {"toCut": {"wp": 112}, "clean": true, "tol": undefined});
        stepVol(c, st, 113, "cut");
        atStep(st, 114);
        cqWorkplaneNew(c, st, 114, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 115);
        cqCenter(c, st, 115, 114, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 116);
        cqRect(c, st, 116, 115, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 117);
        cqExtrude(c, st, 117, 116, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 117, "extrude");
        atStep(st, 118);
        cqCut(c, st, 118, 113, {"toCut": {"wp": 117}, "clean": true, "tol": undefined});
        stepVol(c, st, 118, "cut");
        atStep(st, 119);
        cqCenter(c, st, 119, 114, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 120);
        cqRect(c, st, 120, 119, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 121);
        cqExtrude(c, st, 121, 120, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 121, "extrude");
        atStep(st, 122);
        cqCut(c, st, 122, 118, {"toCut": {"wp": 121}, "clean": true, "tol": undefined});
        stepVol(c, st, 122, "cut");
        atStep(st, 123);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 124);
        cqCenter(c, st, 124, 123, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 125);
        cqRect(c, st, 125, 124, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 126);
        cqExtrude(c, st, 126, 125, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 126, "extrude");
        atStep(st, 127);
        cqCut(c, st, 127, 122, {"toCut": {"wp": 126}, "clean": true, "tol": undefined});
        stepVol(c, st, 127, "cut");
        atStep(st, 128);
        cqCenter(c, st, 128, 123, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 129);
        cqRect(c, st, 129, 128, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 130);
        cqExtrude(c, st, 130, 129, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 130, "extrude");
        atStep(st, 131);
        cqCut(c, st, 131, 127, {"toCut": {"wp": 130}, "clean": true, "tol": undefined});
        stepVol(c, st, 131, "cut");
        atStep(st, 132);
        cqWorkplaneNew(c, st, 132, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 133);
        cqCenter(c, st, 133, 132, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 134);
        cqRect(c, st, 134, 133, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 135);
        cqExtrude(c, st, 135, 134, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 135, "extrude");
        atStep(st, 136);
        cqCut(c, st, 136, 131, {"toCut": {"wp": 135}, "clean": true, "tol": undefined});
        stepVol(c, st, 136, "cut");
        atStep(st, 137);
        cqCenter(c, st, 137, 132, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 138);
        cqRect(c, st, 138, 137, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 139);
        cqExtrude(c, st, 139, 138, {"until": 2.002, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 139, "extrude");
        atStep(st, 140);
        cqCut(c, st, 140, 136, {"toCut": {"wp": 139}, "clean": true, "tol": undefined});
        stepVol(c, st, 140, "cut");
        atStep(st, 141);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 142);
        cqRect(c, st, 142, 141, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 143);
        cqExtrude(c, st, 143, 142, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 143, "extrude");
        atStep(st, 144);
        cqWorkplaneNew(c, st, 144, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 145);
        cqCenter(c, st, 145, 144, {"x": -4.1602540378443855, "y": -5.5});
        atStep(st, 146);
        cqRect(c, st, 146, 145, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 147);
        cqExtrude(c, st, 147, 146, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 147, "extrude");
        atStep(st, 148);
        cqCut(c, st, 148, 143, {"toCut": {"wp": 147}, "clean": true, "tol": undefined});
        stepVol(c, st, 148, "cut");
        atStep(st, 149);
        cqWorkplaneNew(c, st, 149, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 150);
        cqCenter(c, st, 150, 149, {"x": -4.1602540378443855, "y": -0.5});
        atStep(st, 151);
        cqRect(c, st, 151, 150, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 152);
        cqExtrude(c, st, 152, 151, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 152, "extrude");
        atStep(st, 153);
        cqCut(c, st, 153, 148, {"toCut": {"wp": 152}, "clean": true, "tol": undefined});
        stepVol(c, st, 153, "cut");
        atStep(st, 154);
        cqWorkplaneNew(c, st, 154, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 155);
        cqCenter(c, st, 155, 154, {"x": -4.1602540378443855, "y": 4.5});
        atStep(st, 156);
        cqRect(c, st, 156, 155, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 157);
        cqExtrude(c, st, 157, 156, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 157, "extrude");
        atStep(st, 158);
        cqCut(c, st, 158, 153, {"toCut": {"wp": 157}, "clean": true, "tol": undefined});
        stepVol(c, st, 158, "cut");
        atStep(st, 159);
        cqWorkplaneNew(c, st, 159, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 160);
        cqCenter(c, st, 160, 159, {"x": 0.8397459621556145, "y": -5.5});
        atStep(st, 161);
        cqRect(c, st, 161, 160, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 162);
        cqExtrude(c, st, 162, 161, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 162, "extrude");
        atStep(st, 163);
        cqCut(c, st, 163, 158, {"toCut": {"wp": 162}, "clean": true, "tol": undefined});
        stepVol(c, st, 163, "cut");
        atStep(st, 164);
        cqWorkplaneNew(c, st, 164, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 165);
        cqCenter(c, st, 165, 164, {"x": 0.8397459621556145, "y": -0.5});
        atStep(st, 166);
        cqRect(c, st, 166, 165, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 167);
        cqExtrude(c, st, 167, 166, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 167, "extrude");
        atStep(st, 168);
        cqCut(c, st, 168, 163, {"toCut": {"wp": 167}, "clean": true, "tol": undefined});
        stepVol(c, st, 168, "cut");
        atStep(st, 169);
        cqWorkplaneNew(c, st, 169, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 170);
        cqCenter(c, st, 170, 169, {"x": 0.8397459621556145, "y": 4.5});
        atStep(st, 171);
        cqRect(c, st, 171, 170, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 172);
        cqExtrude(c, st, 172, 171, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 172, "extrude");
        atStep(st, 173);
        cqCut(c, st, 173, 168, {"toCut": {"wp": 172}, "clean": true, "tol": undefined});
        stepVol(c, st, 173, "cut");
        atStep(st, 174);
        cqWorkplaneNew(c, st, 174, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 175);
        cqCenter(c, st, 175, 174, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 176);
        cqRect(c, st, 176, 175, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 177);
        cqExtrude(c, st, 177, 176, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 177, "extrude");
        atStep(st, 178);
        cqUnion(c, st, 178, 173, {"toUnion": {"wp": 177}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 178, "union");
        atStep(st, 179);
        cqWorkplaneNew(c, st, 179, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 180);
        cqCenter(c, st, 180, 179, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 181);
        cqRect(c, st, 181, 180, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 182);
        cqExtrude(c, st, 182, 181, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 182, "extrude");
        atStep(st, 183);
        cqUnion(c, st, 183, 178, {"toUnion": {"wp": 182}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 183, "union");
        atStep(st, 184);
        cqWorkplaneNew(c, st, 184, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 185);
        cqCenter(c, st, 185, 184, {"x": 0, "y": 4});
        atStep(st, 186);
        cqCircle(c, st, 186, 185, {"radius": 1.5, "forConstruction": false});
        atStep(st, 187);
        cqExtrude(c, st, 187, 186, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 187, "extrude");
        atStep(st, 188);
        cqCut(c, st, 188, 183, {"toCut": {"wp": 187}, "clean": true, "tol": undefined});
        stepVol(c, st, 188, "cut");
        atStep(st, 189);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 190);
        cqCenter(c, st, 190, 189, {"x": 0, "y": -4});
        atStep(st, 191);
        cqCircle(c, st, 191, 190, {"radius": 1.5, "forConstruction": false});
        atStep(st, 192);
        cqExtrude(c, st, 192, 191, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 192, "extrude");
        atStep(st, 193);
        cqCut(c, st, 193, 188, {"toCut": {"wp": 192}, "clean": true, "tol": undefined});
        stepVol(c, st, 193, "cut");
        atStep(st, 194);
        cqWorkplaneNew(c, st, 194, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 195);
        cqRect(c, st, 195, 194, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 196);
        cqExtrude(c, st, 196, 195, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 196, "extrude");
        atStep(st, 197);
        cqWorkplaneNew(c, st, 197, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 198);
        cqCenter(c, st, 198, 197, {"x": -4.1602540378443855, "y": -5.5});
        atStep(st, 199);
        cqRect(c, st, 199, 198, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 200);
        cqExtrude(c, st, 200, 199, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 200, "extrude");
        atStep(st, 201);
        cqCut(c, st, 201, 196, {"toCut": {"wp": 200}, "clean": true, "tol": undefined});
        stepVol(c, st, 201, "cut");
        atStep(st, 202);
        cqWorkplaneNew(c, st, 202, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 203);
        cqCenter(c, st, 203, 202, {"x": -4.1602540378443855, "y": -0.5});
        atStep(st, 204);
        cqRect(c, st, 204, 203, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 205);
        cqExtrude(c, st, 205, 204, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 205, "extrude");
        atStep(st, 206);
        cqCut(c, st, 206, 201, {"toCut": {"wp": 205}, "clean": true, "tol": undefined});
        stepVol(c, st, 206, "cut");
        atStep(st, 207);
        cqWorkplaneNew(c, st, 207, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 208);
        cqCenter(c, st, 208, 207, {"x": -4.1602540378443855, "y": 4.5});
        atStep(st, 209);
        cqRect(c, st, 209, 208, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 210);
        cqExtrude(c, st, 210, 209, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 210, "extrude");
        atStep(st, 211);
        cqCut(c, st, 211, 206, {"toCut": {"wp": 210}, "clean": true, "tol": undefined});
        stepVol(c, st, 211, "cut");
        atStep(st, 212);
        cqWorkplaneNew(c, st, 212, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 213);
        cqCenter(c, st, 213, 212, {"x": 0.8397459621556145, "y": -5.5});
        atStep(st, 214);
        cqRect(c, st, 214, 213, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 215);
        cqExtrude(c, st, 215, 214, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 215, "extrude");
        atStep(st, 216);
        cqCut(c, st, 216, 211, {"toCut": {"wp": 215}, "clean": true, "tol": undefined});
        stepVol(c, st, 216, "cut");
        atStep(st, 217);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 218);
        cqCenter(c, st, 218, 217, {"x": 0.8397459621556145, "y": -0.5});
        atStep(st, 219);
        cqRect(c, st, 219, 218, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 220);
        cqExtrude(c, st, 220, 219, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 220, "extrude");
        atStep(st, 221);
        cqCut(c, st, 221, 216, {"toCut": {"wp": 220}, "clean": true, "tol": undefined});
        stepVol(c, st, 221, "cut");
        atStep(st, 222);
        cqWorkplaneNew(c, st, 222, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 223);
        cqCenter(c, st, 223, 222, {"x": 0.8397459621556145, "y": 4.5});
        atStep(st, 224);
        cqRect(c, st, 224, 223, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 225);
        cqExtrude(c, st, 225, 224, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 225, "extrude");
        atStep(st, 226);
        cqCut(c, st, 226, 221, {"toCut": {"wp": 225}, "clean": true, "tol": undefined});
        stepVol(c, st, 226, "cut");
        atStep(st, 227);
        cqWorkplaneNew(c, st, 227, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 228);
        cqCenter(c, st, 228, 227, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 229);
        cqRect(c, st, 229, 228, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 230);
        cqExtrude(c, st, 230, 229, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 230, "extrude");
        atStep(st, 231);
        cqUnion(c, st, 231, 226, {"toUnion": {"wp": 230}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 231, "union");
        atStep(st, 232);
        cqWorkplaneNew(c, st, 232, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 233);
        cqCenter(c, st, 233, 232, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 234);
        cqRect(c, st, 234, 233, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 235);
        cqExtrude(c, st, 235, 234, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 235, "extrude");
        atStep(st, 236);
        cqUnion(c, st, 236, 231, {"toUnion": {"wp": 235}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 236, "union");
        atStep(st, 237);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 238);
        cqCenter(c, st, 238, 237, {"x": 0, "y": 4});
        atStep(st, 239);
        cqCircle(c, st, 239, 238, {"radius": 1.5, "forConstruction": false});
        atStep(st, 240);
        cqExtrude(c, st, 240, 239, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 240, "extrude");
        atStep(st, 241);
        cqCut(c, st, 241, 236, {"toCut": {"wp": 240}, "clean": true, "tol": undefined});
        stepVol(c, st, 241, "cut");
        atStep(st, 242);
        cqWorkplaneNew(c, st, 242, {"plane": {"origin": [5.302876193624534e-16, 8.660254037844386, 10], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [-6.123233995736766e-17, -1, 0]}, "obj": undefined});
        atStep(st, 243);
        cqCenter(c, st, 243, 242, {"x": 0, "y": -4});
        atStep(st, 244);
        cqCircle(c, st, 244, 243, {"radius": 1.5, "forConstruction": false});
        atStep(st, 245);
        cqExtrude(c, st, 245, 244, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 245, "extrude");
        atStep(st, 246);
        cqCut(c, st, 246, 241, {"toCut": {"wp": 245}, "clean": true, "tol": undefined});
        stepVol(c, st, 246, "cut");
        atStep(st, 247);
        cqUnion(c, st, 247, 193, {"toUnion": {"wp": 246}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 247, "union");
        atStep(st, 248);
        cqWorkplaneNew(c, st, 248, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 249);
        cqRect(c, st, 249, 248, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 250);
        cqExtrude(c, st, 250, 249, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 250, "extrude");
        atStep(st, 251);
        cqWorkplaneNew(c, st, 251, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 252);
        cqCenter(c, st, 252, 251, {"x": -4.1602540378443855, "y": -5.5});
        atStep(st, 253);
        cqRect(c, st, 253, 252, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 254);
        cqExtrude(c, st, 254, 253, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        atStep(st, 255);
        cqCut(c, st, 255, 250, {"toCut": {"wp": 254}, "clean": true, "tol": undefined});
        stepVol(c, st, 255, "cut");
        atStep(st, 256);
        cqWorkplaneNew(c, st, 256, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 257);
        cqCenter(c, st, 257, 256, {"x": -4.1602540378443855, "y": -0.5});
        atStep(st, 258);
        cqRect(c, st, 258, 257, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 259);
        cqExtrude(c, st, 259, 258, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 259, "extrude");
        atStep(st, 260);
        cqCut(c, st, 260, 255, {"toCut": {"wp": 259}, "clean": true, "tol": undefined});
        stepVol(c, st, 260, "cut");
        atStep(st, 261);
        cqWorkplaneNew(c, st, 261, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 262);
        cqCenter(c, st, 262, 261, {"x": -4.1602540378443855, "y": 4.5});
        atStep(st, 263);
        cqRect(c, st, 263, 262, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 264);
        cqExtrude(c, st, 264, 263, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 264, "extrude");
        atStep(st, 265);
        cqCut(c, st, 265, 260, {"toCut": {"wp": 264}, "clean": true, "tol": undefined});
        stepVol(c, st, 265, "cut");
        atStep(st, 266);
        cqWorkplaneNew(c, st, 266, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 267);
        cqCenter(c, st, 267, 266, {"x": 0.8397459621556145, "y": -5.5});
        atStep(st, 268);
        cqRect(c, st, 268, 267, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 269);
        cqExtrude(c, st, 269, 268, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 269, "extrude");
        atStep(st, 270);
        cqCut(c, st, 270, 265, {"toCut": {"wp": 269}, "clean": true, "tol": undefined});
        stepVol(c, st, 270, "cut");
        atStep(st, 271);
        cqWorkplaneNew(c, st, 271, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 272);
        cqCenter(c, st, 272, 271, {"x": 0.8397459621556145, "y": -0.5});
        atStep(st, 273);
        cqRect(c, st, 273, 272, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 274);
        cqExtrude(c, st, 274, 273, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 274, "extrude");
        atStep(st, 275);
        cqCut(c, st, 275, 270, {"toCut": {"wp": 274}, "clean": true, "tol": undefined});
        stepVol(c, st, 275, "cut");
        atStep(st, 276);
        cqWorkplaneNew(c, st, 276, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 277);
        cqCenter(c, st, 277, 276, {"x": 0.8397459621556145, "y": 4.5});
        atStep(st, 278);
        cqRect(c, st, 278, 277, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 279);
        cqExtrude(c, st, 279, 278, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 279, "extrude");
        atStep(st, 280);
        cqCut(c, st, 280, 275, {"toCut": {"wp": 279}, "clean": true, "tol": undefined});
        stepVol(c, st, 280, "cut");
        atStep(st, 281);
        cqWorkplaneNew(c, st, 281, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 282);
        cqCenter(c, st, 282, 281, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 283);
        cqRect(c, st, 283, 282, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 284);
        cqExtrude(c, st, 284, 283, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 284, "extrude");
        atStep(st, 285);
        cqUnion(c, st, 285, 280, {"toUnion": {"wp": 284}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 285, "union");
        atStep(st, 286);
        cqWorkplaneNew(c, st, 286, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 287);
        cqCenter(c, st, 287, 286, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 288);
        cqRect(c, st, 288, 287, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 289);
        cqExtrude(c, st, 289, 288, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 289, "extrude");
        atStep(st, 290);
        cqUnion(c, st, 290, 285, {"toUnion": {"wp": 289}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 290, "union");
        atStep(st, 291);
        cqWorkplaneNew(c, st, 291, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 292);
        cqCenter(c, st, 292, 291, {"x": 0, "y": 4});
        atStep(st, 293);
        cqCircle(c, st, 293, 292, {"radius": 1.5, "forConstruction": false});
        atStep(st, 294);
        cqExtrude(c, st, 294, 293, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 294, "extrude");
        atStep(st, 295);
        cqCut(c, st, 295, 290, {"toCut": {"wp": 294}, "clean": true, "tol": undefined});
        stepVol(c, st, 295, "cut");
        atStep(st, 296);
        cqWorkplaneNew(c, st, 296, {"plane": {"origin": [-7.5, 4.330127018922192, 10], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 297);
        cqCenter(c, st, 297, 296, {"x": 0, "y": -4});
        atStep(st, 298);
        cqCircle(c, st, 298, 297, {"radius": 1.5, "forConstruction": false});
        atStep(st, 299);
        cqExtrude(c, st, 299, 298, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 299, "extrude");
        atStep(st, 300);
        cqCut(c, st, 300, 295, {"toCut": {"wp": 299}, "clean": true, "tol": undefined});
        stepVol(c, st, 300, "cut");
        atStep(st, 301);
        cqUnion(c, st, 301, 247, {"toUnion": {"wp": 300}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 301, "union");
        atStep(st, 302);
        cqWorkplaneNew(c, st, 302, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 303);
        cqRect(c, st, 303, 302, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 304);
        cqExtrude(c, st, 304, 303, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 304, "extrude");
        atStep(st, 305);
        cqWorkplaneNew(c, st, 305, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 306);
        cqCenter(c, st, 306, 305, {"x": -4.1602540378443855, "y": -5.5});
        atStep(st, 307);
        cqRect(c, st, 307, 306, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 308);
        cqExtrude(c, st, 308, 307, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 308, "extrude");
        atStep(st, 309);
        cqCut(c, st, 309, 304, {"toCut": {"wp": 308}, "clean": true, "tol": undefined});
        stepVol(c, st, 309, "cut");
        atStep(st, 310);
        cqWorkplaneNew(c, st, 310, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 311);
        cqCenter(c, st, 311, 310, {"x": -4.1602540378443855, "y": -0.5});
        atStep(st, 312);
        cqRect(c, st, 312, 311, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 313);
        cqExtrude(c, st, 313, 312, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 313, "extrude");
        atStep(st, 314);
        cqCut(c, st, 314, 309, {"toCut": {"wp": 313}, "clean": true, "tol": undefined});
        stepVol(c, st, 314, "cut");
        atStep(st, 315);
        cqWorkplaneNew(c, st, 315, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 316);
        cqCenter(c, st, 316, 315, {"x": -4.1602540378443855, "y": 4.5});
        atStep(st, 317);
        cqRect(c, st, 317, 316, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 318);
        cqExtrude(c, st, 318, 317, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 318, "extrude");
        atStep(st, 319);
        cqCut(c, st, 319, 314, {"toCut": {"wp": 318}, "clean": true, "tol": undefined});
        stepVol(c, st, 319, "cut");
        atStep(st, 320);
        cqWorkplaneNew(c, st, 320, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 321);
        cqCenter(c, st, 321, 320, {"x": 0.8397459621556145, "y": -5.5});
        atStep(st, 322);
        cqRect(c, st, 322, 321, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 323);
        cqExtrude(c, st, 323, 322, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 323, "extrude");
        atStep(st, 324);
        cqCut(c, st, 324, 319, {"toCut": {"wp": 323}, "clean": true, "tol": undefined});
        stepVol(c, st, 324, "cut");
        atStep(st, 325);
        cqWorkplaneNew(c, st, 325, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 326);
        cqCenter(c, st, 326, 325, {"x": 0.8397459621556145, "y": -0.5});
        atStep(st, 327);
        cqRect(c, st, 327, 326, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 328);
        cqExtrude(c, st, 328, 327, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 328, "extrude");
        atStep(st, 329);
        cqCut(c, st, 329, 324, {"toCut": {"wp": 328}, "clean": true, "tol": undefined});
        stepVol(c, st, 329, "cut");
        atStep(st, 330);
        cqWorkplaneNew(c, st, 330, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 331);
        cqCenter(c, st, 331, 330, {"x": 0.8397459621556145, "y": 4.5});
        atStep(st, 332);
        cqRect(c, st, 332, 331, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 333);
        cqExtrude(c, st, 333, 332, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 333, "extrude");
        atStep(st, 334);
        cqCut(c, st, 334, 329, {"toCut": {"wp": 333}, "clean": true, "tol": undefined});
        stepVol(c, st, 334, "cut");
        atStep(st, 335);
        cqWorkplaneNew(c, st, 335, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 336);
        cqCenter(c, st, 336, 335, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 337);
        cqRect(c, st, 337, 336, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 338);
        cqExtrude(c, st, 338, 337, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 338, "extrude");
        atStep(st, 339);
        cqUnion(c, st, 339, 334, {"toUnion": {"wp": 338}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 339, "union");
        atStep(st, 340);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 341);
        cqCenter(c, st, 341, 340, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 342);
        cqRect(c, st, 342, 341, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 343);
        cqExtrude(c, st, 343, 342, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 343, "extrude");
        atStep(st, 344);
        cqUnion(c, st, 344, 339, {"toUnion": {"wp": 343}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 344, "union");
        atStep(st, 345);
        cqWorkplaneNew(c, st, 345, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 346);
        cqCenter(c, st, 346, 345, {"x": 0, "y": 4});
        atStep(st, 347);
        cqCircle(c, st, 347, 346, {"radius": 1.5, "forConstruction": false});
        atStep(st, 348);
        cqExtrude(c, st, 348, 347, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 348, "extrude");
        atStep(st, 349);
        cqCut(c, st, 349, 344, {"toCut": {"wp": 348}, "clean": true, "tol": undefined});
        stepVol(c, st, 349, "cut");
        atStep(st, 350);
        cqWorkplaneNew(c, st, 350, {"plane": {"origin": [-7.499999999999999, -4.330127018922194, 10], "xDir": [0.5000000000000001, -0.8660254037844386, 0], "zDir": [0.8660254037844386, 0.5000000000000001, 0]}, "obj": undefined});
        atStep(st, 351);
        cqCenter(c, st, 351, 350, {"x": 0, "y": -4});
        atStep(st, 352);
        cqCircle(c, st, 352, 351, {"radius": 1.5, "forConstruction": false});
        atStep(st, 353);
        cqExtrude(c, st, 353, 352, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 353, "extrude");
        atStep(st, 354);
        cqCut(c, st, 354, 349, {"toCut": {"wp": 353}, "clean": true, "tol": undefined});
        stepVol(c, st, 354, "cut");
        atStep(st, 355);
        cqUnion(c, st, 355, 301, {"toUnion": {"wp": 354}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 355, "union");
        atStep(st, 356);
        cqWorkplaneNew(c, st, 356, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 357);
        cqRect(c, st, 357, 356, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 358);
        cqExtrude(c, st, 358, 357, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 358, "extrude");
        atStep(st, 359);
        cqWorkplaneNew(c, st, 359, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 360);
        cqCenter(c, st, 360, 359, {"x": -4.1602540378443855, "y": -5.5});
        atStep(st, 361);
        cqRect(c, st, 361, 360, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 362);
        cqExtrude(c, st, 362, 361, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        atStep(st, 363);
        cqCut(c, st, 363, 358, {"toCut": {"wp": 362}, "clean": true, "tol": undefined});
        stepVol(c, st, 363, "cut");
        atStep(st, 364);
        cqWorkplaneNew(c, st, 364, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 365);
        cqCenter(c, st, 365, 364, {"x": -4.1602540378443855, "y": -0.5});
        atStep(st, 366);
        cqRect(c, st, 366, 365, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 367);
        cqExtrude(c, st, 367, 366, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 367, "extrude");
        atStep(st, 368);
        cqCut(c, st, 368, 363, {"toCut": {"wp": 367}, "clean": true, "tol": undefined});
        stepVol(c, st, 368, "cut");
        atStep(st, 369);
        cqWorkplaneNew(c, st, 369, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 370);
        cqCenter(c, st, 370, 369, {"x": -4.1602540378443855, "y": 4.5});
        atStep(st, 371);
        cqRect(c, st, 371, 370, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 372);
        cqExtrude(c, st, 372, 371, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 372, "extrude");
        atStep(st, 373);
        cqCut(c, st, 373, 368, {"toCut": {"wp": 372}, "clean": true, "tol": undefined});
        stepVol(c, st, 373, "cut");
        atStep(st, 374);
        cqWorkplaneNew(c, st, 374, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 375);
        cqCenter(c, st, 375, 374, {"x": 0.8397459621556145, "y": -5.5});
        atStep(st, 376);
        cqRect(c, st, 376, 375, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 377);
        cqExtrude(c, st, 377, 376, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 377, "extrude");
        atStep(st, 378);
        cqCut(c, st, 378, 373, {"toCut": {"wp": 377}, "clean": true, "tol": undefined});
        stepVol(c, st, 378, "cut");
        atStep(st, 379);
        cqWorkplaneNew(c, st, 379, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 380);
        cqCenter(c, st, 380, 379, {"x": 0.8397459621556145, "y": -0.5});
        atStep(st, 381);
        cqRect(c, st, 381, 380, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 382);
        cqExtrude(c, st, 382, 381, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 382, "extrude");
        atStep(st, 383);
        cqCut(c, st, 383, 378, {"toCut": {"wp": 382}, "clean": true, "tol": undefined});
        stepVol(c, st, 383, "cut");
        atStep(st, 384);
        cqWorkplaneNew(c, st, 384, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 385);
        cqCenter(c, st, 385, 384, {"x": 0.8397459621556145, "y": 4.5});
        atStep(st, 386);
        cqRect(c, st, 386, 385, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 387);
        cqExtrude(c, st, 387, 386, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 387, "extrude");
        atStep(st, 388);
        cqCut(c, st, 388, 383, {"toCut": {"wp": 387}, "clean": true, "tol": undefined});
        stepVol(c, st, 388, "cut");
        atStep(st, 389);
        cqWorkplaneNew(c, st, 389, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 390);
        cqCenter(c, st, 390, 389, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 391);
        cqRect(c, st, 391, 390, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 392);
        cqExtrude(c, st, 392, 391, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 392, "extrude");
        atStep(st, 393);
        cqUnion(c, st, 393, 388, {"toUnion": {"wp": 392}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 393, "union");
        atStep(st, 394);
        cqWorkplaneNew(c, st, 394, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 395);
        cqCenter(c, st, 395, 394, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 396);
        cqRect(c, st, 396, 395, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 397);
        cqExtrude(c, st, 397, 396, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 397, "extrude");
        atStep(st, 398);
        cqUnion(c, st, 398, 393, {"toUnion": {"wp": 397}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 398, "union");
        atStep(st, 399);
        cqWorkplaneNew(c, st, 399, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 400);
        cqCenter(c, st, 400, 399, {"x": 0, "y": 4});
        atStep(st, 401);
        cqCircle(c, st, 401, 400, {"radius": 1.5, "forConstruction": false});
        atStep(st, 402);
        cqExtrude(c, st, 402, 401, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 402, "extrude");
        atStep(st, 403);
        cqCut(c, st, 403, 398, {"toCut": {"wp": 402}, "clean": true, "tol": undefined});
        stepVol(c, st, 403, "cut");
        atStep(st, 404);
        cqWorkplaneNew(c, st, 404, {"plane": {"origin": [-1.59086285808736e-15, -8.660254037844386, 10], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [1.8369701987210294e-16, 1, 0]}, "obj": undefined});
        atStep(st, 405);
        cqCenter(c, st, 405, 404, {"x": 0, "y": -4});
        atStep(st, 406);
        cqCircle(c, st, 406, 405, {"radius": 1.5, "forConstruction": false});
        atStep(st, 407);
        cqExtrude(c, st, 407, 406, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 407, "extrude");
        atStep(st, 408);
        cqCut(c, st, 408, 403, {"toCut": {"wp": 407}, "clean": true, "tol": undefined});
        stepVol(c, st, 408, "cut");
        atStep(st, 409);
        cqUnion(c, st, 409, 355, {"toUnion": {"wp": 408}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 409, "union");
        atStep(st, 410);
        cqWorkplaneNew(c, st, 410, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 411);
        cqRect(c, st, 411, 410, {"xLen": 13.320508075688773, "yLen": 16, "centered": true, "forConstruction": false});
        atStep(st, 412);
        cqExtrude(c, st, 412, 411, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 412, "extrude");
        atStep(st, 413);
        cqWorkplaneNew(c, st, 413, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 414);
        cqCenter(c, st, 414, 413, {"x": -4.1602540378443855, "y": -5.5});
        atStep(st, 415);
        cqRect(c, st, 415, 414, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 416);
        cqExtrude(c, st, 416, 415, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 416, "extrude");
        atStep(st, 417);
        cqCut(c, st, 417, 412, {"toCut": {"wp": 416}, "clean": true, "tol": undefined});
        stepVol(c, st, 417, "cut");
        atStep(st, 418);
        cqWorkplaneNew(c, st, 418, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 419);
        cqCenter(c, st, 419, 418, {"x": -4.1602540378443855, "y": -0.5});
        atStep(st, 420);
        cqRect(c, st, 420, 419, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 421);
        cqExtrude(c, st, 421, 420, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 421, "extrude");
        atStep(st, 422);
        cqCut(c, st, 422, 417, {"toCut": {"wp": 421}, "clean": true, "tol": undefined});
        stepVol(c, st, 422, "cut");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 424);
        cqCenter(c, st, 424, 423, {"x": -4.1602540378443855, "y": 4.5});
        atStep(st, 425);
        cqRect(c, st, 425, 424, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 426);
        cqExtrude(c, st, 426, 425, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 427);
        cqCut(c, st, 427, 422, {"toCut": {"wp": 426}, "clean": true, "tol": undefined});
        stepVol(c, st, 427, "cut");
        atStep(st, 428);
        cqWorkplaneNew(c, st, 428, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 429);
        cqCenter(c, st, 429, 428, {"x": 0.8397459621556145, "y": -5.5});
        atStep(st, 430);
        cqRect(c, st, 430, 429, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 431);
        cqExtrude(c, st, 431, 430, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 431, "extrude");
        atStep(st, 432);
        cqCut(c, st, 432, 427, {"toCut": {"wp": 431}, "clean": true, "tol": undefined});
        stepVol(c, st, 432, "cut");
        atStep(st, 433);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 434);
        cqCenter(c, st, 434, 433, {"x": 0.8397459621556145, "y": -0.5});
        atStep(st, 435);
        cqRect(c, st, 435, 434, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 436);
        cqExtrude(c, st, 436, 435, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 436, "extrude");
        atStep(st, 437);
        cqCut(c, st, 437, 432, {"toCut": {"wp": 436}, "clean": true, "tol": undefined});
        stepVol(c, st, 437, "cut");
        atStep(st, 438);
        cqWorkplaneNew(c, st, 438, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 439);
        cqCenter(c, st, 439, 438, {"x": 0.8397459621556145, "y": 4.5});
        atStep(st, 440);
        cqRect(c, st, 440, 439, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 441);
        cqExtrude(c, st, 441, 440, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 441, "extrude");
        atStep(st, 442);
        cqCut(c, st, 442, 437, {"toCut": {"wp": 441}, "clean": true, "tol": undefined});
        stepVol(c, st, 442, "cut");
        atStep(st, 443);
        cqWorkplaneNew(c, st, 443, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 444);
        cqCenter(c, st, 444, 443, {"x": -3.3301270189221928, "y": -6.5});
        atStep(st, 445);
        cqRect(c, st, 445, 444, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 446);
        cqExtrude(c, st, 446, 445, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 446, "extrude");
        atStep(st, 447);
        cqUnion(c, st, 447, 442, {"toUnion": {"wp": 446}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 447, "union");
        atStep(st, 448);
        cqWorkplaneNew(c, st, 448, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 449);
        cqCenter(c, st, 449, 448, {"x": 3.3301270189221928, "y": -6.5});
        atStep(st, 450);
        cqRect(c, st, 450, 449, {"xLen": 2, "yLen": 3, "centered": true, "forConstruction": false});
        atStep(st, 451);
        cqExtrude(c, st, 451, 450, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 451, "extrude");
        atStep(st, 452);
        cqUnion(c, st, 452, 447, {"toUnion": {"wp": 451}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 452, "union");
        atStep(st, 453);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 454);
        cqCenter(c, st, 454, 453, {"x": 0, "y": 4});
        atStep(st, 455);
        cqCircle(c, st, 455, 454, {"radius": 1.5, "forConstruction": false});
        atStep(st, 456);
        cqExtrude(c, st, 456, 455, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 456, "extrude");
        atStep(st, 457);
        cqCut(c, st, 457, 452, {"toCut": {"wp": 456}, "clean": true, "tol": undefined});
        stepVol(c, st, 457, "cut");
        atStep(st, 458);
        cqWorkplaneNew(c, st, 458, {"plane": {"origin": [7.4999999999999964, -4.330127018922196, 10], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [-0.8660254037844384, 0.5000000000000004, 0]}, "obj": undefined});
        atStep(st, 459);
        cqCenter(c, st, 459, 458, {"x": 0, "y": -4});
        atStep(st, 460);
        cqCircle(c, st, 460, 459, {"radius": 1.5, "forConstruction": false});
        atStep(st, 461);
        cqExtrude(c, st, 461, 460, {"until": 2.001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 461, "extrude");
        atStep(st, 462);
        cqCut(c, st, 462, 457, {"toCut": {"wp": 461}, "clean": true, "tol": undefined});
        stepVol(c, st, 462, "cut");
        atStep(st, 463);
        cqUnion(c, st, 463, 409, {"toUnion": {"wp": 462}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 463, "union");
        atStep(st, 464);
        cqUnion(c, st, 464, 140, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 464, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 464}), {"i": 3521});
    } catch (e) {
        return {"i": 3521, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_3521(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c3521"] = r; }
    return out;
}
