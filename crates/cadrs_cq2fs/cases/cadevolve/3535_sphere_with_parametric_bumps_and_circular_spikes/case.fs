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

const case_3535 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqSphere(c, st, 2, 1, {"radius": 10, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "sphere");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0.3402667097183954, -6.802525640897676, 6.472163672045615], "xDir": [0, -0.6892958706873293, -0.7244799532446682], "zDir": [0.03621518123212254, -0.7240047053828832, 0.6888437030500962]}, "obj": undefined});
        atStep(st, 4);
        cqSphere(c, st, 4, 3, {"radius": 1.2086148326889026, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 4, "sphere");
        atStep(st, 5);
        cqWorkplaneNew(c, st, 5, {"plane": {"origin": [0.3402667097183954, -6.802525640897676, 6.472163672045615], "xDir": [0, -0.6892958706873293, -0.7244799532446682], "zDir": [0.03621518123212254, -0.7240047053828832, 0.6888437030500962]}, "obj": undefined});
        atStep(st, 6);
        cqRect(c, st, 6, 5, {"xLen": 2.417229665377805, "yLen": 2.417229665377805, "centered": true, "forConstruction": false});
        atStep(st, 7);
        cqExtrude(c, st, 7, 6, {"until": -2.417229665377805, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 7, "extrude");
        atStep(st, 8);
        cqCut(c, st, 8, 4, {"toCut": {"wp": 7}, "clean": true, "tol": undefined});
        stepVol(c, st, 8, "cut");
        atStep(st, 9);
        cqUnion(c, st, 9, 2, {"toUnion": {"wp": 8}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 9, "union");
        atStep(st, 10);
        cqWorkplaneNew(c, st, 10, {"plane": {"origin": [-0.4997549130535574, 8.91078301958033, -1.4359981596972635], "xDir": [-1.7347234759768069e-18, 0.15910016577182776, 0.98726244598454], "zDir": [-0.0552852278365468, 0.98575252903364, -0.15885683833831]}, "obj": undefined});
        atStep(st, 11);
        cqSphere(c, st, 11, 10, {"radius": 1.9208518220778703, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 11, "sphere");
        atStep(st, 12);
        cqWorkplaneNew(c, st, 12, {"plane": {"origin": [-0.4997549130535574, 8.91078301958033, -1.4359981596972635], "xDir": [-1.7347234759768069e-18, 0.15910016577182776, 0.98726244598454], "zDir": [-0.0552852278365468, 0.98575252903364, -0.15885683833831]}, "obj": undefined});
        atStep(st, 13);
        cqRect(c, st, 13, 12, {"xLen": 3.8417036441557406, "yLen": 3.8417036441557406, "centered": true, "forConstruction": false});
        atStep(st, 14);
        cqExtrude(c, st, 14, 13, {"until": -3.8417036441557406, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 14, "extrude");
        atStep(st, 15);
        cqCut(c, st, 15, 11, {"toCut": {"wp": 14}, "clean": true, "tol": undefined});
        stepVol(c, st, 15, "cut");
        atStep(st, 16);
        cqUnion(c, st, 16, 9, {"toUnion": {"wp": 15}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 16, "union");
        atStep(st, 17);
        cqWorkplaneNew(c, st, 17, {"plane": {"origin": [-7.771713249622146, 5.286353726815838, 0.21200075007703548], "xDir": [-0.562425597658445, -0.8268478984060738, 1.7347234759768069e-18], "zDir": [-0.8266376549552071, 0.5622825891332551, 0.02254944273721704]}, "obj": undefined});
        atStep(st, 18);
        cqSphere(c, st, 18, 17, {"radius": 1.1968080499713745, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 18, "sphere");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [-7.771713249622146, 5.286353726815838, 0.21200075007703548], "xDir": [-0.562425597658445, -0.8268478984060738, 1.7347234759768069e-18], "zDir": [-0.8266376549552071, 0.5622825891332551, 0.02254944273721704]}, "obj": undefined});
        atStep(st, 20);
        cqRect(c, st, 20, 19, {"xLen": 2.393616099942749, "yLen": 2.393616099942749, "centered": true, "forConstruction": false});
        atStep(st, 21);
        cqExtrude(c, st, 21, 20, {"until": -2.393616099942749, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 21, "extrude");
        atStep(st, 22);
        cqCut(c, st, 22, 18, {"toCut": {"wp": 21}, "clean": true, "tol": undefined});
        stepVol(c, st, 22, "cut");
        atStep(st, 23);
        cqUnion(c, st, 23, 16, {"toUnion": {"wp": 22}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 23, "union");
        atStep(st, 24);
        cqWorkplaneNew(c, st, 24, {"plane": {"origin": [-2.5462800182005, 7.314969707167532, 5.339819456626763], "xDir": [0, -0.5896036929829948, 0.8076926923160903], "zDir": [-0.2706573442500742, 0.7775461693372206, 0.5675971780695452]}, "obj": undefined});
        atStep(st, 25);
        cqSphere(c, st, 25, 24, {"radius": 1.184474965897386, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 25, "sphere");
        atStep(st, 26);
        cqWorkplaneNew(c, st, 26, {"plane": {"origin": [-2.5462800182005, 7.314969707167532, 5.339819456626763], "xDir": [0, -0.5896036929829948, 0.8076926923160903], "zDir": [-0.2706573442500742, 0.7775461693372206, 0.5675971780695452]}, "obj": undefined});
        atStep(st, 27);
        cqRect(c, st, 27, 26, {"xLen": 2.368949931794772, "yLen": 2.368949931794772, "centered": true, "forConstruction": false});
        atStep(st, 28);
        cqExtrude(c, st, 28, 27, {"until": -2.368949931794772, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 28, "extrude");
        atStep(st, 29);
        cqCut(c, st, 29, 25, {"toCut": {"wp": 28}, "clean": true, "tol": undefined});
        stepVol(c, st, 29, "cut");
        atStep(st, 30);
        cqUnion(c, st, 30, 23, {"toUnion": {"wp": 29}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 30, "union");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 31, {"plane": {"origin": [-8.139441557613955, -4.702630574626135, -0.4404730766537755], "xDir": [0.5002650059800455, -0.865872348439298, 0], "zDir": [-0.8649233473890101, -0.4997167126698907, -0.04680609169528838]}, "obj": undefined});
        atStep(st, 32);
        cqSphere(c, st, 32, 31, {"radius": 1.1788140944861367, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 32, "sphere");
        atStep(st, 33);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [-8.139441557613955, -4.702630574626135, -0.4404730766537755], "xDir": [0.5002650059800455, -0.865872348439298, 0], "zDir": [-0.8649233473890101, -0.4997167126698907, -0.04680609169528838]}, "obj": undefined});
        atStep(st, 34);
        cqRect(c, st, 34, 33, {"xLen": 2.3576281889722734, "yLen": 2.3576281889722734, "centered": true, "forConstruction": false});
        atStep(st, 35);
        cqExtrude(c, st, 35, 34, {"until": -2.3576281889722734, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 35, "extrude");
        atStep(st, 36);
        cqCut(c, st, 36, 32, {"toCut": {"wp": 35}, "clean": true, "tol": undefined});
        stepVol(c, st, 36, "cut");
        atStep(st, 37);
        cqUnion(c, st, 37, 30, {"toUnion": {"wp": 36}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 37, "union");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 38, {"plane": {"origin": [-5.296065279541875, -0.15600566099192523, 7.485567807710578], "xDir": [0.8163438911951341, 0, 0.5775661445309855], "zDir": [-0.5774825736506752, -0.017010845950426963, 0.8162257703906703]}, "obj": undefined});
        atStep(st, 39);
        cqSphere(c, st, 39, 38, {"radius": 1.6580949064429582, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 39, "sphere");
        atStep(st, 40);
        cqWorkplaneNew(c, st, 40, {"plane": {"origin": [-5.296065279541875, -0.15600566099192523, 7.485567807710578], "xDir": [0.8163438911951341, 0, 0.5775661445309855], "zDir": [-0.5774825736506752, -0.017010845950426963, 0.8162257703906703]}, "obj": undefined});
        atStep(st, 41);
        cqRect(c, st, 41, 40, {"xLen": 3.3161898128859164, "yLen": 3.3161898128859164, "centered": true, "forConstruction": false});
        atStep(st, 42);
        cqExtrude(c, st, 42, 41, {"until": -3.3161898128859164, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 42, "extrude");
        atStep(st, 43);
        cqCut(c, st, 43, 39, {"toCut": {"wp": 42}, "clean": true, "tol": undefined});
        stepVol(c, st, 43, "cut");
        atStep(st, 44);
        cqUnion(c, st, 44, 37, {"toUnion": {"wp": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 44, "union");
        atStep(st, 45);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [0.3016322866314382, -8.267280710275607, -4.011622454512636], "xDir": [-1.734723475976807e-18, 0.43655930939891846, -0.8996754800355179], "zDir": [0.032807050312592505, -0.8991911881825332, -0.43632431120059234]}, "obj": undefined});
        atStep(st, 46);
        cqSphere(c, st, 46, 45, {"radius": 1.6117399304465327, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 46, "sphere");
        atStep(st, 47);
        cqWorkplaneNew(c, st, 47, {"plane": {"origin": [0.3016322866314382, -8.267280710275607, -4.011622454512636], "xDir": [-1.734723475976807e-18, 0.43655930939891846, -0.8996754800355179], "zDir": [0.032807050312592505, -0.8991911881825332, -0.43632431120059234]}, "obj": undefined});
        atStep(st, 48);
        cqRect(c, st, 48, 47, {"xLen": 3.2234798608930655, "yLen": 3.2234798608930655, "centered": true, "forConstruction": false});
        atStep(st, 49);
        cqExtrude(c, st, 49, 48, {"until": -3.2234798608930655, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 49, "extrude");
        atStep(st, 50);
        cqCut(c, st, 50, 46, {"toCut": {"wp": 49}, "clean": true, "tol": undefined});
        stepVol(c, st, 50, "cut");
        atStep(st, 51);
        cqUnion(c, st, 51, 44, {"toUnion": {"wp": 50}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 51, "union");
        atStep(st, 52);
        cqWorkplaneNew(c, st, 52, {"plane": {"origin": [5.26849815016343, -0.5720842138581812, 7.577997442588448], "xDir": [0.8210652805740849, -3.469446951953614e-18, -0.5708343061132531], "zDir": [0.5697408604212175, -0.06186578090130776, 0.8194925119364802]}, "obj": undefined});
        atStep(st, 53);
        cqSphere(c, st, 53, 52, {"radius": 1.505633468983234, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 53, "sphere");
        atStep(st, 54);
        cqWorkplaneNew(c, st, 54, {"plane": {"origin": [5.26849815016343, -0.5720842138581812, 7.577997442588448], "xDir": [0.8210652805740849, -3.469446951953614e-18, -0.5708343061132531], "zDir": [0.5697408604212175, -0.06186578090130776, 0.8194925119364802]}, "obj": undefined});
        atStep(st, 55);
        cqRect(c, st, 55, 54, {"xLen": 3.011266937966468, "yLen": 3.011266937966468, "centered": true, "forConstruction": false});
        atStep(st, 56);
        cqExtrude(c, st, 56, 55, {"until": -3.011266937966468, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 56, "extrude");
        atStep(st, 57);
        cqCut(c, st, 57, 53, {"toCut": {"wp": 56}, "clean": true, "tol": undefined});
        stepVol(c, st, 57, "cut");
        atStep(st, 58);
        cqUnion(c, st, 58, 51, {"toUnion": {"wp": 57}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 58, "union");
        atStep(st, 59);
        cqWorkplaneNew(c, st, 59, {"plane": {"origin": [5.898601618879763, -4.164129851818661, 5.71209146476253], "xDir": [-0.6956583142662714, 2.7755575615628907e-17, 0.7183728208891325], "zDir": [0.6406927836299838, -0.4522983782493674, 0.6204344719931791]}, "obj": undefined});
        atStep(st, 60);
        cqSphere(c, st, 60, 59, {"radius": 1.5868017571231068, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 60, "sphere");
        atStep(st, 61);
        cqWorkplaneNew(c, st, 61, {"plane": {"origin": [5.898601618879763, -4.164129851818661, 5.71209146476253], "xDir": [-0.6956583142662714, 2.7755575615628907e-17, 0.7183728208891325], "zDir": [0.6406927836299838, -0.4522983782493674, 0.6204344719931791]}, "obj": undefined});
        atStep(st, 62);
        cqRect(c, st, 62, 61, {"xLen": 3.1736035142462136, "yLen": 3.1736035142462136, "centered": true, "forConstruction": false});
        atStep(st, 63);
        cqExtrude(c, st, 63, 62, {"until": -3.1736035142462136, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 63, "extrude");
        atStep(st, 64);
        cqCut(c, st, 64, 60, {"toCut": {"wp": 63}, "clean": true, "tol": undefined});
        stepVol(c, st, 64, "cut");
        atStep(st, 65);
        cqUnion(c, st, 65, 58, {"toUnion": {"wp": 64}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 65, "union");
        atStep(st, 66);
        cqWorkplaneNew(c, st, 66, {"plane": {"origin": [7.270425030353103, 5.331371288177342, -0.5030865043736761], "xDir": [-0.5913439193885829, 0.8064194745925652, 3.4694469519536138e-18], "zDir": [0.8051668931266966, 0.5904254068070824, -0.05571456909457329]}, "obj": undefined});
        atStep(st, 67);
        cqSphere(c, st, 67, 66, {"radius": 1.940576314259682, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 67, "sphere");
        atStep(st, 68);
        cqWorkplaneNew(c, st, 68, {"plane": {"origin": [7.270425030353103, 5.331371288177342, -0.5030865043736761], "xDir": [-0.5913439193885829, 0.8064194745925652, 3.4694469519536138e-18], "zDir": [0.8051668931266966, 0.5904254068070824, -0.05571456909457329]}, "obj": undefined});
        atStep(st, 69);
        cqRect(c, st, 69, 68, {"xLen": 3.881152628519364, "yLen": 3.881152628519364, "centered": true, "forConstruction": false});
        atStep(st, 70);
        cqExtrude(c, st, 70, 69, {"until": -3.881152628519364, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 70, "extrude");
        atStep(st, 71);
        cqCut(c, st, 71, 67, {"toCut": {"wp": 70}, "clean": true, "tol": undefined});
        stepVol(c, st, 71, "cut");
        atStep(st, 72);
        cqUnion(c, st, 72, 65, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [6.0045774047064855, -6.784178188662608, -0.4170148331599602], "xDir": [-0.7488218533642896, -0.6627713270231823, 3.469446951953613e-18], "zDir": [0.6620703402545689, -0.7480298543899003, -0.045980446894565936]}, "obj": undefined});
        atStep(st, 74);
        cqSphere(c, st, 74, 73, {"radius": 1.8612100871405888, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 74, "sphere");
        atStep(st, 75);
        cqWorkplaneNew(c, st, 75, {"plane": {"origin": [6.0045774047064855, -6.784178188662608, -0.4170148331599602], "xDir": [-0.7488218533642896, -0.6627713270231823, 3.469446951953613e-18], "zDir": [0.6620703402545689, -0.7480298543899003, -0.045980446894565936]}, "obj": undefined});
        atStep(st, 76);
        cqRect(c, st, 76, 75, {"xLen": 3.7224201742811776, "yLen": 3.7224201742811776, "centered": true, "forConstruction": false});
        atStep(st, 77);
        cqExtrude(c, st, 77, 76, {"until": -3.7224201742811776, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 77, "extrude");
        atStep(st, 78);
        cqCut(c, st, 78, 74, {"toCut": {"wp": 77}, "clean": true, "tol": undefined});
        stepVol(c, st, 78, "cut");
        atStep(st, 79);
        cqUnion(c, st, 79, 72, {"toUnion": {"wp": 78}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 79, "union");
        atStep(st, 80);
        cqWorkplaneNew(c, st, 80, {"plane": {"origin": [8.968997580448306, 0.7933637652116928, 0.8811683924273667], "xDir": [-0.09777528445368024, 0, 0.9952085177237996], "zDir": [0.991374502364304, 0.08769325678548116, 0.09739860767117858]}, "obj": undefined});
        atStep(st, 81);
        cqSphere(c, st, 81, 80, {"radius": 1.9059345200862647, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 81, "sphere");
        atStep(st, 82);
        cqWorkplaneNew(c, st, 82, {"plane": {"origin": [8.968997580448306, 0.7933637652116928, 0.8811683924273667], "xDir": [-0.09777528445368024, 0, 0.9952085177237996], "zDir": [0.991374502364304, 0.08769325678548116, 0.09739860767117858]}, "obj": undefined});
        atStep(st, 83);
        cqRect(c, st, 83, 82, {"xLen": 3.8118690401725295, "yLen": 3.8118690401725295, "centered": true, "forConstruction": false});
        atStep(st, 84);
        cqExtrude(c, st, 84, 83, {"until": -3.8118690401725295, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 84, "extrude");
        atStep(st, 85);
        cqCut(c, st, 85, 81, {"toCut": {"wp": 84}, "clean": true, "tol": undefined});
        stepVol(c, st, 85, "cut");
        atStep(st, 86);
        cqUnion(c, st, 86, 79, {"toUnion": {"wp": 85}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 86, "union");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [-6.847090335406285, 5.052451106939921, 4.162490311734516], "xDir": [-0.5937493140582695, -0.8046500805041497, 2.7755575615628914e-17], "zDir": [-0.7228066480679077, 0.5333572466906178, 0.43940937280790826]}, "obj": undefined});
        atStep(st, 88);
        cqSphere(c, st, 88, 87, {"radius": 1.054157834024238, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 88, "sphere");
        atStep(st, 89);
        cqWorkplaneNew(c, st, 89, {"plane": {"origin": [-6.847090335406285, 5.052451106939921, 4.162490311734516], "xDir": [-0.5937493140582695, -0.8046500805041497, 2.7755575615628914e-17], "zDir": [-0.7228066480679077, 0.5333572466906178, 0.43940937280790826]}, "obj": undefined});
        atStep(st, 90);
        cqRect(c, st, 90, 89, {"xLen": 2.1083156680484763, "yLen": 2.1083156680484763, "centered": true, "forConstruction": false});
        atStep(st, 91);
        cqExtrude(c, st, 91, 90, {"until": -2.1083156680484763, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 91, "extrude");
        atStep(st, 92);
        cqCut(c, st, 92, 88, {"toCut": {"wp": 91}, "clean": true, "tol": undefined});
        stepVol(c, st, 92, "cut");
        atStep(st, 93);
        cqUnion(c, st, 93, 86, {"toUnion": {"wp": 92}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 93, "union");
        atStep(st, 94);
        cqWorkplaneNew(c, st, 94, {"plane": {"origin": [-3.3845867486103693, -5.990809812303592, 5.880534533753808], "xDir": [2.7755575615628907e-17, -0.7005081031744388, -0.7136444474575205], "zDir": [-0.3739340356553299, -0.661873326449083, 0.649689954296466]}, "obj": undefined});
        atStep(st, 95);
        cqSphere(c, st, 95, 94, {"radius": 1.8974127740001725, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 95, "sphere");
        atStep(st, 96);
        cqWorkplaneNew(c, st, 96, {"plane": {"origin": [-3.3845867486103693, -5.990809812303592, 5.880534533753808], "xDir": [2.7755575615628907e-17, -0.7005081031744388, -0.7136444474575205], "zDir": [-0.3739340356553299, -0.661873326449083, 0.649689954296466]}, "obj": undefined});
        atStep(st, 97);
        cqRect(c, st, 97, 96, {"xLen": 3.794825548000345, "yLen": 3.794825548000345, "centered": true, "forConstruction": false});
        atStep(st, 98);
        cqExtrude(c, st, 98, 97, {"until": -3.794825548000345, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 98, "extrude");
        atStep(st, 99);
        cqCut(c, st, 99, 95, {"toCut": {"wp": 98}, "clean": true, "tol": undefined});
        stepVol(c, st, 99, "cut");
        atStep(st, 100);
        cqUnion(c, st, 100, 93, {"toUnion": {"wp": 99}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 100, "union");
        atStep(st, 101);
        cqWorkplaneNew(c, st, 101, {"plane": {"origin": [-0.6403585734536856, 0.025853433590378016, -9.462646937521756], "xDir": [-0.9977180681978428, 0, 0.0675178227697316], "zDir": [-0.06751757192044251, 0.0027259119096575677, -0.9977143613711434]}, "obj": undefined});
        atStep(st, 102);
        cqSphere(c, st, 102, 101, {"radius": 1.0313506472586238, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 102, "sphere");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [-0.6403585734536856, 0.025853433590378016, -9.462646937521756], "xDir": [-0.9977180681978428, 0, 0.0675178227697316], "zDir": [-0.06751757192044251, 0.0027259119096575677, -0.9977143613711434]}, "obj": undefined});
        atStep(st, 104);
        cqRect(c, st, 104, 103, {"xLen": 2.0627012945172476, "yLen": 2.0627012945172476, "centered": true, "forConstruction": false});
        atStep(st, 105);
        cqExtrude(c, st, 105, 104, {"until": -2.0627012945172476, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 105, "extrude");
        atStep(st, 106);
        cqCut(c, st, 106, 102, {"toCut": {"wp": 105}, "clean": true, "tol": undefined});
        stepVol(c, st, 106, "cut");
        atStep(st, 107);
        cqUnion(c, st, 107, 100, {"toUnion": {"wp": 106}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 107, "union");
        atStep(st, 108);
        cqWorkplaneNew(c, st, 108, {"plane": {"origin": [-6.575645611505127, -2.9695520823878807, -5.6696470427529695], "xDir": [0.6530048356252944, 0, -0.7573537381237266], "zDir": [-0.7165995726694945, -0.3236153343689519, -0.6178658169952189]}, "obj": undefined});
        atStep(st, 109);
        cqSphere(c, st, 109, 108, {"radius": 1.647642945112655, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 109, "sphere");
        atStep(st, 110);
        cqWorkplaneNew(c, st, 110, {"plane": {"origin": [-6.575645611505127, -2.9695520823878807, -5.6696470427529695], "xDir": [0.6530048356252944, 0, -0.7573537381237266], "zDir": [-0.7165995726694945, -0.3236153343689519, -0.6178658169952189]}, "obj": undefined});
        atStep(st, 111);
        cqRect(c, st, 111, 110, {"xLen": 3.29528589022531, "yLen": 3.29528589022531, "centered": true, "forConstruction": false});
        atStep(st, 112);
        cqExtrude(c, st, 112, 111, {"until": -3.29528589022531, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 112, "extrude");
        atStep(st, 113);
        cqCut(c, st, 113, 109, {"toCut": {"wp": 112}, "clean": true, "tol": undefined});
        stepVol(c, st, 113, "cut");
        atStep(st, 114);
        cqUnion(c, st, 114, 107, {"toUnion": {"wp": 113}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 114, "union");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 115, {"plane": {"origin": [7.542129986799179, -1.559910931754712, -4.722992651741917], "xDir": [0.5307390522014396, 0, 0.8475352844975349], "zDir": [0.8348065678805252, -0.1726599638850983, -0.5227681427695596]}, "obj": undefined});
        atStep(st, 116);
        cqSphere(c, st, 116, 115, {"radius": 1.9308321784104976, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 116, "sphere");
        atStep(st, 117);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [7.542129986799179, -1.559910931754712, -4.722992651741917], "xDir": [0.5307390522014396, 0, 0.8475352844975349], "zDir": [0.8348065678805252, -0.1726599638850983, -0.5227681427695596]}, "obj": undefined});
        atStep(st, 118);
        cqRect(c, st, 118, 117, {"xLen": 3.861664356820995, "yLen": 3.861664356820995, "centered": true, "forConstruction": false});
        atStep(st, 119);
        cqExtrude(c, st, 119, 118, {"until": -3.861664356820995, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 119, "extrude");
        atStep(st, 120);
        cqCut(c, st, 120, 116, {"toCut": {"wp": 119}, "clean": true, "tol": undefined});
        stepVol(c, st, 120, "cut");
        atStep(st, 121);
        cqUnion(c, st, 121, 114, {"toUnion": {"wp": 120}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 121, "union");
        atStep(st, 122);
        cqWorkplaneNew(c, st, 122, {"plane": {"origin": [-2.143657189982609, 4.551449455189922, -7.760718817210681], "xDir": [0, -0.8625979343365868, -0.5058901102791531], "zDir": [-0.2317775862855901, 0.4921141186914044, -0.8391083628949292]}, "obj": undefined});
        atStep(st, 123);
        cqSphere(c, st, 123, 122, {"radius": 1.5024634233505885, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 123, "sphere");
        atStep(st, 124);
        cqWorkplaneNew(c, st, 124, {"plane": {"origin": [-2.143657189982609, 4.551449455189922, -7.760718817210681], "xDir": [0, -0.8625979343365868, -0.5058901102791531], "zDir": [-0.2317775862855901, 0.4921141186914044, -0.8391083628949292]}, "obj": undefined});
        atStep(st, 125);
        cqRect(c, st, 125, 124, {"xLen": 3.004926846701177, "yLen": 3.004926846701177, "centered": true, "forConstruction": false});
        atStep(st, 126);
        cqExtrude(c, st, 126, 125, {"until": -3.004926846701177, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 126, "extrude");
        atStep(st, 127);
        cqCut(c, st, 127, 123, {"toCut": {"wp": 126}, "clean": true, "tol": undefined});
        stepVol(c, st, 127, "cut");
        atStep(st, 128);
        cqUnion(c, st, 128, 121, {"toUnion": {"wp": 127}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 128, "union");
        atStep(st, 129);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [8.474177411963066, -3.8048599849371887, 0.14754253236791007], "xDir": [0.4096018051262153, 0.9122644141022634, 0], "zDir": [0.9121493633649512, -0.4095501479652478, 0.01588128504114783]}, "obj": undefined});
        atStep(st, 130);
        cqSphere(c, st, 130, 129, {"radius": 1.419320667611878, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 130, "sphere");
        atStep(st, 131);
        cqWorkplaneNew(c, st, 131, {"plane": {"origin": [8.474177411963066, -3.8048599849371887, 0.14754253236791007], "xDir": [0.4096018051262153, 0.9122644141022634, 0], "zDir": [0.9121493633649512, -0.4095501479652478, 0.01588128504114783]}, "obj": undefined});
        atStep(st, 132);
        cqRect(c, st, 132, 131, {"xLen": 2.8386413352237563, "yLen": 2.8386413352237563, "centered": true, "forConstruction": false});
        atStep(st, 133);
        cqExtrude(c, st, 133, 132, {"until": -2.8386413352237563, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 133, "extrude");
        atStep(st, 134);
        cqCut(c, st, 134, 130, {"toCut": {"wp": 133}, "clean": true, "tol": undefined});
        stepVol(c, st, 134, "cut");
        atStep(st, 135);
        cqUnion(c, st, 135, 128, {"toUnion": {"wp": 134}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 135, "union");
        atStep(st, 136);
        cqWorkplaneNew(c, st, 136, {"plane": {"origin": [-8.125512554581698, -2.4964322090749604, 3.856141495076066], "xDir": [-0.42874137107503263, 0, -0.9034272725176616], "zDir": [-0.8705164638324038, -0.26745209292869354, 0.4131228197337793]}, "obj": undefined});
        atStep(st, 137);
        cqSphere(c, st, 137, 136, {"radius": 1.331742954499564, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 137, "sphere");
        atStep(st, 138);
        cqWorkplaneNew(c, st, 138, {"plane": {"origin": [-8.125512554581698, -2.4964322090749604, 3.856141495076066], "xDir": [-0.42874137107503263, 0, -0.9034272725176616], "zDir": [-0.8705164638324038, -0.26745209292869354, 0.4131228197337793]}, "obj": undefined});
        atStep(st, 139);
        cqRect(c, st, 139, 138, {"xLen": 2.663485908999128, "yLen": 2.663485908999128, "centered": true, "forConstruction": false});
        atStep(st, 140);
        cqExtrude(c, st, 140, 139, {"until": -2.663485908999128, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 140, "extrude");
        atStep(st, 141);
        cqCut(c, st, 141, 137, {"toCut": {"wp": 140}, "clean": true, "tol": undefined});
        stepVol(c, st, 141, "cut");
        atStep(st, 142);
        cqUnion(c, st, 142, 135, {"toUnion": {"wp": 141}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 142, "union");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 143, {"plane": {"origin": [2.7432038671983614, 6.533046053238568, -5.61700881748185], "xDir": [0, 0.6519451358975923, 0.7582661404674943], "zDir": [0.3033867086137216, 0.7225271745253746, -0.6212173428912877]}, "obj": undefined});
        atStep(st, 144);
        cqSphere(c, st, 144, 143, {"radius": 1.9161236183812689, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 144, "sphere");
        atStep(st, 145);
        cqWorkplaneNew(c, st, 145, {"plane": {"origin": [2.7432038671983614, 6.533046053238568, -5.61700881748185], "xDir": [0, 0.6519451358975923, 0.7582661404674943], "zDir": [0.3033867086137216, 0.7225271745253746, -0.6212173428912877]}, "obj": undefined});
        atStep(st, 146);
        cqRect(c, st, 146, 145, {"xLen": 3.8322472367625378, "yLen": 3.8322472367625378, "centered": true, "forConstruction": false});
        atStep(st, 147);
        cqExtrude(c, st, 147, 146, {"until": -3.8322472367625378, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 147, "extrude");
        atStep(st, 148);
        cqCut(c, st, 148, 144, {"toCut": {"wp": 147}, "clean": true, "tol": undefined});
        stepVol(c, st, 148, "cut");
        atStep(st, 149);
        cqUnion(c, st, 149, 142, {"toUnion": {"wp": 148}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 149, "union");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [-4.872479462188399, -7.33294365817463, 2.038265919129524], "xDir": [-0.8328959349075676, 0.553429635648877, 1.3877787807814457e-17], "zDir": [-0.539169102165169, -0.8114342357084136, 0.22554635973721335]}, "obj": undefined});
        atStep(st, 151);
        cqSphere(c, st, 151, 150, {"radius": 1.925969264107557, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 151, "sphere");
        atStep(st, 152);
        cqWorkplaneNew(c, st, 152, {"plane": {"origin": [-4.872479462188399, -7.33294365817463, 2.038265919129524], "xDir": [-0.8328959349075676, 0.553429635648877, 1.3877787807814457e-17], "zDir": [-0.539169102165169, -0.8114342357084136, 0.22554635973721335]}, "obj": undefined});
        atStep(st, 153);
        cqRect(c, st, 153, 152, {"xLen": 3.851938528215114, "yLen": 3.851938528215114, "centered": true, "forConstruction": false});
        atStep(st, 154);
        cqExtrude(c, st, 154, 153, {"until": -3.851938528215114, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 154, "extrude");
        atStep(st, 155);
        cqCut(c, st, 155, 151, {"toCut": {"wp": 154}, "clean": true, "tol": undefined});
        stepVol(c, st, 155, "cut");
        atStep(st, 156);
        cqUnion(c, st, 156, 149, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        atStep(st, 157);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [2.683180853478039, -4.086978053252712, 7.782113315904186], "xDir": [-1.387778780781446e-17, 0.8853338162127052, 0.4649559483115018], "zDir": [0.29195355801142053, -0.4446989783096573, 0.8467620318925613]}, "obj": undefined});
        atStep(st, 158);
        cqSphere(c, st, 158, 157, {"radius": 1.6191255091806114, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 158, "sphere");
        atStep(st, 159);
        cqWorkplaneNew(c, st, 159, {"plane": {"origin": [2.683180853478039, -4.086978053252712, 7.782113315904186], "xDir": [-1.387778780781446e-17, 0.8853338162127052, 0.4649559483115018], "zDir": [0.29195355801142053, -0.4446989783096573, 0.8467620318925613]}, "obj": undefined});
        atStep(st, 160);
        cqRect(c, st, 160, 159, {"xLen": 3.2382510183612228, "yLen": 3.2382510183612228, "centered": true, "forConstruction": false});
        atStep(st, 161);
        cqExtrude(c, st, 161, 160, {"until": -3.2382510183612228, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 161, "extrude");
        atStep(st, 162);
        cqCut(c, st, 162, 158, {"toCut": {"wp": 161}, "clean": true, "tol": undefined});
        stepVol(c, st, 162, "cut");
        atStep(st, 163);
        cqUnion(c, st, 163, 156, {"toUnion": {"wp": 162}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 163, "union");
        atStep(st, 164);
        cqWorkplaneNew(c, st, 164, {"plane": {"origin": [0.9678720038459409, -0.5565771961239536, 9.074357963703456], "xDir": [0.9943598883111342, -8.673617379884035e-19, -0.10605853345143332], "zDir": [0.10586182985520488, -0.06087610779445533, 0.9925156787071454]}, "obj": undefined});
        atStep(st, 165);
        cqSphere(c, st, 165, 164, {"radius": 1.7144289840866818, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 165, "sphere");
        atStep(st, 166);
        cqWorkplaneNew(c, st, 166, {"plane": {"origin": [0.9678720038459409, -0.5565771961239536, 9.074357963703456], "xDir": [0.9943598883111342, -8.673617379884035e-19, -0.10605853345143332], "zDir": [0.10586182985520488, -0.06087610779445533, 0.9925156787071454]}, "obj": undefined});
        atStep(st, 167);
        cqRect(c, st, 167, 166, {"xLen": 3.4288579681733635, "yLen": 3.4288579681733635, "centered": true, "forConstruction": false});
        atStep(st, 168);
        cqExtrude(c, st, 168, 167, {"until": -3.4288579681733635, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 168, "extrude");
        atStep(st, 169);
        cqCut(c, st, 169, 165, {"toCut": {"wp": 168}, "clean": true, "tol": undefined});
        stepVol(c, st, 169, "cut");
        atStep(st, 170);
        cqUnion(c, st, 170, 163, {"toUnion": {"wp": 169}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 170, "union");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [6.566508649774372, 3.738528324659438, 5.47370213058717], "xDir": [-0.6402956497037301, -2.7755575615628907e-17, 0.7681285575803559], "zDir": [0.7037729332671214, 0.40068096845318174, 0.5866501682604484]}, "obj": undefined});
        atStep(st, 172);
        cqSphere(c, st, 172, 171, {"radius": 1.339127041187268, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 172, "sphere");
        atStep(st, 173);
        cqWorkplaneNew(c, st, 173, {"plane": {"origin": [6.566508649774372, 3.738528324659438, 5.47370213058717], "xDir": [-0.6402956497037301, -2.7755575615628907e-17, 0.7681285575803559], "zDir": [0.7037729332671214, 0.40068096845318174, 0.5866501682604484]}, "obj": undefined});
        atStep(st, 174);
        cqRect(c, st, 174, 173, {"xLen": 2.678254082374536, "yLen": 2.678254082374536, "centered": true, "forConstruction": false});
        atStep(st, 175);
        cqExtrude(c, st, 175, 174, {"until": -2.678254082374536, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 175, "extrude");
        atStep(st, 176);
        cqCut(c, st, 176, 172, {"toCut": {"wp": 175}, "clean": true, "tol": undefined});
        stepVol(c, st, 176, "cut");
        atStep(st, 177);
        cqUnion(c, st, 177, 170, {"toUnion": {"wp": 176}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 177, "union");
        atStep(st, 178);
        cqWorkplaneNew(c, st, 178, {"plane": {"origin": [-9.154550930183818, 0.7816169823084614, 2.1272952164941277], "xDir": [-0.2263449079089054, 3.469446951953614e-18, -0.9740472178820232], "zDir": [-0.9706961884312792, 0.08287819154934206, 0.2255662100814244]}, "obj": undefined});
        atStep(st, 179);
        cqSphere(c, st, 179, 178, {"radius": 1.1381747671672904, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 179, "sphere");
        atStep(st, 180);
        cqWorkplaneNew(c, st, 180, {"plane": {"origin": [-9.154550930183818, 0.7816169823084614, 2.1272952164941277], "xDir": [-0.2263449079089054, 3.469446951953614e-18, -0.9740472178820232], "zDir": [-0.9706961884312792, 0.08287819154934206, 0.2255662100814244]}, "obj": undefined});
        atStep(st, 181);
        cqRect(c, st, 181, 180, {"xLen": 2.2763495343345808, "yLen": 2.2763495343345808, "centered": true, "forConstruction": false});
        atStep(st, 182);
        cqExtrude(c, st, 182, 181, {"until": -2.2763495343345808, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 182, "extrude");
        atStep(st, 183);
        cqCut(c, st, 183, 179, {"toCut": {"wp": 182}, "clean": true, "tol": undefined});
        stepVol(c, st, 183, "cut");
        atStep(st, 184);
        cqUnion(c, st, 184, 177, {"toUnion": {"wp": 183}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 184, "union");
        atStep(st, 185);
        cqWorkplaneNew(c, st, 185, {"plane": {"origin": [-2.0568257203498974, -6.028580296125874, -6.372973325441781], "xDir": [0, -0.7264631805511318, 0.6872053894604827], "zDir": [-0.22826988868281645, -0.6690617194722608, -0.7072830221753923]}, "obj": undefined});
        atStep(st, 186);
        cqSphere(c, st, 186, 185, {"radius": 1.979000978023168, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 186, "sphere");
        atStep(st, 187);
        cqWorkplaneNew(c, st, 187, {"plane": {"origin": [-2.0568257203498974, -6.028580296125874, -6.372973325441781], "xDir": [0, -0.7264631805511318, 0.6872053894604827], "zDir": [-0.22826988868281645, -0.6690617194722608, -0.7072830221753923]}, "obj": undefined});
        atStep(st, 188);
        cqRect(c, st, 188, 187, {"xLen": 3.958001956046336, "yLen": 3.958001956046336, "centered": true, "forConstruction": false});
        atStep(st, 189);
        cqExtrude(c, st, 189, 188, {"until": -3.958001956046336, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 189, "extrude");
        atStep(st, 190);
        cqCut(c, st, 190, 186, {"toCut": {"wp": 189}, "clean": true, "tol": undefined});
        stepVol(c, st, 190, "cut");
        atStep(st, 191);
        cqUnion(c, st, 191, 184, {"toUnion": {"wp": 190}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 191, "union");
        atStep(st, 192);
        cqWorkplaneNew(c, st, 192, {"plane": {"origin": [3.4032794987804995, 0.4655343621275019, -8.503950600621895], "xDir": [-0.9284127063079508, 0, -0.37155059785437894], "zDir": [0.37107164610124793, 0.05075886424645611, -0.9272159247770284]}, "obj": undefined});
        atStep(st, 193);
        cqSphere(c, st, 193, 192, {"radius": 1.6570221166836054, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 193, "sphere");
        atStep(st, 194);
        cqWorkplaneNew(c, st, 194, {"plane": {"origin": [3.4032794987804995, 0.4655343621275019, -8.503950600621895], "xDir": [-0.9284127063079508, 0, -0.37155059785437894], "zDir": [0.37107164610124793, 0.05075886424645611, -0.9272159247770284]}, "obj": undefined});
        atStep(st, 195);
        cqRect(c, st, 195, 194, {"xLen": 3.314044233367211, "yLen": 3.314044233367211, "centered": true, "forConstruction": false});
        atStep(st, 196);
        cqExtrude(c, st, 196, 195, {"until": -3.314044233367211, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 196, "extrude");
        atStep(st, 197);
        cqCut(c, st, 197, 193, {"toCut": {"wp": 196}, "clean": true, "tol": undefined});
        stepVol(c, st, 197, "cut");
        atStep(st, 198);
        cqUnion(c, st, 198, 191, {"toUnion": {"wp": 197}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 198, "union");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 199, {"plane": {"origin": [1.4397663213830971, 3.325495030903778, 8.63309308132252], "xDir": [0, 0.9331617009405936, -0.35945686792389736], "zDir": [0.15377508481385202, 0.3551814435651067, 0.9220625604792224]}, "obj": undefined});
        atStep(st, 200);
        cqSphere(c, st, 200, 199, {"radius": 1.274387549504984, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 200, "sphere");
        atStep(st, 201);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [1.4397663213830971, 3.325495030903778, 8.63309308132252], "xDir": [0, 0.9331617009405936, -0.35945686792389736], "zDir": [0.15377508481385202, 0.3551814435651067, 0.9220625604792224]}, "obj": undefined});
        atStep(st, 202);
        cqRect(c, st, 202, 201, {"xLen": 2.548775099009968, "yLen": 2.548775099009968, "centered": true, "forConstruction": false});
        atStep(st, 203);
        cqExtrude(c, st, 203, 202, {"until": -2.548775099009968, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 203, "extrude");
        atStep(st, 204);
        cqCut(c, st, 204, 200, {"toCut": {"wp": 203}, "clean": true, "tol": undefined});
        stepVol(c, st, 204, "cut");
        atStep(st, 205);
        cqUnion(c, st, 205, 198, {"toUnion": {"wp": 204}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 205, "union");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 206, {"plane": {"origin": [4.072840901624031, 7.971037194084717, -1.0396693508354324], "xDir": [0.8904910294220203, -0.45500079837172897, 0], "zDir": [0.4519624682814872, 0.8845446537245147, -0.1153719326018421]}, "obj": undefined});
        atStep(st, 207);
        cqSphere(c, st, 207, 206, {"radius": 1.9770835524890504, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 207, "sphere");
        atStep(st, 208);
        cqWorkplaneNew(c, st, 208, {"plane": {"origin": [4.072840901624031, 7.971037194084717, -1.0396693508354324], "xDir": [0.8904910294220203, -0.45500079837172897, 0], "zDir": [0.4519624682814872, 0.8845446537245147, -0.1153719326018421]}, "obj": undefined});
        atStep(st, 209);
        cqRect(c, st, 209, 208, {"xLen": 3.954167104978101, "yLen": 3.954167104978101, "centered": true, "forConstruction": false});
        atStep(st, 210);
        cqExtrude(c, st, 210, 209, {"until": -3.954167104978101, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 210, "extrude");
        atStep(st, 211);
        cqCut(c, st, 211, 207, {"toCut": {"wp": 210}, "clean": true, "tol": undefined});
        stepVol(c, st, 211, "cut");
        atStep(st, 212);
        cqUnion(c, st, 212, 205, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [-5.310384743317648, 6.658254396352906, -3.4677022647385813], "xDir": [0.7817969075584854, 0.6235331549580897, 2.7755575615628914e-17], "zDir": [-0.577497260801032, 0.7240762885304619, -0.3771079979995864]}, "obj": undefined});
        atStep(st, 214);
        cqSphere(c, st, 214, 213, {"radius": 1.6089699336348509, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 214, "sphere");
        atStep(st, 215);
        cqWorkplaneNew(c, st, 215, {"plane": {"origin": [-5.310384743317648, 6.658254396352906, -3.4677022647385813], "xDir": [0.7817969075584854, 0.6235331549580897, 2.7755575615628914e-17], "zDir": [-0.577497260801032, 0.7240762885304619, -0.3771079979995864]}, "obj": undefined});
        atStep(st, 216);
        cqRect(c, st, 216, 215, {"xLen": 3.2179398672697017, "yLen": 3.2179398672697017, "centered": true, "forConstruction": false});
        atStep(st, 217);
        cqExtrude(c, st, 217, 216, {"until": -3.2179398672697017, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 217, "extrude");
        atStep(st, 218);
        cqCut(c, st, 218, 214, {"toCut": {"wp": 217}, "clean": true, "tol": undefined});
        stepVol(c, st, 218, "cut");
        atStep(st, 219);
        cqUnion(c, st, 219, 212, {"toUnion": {"wp": 218}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 219, "union");
        atStep(st, 220);
        cqWorkplaneNew(c, st, 220, {"plane": {"origin": [8.302393509773003, -0.6890392552664435, 4.210967847572195], "xDir": [-0.4523427158673472, -3.469446951953614e-18, 0.8918441945775912], "zDir": [0.8894112159809967, -0.07381476693001554, 0.4511087109226248]}, "obj": undefined});
        atStep(st, 221);
        cqSphere(c, st, 221, 220, {"radius": 1.330585086864044, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 221, "sphere");
        atStep(st, 222);
        cqWorkplaneNew(c, st, 222, {"plane": {"origin": [8.302393509773003, -0.6890392552664435, 4.210967847572195], "xDir": [-0.4523427158673472, -3.469446951953614e-18, 0.8918441945775912], "zDir": [0.8894112159809967, -0.07381476693001554, 0.4511087109226248]}, "obj": undefined});
        atStep(st, 223);
        cqRect(c, st, 223, 222, {"xLen": 2.661170173728088, "yLen": 2.661170173728088, "centered": true, "forConstruction": false});
        atStep(st, 224);
        cqExtrude(c, st, 224, 223, {"until": -2.661170173728088, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 224, "extrude");
        atStep(st, 225);
        cqCut(c, st, 225, 221, {"toCut": {"wp": 224}, "clean": true, "tol": undefined});
        stepVol(c, st, 225, "cut");
        atStep(st, 226);
        cqUnion(c, st, 226, 219, {"toUnion": {"wp": 225}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 226, "union");
        atStep(st, 227);
        cqWorkplaneNew(c, st, 227, {"plane": {"origin": [-3.8539237997485345, -7.995502937352426, -1.7775266108648848], "xDir": [-0.9008149915900922, 0.43420312173744474, 1.3877787807814457e-17], "zDir": [-0.4257494467842683, -0.8832766627513241, -0.1963663555749129]}, "obj": undefined});
        atStep(st, 228);
        cqSphere(c, st, 228, 227, {"radius": 1.895813000544624, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 228, "sphere");
        atStep(st, 229);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [-3.8539237997485345, -7.995502937352426, -1.7775266108648848], "xDir": [-0.9008149915900922, 0.43420312173744474, 1.3877787807814457e-17], "zDir": [-0.4257494467842683, -0.8832766627513241, -0.1963663555749129]}, "obj": undefined});
        atStep(st, 230);
        cqRect(c, st, 230, 229, {"xLen": 3.791626001089248, "yLen": 3.791626001089248, "centered": true, "forConstruction": false});
        atStep(st, 231);
        cqExtrude(c, st, 231, 230, {"until": -3.791626001089248, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 231, "extrude");
        atStep(st, 232);
        cqCut(c, st, 232, 228, {"toCut": {"wp": 231}, "clean": true, "tol": undefined});
        stepVol(c, st, 232, "cut");
        atStep(st, 233);
        cqUnion(c, st, 233, 226, {"toUnion": {"wp": 232}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 233, "union");
        atStep(st, 234);
        cqWorkplaneNew(c, st, 234, {"plane": {"origin": [3.452010541854135, -4.512149621209285, -7.565420812338163], "xDir": [0, -0.8588473494530074, 0.5122316178620214], "zDir": [0.36486548743938946, -0.4769184945935046, -0.799638621872582]}, "obj": undefined});
        atStep(st, 235);
        cqSphere(c, st, 235, 234, {"radius": 1.077900428016918, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 235, "sphere");
        atStep(st, 236);
        cqWorkplaneNew(c, st, 236, {"plane": {"origin": [3.452010541854135, -4.512149621209285, -7.565420812338163], "xDir": [0, -0.8588473494530074, 0.5122316178620214], "zDir": [0.36486548743938946, -0.4769184945935046, -0.799638621872582]}, "obj": undefined});
        atStep(st, 237);
        cqRect(c, st, 237, 236, {"xLen": 2.155800856033836, "yLen": 2.155800856033836, "centered": true, "forConstruction": false});
        atStep(st, 238);
        cqExtrude(c, st, 238, 237, {"until": -2.155800856033836, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 238, "extrude");
        atStep(st, 239);
        cqCut(c, st, 239, 235, {"toCut": {"wp": 238}, "clean": true, "tol": undefined});
        stepVol(c, st, 239, "cut");
        atStep(st, 240);
        cqUnion(c, st, 240, 233, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        atStep(st, 241);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [-7.770167102580626, 2.3648839056476936, -4.099272286754875], "xDir": [0.4666117829948953, 0, -0.8844622343380891], "zDir": [-0.8540594330298767, 0.2599366758751109, -0.45057231316147583]}, "obj": undefined});
        atStep(st, 242);
        cqSphere(c, st, 242, 241, {"radius": 1.8041536640722071, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 242, "sphere");
        atStep(st, 243);
        cqWorkplaneNew(c, st, 243, {"plane": {"origin": [-7.770167102580626, 2.3648839056476936, -4.099272286754875], "xDir": [0.4666117829948953, 0, -0.8844622343380891], "zDir": [-0.8540594330298767, 0.2599366758751109, -0.45057231316147583]}, "obj": undefined});
        atStep(st, 244);
        cqRect(c, st, 244, 243, {"xLen": 3.6083073281444142, "yLen": 3.6083073281444142, "centered": true, "forConstruction": false});
        atStep(st, 245);
        cqExtrude(c, st, 245, 244, {"until": -3.6083073281444142, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 245, "extrude");
        atStep(st, 246);
        cqCut(c, st, 246, 242, {"toCut": {"wp": 245}, "clean": true, "tol": undefined});
        stepVol(c, st, 246, "cut");
        atStep(st, 247);
        cqUnion(c, st, 247, 240, {"toUnion": {"wp": 246}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 247, "union");
        atStep(st, 248);
        cqWorkplaneNew(c, st, 248, {"plane": {"origin": [2.631817011797822, 8.458563237226052, 3.204155566834326], "xDir": [1.3877787807814457e-17, -0.35424198494099485, 0.935153792755536], "zDir": [0.279379851858323, 0.8979165852173654, 0.34013630263058836]}, "obj": undefined});
        atStep(st, 249);
        cqSphere(c, st, 249, 248, {"radius": 1.159579015508609, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 249, "sphere");
        atStep(st, 250);
        cqWorkplaneNew(c, st, 250, {"plane": {"origin": [2.631817011797822, 8.458563237226052, 3.204155566834326], "xDir": [1.3877787807814457e-17, -0.35424198494099485, 0.935153792755536], "zDir": [0.279379851858323, 0.8979165852173654, 0.34013630263058836]}, "obj": undefined});
        atStep(st, 251);
        cqRect(c, st, 251, 250, {"xLen": 2.319158031017218, "yLen": 2.319158031017218, "centered": true, "forConstruction": false});
        atStep(st, 252);
        cqExtrude(c, st, 252, 251, {"until": -2.319158031017218, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 252, "extrude");
        atStep(st, 253);
        cqCut(c, st, 253, 249, {"toCut": {"wp": 252}, "clean": true, "tol": undefined});
        stepVol(c, st, 253, "cut");
        atStep(st, 254);
        cqUnion(c, st, 254, 247, {"toUnion": {"wp": 253}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 254, "union");
        atStep(st, 255);
        cqWorkplaneNew(c, st, 255, {"plane": {"origin": [2.1981103243303486, -8.892842297184824, 2.3055710981414315], "xDir": [6.9388939039072276e-18, -0.2509641366255524, -0.9679963853898376], "zDir": [0.23269875569878873, -0.9414237830900924, 0.2440748854933139]}, "obj": undefined});
        atStep(st, 256);
        cqSphere(c, st, 256, 255, {"radius": 1.107674446049557, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 256, "sphere");
        atStep(st, 257);
        cqWorkplaneNew(c, st, 257, {"plane": {"origin": [2.1981103243303486, -8.892842297184824, 2.3055710981414315], "xDir": [6.9388939039072276e-18, -0.2509641366255524, -0.9679963853898376], "zDir": [0.23269875569878873, -0.9414237830900924, 0.2440748854933139]}, "obj": undefined});
        atStep(st, 258);
        cqRect(c, st, 258, 257, {"xLen": 2.2153488920991133, "yLen": 2.2153488920991133, "centered": true, "forConstruction": false});
        atStep(st, 259);
        cqExtrude(c, st, 259, 258, {"until": -2.2153488920991133, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 259, "extrude");
        atStep(st, 260);
        cqCut(c, st, 260, 256, {"toCut": {"wp": 259}, "clean": true, "tol": undefined});
        stepVol(c, st, 260, "cut");
        atStep(st, 261);
        cqUnion(c, st, 261, 254, {"toUnion": {"wp": 260}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 261, "union");
        atStep(st, 262);
        cqWorkplaneNew(c, st, 262, {"plane": {"origin": [6.838491287232178, 4.024409051898499, -4.984572504633305], "xDir": [0.5890311634630176, 0, 0.8081103194919639], "zDir": [0.7297871915267852, 0.4294751658196071, -0.5319414789501461]}, "obj": undefined});
        atStep(st, 263);
        cqSphere(c, st, 263, 262, {"radius": 1.258944068543063, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 263, "sphere");
        atStep(st, 264);
        cqWorkplaneNew(c, st, 264, {"plane": {"origin": [6.838491287232178, 4.024409051898499, -4.984572504633305], "xDir": [0.5890311634630176, 0, 0.8081103194919639], "zDir": [0.7297871915267852, 0.4294751658196071, -0.5319414789501461]}, "obj": undefined});
        atStep(st, 265);
        cqRect(c, st, 265, 264, {"xLen": 2.517888137086126, "yLen": 2.517888137086126, "centered": true, "forConstruction": false});
        atStep(st, 266);
        cqExtrude(c, st, 266, 265, {"until": -2.517888137086126, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 266, "extrude");
        atStep(st, 267);
        cqCut(c, st, 267, 263, {"toCut": {"wp": 266}, "clean": true, "tol": undefined});
        stepVol(c, st, 267, "cut");
        atStep(st, 268);
        cqUnion(c, st, 268, 261, {"toUnion": {"wp": 267}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 268, "union");
        atStep(st, 269);
        cqWorkplaneNew(c, st, 269, {"plane": {"origin": [5.922409064106023, -5.742284434618597, -3.941844945300699], "xDir": [0.6961047447298251, 0.7179402373210636, -2.7755575615628907e-17], "zDir": [0.6477826151772209, -0.6280809021945151, -0.4311520193304035]}, "obj": undefined});
        atStep(st, 270);
        cqSphere(c, st, 270, 269, {"radius": 1.7148255437952291, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 270, "sphere");
        atStep(st, 271);
        cqWorkplaneNew(c, st, 271, {"plane": {"origin": [5.922409064106023, -5.742284434618597, -3.941844945300699], "xDir": [0.6961047447298251, 0.7179402373210636, -2.7755575615628907e-17], "zDir": [0.6477826151772209, -0.6280809021945151, -0.4311520193304035]}, "obj": undefined});
        atStep(st, 272);
        cqRect(c, st, 272, 271, {"xLen": 3.4296510875904582, "yLen": 3.4296510875904582, "centered": true, "forConstruction": false});
        atStep(st, 273);
        cqExtrude(c, st, 273, 272, {"until": -3.4296510875904582, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 273, "extrude");
        atStep(st, 274);
        cqCut(c, st, 274, 270, {"toCut": {"wp": 273}, "clean": true, "tol": undefined});
        stepVol(c, st, 274, "cut");
        atStep(st, 275);
        cqUnion(c, st, 275, 268, {"toUnion": {"wp": 274}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 275, "union");
        atStep(st, 276);
        cqWorkplaneNew(c, st, 276, {"plane": {"origin": [-2.4698215586532974, 1.8784214890626283, 8.656665285409776], "xDir": [0.9616269153014632, 0, 0.2743604850699036], "zDir": [-0.2685757582709056, 0.20426515187293265, 0.9413515867089914]}, "obj": undefined});
        atStep(st, 277);
        cqSphere(c, st, 277, 276, {"radius": 1.608008298633929, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 277, "sphere");
        atStep(st, 278);
        cqWorkplaneNew(c, st, 278, {"plane": {"origin": [-2.4698215586532974, 1.8784214890626283, 8.656665285409776], "xDir": [0.9616269153014632, 0, 0.2743604850699036], "zDir": [-0.2685757582709056, 0.20426515187293265, 0.9413515867089914]}, "obj": undefined});
        atStep(st, 279);
        cqRect(c, st, 279, 278, {"xLen": 3.216016597267858, "yLen": 3.216016597267858, "centered": true, "forConstruction": false});
        atStep(st, 280);
        cqExtrude(c, st, 280, 279, {"until": -3.216016597267858, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 280, "extrude");
        atStep(st, 281);
        cqCut(c, st, 281, 277, {"toCut": {"wp": 280}, "clean": true, "tol": undefined});
        stepVol(c, st, 281, "cut");
        atStep(st, 282);
        cqUnion(c, st, 282, 275, {"toUnion": {"wp": 281}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 282, "union");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [-4.341885021015162, 8.152679449877763, 0.9869531570883529], "xDir": [0.8826319774190949, 0.4700646683566616, 6.9388939039072276e-18], "zDir": [-0.4674040667830307, 0.8776362136739322, 0.10624554015472087]}, "obj": undefined});
        atStep(st, 284);
        cqSphere(c, st, 284, 283, {"radius": 1.4212783774058688, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 284, "sphere");
        atStep(st, 285);
        cqWorkplaneNew(c, st, 285, {"plane": {"origin": [-4.341885021015162, 8.152679449877763, 0.9869531570883529], "xDir": [0.8826319774190949, 0.4700646683566616, 6.9388939039072276e-18], "zDir": [-0.4674040667830307, 0.8776362136739322, 0.10624554015472087]}, "obj": undefined});
        atStep(st, 286);
        cqRect(c, st, 286, 285, {"xLen": 2.8425567548117376, "yLen": 2.8425567548117376, "centered": true, "forConstruction": false});
        atStep(st, 287);
        cqExtrude(c, st, 287, 286, {"until": -2.8425567548117376, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 287, "extrude");
        atStep(st, 288);
        cqCut(c, st, 288, 284, {"toCut": {"wp": 287}, "clean": true, "tol": undefined});
        stepVol(c, st, 288, "cut");
        atStep(st, 289);
        cqUnion(c, st, 289, 282, {"toUnion": {"wp": 288}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 289, "union");
        atStep(st, 290);
        cqWorkplaneNew(c, st, 290, {"plane": {"origin": [-9.15099955347925, -0.7800818333965391, -2.0966794221574467], "xDir": [0.22333318119164663, -3.469446951953615e-18, -0.9747421660002296], "zDir": [-0.9713945108073728, -0.08280704271850205, -0.22256616555424855]}, "obj": undefined});
        atStep(st, 291);
        cqSphere(c, st, 291, 290, {"radius": 1.1590461925229238, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 291, "sphere");
        atStep(st, 292);
        cqWorkplaneNew(c, st, 292, {"plane": {"origin": [-9.15099955347925, -0.7800818333965391, -2.0966794221574467], "xDir": [0.22333318119164663, -3.469446951953615e-18, -0.9747421660002296], "zDir": [-0.9713945108073728, -0.08280704271850205, -0.22256616555424855]}, "obj": undefined});
        atStep(st, 293);
        cqRect(c, st, 293, 292, {"xLen": 2.3180923850458477, "yLen": 2.3180923850458477, "centered": true, "forConstruction": false});
        atStep(st, 294);
        cqExtrude(c, st, 294, 293, {"until": -2.3180923850458477, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 294, "extrude");
        atStep(st, 295);
        cqCut(c, st, 295, 291, {"toCut": {"wp": 294}, "clean": true, "tol": undefined});
        stepVol(c, st, 295, "cut");
        atStep(st, 296);
        cqUnion(c, st, 296, 289, {"toUnion": {"wp": 295}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 296, "union");
        atStep(st, 297);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [1.35713423946391, 6.268794902521761, 6.367737729754088], "xDir": [0, 0.7126215146861877, -0.7015486988131072], "zDir": [0.1501566286600799, 0.6935946945793594, 0.7045419693111292]}, "obj": undefined});
        atStep(st, 298);
        cqSphere(c, st, 298, 297, {"radius": 1.9237518639799775, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 298, "sphere");
        atStep(st, 299);
        cqWorkplaneNew(c, st, 299, {"plane": {"origin": [1.35713423946391, 6.268794902521761, 6.367737729754088], "xDir": [0, 0.7126215146861877, -0.7015486988131072], "zDir": [0.1501566286600799, 0.6935946945793594, 0.7045419693111292]}, "obj": undefined});
        atStep(st, 300);
        cqRect(c, st, 300, 299, {"xLen": 3.847503727959955, "yLen": 3.847503727959955, "centered": true, "forConstruction": false});
        atStep(st, 301);
        cqExtrude(c, st, 301, 300, {"until": -3.847503727959955, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 301, "extrude");
        atStep(st, 302);
        cqCut(c, st, 302, 298, {"toCut": {"wp": 301}, "clean": true, "tol": undefined});
        stepVol(c, st, 302, "cut");
        atStep(st, 303);
        cqUnion(c, st, 303, 296, {"toUnion": {"wp": 302}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 303, "union");
        atStep(st, 304);
        cqWorkplaneNew(c, st, 304, {"plane": {"origin": [10, 0, 0], "xDir": [0, 0, 1], "zDir": [1, 0, 0]}, "obj": undefined});
        atStep(st, 305);
        cqCircle(c, st, 305, 304, {"radius": 0.5, "forConstruction": false});
        atStep(st, 306);
        cqWorkplane(c, st, 306, 305, {"offset": 1.000001, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 307);
        cqCircle(c, st, 307, 306, {"radius": 0.1, "forConstruction": false});
        atStep(st, 308);
        cqLoft(c, st, 308, 307, {"ruled": false, "combine": true, "clean": true});
        stepVol(c, st, 308, "loft");
        atStep(st, 309);
        cqTranslate(c, st, 309, 308, {"vec": [-1e-6, 0, 0]});
        stepVol(c, st, 309, "translate");
        atStep(st, 310);
        cqRotate(c, st, 310, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 0});
        stepVol(c, st, 310, "rotate");
        atStep(st, 311);
        cqUnion(c, st, 311, 303, {"toUnion": {"wp": 310}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 311, "union");
        atStep(st, 312);
        cqRotate(c, st, 312, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 10});
        stepVol(c, st, 312, "rotate");
        atStep(st, 313);
        cqUnion(c, st, 313, 311, {"toUnion": {"wp": 312}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 313, "union");
        atStep(st, 314);
        cqRotate(c, st, 314, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 20});
        stepVol(c, st, 314, "rotate");
        atStep(st, 315);
        cqUnion(c, st, 315, 313, {"toUnion": {"wp": 314}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 315, "union");
        atStep(st, 316);
        cqRotate(c, st, 316, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 30});
        stepVol(c, st, 316, "rotate");
        atStep(st, 317);
        cqUnion(c, st, 317, 315, {"toUnion": {"wp": 316}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 317, "union");
        atStep(st, 318);
        cqRotate(c, st, 318, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 40});
        stepVol(c, st, 318, "rotate");
        atStep(st, 319);
        cqUnion(c, st, 319, 317, {"toUnion": {"wp": 318}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 319, "union");
        atStep(st, 320);
        cqRotate(c, st, 320, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 50});
        stepVol(c, st, 320, "rotate");
        atStep(st, 321);
        cqUnion(c, st, 321, 319, {"toUnion": {"wp": 320}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 321, "union");
        atStep(st, 322);
        cqRotate(c, st, 322, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 60});
        stepVol(c, st, 322, "rotate");
        atStep(st, 323);
        cqUnion(c, st, 323, 321, {"toUnion": {"wp": 322}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 323, "union");
        atStep(st, 324);
        cqRotate(c, st, 324, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 70});
        stepVol(c, st, 324, "rotate");
        atStep(st, 325);
        cqUnion(c, st, 325, 323, {"toUnion": {"wp": 324}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 325, "union");
        atStep(st, 326);
        cqRotate(c, st, 326, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 80});
        stepVol(c, st, 326, "rotate");
        atStep(st, 327);
        cqUnion(c, st, 327, 325, {"toUnion": {"wp": 326}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 327, "union");
        atStep(st, 328);
        cqRotate(c, st, 328, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 90});
        stepVol(c, st, 328, "rotate");
        atStep(st, 329);
        cqUnion(c, st, 329, 327, {"toUnion": {"wp": 328}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 329, "union");
        atStep(st, 330);
        cqRotate(c, st, 330, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 100});
        stepVol(c, st, 330, "rotate");
        atStep(st, 331);
        cqUnion(c, st, 331, 329, {"toUnion": {"wp": 330}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 331, "union");
        atStep(st, 332);
        cqRotate(c, st, 332, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 110});
        stepVol(c, st, 332, "rotate");
        atStep(st, 333);
        cqUnion(c, st, 333, 331, {"toUnion": {"wp": 332}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 333, "union");
        atStep(st, 334);
        cqRotate(c, st, 334, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 120});
        stepVol(c, st, 334, "rotate");
        atStep(st, 335);
        cqUnion(c, st, 335, 333, {"toUnion": {"wp": 334}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 335, "union");
        atStep(st, 336);
        cqRotate(c, st, 336, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 130});
        stepVol(c, st, 336, "rotate");
        atStep(st, 337);
        cqUnion(c, st, 337, 335, {"toUnion": {"wp": 336}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 337, "union");
        atStep(st, 338);
        cqRotate(c, st, 338, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 140});
        stepVol(c, st, 338, "rotate");
        atStep(st, 339);
        cqUnion(c, st, 339, 337, {"toUnion": {"wp": 338}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 339, "union");
        atStep(st, 340);
        cqRotate(c, st, 340, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 150});
        stepVol(c, st, 340, "rotate");
        atStep(st, 341);
        cqUnion(c, st, 341, 339, {"toUnion": {"wp": 340}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 341, "union");
        atStep(st, 342);
        cqRotate(c, st, 342, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 160});
        stepVol(c, st, 342, "rotate");
        atStep(st, 343);
        cqUnion(c, st, 343, 341, {"toUnion": {"wp": 342}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 343, "union");
        atStep(st, 344);
        cqRotate(c, st, 344, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 170});
        stepVol(c, st, 344, "rotate");
        atStep(st, 345);
        cqUnion(c, st, 345, 343, {"toUnion": {"wp": 344}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 345, "union");
        atStep(st, 346);
        cqRotate(c, st, 346, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 180});
        stepVol(c, st, 346, "rotate");
        atStep(st, 347);
        cqUnion(c, st, 347, 345, {"toUnion": {"wp": 346}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 347, "union");
        atStep(st, 348);
        cqRotate(c, st, 348, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 190});
        stepVol(c, st, 348, "rotate");
        atStep(st, 349);
        cqUnion(c, st, 349, 347, {"toUnion": {"wp": 348}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 349, "union");
        atStep(st, 350);
        cqRotate(c, st, 350, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 200});
        stepVol(c, st, 350, "rotate");
        atStep(st, 351);
        cqUnion(c, st, 351, 349, {"toUnion": {"wp": 350}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 351, "union");
        atStep(st, 352);
        cqRotate(c, st, 352, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 210});
        stepVol(c, st, 352, "rotate");
        atStep(st, 353);
        cqUnion(c, st, 353, 351, {"toUnion": {"wp": 352}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 353, "union");
        atStep(st, 354);
        cqRotate(c, st, 354, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 220});
        stepVol(c, st, 354, "rotate");
        atStep(st, 355);
        cqUnion(c, st, 355, 353, {"toUnion": {"wp": 354}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 355, "union");
        atStep(st, 356);
        cqRotate(c, st, 356, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 230});
        stepVol(c, st, 356, "rotate");
        atStep(st, 357);
        cqUnion(c, st, 357, 355, {"toUnion": {"wp": 356}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 357, "union");
        atStep(st, 358);
        cqRotate(c, st, 358, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 240});
        stepVol(c, st, 358, "rotate");
        atStep(st, 359);
        cqUnion(c, st, 359, 357, {"toUnion": {"wp": 358}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 359, "union");
        atStep(st, 360);
        cqRotate(c, st, 360, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 250});
        stepVol(c, st, 360, "rotate");
        atStep(st, 361);
        cqUnion(c, st, 361, 359, {"toUnion": {"wp": 360}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 361, "union");
        atStep(st, 362);
        cqRotate(c, st, 362, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 260});
        stepVol(c, st, 362, "rotate");
        atStep(st, 363);
        cqUnion(c, st, 363, 361, {"toUnion": {"wp": 362}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 363, "union");
        atStep(st, 364);
        cqRotate(c, st, 364, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 270});
        stepVol(c, st, 364, "rotate");
        atStep(st, 365);
        cqUnion(c, st, 365, 363, {"toUnion": {"wp": 364}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 365, "union");
        atStep(st, 366);
        cqRotate(c, st, 366, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 280});
        stepVol(c, st, 366, "rotate");
        atStep(st, 367);
        cqUnion(c, st, 367, 365, {"toUnion": {"wp": 366}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 367, "union");
        atStep(st, 368);
        cqRotate(c, st, 368, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 290});
        stepVol(c, st, 368, "rotate");
        atStep(st, 369);
        cqUnion(c, st, 369, 367, {"toUnion": {"wp": 368}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 369, "union");
        atStep(st, 370);
        cqRotate(c, st, 370, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 300});
        stepVol(c, st, 370, "rotate");
        atStep(st, 371);
        cqUnion(c, st, 371, 369, {"toUnion": {"wp": 370}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 371, "union");
        atStep(st, 372);
        cqRotate(c, st, 372, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 310});
        stepVol(c, st, 372, "rotate");
        atStep(st, 373);
        cqUnion(c, st, 373, 371, {"toUnion": {"wp": 372}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 373, "union");
        atStep(st, 374);
        cqRotate(c, st, 374, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 320});
        stepVol(c, st, 374, "rotate");
        atStep(st, 375);
        cqUnion(c, st, 375, 373, {"toUnion": {"wp": 374}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 375, "union");
        atStep(st, 376);
        cqRotate(c, st, 376, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 330});
        stepVol(c, st, 376, "rotate");
        atStep(st, 377);
        cqUnion(c, st, 377, 375, {"toUnion": {"wp": 376}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 377, "union");
        atStep(st, 378);
        cqRotate(c, st, 378, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 340});
        stepVol(c, st, 378, "rotate");
        atStep(st, 379);
        cqUnion(c, st, 379, 377, {"toUnion": {"wp": 378}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 379, "union");
        atStep(st, 380);
        cqRotate(c, st, 380, 309, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 350});
        stepVol(c, st, 380, "rotate");
        atStep(st, 381);
        cqUnion(c, st, 381, 379, {"toUnion": {"wp": 380}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 381, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 381}), {"i": 3535});
    } catch (e) {
        return {"i": 3535, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_3535(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c3535"] = r; }
    return out;
}
