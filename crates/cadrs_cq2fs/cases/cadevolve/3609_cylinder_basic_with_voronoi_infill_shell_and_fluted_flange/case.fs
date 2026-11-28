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

const case_3609 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqCircle(c, st, 2, 1, {"radius": 15, "forConstruction": false});
        atStep(st, 3);
        cqCircle(c, st, 3, 2, {"radius": 10, "forConstruction": false});
        atStep(st, 4);
        cqExtrude(c, st, 4, 3, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 4, "extrude");
        atStep(st, 5);
        cqWorkplaneNew(c, st, 5, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 6);
        cqWorkplane(c, st, 6, 5, {"offset": 3, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 7);
        cqCircle(c, st, 7, 6, {"radius": 10, "forConstruction": false});
        atStep(st, 8);
        cqExtrude(c, st, 8, 7, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 8, "extrude");
        atStep(st, 9);
        cqSelect("Faces")(c, st, 9, 8, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 10);
        cqShell(c, st, 10, 9, {"thickness": -2, "kind": "arc"});
        stepVol(c, st, 10, "shell");
        atStep(st, 11);
        cqUnion(c, st, 11, 4, {"toUnion": {"wp": 10}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 11, "union");
        atStep(st, 12);
        cqWorkplaneNew(c, st, 12, {"plane": {"origin": [-1.2754305144501052, 4.874542114256368, 5], "xDir": [0.4920929183063081, 0.8705426811781151, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 13);
        cqRect(c, st, 13, 12, {"xLen": 2.289347570824707, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 14);
        cqExtrude(c, st, 14, 13, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 14, "extrude");
        atStep(st, 15);
        cqWorkplaneNew(c, st, 15, {"plane": {"origin": [-1.0505141976673906, 3.707596917211408, 5], "xDir": [0.977405015668539, -0.211375105785859, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 16);
        cqRect(c, st, 16, 15, {"xLen": 1.612846604466999, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 17);
        cqExtrude(c, st, 17, 16, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 17, "extrude");
        atStep(st, 18);
        cqUnion(c, st, 18, 14, {"toUnion": {"wp": 17}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 18, "union");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [0.5471751464241198, 5.5643896020243755, 5], "xDir": [0.9716110762643544, -0.23658384661769075, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 20);
        cqRect(c, st, 20, 19, {"xLen": 2.5922302206429313, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 21);
        cqExtrude(c, st, 21, 20, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 21, "extrude");
        atStep(st, 22);
        cqUnion(c, st, 22, 18, {"toUnion": {"wp": 21}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 22, "union");
        atStep(st, 23);
        cqWorkplaneNew(c, st, 23, {"plane": {"origin": [0.7720914632068345, 4.3974444049794155, 5], "xDir": [0.7688414648376116, 0.6394394435334402, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 24);
        cqRect(c, st, 24, 23, {"xLen": 2.690810857184179, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 25);
        cqExtrude(c, st, 25, 24, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 25, "extrude");
        atStep(st, 26);
        cqUnion(c, st, 26, 22, {"toUnion": {"wp": 25}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 26, "union");
        atStep(st, 27);
        cqWorkplaneNew(c, st, 27, {"plane": {"origin": [4.974455599212099, 1.5803731857522931, 5], "xDir": [0.8913382684856254, 0.4533388259713117, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 28);
        cqRect(c, st, 28, 27, {"xLen": 2.028195282057715, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 29);
        cqExtrude(c, st, 29, 28, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 29, "extrude");
        atStep(st, 30);
        cqUnion(c, st, 30, 26, {"toUnion": {"wp": 29}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 30, "union");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 31, {"plane": {"origin": [3.4734055433904736, 1.2544372858027772, 5], "xDir": [-0.9758067112863952, 0.21863499767518857, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 32);
        cqRect(c, st, 32, 31, {"xLen": 1.223902261554227, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 33);
        cqExtrude(c, st, 33, 32, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 33, "extrude");
        atStep(st, 34);
        cqUnion(c, st, 34, 30, {"toUnion": {"wp": 33}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 34, "union");
        atStep(st, 35);
        cqWorkplaneNew(c, st, 35, {"plane": {"origin": [2.8926496335638445, 1.6006150671614183, 5], "xDir": [-0.07694333516733874, -0.997035467359776, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 36);
        cqRect(c, st, 36, 35, {"xLen": 0.4260306764538005, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 37);
        cqExtrude(c, st, 37, 36, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 37, "extrude");
        atStep(st, 38);
        cqUnion(c, st, 38, 34, {"toUnion": {"wp": 37}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 38, "union");
        atStep(st, 39);
        cqWorkplaneNew(c, st, 39, {"plane": {"origin": [4.088964118192206, 2.7403511314088016, 5], "xDir": [0.786231323890055, 0.6179322821588881, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 40);
        cqRect(c, st, 40, 39, {"xLen": 3.001468748982033, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 41);
        cqExtrude(c, st, 41, 40, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 41, "extrude");
        atStep(st, 42);
        cqUnion(c, st, 42, 38, {"toUnion": {"wp": 41}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 42, "union");
        atStep(st, 43);
        cqWorkplaneNew(c, st, 43, {"plane": {"origin": [5.573624063448855, 2.8539031840544595, 5], "xDir": [0.35068000355920387, -0.9364953470806552, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 44);
        cqRect(c, st, 44, 43, {"xLen": 1.73796947701819, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 45);
        cqExtrude(c, st, 45, 44, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 45, "extrude");
        atStep(st, 46);
        cqUnion(c, st, 46, 42, {"toUnion": {"wp": 45}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 46, "union");
        atStep(st, 47);
        cqWorkplaneNew(c, st, 47, {"plane": {"origin": [6.048922406992723, -1.0118949676076123, 5], "xDir": [-0.8022018772373586, -0.5970528855611184, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 48);
        cqRect(c, st, 48, 47, {"xLen": 0.04203349419486268, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 49);
        cqExtrude(c, st, 49, 48, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 49, "extrude");
        atStep(st, 50);
        cqUnion(c, st, 50, 46, {"toUnion": {"wp": 49}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 50, "union");
        atStep(st, 51);
        cqWorkplaneNew(c, st, 51, {"plane": {"origin": [5.674424197413302, -0.9366424269134576, 5], "xDir": [0.9711618249593204, -0.23842128625960077, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 52);
        cqRect(c, st, 52, 51, {"xLen": 0.7365168737340364, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 53);
        cqExtrude(c, st, 53, 52, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 53, "extrude");
        atStep(st, 54);
        cqUnion(c, st, 54, 50, {"toUnion": {"wp": 53}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 54, "union");
        atStep(st, 55);
        cqWorkplaneNew(c, st, 55, {"plane": {"origin": [4.69366861279547, 0.13590078751416046, 5], "xDir": [-0.5347131990054212, 0.8450336057278366, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 56);
        cqRect(c, st, 56, 55, {"xLen": 2.3306589408019205, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 57);
        cqExtrude(c, st, 57, 56, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 57, "extrude");
        atStep(st, 58);
        cqUnion(c, st, 58, 54, {"toUnion": {"wp": 57}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 58, "union");
        atStep(st, 59);
        cqWorkplaneNew(c, st, 59, {"plane": {"origin": [4.582963830803974, -1.5021487724308409, 5], "xDir": [0.746893121893585, 0.6649441062736434, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 60);
        cqRect(c, st, 60, 59, {"xLen": 1.9649982293167751, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 61);
        cqExtrude(c, st, 61, 60, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 61, "extrude");
        atStep(st, 62);
        cqUnion(c, st, 62, 58, {"toUnion": {"wp": 61}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 62, "union");
        atStep(st, 63);
        cqWorkplaneNew(c, st, 63, {"plane": {"origin": [2.7493100320146313, -1.85313413668107, 5], "xDir": [-0.9642350096708664, 0.2650487617873814, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 64);
        cqRect(c, st, 64, 63, {"xLen": 2.2812529243464734, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 65);
        cqExtrude(c, st, 65, 64, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 65, "extrude");
        atStep(st, 66);
        cqUnion(c, st, 66, 62, {"toUnion": {"wp": 65}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 66, "union");
        atStep(st, 67);
        cqWorkplaneNew(c, st, 67, {"plane": {"origin": [1.606932858499713, -1.5167324039091246, 5], "xDir": [0.7804754828808326, -0.6251863887049456, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 68);
        cqRect(c, st, 68, 67, {"xLen": 0.10902381090414766, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 69);
        cqExtrude(c, st, 69, 68, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 69, "extrude");
        atStep(st, 70);
        cqUnion(c, st, 70, 66, {"toUnion": {"wp": 69}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 70, "union");
        atStep(st, 71);
        cqWorkplaneNew(c, st, 71, {"plane": {"origin": [1.7937609650264676, -0.31147131800978367, 5], "xDir": [0.1921965790026202, 0.9813564464656508, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 72);
        cqRect(c, st, 72, 71, {"xLen": 2.3868615502681716, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 73);
        cqExtrude(c, st, 73, 72, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 73, "extrude");
        atStep(st, 74);
        cqUnion(c, st, 74, 70, {"toUnion": {"wp": 73}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 74, "union");
        atStep(st, 75);
        cqWorkplaneNew(c, st, 75, {"plane": {"origin": [2.449696900141276, 1.123970443218055, 5], "xDir": [0.8500883244886273, 0.5266401433314007, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 76);
        cqRect(c, st, 76, 75, {"xLen": 1.0035724772815615, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 77);
        cqExtrude(c, st, 77, 76, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 77, "extrude");
        atStep(st, 78);
        cqUnion(c, st, 78, 74, {"toUnion": {"wp": 77}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 78, "union");
        atStep(st, 79);
        cqWorkplaneNew(c, st, 79, {"plane": {"origin": [0.2467635874112502, 2.518921497255145, 5], "xDir": [0.447190264199996, -0.894438856269526, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 80);
        cqRect(c, st, 80, 79, {"xLen": 2.2767740958508864, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 81);
        cqExtrude(c, st, 81, 80, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 81, "extrude");
        atStep(st, 82);
        cqUnion(c, st, 82, 78, {"toUnion": {"wp": 81}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 82, "union");
        atStep(st, 83);
        cqWorkplaneNew(c, st, 83, {"plane": {"origin": [2.3320305754831523, 5.49108623377818, 5], "xDir": [-0.9139630745688958, -0.4057973611601867, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 84);
        cqRect(c, st, 84, 83, {"xLen": 1.1500150200378485, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 85);
        cqExtrude(c, st, 85, 84, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 85, "extrude");
        atStep(st, 86);
        cqUnion(c, st, 86, 82, {"toUnion": {"wp": 85}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 86, "union");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [2.883302975684537, 3.7687108392282127, 5], "xDir": [0.013158655862535024, -0.9999134211399964, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 88);
        cqRect(c, st, 88, 87, {"xLen": 3.911762525465933, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 89);
        cqExtrude(c, st, 89, 88, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 89, "extrude");
        atStep(st, 90);
        cqUnion(c, st, 90, 86, {"toUnion": {"wp": 89}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 90, "union");
        atStep(st, 91);
        cqWorkplaneNew(c, st, 91, {"plane": {"origin": [1.3894867347092834, 1.1802067773476097, 5], "xDir": [0.8923483916425191, -0.45134725869668174, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 92);
        cqRect(c, st, 92, 91, {"xLen": 1.4201797157006462, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 93);
        cqExtrude(c, st, 93, 92, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 93, "extrude");
        atStep(st, 94);
        cqUnion(c, st, 94, 90, {"toUnion": {"wp": 93}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 94, "union");
        atStep(st, 95);
        cqWorkplaneNew(c, st, 95, {"plane": {"origin": [2.980371319744769, 5.904289085763928, 5], "xDir": [-0.5638663778453927, -0.8258660351022536, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 96);
        cqRect(c, st, 96, 95, {"xLen": 0.43558231995945634, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 97);
        cqExtrude(c, st, 97, 96, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 97, "extrude");
        atStep(st, 98);
        cqUnion(c, st, 98, 94, {"toUnion": {"wp": 97}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 98, "union");
        atStep(st, 99);
        cqWorkplaneNew(c, st, 99, {"plane": {"origin": [-5.123057299662434, -4.054962582260259, 5], "xDir": [-0.2719285973314497, 0.9623174309724158, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 100);
        cqRect(c, st, 100, 99, {"xLen": 0.20919212768656295, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 101);
        cqExtrude(c, st, 101, 100, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 101, "extrude");
        atStep(st, 102);
        cqUnion(c, st, 102, 98, {"toUnion": {"wp": 101}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 102, "union");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [-5.530131906709624, -3.4787758039392034, 5], "xDir": [-0.6228940075471908, 0.7823062414181547, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 104);
        cqRect(c, st, 104, 103, {"xLen": 1.2157186986301527, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 105);
        cqExtrude(c, st, 105, 104, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 105, "extrude");
        atStep(st, 106);
        cqUnion(c, st, 106, 102, {"toUnion": {"wp": 105}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 106, "union");
        atStep(st, 107);
        cqWorkplaneNew(c, st, 107, {"plane": {"origin": [-5.957620912327087, -3.004013635519697, 5], "xDir": [0.9998758320089928, 0.015758190388639903, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 108);
        cqRect(c, st, 108, 107, {"xLen": 0.09772625346769802, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 109);
        cqExtrude(c, st, 109, 108, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 109, "extrude");
        atStep(st, 110);
        cqUnion(c, st, 110, 106, {"toUnion": {"wp": 109}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 110, "union");
        atStep(st, 111);
        cqWorkplaneNew(c, st, 111, {"plane": {"origin": [-4.361531194110707, -1.6467763432226774, 5], "xDir": [-0.6385875758141295, 0.7695491589338743, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 112);
        cqRect(c, st, 112, 111, {"xLen": 1.7831359977344838, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 113);
        cqExtrude(c, st, 113, 112, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 113, "extrude");
        atStep(st, 114);
        cqUnion(c, st, 114, 110, {"toUnion": {"wp": 113}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 114, "union");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 115, {"plane": {"origin": [-4.471843453815175, -3.1435948568480434, 5], "xDir": [-0.6424477582472715, -0.7663294839186702, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 116);
        cqRect(c, st, 116, 115, {"xLen": 2.115834316641698, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 117);
        cqExtrude(c, st, 117, 116, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 117, "extrude");
        atStep(st, 118);
        cqUnion(c, st, 118, 114, {"toUnion": {"wp": 117}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 118, "union");
        atStep(st, 119);
        cqWorkplaneNew(c, st, 119, {"plane": {"origin": [-5.419819647005157, -1.9819572903138376, 5], "xDir": [0.4318169315474776, 0.90196127279885, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 120);
        cqRect(c, st, 120, 119, {"xLen": 2.2645902469463675, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 121);
        cqExtrude(c, st, 121, 120, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 121, "extrude");
        atStep(st, 122);
        cqUnion(c, st, 122, 118, {"toUnion": {"wp": 121}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 122, "union");
        atStep(st, 123);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [-2.8555560775616247, -1.1275867005107632, 5], "xDir": [0.6628674878862821, -0.7487367317711411, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 124);
        cqRect(c, st, 124, 123, {"xLen": 4.000611807701113, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 125);
        cqExtrude(c, st, 125, 124, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 125, "extrude");
        atStep(st, 126);
        cqUnion(c, st, 126, 122, {"toUnion": {"wp": 125}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 126, "union");
        atStep(st, 127);
        cqWorkplaneNew(c, st, 127, {"plane": {"origin": [-4.567417249595549, -0.22731527770675852, 5], "xDir": [-0.5426072976673204, -0.8399865001999544, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 128);
        cqRect(c, st, 128, 127, {"xLen": 1.4224778185015206, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 129);
        cqExtrude(c, st, 129, 128, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 129, "extrude");
        atStep(st, 130);
        cqUnion(c, st, 130, 126, {"toUnion": {"wp": 129}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 130, "union");
        atStep(st, 131);
        cqWorkplaneNew(c, st, 131, {"plane": {"origin": [-3.115807991558106, -2.6307509431600296, 5], "xDir": [0.9151840753672892, -0.4030361127667345, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 132);
        cqRect(c, st, 132, 131, {"xLen": 1.4781265839029494, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 133);
        cqExtrude(c, st, 133, 132, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 133, "extrude");
        atStep(st, 134);
        cqUnion(c, st, 134, 130, {"toUnion": {"wp": 133}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 134, "union");
        atStep(st, 135);
        cqWorkplaneNew(c, st, 135, {"plane": {"origin": [-2.1196842759561383, -2.8959526414743944, 5], "xDir": [0.9948214150794517, 0.10163833971153448, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 136);
        cqRect(c, st, 136, 135, {"xLen": 0.6428184099633915, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 137);
        cqExtrude(c, st, 137, 136, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 137, "extrude");
        atStep(st, 138);
        cqUnion(c, st, 138, 134, {"toUnion": {"wp": 137}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 138, "union");
        atStep(st, 139);
        cqWorkplaneNew(c, st, 139, {"plane": {"origin": [-1.664778921954397, -2.744287174507199, 5], "xDir": [-0.7505574914492739, -0.6608051543604767, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 140);
        cqRect(c, st, 140, 139, {"xLen": 0.3601605351278389, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 141);
        cqExtrude(c, st, 141, 140, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 141, "extrude");
        atStep(st, 142);
        cqUnion(c, st, 142, 138, {"toUnion": {"wp": 141}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 142, "union");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 143, {"plane": {"origin": [-4.942108056660375, -0.8927086497281859, 5], "xDir": [0.16306499736187804, -0.9866153286034892, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 144);
        cqRect(c, st, 144, 143, {"xLen": 0.13776856666122525, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 145);
        cqExtrude(c, st, 145, 144, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 145, "extrude");
        atStep(st, 146);
        cqUnion(c, st, 146, 142, {"toUnion": {"wp": 145}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 146, "union");
        atStep(st, 147);
        cqWorkplaneNew(c, st, 147, {"plane": {"origin": [-3.8479974923647458, 0.945373778851508, 5], "xDir": [0.6660154650484011, 0.7459379332869203, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 148);
        cqRect(c, st, 148, 147, {"xLen": 0.6244980651551737, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 149);
        cqExtrude(c, st, 149, 148, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 149, "extrude");
        atStep(st, 150);
        cqUnion(c, st, 150, 146, {"toUnion": {"wp": 149}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 150, "union");
        atStep(st, 151);
        cqWorkplaneNew(c, st, 151, {"plane": {"origin": [-5.250576107157961, 1.0758237523662082, 5], "xDir": [-0.956720801143366, 0.29100740310101414, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 152);
        cqRect(c, st, 152, 151, {"xLen": 2.4973135918493017, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 153);
        cqExtrude(c, st, 153, 152, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 153, "extrude");
        atStep(st, 154);
        cqUnion(c, st, 154, 150, {"toUnion": {"wp": 153}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 154, "union");
        atStep(st, 155);
        cqWorkplaneNew(c, st, 155, {"plane": {"origin": [-4.67579866557432, 2.1235967346552553, 5], "xDir": [-0.7386249851124291, 0.6741165562183322, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 156);
        cqRect(c, st, 156, 155, {"xLen": 2.804573034298293, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 157);
        cqExtrude(c, st, 157, 156, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 157, "extrude");
        atStep(st, 158);
        cqUnion(c, st, 158, 154, {"toUnion": {"wp": 157}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 158, "union");
        atStep(st, 159);
        cqWorkplaneNew(c, st, 159, {"plane": {"origin": [-6.259444348117134, 2.2826892692395098, 5], "xDir": [0.21505870286234396, 0.9766011234496744, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 160);
        cqRect(c, st, 160, 159, {"xLen": 1.7274138336997362, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 161);
        cqExtrude(c, st, 161, 160, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 161, "extrude");
        atStep(st, 162);
        cqUnion(c, st, 162, 158, {"toUnion": {"wp": 161}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 162, "union");
        atStep(st, 163);
        cqWorkplaneNew(c, st, 163, {"plane": {"origin": [-5.892629591176567, 3.0975438534967914, 5], "xDir": [0.9877183655727998, -0.15624477690533264, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 164);
        cqRect(c, st, 164, 163, {"xLen": 0.3666370375620079, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 165);
        cqExtrude(c, st, 165, 164, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 165, "extrude");
        atStep(st, 166);
        cqUnion(c, st, 166, 162, {"toUnion": {"wp": 165}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 166, "union");
        atStep(st, 167);
        cqWorkplaneNew(c, st, 167, {"plane": {"origin": [-6.953365521204555, 1.2812023070650542, 5], "xDir": [0.9549146445075788, 0.2968804838735015, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 168);
        cqRect(c, st, 168, 167, {"xLen": 1.0643327899919353, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 169);
        cqExtrude(c, st, 169, 168, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 169, "extrude");
        atStep(st, 170);
        cqUnion(c, st, 170, 166, {"toUnion": {"wp": 169}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 170, "union");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [-0.28629022957091527, -3.995327063197524, 5], "xDir": [-0.23720840980430224, 0.9714587846728828, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 172);
        cqRect(c, st, 172, 171, {"xLen": 3.716493910045368, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 173);
        cqExtrude(c, st, 173, 172, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 173, "extrude");
        atStep(st, 174);
        cqUnion(c, st, 174, 170, {"toUnion": {"wp": 173}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 174, "union");
        atStep(st, 175);
        cqWorkplaneNew(c, st, 175, {"plane": {"origin": [0.7368772457112611, -5.760607075883488, 5], "xDir": [0.9976577076821586, 0.06840393484574275, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 176);
        cqRect(c, st, 176, 175, {"xLen": 1.1674859334482215, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 177);
        cqExtrude(c, st, 177, 176, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 177, "extrude");
        atStep(st, 178);
        cqUnion(c, st, 178, 174, {"toUnion": {"wp": 177}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 178, "union");
        atStep(st, 179);
        cqWorkplaneNew(c, st, 179, {"plane": {"origin": [0.7255057515254317, -3.909210490728644, 5], "xDir": [-0.31146720756484597, 0.950256901375495, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 180);
        cqRect(c, st, 180, 179, {"xLen": 3.812582190500892, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 181);
        cqExtrude(c, st, 181, 180, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 181, "extrude");
        atStep(st, 182);
        cqUnion(c, st, 182, 178, {"toUnion": {"wp": 181}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 182, "union");
        atStep(st, 183);
        cqWorkplaneNew(c, st, 183, {"plane": {"origin": [-0.2928320376615391, -2.140159440344361, 5], "xDir": [-0.9950473582203556, -0.09940198639208116, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 184);
        cqRect(c, st, 184, 183, {"xLen": 0.8534078733759243, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 185);
        cqExtrude(c, st, 185, 184, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 185, "extrude");
        atStep(st, 186);
        cqUnion(c, st, 186, 182, {"toUnion": {"wp": 185}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 186, "union");
        atStep(st, 187);
        cqWorkplaneNew(c, st, 187, {"plane": {"origin": [-0.7222523487003272, -2.1863456969507835, 5], "xDir": [-0.7881949294690742, -0.6154256682648532, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 188);
        cqRect(c, st, 188, 187, {"xLen": 0.012255054973415704, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 189);
        cqExtrude(c, st, 189, 188, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 189, "extrude");
        atStep(st, 190);
        cqUnion(c, st, 190, 186, {"toUnion": {"wp": 189}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 190, "union");
        atStep(st, 191);
        cqWorkplaneNew(c, st, 191, {"plane": {"origin": [-1.443041972017013, -6.043127517053804, 5], "xDir": [-0.7110918815172841, 0.7030990940402418, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 192);
        cqRect(c, st, 192, 191, {"xLen": 0.572234464638846, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 193);
        cqExtrude(c, st, 193, 192, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 193, "extrude");
        atStep(st, 194);
        cqUnion(c, st, 194, 190, {"toUnion": {"wp": 193}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 194, "union");
        atStep(st, 195);
        cqWorkplaneNew(c, st, 195, {"plane": {"origin": [-0.5425423776493871, -6.022416837816419, 5], "xDir": [0.9528891450681988, 0.30331877161032655, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 196);
        cqRect(c, st, 196, 195, {"xLen": 1.4630116355312277, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 197);
        cqExtrude(c, st, 197, 196, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 197, "extrude");
        atStep(st, 198);
        cqUnion(c, st, 198, 194, {"toUnion": {"wp": 197}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 198, "union");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 199, {"plane": {"origin": [-1.7232185644591178, -4.352621946866384, 5], "xDir": [-0.05144528654271841, 0.9986758145126664, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 200);
        cqRect(c, st, 200, 199, {"xLen": 2.9826231530020566, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 201);
        cqExtrude(c, st, 201, 200, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 201, "extrude");
        atStep(st, 202);
        cqUnion(c, st, 202, 198, {"toUnion": {"wp": 201}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 202, "union");
        atStep(st, 203);
        cqWorkplaneNew(c, st, 203, {"plane": {"origin": [-1.1283501814338202, -2.4077029700757246, 5], "xDir": [0.8790784236369711, 0.47667717073081867, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 204);
        cqRect(c, st, 204, 203, {"xLen": 0.9129291217912938, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 205);
        cqExtrude(c, st, 205, 204, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 205, "extrude");
        atStep(st, 206);
        cqUnion(c, st, 206, 202, {"toUnion": {"wp": 205}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 206, "union");
        atStep(st, 207);
        cqWorkplaneNew(c, st, 207, {"plane": {"origin": [-0.5144713435573067, 1.067329129743086, 5], "xDir": [0.9464386357680148, 0.322883738713456, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 208);
        cqRect(c, st, 208, 207, {"xLen": 2.6844012652999094, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 209);
        cqExtrude(c, st, 209, 208, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 209, "extrude");
        atStep(st, 210);
        cqUnion(c, st, 210, 206, {"toUnion": {"wp": 209}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 210, "union");
        atStep(st, 211);
        cqWorkplaneNew(c, st, 211, {"plane": {"origin": [-1.251102270927309, -0.7743101439414813, 5], "xDir": [0.3543699556257375, -0.9351053066632724, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 212);
        cqRect(c, st, 212, 211, {"xLen": 3.011991281144754, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 213);
        cqExtrude(c, st, 213, 212, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 213, "extrude");
        atStep(st, 214);
        cqUnion(c, st, 214, 210, {"toUnion": {"wp": 213}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 214, "union");
        atStep(st, 215);
        cqWorkplaneNew(c, st, 215, {"plane": {"origin": [0.8480731200256473, -1.790198262017187, 5], "xDir": [-0.9188875143337604, -0.3945196268901249, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 216);
        cqRect(c, st, 216, 215, {"xLen": 1.559090795270992, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 217);
        cqExtrude(c, st, 217, 216, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 217, "extrude");
        atStep(st, 218);
        cqUnion(c, st, 218, 214, {"toUnion": {"wp": 217}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 218, "union");
        atStep(st, 219);
        cqWorkplaneNew(c, st, 219, {"plane": {"origin": [-2.507078484955509, 1.0187653623793629, 5], "xDir": [-0.8825628307368193, 0.47019448082874454, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 220);
        cqRect(c, st, 220, 219, {"xLen": 1.6368162821971408, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 221);
        cqExtrude(c, st, 221, 220, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 221, "extrude");
        atStep(st, 222);
        cqUnion(c, st, 222, 218, {"toUnion": {"wp": 221}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 222, "union");
        atStep(st, 223);
        cqWorkplaneNew(c, st, 223, {"plane": {"origin": [-3.434704949191596, 1.290934265136248, 5], "xDir": [-0.8767370721905371, -0.4809699639756779, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 224);
        cqRect(c, st, 224, 223, {"xLen": 0.4683955202602669, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 225);
        cqExtrude(c, st, 225, 224, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 225, "extrude");
        atStep(st, 226);
        cqUnion(c, st, 226, 222, {"toUnion": {"wp": 225}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 226, "union");
        atStep(st, 227);
        cqWorkplaneNew(c, st, 227, {"plane": {"origin": [-4.118727002029481, 0.5412855926502815, 5], "xDir": [0.3442767418051207, 0.938868214954607, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 228);
        cqRect(c, st, 228, 227, {"xLen": 0.36463006296945893, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 229);
        cqExtrude(c, st, 229, 228, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 229, "extrude");
        atStep(st, 230);
        cqUnion(c, st, 230, 226, {"toUnion": {"wp": 229}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 230, "union");
        atStep(st, 231);
        cqWorkplaneNew(c, st, 231, {"plane": {"origin": [-2.959033604707373, -5.060874948831601, 5], "xDir": [0.7511387509639582, -0.6601443605760068, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 232);
        cqRect(c, st, 232, 231, {"xLen": 2.257284686291263, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 233);
        cqExtrude(c, st, 233, 232, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 233, "extrude");
        atStep(st, 234);
        cqUnion(c, st, 234, 230, {"toUnion": {"wp": 233}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 234, "union");
        atStep(st, 235);
        cqWorkplaneNew(c, st, 235, {"plane": {"origin": [-4.305336606167218, -4.47273286327583, 5], "xDir": [-0.9538611568032492, -0.30024805335250243, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 236);
        cqRect(c, st, 236, 235, {"xLen": 1.0453009811515617, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 237);
        cqExtrude(c, st, 237, 236, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 237, "extrude");
        atStep(st, 238);
        cqUnion(c, st, 238, 234, {"toUnion": {"wp": 237}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 238, "union");
        atStep(st, 239);
        cqWorkplaneNew(c, st, 239, {"plane": {"origin": [-4.888509261327169, -5.129507865433865, 5], "xDir": [-0.16694769809319973, -0.9859657530063516, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 240);
        cqRect(c, st, 240, 239, {"xLen": 1.013930165940979, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 241);
        cqExtrude(c, st, 241, 240, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 241, "extrude");
        atStep(st, 242);
        cqUnion(c, st, 242, 238, {"toUnion": {"wp": 241}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 242, "union");
        atStep(st, 243);
        cqWorkplaneNew(c, st, 243, {"plane": {"origin": [-3.694909885344875, -5.7538654261448166, 5], "xDir": [0.9952895668163552, -0.09694678017609576, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 244);
        cqRect(c, st, 244, 243, {"xLen": 2.5685711419372232, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 245);
        cqExtrude(c, st, 245, 244, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 245, "extrude");
        atStep(st, 246);
        cqUnion(c, st, 246, 242, {"toUnion": {"wp": 245}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 246, "union");
        atStep(st, 247);
        cqWorkplaneNew(c, st, 247, {"plane": {"origin": [-2.2639702302694795, -5.842157301921661, 5], "xDir": [0.9730104703441556, 0.23076096853810668, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 248);
        cqRect(c, st, 248, 247, {"xLen": 0.3138786891440948, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 249);
        cqExtrude(c, st, 249, 248, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 249, "extrude");
        atStep(st, 250);
        cqUnion(c, st, 250, 246, {"toUnion": {"wp": 249}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 250, "union");
        atStep(st, 251);
        cqWorkplaneNew(c, st, 251, {"plane": {"origin": [-1.878882108936739, -5.823950288493599, 5], "xDir": [-0.9970107775686802, 0.07726260034386526, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 252);
        cqRect(c, st, 252, 251, {"xLen": 0.4661624549774367, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 253);
        cqExtrude(c, st, 253, 252, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 253, "extrude");
        atStep(st, 254);
        cqUnion(c, st, 254, 250, {"toUnion": {"wp": 253}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 254, "union");
        atStep(st, 255);
        cqWorkplaneNew(c, st, 255, {"plane": {"origin": [-3.1231148203492043, -3.62221410516673, 5], "xDir": [-0.7020016736725364, -0.7121752945454916, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 256);
        cqRect(c, st, 256, 255, {"xLen": 1.94781810332986, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 257);
        cqExtrude(c, st, 257, 256, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 257, "extrude");
        atStep(st, 258);
        cqUnion(c, st, 258, 254, {"toUnion": {"wp": 257}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 258, "union");
        atStep(st, 259);
        cqWorkplaneNew(c, st, 259, {"plane": {"origin": [-4.949243623223379, -4.392637426681346, 5], "xDir": [0.5228249167889667, -0.8524400896160446, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 260);
        cqRect(c, st, 260, 259, {"xLen": 0.5560982686310582, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 261);
        cqExtrude(c, st, 261, 260, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 261, "extrude");
        atStep(st, 262);
        cqUnion(c, st, 262, 258, {"toUnion": {"wp": 261}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 262, "union");
        atStep(st, 263);
        cqWorkplaneNew(c, st, 263, {"plane": {"origin": [1.6764122177869176, -5.8243834371112495, 5], "xDir": [0.960335234780742, -0.2788480533168859, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 264);
        cqRect(c, st, 264, 263, {"xLen": 0.7438221343604915, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 265);
        cqExtrude(c, st, 265, 264, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 265, "extrude");
        atStep(st, 266);
        cqUnion(c, st, 266, 262, {"toUnion": {"wp": 265}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 266, "union");
        atStep(st, 267);
        cqWorkplaneNew(c, st, 267, {"plane": {"origin": [2.0749440418799736, -6.034441720230154, 5], "xDir": [-0.3625495077000824, 0.9319645135231426, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 268);
        cqRect(c, st, 268, 267, {"xLen": 0.22823102057103653, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 269);
        cqExtrude(c, st, 269, 268, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 269, "extrude");
        atStep(st, 270);
        cqUnion(c, st, 270, 266, {"toUnion": {"wp": 269}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 270, "union");
        atStep(st, 271);
        cqWorkplaneNew(c, st, 271, {"plane": {"origin": [4.081668524187343, -2.6457564793490707, 5], "xDir": [0.4285059529864252, -0.903538957796063, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 272);
        cqRect(c, st, 272, 271, {"xLen": 1.085289587076616, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 273);
        cqExtrude(c, st, 273, 272, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 273, "extrude");
        atStep(st, 274);
        cqUnion(c, st, 274, 270, {"toUnion": {"wp": 273}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 274, "union");
        atStep(st, 275);
        cqWorkplaneNew(c, st, 275, {"plane": {"origin": [2.064829984374981, -2.934438994982784, 5], "xDir": [0.28751552146858267, -0.9577759784598112, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 276);
        cqRect(c, st, 276, 275, {"xLen": 2.8892486779374984, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 277);
        cqExtrude(c, st, 277, 276, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 277, "extrude");
        atStep(st, 278);
        cqUnion(c, st, 278, 274, {"toUnion": {"wp": 277}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 278, "union");
        atStep(st, 279);
        cqWorkplaneNew(c, st, 279, {"plane": {"origin": [3.3971884765476927, -3.7270613376507846, 5], "xDir": [-0.8405525214075007, -0.5417300607825757, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 280);
        cqRect(c, st, 280, 279, {"xLen": 2.1819137975872884, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 281);
        cqExtrude(c, st, 281, 280, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 281, "extrude");
        atStep(st, 282);
        cqUnion(c, st, 282, 278, {"toUnion": {"wp": 281}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 282, "union");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [2.17902041502042, -5.722318514724012, 5], "xDir": [0.5772084589887053, 0.8165968374123698, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 284);
        cqRect(c, st, 284, 283, {"xLen": 0.5039735400629346, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 285);
        cqExtrude(c, st, 285, 284, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 285, "extrude");
        atStep(st, 286);
        cqUnion(c, st, 286, 282, {"toUnion": {"wp": 285}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 286, "union");
        atStep(st, 287);
        cqWorkplaneNew(c, st, 287, {"plane": {"origin": [3.19883006935351, -5.232025648355673, 5], "xDir": [-0.950920828222418, -0.30943428777818177, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 288);
        cqRect(c, st, 288, 287, {"xLen": 1.8389769856069973, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 289);
        cqExtrude(c, st, 289, 288, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 289, "extrude");
        atStep(st, 290);
        cqUnion(c, st, 290, 286, {"toUnion": {"wp": 289}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 290, "union");
        atStep(st, 291);
        cqWorkplaneNew(c, st, 291, {"plane": {"origin": [2.402325607377806, -4.917306199995997, 5], "xDir": [-0.12884200516076932, -0.9916651338562592, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 292);
        cqRect(c, st, 292, 291, {"xLen": 1.2085545710784673, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 293);
        cqExtrude(c, st, 293, 292, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 293, "extrude");
        atStep(st, 294);
        cqUnion(c, st, 294, 290, {"toUnion": {"wp": 293}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 294, "union");
        atStep(st, 295);
        cqWorkplaneNew(c, st, 295, {"plane": {"origin": [4.424397312130875, -4.315331512183918, 5], "xDir": [0.485642227509022, 0.8741576670487284, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 296);
        cqRect(c, st, 296, 295, {"xLen": 1.4463589192443278, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 297);
        cqExtrude(c, st, 297, 296, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 297, "extrude");
        atStep(st, 298);
        cqUnion(c, st, 298, 294, {"toUnion": {"wp": 297}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 298, "union");
        atStep(st, 299);
        cqWorkplaneNew(c, st, 299, {"plane": {"origin": [4.545430209843884, -3.333584979962496, 5], "xDir": [0.7603502571630121, -0.6495132688653416, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 300);
        cqRect(c, st, 300, 299, {"xLen": 0.6082332690490887, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 301);
        cqExtrude(c, st, 301, 300, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 301, "extrude");
        atStep(st, 302);
        cqUnion(c, st, 302, 298, {"toUnion": {"wp": 301}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 302, "union");
        atStep(st, 303);
        cqWorkplaneNew(c, st, 303, {"plane": {"origin": [4.776134583451363, -3.6071357061359537, 5], "xDir": [-0.006981770851902064, -0.9999756271408676, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 304);
        cqRect(c, st, 304, 303, {"xLen": 0.15204957941716812, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 305);
        cqExtrude(c, st, 305, 304, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 305, "extrude");
        atStep(st, 306);
        cqUnion(c, st, 306, 302, {"toUnion": {"wp": 305}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 306, "union");
        atStep(st, 307);
        cqWorkplaneNew(c, st, 307, {"plane": {"origin": [-2.498575138553216, 4.030172286937479, 5], "xDir": [0.5797281164204927, -0.814809984616995, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 308);
        cqRect(c, st, 308, 307, {"xLen": 0.5514915921671111, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 309);
        cqExtrude(c, st, 309, 308, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 309, "extrude");
        atStep(st, 310);
        cqUnion(c, st, 310, 306, {"toUnion": {"wp": 309}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 310, "union");
        atStep(st, 311);
        cqWorkplaneNew(c, st, 311, {"plane": {"origin": [-3.843921129722915, 4.0379972173812915, 5], "xDir": [-0.9836777205355748, -0.17993927342838711, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 312);
        cqRect(c, st, 312, 311, {"xLen": 2.4103186957408314, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 313);
        cqExtrude(c, st, 313, 312, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 313, "extrude");
        atStep(st, 314);
        cqUnion(c, st, 314, 310, {"toUnion": {"wp": 313}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 314, "union");
        atStep(st, 315);
        cqWorkplaneNew(c, st, 315, {"plane": {"origin": [-2.562342046494241, 3.3209682544506585, 5], "xDir": [-0.4190554274180881, -0.9079606537463192, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 316);
        cqRect(c, st, 316, 315, {"xLen": 1.0672788575641352, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 317);
        cqExtrude(c, st, 317, 316, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 317, "extrude");
        atStep(st, 318);
        cqUnion(c, st, 318, 314, {"toUnion": {"wp": 317}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 318, "union");
        atStep(st, 319);
        cqWorkplaneNew(c, st, 319, {"plane": {"origin": [-3.9434932622281353, 3.227426052809136, 5], "xDir": [-0.9474138519910388, 0.32001092646267887, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 320);
        cqRect(c, st, 320, 319, {"xLen": 2.443550333121854, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 321);
        cqExtrude(c, st, 321, 320, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 321, "extrude");
        atStep(st, 322);
        cqUnion(c, st, 322, 318, {"toUnion": {"wp": 321}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 322, "union");
        atStep(st, 323);
        cqWorkplaneNew(c, st, 323, {"plane": {"origin": [-5.065214754482421, 3.719774587874726, 5], "xDir": [-0.33305658805942195, -0.942906840122722, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 324);
        cqRect(c, st, 324, 323, {"xLen": 0.21500985626987448, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 325);
        cqExtrude(c, st, 325, 324, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 325, "extrude");
        atStep(st, 326);
        cqUnion(c, st, 326, 322, {"toUnion": {"wp": 325}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 326, "union");
        atStep(st, 327);
        cqWorkplaneNew(c, st, 327, {"plane": {"origin": [-2.896351343834504, 4.668702036806094, 5], "xDir": [0.4984006780882699, -0.8669468057967298, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 328);
        cqRect(c, st, 328, 327, {"xLen": 0.9547282929850364, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 329);
        cqExtrude(c, st, 329, 328, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 329, "extrude");
        atStep(st, 330);
        cqUnion(c, st, 330, 326, {"toUnion": {"wp": 329}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 330, "union");
        atStep(st, 331);
        cqWorkplaneNew(c, st, 331, {"plane": {"origin": [-3.007670818035588, 2.1200105016090514, 5], "xDir": [0.29562391486202394, 0.9553044022517906, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 332);
        cqRect(c, st, 332, 331, {"xLen": 1.4999075614664596, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 333);
        cqExtrude(c, st, 333, 332, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 333, "extrude");
        atStep(st, 334);
        cqUnion(c, st, 334, 330, {"toUnion": {"wp": 333}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 334, "union");
        atStep(st, 335);
        cqWorkplaneNew(c, st, 335, {"plane": {"origin": [-2.0887169628006133, 3.841773293550816, 5], "xDir": [-0.9896327858187572, -0.1436208523599754, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 336);
        cqRect(c, st, 336, 335, {"xLen": 0.5052390914300197, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 337);
        cqExtrude(c, st, 337, 336, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 337, "extrude");
        atStep(st, 338);
        cqUnion(c, st, 338, 334, {"toUnion": {"wp": 337}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 338, "union");
        atStep(st, 339);
        cqWorkplaneNew(c, st, 339, {"plane": {"origin": [-5.406291251236792, 3.343654374108315, 5], "xDir": [0.7432833265787299, 0.6689767532807528, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 340);
        cqRect(c, st, 340, 339, {"xLen": 0.8214129424786463, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 341);
        cqExtrude(c, st, 341, 340, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 341, "extrude");
        atStep(st, 342);
        cqUnion(c, st, 342, 338, {"toUnion": {"wp": 341}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 342, "union");
        atStep(st, 343);
        cqWorkplaneNew(c, st, 343, {"plane": {"origin": [-1.217471818698371, 6.205473343922691, 5], "xDir": [0.8620709018862028, -0.5067876874205892, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 344);
        cqRect(c, st, 344, 343, {"xLen": 1.1437452262953527, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 345);
        cqExtrude(c, st, 345, 344, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 345, "extrude");
        atStep(st, 346);
        cqUnion(c, st, 346, 342, {"toUnion": {"wp": 345}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 346, "union");
        atStep(st, 347);
        cqWorkplaneNew(c, st, 347, {"plane": {"origin": [-0.5494333305381962, 6.459008620267362, 5], "xDir": [0.3066354431994396, 0.9518270352190482, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 348);
        cqRect(c, st, 348, 347, {"xLen": 1.141705909489968, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 349);
        cqExtrude(c, st, 349, 348, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 349, "extrude");
        atStep(st, 350);
        cqUnion(c, st, 350, 346, {"toUnion": {"wp": 349}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 350, "union");
        atStep(st, 351);
        cqWorkplaneNew(c, st, 351, {"plane": {"origin": [-2.422368258110006, 5.788921350924317, 5], "xDir": [0.7098593218157182, 0.7043434838424564, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 352);
        cqRect(c, st, 352, 351, {"xLen": 2.005754318223096, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 353);
        cqExtrude(c, st, 353, 352, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 353, "extrude");
        atStep(st, 354);
        cqUnion(c, st, 354, 350, {"toUnion": {"wp": 353}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 354, "union");
        atStep(st, 355);
        cqWorkplaneNew(c, st, 355, {"plane": {"origin": [-0.7183108650979729, 5.8933424226449755, 5], "xDir": [-0.2663675471474208, 0.9638715318063223, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 356);
        cqRect(c, st, 356, 355, {"xLen": 0.04629853963965428, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 357);
        cqExtrude(c, st, 357, 356, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 357, "extrude");
        atStep(st, 358);
        cqUnion(c, st, 358, 354, {"toUnion": {"wp": 357}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 358, "union");
        atStep(st, 359);
        cqWorkplaneNew(c, st, 359, {"plane": {"origin": [1.3643934252455134, 6.5432586516325095, 5], "xDir": [-0.9668649927342527, 0.2552882406711939, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 360);
        cqRect(c, st, 360, 359, {"xLen": 3.5967441578095984, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 361);
        cqExtrude(c, st, 361, 360, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 361, "extrude");
        atStep(st, 362);
        cqUnion(c, st, 362, 358, {"toUnion": {"wp": 361}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 362, "union");
        atStep(st, 363);
        cqWorkplaneNew(c, st, 363, {"plane": {"origin": [-6.666737981891973, -1.7495233469670448, 5], "xDir": [0.28488641065699827, 0.9585612828729168, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 364);
        cqRect(c, st, 364, 363, {"xLen": 2.149973316931467, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 365);
        cqExtrude(c, st, 365, 364, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 365, "extrude");
        atStep(st, 366);
        cqUnion(c, st, 366, 362, {"toUnion": {"wp": 365}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 366, "union");
        atStep(st, 367);
        cqWorkplaneNew(c, st, 367, {"plane": {"origin": [-6.4154311344440265, -0.7014770695299443, 5], "xDir": [0.9523024691141692, -0.3051557099565022, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 368);
        cqRect(c, st, 368, 367, {"xLen": 0.11538821953832186, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 369);
        cqExtrude(c, st, 369, 368, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 369, "extrude");
        atStep(st, 370);
        cqUnion(c, st, 370, 366, {"toUnion": {"wp": 369}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 370, "union");
        atStep(st, 371);
        cqWorkplaneNew(c, st, 371, {"plane": {"origin": [-6.489732522175546, -2.892373783675491, 5], "xDir": [-0.9739968792414212, 0.2265614248453885, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 372);
        cqRect(c, st, 372, 371, {"xLen": 0.9923123177299384, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 373);
        cqExtrude(c, st, 373, 372, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 373, "extrude");
        atStep(st, 374);
        cqUnion(c, st, 374, 370, {"toUnion": {"wp": 373}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 374, "union");
        atStep(st, 375);
        cqWorkplaneNew(c, st, 375, {"plane": {"origin": [-6.965956191365776, 0.2196705538572035, 5], "xDir": [0.4809012995197861, -0.8767747373870788, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 376);
        cqRect(c, st, 376, 375, {"xLen": 2.0610583262308, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 377);
        cqExtrude(c, st, 377, 376, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 377, "extrude");
        atStep(st, 378);
        cqUnion(c, st, 378, 374, {"toUnion": {"wp": 377}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 378, "union");
        atStep(st, 379);
        cqWorkplaneNew(c, st, 379, {"plane": {"origin": [-5.656914781698734, -0.7719145582255993, 5], "xDir": [-0.9971925678577224, 0.07487978772220277, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 380);
        cqRect(c, st, 380, 379, {"xLen": 1.4111098141661274, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 381);
        cqExtrude(c, st, 381, 380, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 381, "extrude");
        atStep(st, 382);
        cqUnion(c, st, 382, 378, {"toUnion": {"wp": 381}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 382, "union");
        atStep(st, 383);
        cqWorkplaneNew(c, st, 383, {"plane": {"origin": [6.081634076215293, -3.552543348622097, 5], "xDir": [-0.6041411698906853, 0.7968773097805674, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 384);
        cqRect(c, st, 384, 383, {"xLen": 0.9979986814545404, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 385);
        cqExtrude(c, st, 385, 384, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 385, "extrude");
        atStep(st, 386);
        cqUnion(c, st, 386, 382, {"toUnion": {"wp": 385}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 386, "union");
        atStep(st, 387);
        cqWorkplaneNew(c, st, 387, {"plane": {"origin": [6.114903407987574, -2.7287837309515948, 5], "xDir": [0.6177394965706219, 0.786382803968064, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 388);
        cqRect(c, st, 388, 387, {"xLen": 1.083742836947344, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 389);
        cqExtrude(c, st, 389, 388, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 389, "extrude");
        atStep(st, 390);
        cqUnion(c, st, 390, 386, {"toUnion": {"wp": 389}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 390, "union");
        atStep(st, 391);
        cqWorkplaneNew(c, st, 391, {"plane": {"origin": [5.2784167009229055, -3.343007432884966, 5], "xDir": [0.9363607172400534, 0.3510393243066832, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 392);
        cqRect(c, st, 392, 391, {"xLen": 1.071705210551033, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 393);
        cqExtrude(c, st, 393, 392, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 393, "extrude");
        atStep(st, 394);
        cqUnion(c, st, 394, 390, {"toUnion": {"wp": 393}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 394, "union");
        atStep(st, 395);
        cqWorkplaneNew(c, st, 395, {"plane": {"origin": [6.240850759129625, -1.6635542213046792, 5], "xDir": [0.31053439674195515, -0.9505621433868016, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 396);
        cqRect(c, st, 396, 395, {"xLen": 1.3447014456526087, "yLen": 0.4, "centered": true, "forConstruction": false});
        atStep(st, 397);
        cqExtrude(c, st, 397, 396, {"until": 16, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 397, "extrude");
        atStep(st, 398);
        cqUnion(c, st, 398, 394, {"toUnion": {"wp": 397}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 398, "union");
        atStep(st, 399);
        cqUnion(c, st, 399, 11, {"toUnion": {"wp": 398}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 399, "union");
        atStep(st, 400);
        cqWorkplaneNew(c, st, 400, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 401);
        cqTransformed(c, st, 401, 400, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 402);
        cqCenter(c, st, 402, 401, {"x": 17.5, "y": 0});
        atStep(st, 403);
        cqRect(c, st, 403, 402, {"xLen": 5, "yLen": 6.828161270564489, "centered": true, "forConstruction": false});
        atStep(st, 404);
        cqExtrude(c, st, 404, 403, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 404, "extrude");
        atStep(st, 405);
        cqWorkplaneNew(c, st, 405, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 406);
        cqTransformed(c, st, 406, 405, {"rotate": [0, 0, 45], "offset": [0, 0, 0]});
        atStep(st, 407);
        cqCenter(c, st, 407, 406, {"x": 17.5, "y": 0});
        atStep(st, 408);
        cqRect(c, st, 408, 407, {"xLen": 5, "yLen": 6.828161270564489, "centered": true, "forConstruction": false});
        atStep(st, 409);
        cqExtrude(c, st, 409, 408, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 409, "extrude");
        atStep(st, 410);
        cqUnion(c, st, 410, 404, {"toUnion": {"wp": 409}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 410, "union");
        atStep(st, 411);
        cqWorkplaneNew(c, st, 411, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 412);
        cqTransformed(c, st, 412, 411, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 413);
        cqCenter(c, st, 413, 412, {"x": 17.5, "y": 0});
        atStep(st, 414);
        cqRect(c, st, 414, 413, {"xLen": 5, "yLen": 6.828161270564489, "centered": true, "forConstruction": false});
        atStep(st, 415);
        cqExtrude(c, st, 415, 414, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 415, "extrude");
        atStep(st, 416);
        cqUnion(c, st, 416, 410, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        atStep(st, 417);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 418);
        cqTransformed(c, st, 418, 417, {"rotate": [0, 0, 135], "offset": [0, 0, 0]});
        atStep(st, 419);
        cqCenter(c, st, 419, 418, {"x": 17.5, "y": 0});
        atStep(st, 420);
        cqRect(c, st, 420, 419, {"xLen": 5, "yLen": 6.828161270564489, "centered": true, "forConstruction": false});
        atStep(st, 421);
        cqExtrude(c, st, 421, 420, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 421, "extrude");
        atStep(st, 422);
        cqUnion(c, st, 422, 416, {"toUnion": {"wp": 421}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 422, "union");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 424);
        cqTransformed(c, st, 424, 423, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 425);
        cqCenter(c, st, 425, 424, {"x": 17.5, "y": 0});
        atStep(st, 426);
        cqRect(c, st, 426, 425, {"xLen": 5, "yLen": 6.828161270564489, "centered": true, "forConstruction": false});
        atStep(st, 427);
        cqExtrude(c, st, 427, 426, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 427, "extrude");
        atStep(st, 428);
        cqUnion(c, st, 428, 422, {"toUnion": {"wp": 427}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 428, "union");
        atStep(st, 429);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 430);
        cqTransformed(c, st, 430, 429, {"rotate": [0, 0, 225], "offset": [0, 0, 0]});
        atStep(st, 431);
        cqCenter(c, st, 431, 430, {"x": 17.5, "y": 0});
        atStep(st, 432);
        cqRect(c, st, 432, 431, {"xLen": 5, "yLen": 6.828161270564489, "centered": true, "forConstruction": false});
        atStep(st, 433);
        cqExtrude(c, st, 433, 432, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 433, "extrude");
        atStep(st, 434);
        cqUnion(c, st, 434, 428, {"toUnion": {"wp": 433}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 434, "union");
        atStep(st, 435);
        cqWorkplaneNew(c, st, 435, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 436);
        cqTransformed(c, st, 436, 435, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 437);
        cqCenter(c, st, 437, 436, {"x": 17.5, "y": 0});
        atStep(st, 438);
        cqRect(c, st, 438, 437, {"xLen": 5, "yLen": 6.828161270564489, "centered": true, "forConstruction": false});
        atStep(st, 439);
        cqExtrude(c, st, 439, 438, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 439, "extrude");
        atStep(st, 440);
        cqUnion(c, st, 440, 434, {"toUnion": {"wp": 439}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 440, "union");
        atStep(st, 441);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 442);
        cqTransformed(c, st, 442, 441, {"rotate": [0, 0, 315], "offset": [0, 0, 0]});
        atStep(st, 443);
        cqCenter(c, st, 443, 442, {"x": 17.5, "y": 0});
        atStep(st, 444);
        cqRect(c, st, 444, 443, {"xLen": 5, "yLen": 6.828161270564489, "centered": true, "forConstruction": false});
        atStep(st, 445);
        cqExtrude(c, st, 445, 444, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 445, "extrude");
        atStep(st, 446);
        cqUnion(c, st, 446, 440, {"toUnion": {"wp": 445}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 446, "union");
        atStep(st, 447);
        cqUnion(c, st, 447, 399, {"toUnion": {"wp": 446}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 447, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 447}), {"i": 3609});
    } catch (e) {
        return {"i": 3609, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_3609(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c3609"] = r; }
    return out;
}
