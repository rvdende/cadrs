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
    return makeId(prefix ~ s.n);
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
const skSpline = function(sk, nm, e, to2, P, st) {
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
            } else if (e.k == "spline") skSpline(sk, nm, e, to2, P, st);
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
            else if (e.k == "spline") skSpline(sk, "p" ~ j, e, to2, planar, st);
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
    return {"status": "ok",
            "vol": evVolume(c, {"entities": B, "accuracy": "HIGH"}) / (MM ^ 3),
            "area": evArea(c, {"entities": faces}) / (MM ^ 2),
            "solids": size(ents(c, qBodyType(qEntityFilter(B, EntityType.BODY), BodyType.SOLID))),
            "faces": size(ents(c, faces)),
            "edges": size(ents(c, qOwnedByBody(B, EntityType.EDGE))),
            "vertices": size(ents(c, qOwnedByBody(B, EntityType.VERTEX))),
            "bbox": [bb.minCorner[0] / MM, bb.minCorner[1] / MM, bb.minCorner[2] / MM, bb.maxCorner[0] / MM, bb.maxCorner[1] / MM, bb.maxCorner[2] / MM],
            "other": other, "warn": st[].warn, "log": st[].log};
};
const newState = function() { return new box({"wps": {}, "ctx": [], "shapes": {}, "n": 0, "log": [], "warn": [], "cur": 0}); };
const atStep = function(st, i) { var s = st[]; s.cur = i; st[] = s; };
const stepVol = function(c, st, out, op) {   // after a solid-producing step: its solids and volume
    const w = st[].wps[out];
    if (w == undefined) return;
    const q = solidsQ(c, w.objs);
    if (q == undefined) return;
    logStep(c, st, out, op, {"solids": size(ents(c, q)), "vol": evVolume(c, {"entities": q}) / (MM ^ 3)});
};

const case_2351 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqCircle(c, st, 2, 1, {"radius": 10, "forConstruction": false});
        atStep(st, 3);
        cqExtrude(c, st, 3, 2, {"until": 25, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 3, "extrude");
        atStep(st, 4);
        cqWorkplaneNew(c, st, 4, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 5);
        cqPolyline(c, st, 5, 4, {"listOfXYTuple": [[10, 0], [12, 0], [12, 0.6944444444444444], [10, 0.6944444444444444]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 6);
        cqClose(c, st, 6, 5, {});
        atStep(st, 7);
        cqExtrude(c, st, 7, 6, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 7, "extrude");
        atStep(st, 8);
        cqRotate(c, st, 8, 7, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 5});
        stepVol(c, st, 8, "rotate");
        atStep(st, 9);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 10);
        cqPolyline(c, st, 10, 9, {"listOfXYTuple": [[10, 0.6944444444444444], [12, 0.6944444444444444], [12, 1.3888888888888888], [10, 1.3888888888888888]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 11);
        cqClose(c, st, 11, 10, {});
        atStep(st, 12);
        cqExtrude(c, st, 12, 11, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 12, "extrude");
        atStep(st, 13);
        cqRotate(c, st, 13, 12, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 15});
        stepVol(c, st, 13, "rotate");
        atStep(st, 14);
        cqUnion(c, st, 14, 8, {"toUnion": {"wp": 13}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 14, "union");
        atStep(st, 15);
        cqWorkplaneNew(c, st, 15, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 16);
        cqPolyline(c, st, 16, 15, {"listOfXYTuple": [[10, 1.3888888888888888], [12, 1.3888888888888888], [12, 2.083333333333333], [10, 2.083333333333333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 17);
        cqClose(c, st, 17, 16, {});
        atStep(st, 18);
        cqExtrude(c, st, 18, 17, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 18, "extrude");
        atStep(st, 19);
        cqRotate(c, st, 19, 18, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 25});
        stepVol(c, st, 19, "rotate");
        atStep(st, 20);
        cqUnion(c, st, 20, 14, {"toUnion": {"wp": 19}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 20, "union");
        atStep(st, 21);
        cqWorkplaneNew(c, st, 21, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 22);
        cqPolyline(c, st, 22, 21, {"listOfXYTuple": [[10, 2.083333333333333], [12, 2.083333333333333], [12, 2.7777777777777777], [10, 2.7777777777777777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 23);
        cqClose(c, st, 23, 22, {});
        atStep(st, 24);
        cqExtrude(c, st, 24, 23, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 24, "extrude");
        atStep(st, 25);
        cqRotate(c, st, 25, 24, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 35});
        stepVol(c, st, 25, "rotate");
        atStep(st, 26);
        cqUnion(c, st, 26, 20, {"toUnion": {"wp": 25}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 26, "union");
        atStep(st, 27);
        cqWorkplaneNew(c, st, 27, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 28);
        cqPolyline(c, st, 28, 27, {"listOfXYTuple": [[10, 2.7777777777777777], [12, 2.7777777777777777], [12, 3.4722222222222223], [10, 3.4722222222222223]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 29);
        cqClose(c, st, 29, 28, {});
        atStep(st, 30);
        cqExtrude(c, st, 30, 29, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 30, "extrude");
        atStep(st, 31);
        cqRotate(c, st, 31, 30, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 45});
        stepVol(c, st, 31, "rotate");
        atStep(st, 32);
        cqUnion(c, st, 32, 26, {"toUnion": {"wp": 31}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 32, "union");
        atStep(st, 33);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 34);
        cqPolyline(c, st, 34, 33, {"listOfXYTuple": [[10, 3.4722222222222223], [12, 3.4722222222222223], [12, 4.166666666666666], [10, 4.166666666666666]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 35);
        cqClose(c, st, 35, 34, {});
        atStep(st, 36);
        cqExtrude(c, st, 36, 35, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 36, "extrude");
        atStep(st, 37);
        cqRotate(c, st, 37, 36, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 55.00000000000001});
        stepVol(c, st, 37, "rotate");
        atStep(st, 38);
        cqUnion(c, st, 38, 32, {"toUnion": {"wp": 37}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 38, "union");
        atStep(st, 39);
        cqWorkplaneNew(c, st, 39, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 40);
        cqPolyline(c, st, 40, 39, {"listOfXYTuple": [[10, 4.166666666666666], [12, 4.166666666666666], [12, 4.861111111111112], [10, 4.861111111111112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 41);
        cqClose(c, st, 41, 40, {});
        atStep(st, 42);
        cqExtrude(c, st, 42, 41, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 42, "extrude");
        atStep(st, 43);
        cqRotate(c, st, 43, 42, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 65});
        stepVol(c, st, 43, "rotate");
        atStep(st, 44);
        cqUnion(c, st, 44, 38, {"toUnion": {"wp": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 44, "union");
        atStep(st, 45);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 46);
        cqPolyline(c, st, 46, 45, {"listOfXYTuple": [[10, 4.861111111111112], [12, 4.861111111111112], [12, 5.555555555555555], [10, 5.555555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 47);
        cqClose(c, st, 47, 46, {});
        atStep(st, 48);
        cqExtrude(c, st, 48, 47, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 48, "extrude");
        atStep(st, 49);
        cqRotate(c, st, 49, 48, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 75});
        stepVol(c, st, 49, "rotate");
        atStep(st, 50);
        cqUnion(c, st, 50, 44, {"toUnion": {"wp": 49}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 50, "union");
        atStep(st, 51);
        cqWorkplaneNew(c, st, 51, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 52);
        cqPolyline(c, st, 52, 51, {"listOfXYTuple": [[10, 5.555555555555555], [12, 5.555555555555555], [12, 6.25], [10, 6.25]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 53);
        cqClose(c, st, 53, 52, {});
        atStep(st, 54);
        cqExtrude(c, st, 54, 53, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 54, "extrude");
        atStep(st, 55);
        cqRotate(c, st, 55, 54, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 85});
        stepVol(c, st, 55, "rotate");
        atStep(st, 56);
        cqUnion(c, st, 56, 50, {"toUnion": {"wp": 55}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 56, "union");
        atStep(st, 57);
        cqWorkplaneNew(c, st, 57, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 58);
        cqPolyline(c, st, 58, 57, {"listOfXYTuple": [[10, 6.25], [12, 6.25], [12, 6.944444444444445], [10, 6.944444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 59);
        cqClose(c, st, 59, 58, {});
        atStep(st, 60);
        cqExtrude(c, st, 60, 59, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 60, "extrude");
        atStep(st, 61);
        cqRotate(c, st, 61, 60, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 95});
        stepVol(c, st, 61, "rotate");
        atStep(st, 62);
        cqUnion(c, st, 62, 56, {"toUnion": {"wp": 61}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 62, "union");
        atStep(st, 63);
        cqWorkplaneNew(c, st, 63, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 64);
        cqPolyline(c, st, 64, 63, {"listOfXYTuple": [[10, 6.944444444444445], [12, 6.944444444444445], [12, 7.638888888888889], [10, 7.638888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 65);
        cqClose(c, st, 65, 64, {});
        atStep(st, 66);
        cqExtrude(c, st, 66, 65, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 66, "extrude");
        atStep(st, 67);
        cqRotate(c, st, 67, 66, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 105});
        stepVol(c, st, 67, "rotate");
        atStep(st, 68);
        cqUnion(c, st, 68, 62, {"toUnion": {"wp": 67}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 68, "union");
        atStep(st, 69);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 70);
        cqPolyline(c, st, 70, 69, {"listOfXYTuple": [[10, 7.638888888888889], [12, 7.638888888888889], [12, 8.333333333333332], [10, 8.333333333333332]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 71);
        cqClose(c, st, 71, 70, {});
        atStep(st, 72);
        cqExtrude(c, st, 72, 71, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 72, "extrude");
        atStep(st, 73);
        cqRotate(c, st, 73, 72, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 115});
        stepVol(c, st, 73, "rotate");
        atStep(st, 74);
        cqUnion(c, st, 74, 68, {"toUnion": {"wp": 73}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 74, "union");
        atStep(st, 75);
        cqWorkplaneNew(c, st, 75, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 76);
        cqPolyline(c, st, 76, 75, {"listOfXYTuple": [[10, 8.333333333333332], [12, 8.333333333333332], [12, 9.027777777777777], [10, 9.027777777777777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 77);
        cqClose(c, st, 77, 76, {});
        atStep(st, 78);
        cqExtrude(c, st, 78, 77, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 78, "extrude");
        atStep(st, 79);
        cqRotate(c, st, 79, 78, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 125});
        stepVol(c, st, 79, "rotate");
        atStep(st, 80);
        cqUnion(c, st, 80, 74, {"toUnion": {"wp": 79}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 80, "union");
        atStep(st, 81);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 82);
        cqPolyline(c, st, 82, 81, {"listOfXYTuple": [[10, 9.027777777777777], [12, 9.027777777777777], [12, 9.722222222222223], [10, 9.722222222222223]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 83);
        cqClose(c, st, 83, 82, {});
        atStep(st, 84);
        cqExtrude(c, st, 84, 83, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 84, "extrude");
        atStep(st, 85);
        cqRotate(c, st, 85, 84, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 135});
        stepVol(c, st, 85, "rotate");
        atStep(st, 86);
        cqUnion(c, st, 86, 80, {"toUnion": {"wp": 85}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 86, "union");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 88);
        cqPolyline(c, st, 88, 87, {"listOfXYTuple": [[10, 9.722222222222223], [12, 9.722222222222223], [12, 10.416666666666668], [10, 10.416666666666668]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 89);
        cqClose(c, st, 89, 88, {});
        atStep(st, 90);
        cqExtrude(c, st, 90, 89, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 90, "extrude");
        atStep(st, 91);
        cqRotate(c, st, 91, 90, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 145});
        stepVol(c, st, 91, "rotate");
        atStep(st, 92);
        cqUnion(c, st, 92, 86, {"toUnion": {"wp": 91}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 92, "union");
        atStep(st, 93);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 94);
        cqPolyline(c, st, 94, 93, {"listOfXYTuple": [[10, 10.416666666666668], [12, 10.416666666666668], [12, 11.11111111111111], [10, 11.11111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 95);
        cqClose(c, st, 95, 94, {});
        atStep(st, 96);
        cqExtrude(c, st, 96, 95, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 96, "extrude");
        atStep(st, 97);
        cqRotate(c, st, 97, 96, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 155});
        stepVol(c, st, 97, "rotate");
        atStep(st, 98);
        cqUnion(c, st, 98, 92, {"toUnion": {"wp": 97}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 98, "union");
        atStep(st, 99);
        cqWorkplaneNew(c, st, 99, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 100);
        cqPolyline(c, st, 100, 99, {"listOfXYTuple": [[10, 11.11111111111111], [12, 11.11111111111111], [12, 11.805555555555555], [10, 11.805555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 101);
        cqClose(c, st, 101, 100, {});
        atStep(st, 102);
        cqExtrude(c, st, 102, 101, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 102, "extrude");
        atStep(st, 103);
        cqRotate(c, st, 103, 102, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 165});
        stepVol(c, st, 103, "rotate");
        atStep(st, 104);
        cqUnion(c, st, 104, 98, {"toUnion": {"wp": 103}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 104, "union");
        atStep(st, 105);
        cqWorkplaneNew(c, st, 105, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 106);
        cqPolyline(c, st, 106, 105, {"listOfXYTuple": [[10, 11.805555555555555], [12, 11.805555555555555], [12, 12.5], [10, 12.5]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 107);
        cqClose(c, st, 107, 106, {});
        atStep(st, 108);
        cqExtrude(c, st, 108, 107, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 108, "extrude");
        atStep(st, 109);
        cqRotate(c, st, 109, 108, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 175});
        stepVol(c, st, 109, "rotate");
        atStep(st, 110);
        cqUnion(c, st, 110, 104, {"toUnion": {"wp": 109}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 110, "union");
        atStep(st, 111);
        cqWorkplaneNew(c, st, 111, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 112);
        cqPolyline(c, st, 112, 111, {"listOfXYTuple": [[10, 12.5], [12, 12.5], [12, 13.194444444444445], [10, 13.194444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 113);
        cqClose(c, st, 113, 112, {});
        atStep(st, 114);
        cqExtrude(c, st, 114, 113, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 114, "extrude");
        atStep(st, 115);
        cqRotate(c, st, 115, 114, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 184.99999999999997});
        stepVol(c, st, 115, "rotate");
        atStep(st, 116);
        cqUnion(c, st, 116, 110, {"toUnion": {"wp": 115}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 116, "union");
        atStep(st, 117);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 118);
        cqPolyline(c, st, 118, 117, {"listOfXYTuple": [[10, 13.194444444444445], [12, 13.194444444444445], [12, 13.88888888888889], [10, 13.88888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 119);
        cqClose(c, st, 119, 118, {});
        atStep(st, 120);
        cqExtrude(c, st, 120, 119, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 120, "extrude");
        atStep(st, 121);
        cqRotate(c, st, 121, 120, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 195});
        stepVol(c, st, 121, "rotate");
        atStep(st, 122);
        cqUnion(c, st, 122, 116, {"toUnion": {"wp": 121}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 122, "union");
        atStep(st, 123);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 124);
        cqPolyline(c, st, 124, 123, {"listOfXYTuple": [[10, 13.88888888888889], [12, 13.88888888888889], [12, 14.583333333333334], [10, 14.583333333333334]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 125);
        cqClose(c, st, 125, 124, {});
        atStep(st, 126);
        cqExtrude(c, st, 126, 125, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 126, "extrude");
        atStep(st, 127);
        cqRotate(c, st, 127, 126, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 205});
        stepVol(c, st, 127, "rotate");
        atStep(st, 128);
        cqUnion(c, st, 128, 122, {"toUnion": {"wp": 127}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 128, "union");
        atStep(st, 129);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 130);
        cqPolyline(c, st, 130, 129, {"listOfXYTuple": [[10, 14.583333333333334], [12, 14.583333333333334], [12, 15.27777777777778], [10, 15.27777777777778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 131);
        cqClose(c, st, 131, 130, {});
        atStep(st, 132);
        cqExtrude(c, st, 132, 131, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 132, "extrude");
        atStep(st, 133);
        cqRotate(c, st, 133, 132, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 215});
        stepVol(c, st, 133, "rotate");
        atStep(st, 134);
        cqUnion(c, st, 134, 128, {"toUnion": {"wp": 133}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 134, "union");
        atStep(st, 135);
        cqWorkplaneNew(c, st, 135, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 136);
        cqPolyline(c, st, 136, 135, {"listOfXYTuple": [[10, 15.27777777777778], [12, 15.27777777777778], [12, 15.97222222222222], [10, 15.97222222222222]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 137);
        cqClose(c, st, 137, 136, {});
        atStep(st, 138);
        cqExtrude(c, st, 138, 137, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 138, "extrude");
        atStep(st, 139);
        cqRotate(c, st, 139, 138, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 225});
        stepVol(c, st, 139, "rotate");
        atStep(st, 140);
        cqUnion(c, st, 140, 134, {"toUnion": {"wp": 139}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 140, "union");
        atStep(st, 141);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 142);
        cqPolyline(c, st, 142, 141, {"listOfXYTuple": [[10, 15.97222222222222], [12, 15.97222222222222], [12, 16.666666666666664], [10, 16.666666666666664]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 143);
        cqClose(c, st, 143, 142, {});
        atStep(st, 144);
        cqExtrude(c, st, 144, 143, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 144, "extrude");
        atStep(st, 145);
        cqRotate(c, st, 145, 144, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 235});
        stepVol(c, st, 145, "rotate");
        atStep(st, 146);
        cqUnion(c, st, 146, 140, {"toUnion": {"wp": 145}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 146, "union");
        atStep(st, 147);
        cqWorkplaneNew(c, st, 147, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 148);
        cqPolyline(c, st, 148, 147, {"listOfXYTuple": [[10, 16.666666666666664], [12, 16.666666666666664], [12, 17.36111111111111], [10, 17.36111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 149);
        cqClose(c, st, 149, 148, {});
        atStep(st, 150);
        cqExtrude(c, st, 150, 149, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 150, "extrude");
        atStep(st, 151);
        cqRotate(c, st, 151, 150, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 245});
        stepVol(c, st, 151, "rotate");
        atStep(st, 152);
        cqUnion(c, st, 152, 146, {"toUnion": {"wp": 151}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 152, "union");
        atStep(st, 153);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 154);
        cqPolyline(c, st, 154, 153, {"listOfXYTuple": [[10, 17.36111111111111], [12, 17.36111111111111], [12, 18.055555555555554], [10, 18.055555555555554]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 155);
        cqClose(c, st, 155, 154, {});
        atStep(st, 156);
        cqExtrude(c, st, 156, 155, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 156, "extrude");
        atStep(st, 157);
        cqRotate(c, st, 157, 156, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 255});
        stepVol(c, st, 157, "rotate");
        atStep(st, 158);
        cqUnion(c, st, 158, 152, {"toUnion": {"wp": 157}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 158, "union");
        atStep(st, 159);
        cqWorkplaneNew(c, st, 159, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 160);
        cqPolyline(c, st, 160, 159, {"listOfXYTuple": [[10, 18.055555555555554], [12, 18.055555555555554], [12, 18.75], [10, 18.75]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 161);
        cqClose(c, st, 161, 160, {});
        atStep(st, 162);
        cqExtrude(c, st, 162, 161, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 162, "extrude");
        atStep(st, 163);
        cqRotate(c, st, 163, 162, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 265});
        stepVol(c, st, 163, "rotate");
        atStep(st, 164);
        cqUnion(c, st, 164, 158, {"toUnion": {"wp": 163}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 164, "union");
        atStep(st, 165);
        cqWorkplaneNew(c, st, 165, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 166);
        cqPolyline(c, st, 166, 165, {"listOfXYTuple": [[10, 18.75], [12, 18.75], [12, 19.444444444444446], [10, 19.444444444444446]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 167);
        cqClose(c, st, 167, 166, {});
        atStep(st, 168);
        cqExtrude(c, st, 168, 167, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 168, "extrude");
        atStep(st, 169);
        cqRotate(c, st, 169, 168, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 275});
        stepVol(c, st, 169, "rotate");
        atStep(st, 170);
        cqUnion(c, st, 170, 164, {"toUnion": {"wp": 169}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 170, "union");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 172);
        cqPolyline(c, st, 172, 171, {"listOfXYTuple": [[10, 19.444444444444446], [12, 19.444444444444446], [12, 20.13888888888889], [10, 20.13888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 173);
        cqClose(c, st, 173, 172, {});
        atStep(st, 174);
        cqExtrude(c, st, 174, 173, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 174, "extrude");
        atStep(st, 175);
        cqRotate(c, st, 175, 174, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 285});
        stepVol(c, st, 175, "rotate");
        atStep(st, 176);
        cqUnion(c, st, 176, 170, {"toUnion": {"wp": 175}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 176, "union");
        atStep(st, 177);
        cqWorkplaneNew(c, st, 177, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 178);
        cqPolyline(c, st, 178, 177, {"listOfXYTuple": [[10, 20.13888888888889], [12, 20.13888888888889], [12, 20.833333333333336], [10, 20.833333333333336]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 179);
        cqClose(c, st, 179, 178, {});
        atStep(st, 180);
        cqExtrude(c, st, 180, 179, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 180, "extrude");
        atStep(st, 181);
        cqRotate(c, st, 181, 180, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 295});
        stepVol(c, st, 181, "rotate");
        atStep(st, 182);
        cqUnion(c, st, 182, 176, {"toUnion": {"wp": 181}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 182, "union");
        atStep(st, 183);
        cqWorkplaneNew(c, st, 183, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 184);
        cqPolyline(c, st, 184, 183, {"listOfXYTuple": [[10, 20.833333333333336], [12, 20.833333333333336], [12, 21.52777777777778], [10, 21.52777777777778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 185);
        cqClose(c, st, 185, 184, {});
        atStep(st, 186);
        cqExtrude(c, st, 186, 185, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 186, "extrude");
        atStep(st, 187);
        cqRotate(c, st, 187, 186, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 305});
        stepVol(c, st, 187, "rotate");
        atStep(st, 188);
        cqUnion(c, st, 188, 182, {"toUnion": {"wp": 187}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 188, "union");
        atStep(st, 189);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 190);
        cqPolyline(c, st, 190, 189, {"listOfXYTuple": [[10, 21.52777777777778], [12, 21.52777777777778], [12, 22.22222222222222], [10, 22.22222222222222]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 191);
        cqClose(c, st, 191, 190, {});
        atStep(st, 192);
        cqExtrude(c, st, 192, 191, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 192, "extrude");
        atStep(st, 193);
        cqRotate(c, st, 193, 192, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 315});
        stepVol(c, st, 193, "rotate");
        atStep(st, 194);
        cqUnion(c, st, 194, 188, {"toUnion": {"wp": 193}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 194, "union");
        atStep(st, 195);
        cqWorkplaneNew(c, st, 195, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 196);
        cqPolyline(c, st, 196, 195, {"listOfXYTuple": [[10, 22.22222222222222], [12, 22.22222222222222], [12, 22.916666666666664], [10, 22.916666666666664]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 197);
        cqClose(c, st, 197, 196, {});
        atStep(st, 198);
        cqExtrude(c, st, 198, 197, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 198, "extrude");
        atStep(st, 199);
        cqRotate(c, st, 199, 198, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 325});
        stepVol(c, st, 199, "rotate");
        atStep(st, 200);
        cqUnion(c, st, 200, 194, {"toUnion": {"wp": 199}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 200, "union");
        atStep(st, 201);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 202);
        cqPolyline(c, st, 202, 201, {"listOfXYTuple": [[10, 22.916666666666664], [12, 22.916666666666664], [12, 23.61111111111111], [10, 23.61111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 203);
        cqClose(c, st, 203, 202, {});
        atStep(st, 204);
        cqExtrude(c, st, 204, 203, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 204, "extrude");
        atStep(st, 205);
        cqRotate(c, st, 205, 204, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 335});
        stepVol(c, st, 205, "rotate");
        atStep(st, 206);
        cqUnion(c, st, 206, 200, {"toUnion": {"wp": 205}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 206, "union");
        atStep(st, 207);
        cqWorkplaneNew(c, st, 207, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 208);
        cqPolyline(c, st, 208, 207, {"listOfXYTuple": [[10, 23.61111111111111], [12, 23.61111111111111], [12, 24.305555555555554], [10, 24.305555555555554]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 209);
        cqClose(c, st, 209, 208, {});
        atStep(st, 210);
        cqExtrude(c, st, 210, 209, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 210, "extrude");
        atStep(st, 211);
        cqRotate(c, st, 211, 210, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 345});
        stepVol(c, st, 211, "rotate");
        atStep(st, 212);
        cqUnion(c, st, 212, 206, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 214);
        cqPolyline(c, st, 214, 213, {"listOfXYTuple": [[10, 24.305555555555554], [12, 24.305555555555554], [12, 25], [10, 25]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 215);
        cqClose(c, st, 215, 214, {});
        atStep(st, 216);
        cqExtrude(c, st, 216, 215, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 216, "extrude");
        atStep(st, 217);
        cqRotate(c, st, 217, 216, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 355});
        stepVol(c, st, 217, "rotate");
        atStep(st, 218);
        cqUnion(c, st, 218, 212, {"toUnion": {"wp": 217}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 218, "union");
        atStep(st, 219);
        cqUnion(c, st, 219, 3, {"toUnion": {"wp": 218}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 219, "union");
        atStep(st, 220);
        cqWorkplaneNew(c, st, 220, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 221);
        cqPolyline(c, st, 221, 220, {"listOfXYTuple": [[10, 0], [12, 0], [12, 0.6944444444444444], [10, 0.6944444444444444]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 222);
        cqClose(c, st, 222, 221, {});
        atStep(st, 223);
        cqExtrude(c, st, 223, 222, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 223, "extrude");
        atStep(st, 224);
        cqRotate(c, st, 224, 223, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 125});
        stepVol(c, st, 224, "rotate");
        atStep(st, 225);
        cqWorkplaneNew(c, st, 225, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 226);
        cqPolyline(c, st, 226, 225, {"listOfXYTuple": [[10, 0.6944444444444444], [12, 0.6944444444444444], [12, 1.3888888888888888], [10, 1.3888888888888888]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 227);
        cqClose(c, st, 227, 226, {});
        atStep(st, 228);
        cqExtrude(c, st, 228, 227, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 228, "extrude");
        atStep(st, 229);
        cqRotate(c, st, 229, 228, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 135});
        stepVol(c, st, 229, "rotate");
        atStep(st, 230);
        cqUnion(c, st, 230, 224, {"toUnion": {"wp": 229}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 230, "union");
        atStep(st, 231);
        cqWorkplaneNew(c, st, 231, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 232);
        cqPolyline(c, st, 232, 231, {"listOfXYTuple": [[10, 1.3888888888888888], [12, 1.3888888888888888], [12, 2.083333333333333], [10, 2.083333333333333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 233);
        cqClose(c, st, 233, 232, {});
        atStep(st, 234);
        cqExtrude(c, st, 234, 233, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 234, "extrude");
        atStep(st, 235);
        cqRotate(c, st, 235, 234, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 145});
        stepVol(c, st, 235, "rotate");
        atStep(st, 236);
        cqUnion(c, st, 236, 230, {"toUnion": {"wp": 235}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 236, "union");
        atStep(st, 237);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 238);
        cqPolyline(c, st, 238, 237, {"listOfXYTuple": [[10, 2.083333333333333], [12, 2.083333333333333], [12, 2.7777777777777777], [10, 2.7777777777777777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 239);
        cqClose(c, st, 239, 238, {});
        atStep(st, 240);
        cqExtrude(c, st, 240, 239, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 240, "extrude");
        atStep(st, 241);
        cqRotate(c, st, 241, 240, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 155});
        stepVol(c, st, 241, "rotate");
        atStep(st, 242);
        cqUnion(c, st, 242, 236, {"toUnion": {"wp": 241}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 242, "union");
        atStep(st, 243);
        cqWorkplaneNew(c, st, 243, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 244);
        cqPolyline(c, st, 244, 243, {"listOfXYTuple": [[10, 2.7777777777777777], [12, 2.7777777777777777], [12, 3.4722222222222223], [10, 3.4722222222222223]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 245);
        cqClose(c, st, 245, 244, {});
        atStep(st, 246);
        cqExtrude(c, st, 246, 245, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 246, "extrude");
        atStep(st, 247);
        cqRotate(c, st, 247, 246, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 165});
        stepVol(c, st, 247, "rotate");
        atStep(st, 248);
        cqUnion(c, st, 248, 242, {"toUnion": {"wp": 247}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 248, "union");
        atStep(st, 249);
        cqWorkplaneNew(c, st, 249, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 250);
        cqPolyline(c, st, 250, 249, {"listOfXYTuple": [[10, 3.4722222222222223], [12, 3.4722222222222223], [12, 4.166666666666666], [10, 4.166666666666666]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 251);
        cqClose(c, st, 251, 250, {});
        atStep(st, 252);
        cqExtrude(c, st, 252, 251, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 252, "extrude");
        atStep(st, 253);
        cqRotate(c, st, 253, 252, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 175});
        stepVol(c, st, 253, "rotate");
        atStep(st, 254);
        cqUnion(c, st, 254, 248, {"toUnion": {"wp": 253}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 254, "union");
        atStep(st, 255);
        cqWorkplaneNew(c, st, 255, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 256);
        cqPolyline(c, st, 256, 255, {"listOfXYTuple": [[10, 4.166666666666666], [12, 4.166666666666666], [12, 4.861111111111112], [10, 4.861111111111112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 257);
        cqClose(c, st, 257, 256, {});
        atStep(st, 258);
        cqExtrude(c, st, 258, 257, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 258, "extrude");
        atStep(st, 259);
        cqRotate(c, st, 259, 258, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 185});
        stepVol(c, st, 259, "rotate");
        atStep(st, 260);
        cqUnion(c, st, 260, 254, {"toUnion": {"wp": 259}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 260, "union");
        atStep(st, 261);
        cqWorkplaneNew(c, st, 261, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 262);
        cqPolyline(c, st, 262, 261, {"listOfXYTuple": [[10, 4.861111111111112], [12, 4.861111111111112], [12, 5.555555555555555], [10, 5.555555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 263);
        cqClose(c, st, 263, 262, {});
        atStep(st, 264);
        cqExtrude(c, st, 264, 263, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 264, "extrude");
        atStep(st, 265);
        cqRotate(c, st, 265, 264, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 195});
        stepVol(c, st, 265, "rotate");
        atStep(st, 266);
        cqUnion(c, st, 266, 260, {"toUnion": {"wp": 265}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 266, "union");
        atStep(st, 267);
        cqWorkplaneNew(c, st, 267, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 268);
        cqPolyline(c, st, 268, 267, {"listOfXYTuple": [[10, 5.555555555555555], [12, 5.555555555555555], [12, 6.25], [10, 6.25]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 269);
        cqClose(c, st, 269, 268, {});
        atStep(st, 270);
        cqExtrude(c, st, 270, 269, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 270, "extrude");
        atStep(st, 271);
        cqRotate(c, st, 271, 270, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 205});
        stepVol(c, st, 271, "rotate");
        atStep(st, 272);
        cqUnion(c, st, 272, 266, {"toUnion": {"wp": 271}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 272, "union");
        atStep(st, 273);
        cqWorkplaneNew(c, st, 273, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 274);
        cqPolyline(c, st, 274, 273, {"listOfXYTuple": [[10, 6.25], [12, 6.25], [12, 6.944444444444445], [10, 6.944444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 275);
        cqClose(c, st, 275, 274, {});
        atStep(st, 276);
        cqExtrude(c, st, 276, 275, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 276, "extrude");
        atStep(st, 277);
        cqRotate(c, st, 277, 276, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 215});
        stepVol(c, st, 277, "rotate");
        atStep(st, 278);
        cqUnion(c, st, 278, 272, {"toUnion": {"wp": 277}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 278, "union");
        atStep(st, 279);
        cqWorkplaneNew(c, st, 279, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 280);
        cqPolyline(c, st, 280, 279, {"listOfXYTuple": [[10, 6.944444444444445], [12, 6.944444444444445], [12, 7.638888888888889], [10, 7.638888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 281);
        cqClose(c, st, 281, 280, {});
        atStep(st, 282);
        cqExtrude(c, st, 282, 281, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 282, "extrude");
        atStep(st, 283);
        cqRotate(c, st, 283, 282, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 225});
        stepVol(c, st, 283, "rotate");
        atStep(st, 284);
        cqUnion(c, st, 284, 278, {"toUnion": {"wp": 283}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 284, "union");
        atStep(st, 285);
        cqWorkplaneNew(c, st, 285, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 286);
        cqPolyline(c, st, 286, 285, {"listOfXYTuple": [[10, 7.638888888888889], [12, 7.638888888888889], [12, 8.333333333333332], [10, 8.333333333333332]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 287);
        cqClose(c, st, 287, 286, {});
        atStep(st, 288);
        cqExtrude(c, st, 288, 287, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 288, "extrude");
        atStep(st, 289);
        cqRotate(c, st, 289, 288, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 235});
        stepVol(c, st, 289, "rotate");
        atStep(st, 290);
        cqUnion(c, st, 290, 284, {"toUnion": {"wp": 289}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 290, "union");
        atStep(st, 291);
        cqWorkplaneNew(c, st, 291, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 292);
        cqPolyline(c, st, 292, 291, {"listOfXYTuple": [[10, 8.333333333333332], [12, 8.333333333333332], [12, 9.027777777777777], [10, 9.027777777777777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 293);
        cqClose(c, st, 293, 292, {});
        atStep(st, 294);
        cqExtrude(c, st, 294, 293, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 294, "extrude");
        atStep(st, 295);
        cqRotate(c, st, 295, 294, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 245});
        stepVol(c, st, 295, "rotate");
        atStep(st, 296);
        cqUnion(c, st, 296, 290, {"toUnion": {"wp": 295}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 296, "union");
        atStep(st, 297);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 298);
        cqPolyline(c, st, 298, 297, {"listOfXYTuple": [[10, 9.027777777777777], [12, 9.027777777777777], [12, 9.722222222222223], [10, 9.722222222222223]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 299);
        cqClose(c, st, 299, 298, {});
        atStep(st, 300);
        cqExtrude(c, st, 300, 299, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 300, "extrude");
        atStep(st, 301);
        cqRotate(c, st, 301, 300, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 255});
        stepVol(c, st, 301, "rotate");
        atStep(st, 302);
        cqUnion(c, st, 302, 296, {"toUnion": {"wp": 301}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 302, "union");
        atStep(st, 303);
        cqWorkplaneNew(c, st, 303, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 304);
        cqPolyline(c, st, 304, 303, {"listOfXYTuple": [[10, 9.722222222222223], [12, 9.722222222222223], [12, 10.416666666666668], [10, 10.416666666666668]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 305);
        cqClose(c, st, 305, 304, {});
        atStep(st, 306);
        cqExtrude(c, st, 306, 305, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 306, "extrude");
        atStep(st, 307);
        cqRotate(c, st, 307, 306, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 265});
        stepVol(c, st, 307, "rotate");
        atStep(st, 308);
        cqUnion(c, st, 308, 302, {"toUnion": {"wp": 307}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 308, "union");
        atStep(st, 309);
        cqWorkplaneNew(c, st, 309, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 310);
        cqPolyline(c, st, 310, 309, {"listOfXYTuple": [[10, 10.416666666666668], [12, 10.416666666666668], [12, 11.11111111111111], [10, 11.11111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 311);
        cqClose(c, st, 311, 310, {});
        atStep(st, 312);
        cqExtrude(c, st, 312, 311, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 312, "extrude");
        atStep(st, 313);
        cqRotate(c, st, 313, 312, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 275});
        stepVol(c, st, 313, "rotate");
        atStep(st, 314);
        cqUnion(c, st, 314, 308, {"toUnion": {"wp": 313}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 314, "union");
        atStep(st, 315);
        cqWorkplaneNew(c, st, 315, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 316);
        cqPolyline(c, st, 316, 315, {"listOfXYTuple": [[10, 11.11111111111111], [12, 11.11111111111111], [12, 11.805555555555555], [10, 11.805555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 317);
        cqClose(c, st, 317, 316, {});
        atStep(st, 318);
        cqExtrude(c, st, 318, 317, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 318, "extrude");
        atStep(st, 319);
        cqRotate(c, st, 319, 318, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 285});
        stepVol(c, st, 319, "rotate");
        atStep(st, 320);
        cqUnion(c, st, 320, 314, {"toUnion": {"wp": 319}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 320, "union");
        atStep(st, 321);
        cqWorkplaneNew(c, st, 321, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 322);
        cqPolyline(c, st, 322, 321, {"listOfXYTuple": [[10, 11.805555555555555], [12, 11.805555555555555], [12, 12.5], [10, 12.5]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 323);
        cqClose(c, st, 323, 322, {});
        atStep(st, 324);
        cqExtrude(c, st, 324, 323, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 324, "extrude");
        atStep(st, 325);
        cqRotate(c, st, 325, 324, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 295});
        stepVol(c, st, 325, "rotate");
        atStep(st, 326);
        cqUnion(c, st, 326, 320, {"toUnion": {"wp": 325}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 326, "union");
        atStep(st, 327);
        cqWorkplaneNew(c, st, 327, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 328);
        cqPolyline(c, st, 328, 327, {"listOfXYTuple": [[10, 12.5], [12, 12.5], [12, 13.194444444444445], [10, 13.194444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 329);
        cqClose(c, st, 329, 328, {});
        atStep(st, 330);
        cqExtrude(c, st, 330, 329, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 330, "extrude");
        atStep(st, 331);
        cqRotate(c, st, 331, 330, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 305});
        stepVol(c, st, 331, "rotate");
        atStep(st, 332);
        cqUnion(c, st, 332, 326, {"toUnion": {"wp": 331}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 332, "union");
        atStep(st, 333);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 334);
        cqPolyline(c, st, 334, 333, {"listOfXYTuple": [[10, 13.194444444444445], [12, 13.194444444444445], [12, 13.88888888888889], [10, 13.88888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 335);
        cqClose(c, st, 335, 334, {});
        atStep(st, 336);
        cqExtrude(c, st, 336, 335, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 336, "extrude");
        atStep(st, 337);
        cqRotate(c, st, 337, 336, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 315});
        stepVol(c, st, 337, "rotate");
        atStep(st, 338);
        cqUnion(c, st, 338, 332, {"toUnion": {"wp": 337}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 338, "union");
        atStep(st, 339);
        cqWorkplaneNew(c, st, 339, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 340);
        cqPolyline(c, st, 340, 339, {"listOfXYTuple": [[10, 13.88888888888889], [12, 13.88888888888889], [12, 14.583333333333334], [10, 14.583333333333334]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 341);
        cqClose(c, st, 341, 340, {});
        atStep(st, 342);
        cqExtrude(c, st, 342, 341, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 342, "extrude");
        atStep(st, 343);
        cqRotate(c, st, 343, 342, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 325});
        stepVol(c, st, 343, "rotate");
        atStep(st, 344);
        cqUnion(c, st, 344, 338, {"toUnion": {"wp": 343}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 344, "union");
        atStep(st, 345);
        cqWorkplaneNew(c, st, 345, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 346);
        cqPolyline(c, st, 346, 345, {"listOfXYTuple": [[10, 14.583333333333334], [12, 14.583333333333334], [12, 15.27777777777778], [10, 15.27777777777778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 347);
        cqClose(c, st, 347, 346, {});
        atStep(st, 348);
        cqExtrude(c, st, 348, 347, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 348, "extrude");
        atStep(st, 349);
        cqRotate(c, st, 349, 348, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 335});
        stepVol(c, st, 349, "rotate");
        atStep(st, 350);
        cqUnion(c, st, 350, 344, {"toUnion": {"wp": 349}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 350, "union");
        atStep(st, 351);
        cqWorkplaneNew(c, st, 351, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 352);
        cqPolyline(c, st, 352, 351, {"listOfXYTuple": [[10, 15.27777777777778], [12, 15.27777777777778], [12, 15.97222222222222], [10, 15.97222222222222]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 353);
        cqClose(c, st, 353, 352, {});
        atStep(st, 354);
        cqExtrude(c, st, 354, 353, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 354, "extrude");
        atStep(st, 355);
        cqRotate(c, st, 355, 354, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 345});
        stepVol(c, st, 355, "rotate");
        atStep(st, 356);
        cqUnion(c, st, 356, 350, {"toUnion": {"wp": 355}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 356, "union");
        atStep(st, 357);
        cqWorkplaneNew(c, st, 357, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 358);
        cqPolyline(c, st, 358, 357, {"listOfXYTuple": [[10, 15.97222222222222], [12, 15.97222222222222], [12, 16.666666666666664], [10, 16.666666666666664]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 359);
        cqClose(c, st, 359, 358, {});
        atStep(st, 360);
        cqExtrude(c, st, 360, 359, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 360, "extrude");
        atStep(st, 361);
        cqRotate(c, st, 361, 360, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 355});
        stepVol(c, st, 361, "rotate");
        atStep(st, 362);
        cqUnion(c, st, 362, 356, {"toUnion": {"wp": 361}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 362, "union");
        atStep(st, 363);
        cqWorkplaneNew(c, st, 363, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 364);
        cqPolyline(c, st, 364, 363, {"listOfXYTuple": [[10, 16.666666666666664], [12, 16.666666666666664], [12, 17.36111111111111], [10, 17.36111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 365);
        cqClose(c, st, 365, 364, {});
        atStep(st, 366);
        cqExtrude(c, st, 366, 365, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 366, "extrude");
        atStep(st, 367);
        cqRotate(c, st, 367, 366, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 365});
        stepVol(c, st, 367, "rotate");
        atStep(st, 368);
        cqUnion(c, st, 368, 362, {"toUnion": {"wp": 367}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 368, "union");
        atStep(st, 369);
        cqWorkplaneNew(c, st, 369, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 370);
        cqPolyline(c, st, 370, 369, {"listOfXYTuple": [[10, 17.36111111111111], [12, 17.36111111111111], [12, 18.055555555555554], [10, 18.055555555555554]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 371);
        cqClose(c, st, 371, 370, {});
        atStep(st, 372);
        cqExtrude(c, st, 372, 371, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 372, "extrude");
        atStep(st, 373);
        cqRotate(c, st, 373, 372, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 375});
        stepVol(c, st, 373, "rotate");
        atStep(st, 374);
        cqUnion(c, st, 374, 368, {"toUnion": {"wp": 373}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 374, "union");
        atStep(st, 375);
        cqWorkplaneNew(c, st, 375, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 376);
        cqPolyline(c, st, 376, 375, {"listOfXYTuple": [[10, 18.055555555555554], [12, 18.055555555555554], [12, 18.75], [10, 18.75]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 377);
        cqClose(c, st, 377, 376, {});
        atStep(st, 378);
        cqExtrude(c, st, 378, 377, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 378, "extrude");
        atStep(st, 379);
        cqRotate(c, st, 379, 378, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 385});
        stepVol(c, st, 379, "rotate");
        atStep(st, 380);
        cqUnion(c, st, 380, 374, {"toUnion": {"wp": 379}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 380, "union");
        atStep(st, 381);
        cqWorkplaneNew(c, st, 381, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 382);
        cqPolyline(c, st, 382, 381, {"listOfXYTuple": [[10, 18.75], [12, 18.75], [12, 19.444444444444446], [10, 19.444444444444446]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 383);
        cqClose(c, st, 383, 382, {});
        atStep(st, 384);
        cqExtrude(c, st, 384, 383, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 384, "extrude");
        atStep(st, 385);
        cqRotate(c, st, 385, 384, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 395});
        stepVol(c, st, 385, "rotate");
        atStep(st, 386);
        cqUnion(c, st, 386, 380, {"toUnion": {"wp": 385}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 386, "union");
        atStep(st, 387);
        cqWorkplaneNew(c, st, 387, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 388);
        cqPolyline(c, st, 388, 387, {"listOfXYTuple": [[10, 19.444444444444446], [12, 19.444444444444446], [12, 20.13888888888889], [10, 20.13888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 389);
        cqClose(c, st, 389, 388, {});
        atStep(st, 390);
        cqExtrude(c, st, 390, 389, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 390, "extrude");
        atStep(st, 391);
        cqRotate(c, st, 391, 390, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 405});
        stepVol(c, st, 391, "rotate");
        atStep(st, 392);
        cqUnion(c, st, 392, 386, {"toUnion": {"wp": 391}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 392, "union");
        atStep(st, 393);
        cqWorkplaneNew(c, st, 393, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 394);
        cqPolyline(c, st, 394, 393, {"listOfXYTuple": [[10, 20.13888888888889], [12, 20.13888888888889], [12, 20.833333333333336], [10, 20.833333333333336]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 395);
        cqClose(c, st, 395, 394, {});
        atStep(st, 396);
        cqExtrude(c, st, 396, 395, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 396, "extrude");
        atStep(st, 397);
        cqRotate(c, st, 397, 396, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 415});
        stepVol(c, st, 397, "rotate");
        atStep(st, 398);
        cqUnion(c, st, 398, 392, {"toUnion": {"wp": 397}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 398, "union");
        atStep(st, 399);
        cqWorkplaneNew(c, st, 399, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 400);
        cqPolyline(c, st, 400, 399, {"listOfXYTuple": [[10, 20.833333333333336], [12, 20.833333333333336], [12, 21.52777777777778], [10, 21.52777777777778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 401);
        cqClose(c, st, 401, 400, {});
        atStep(st, 402);
        cqExtrude(c, st, 402, 401, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 402, "extrude");
        atStep(st, 403);
        cqRotate(c, st, 403, 402, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 425});
        stepVol(c, st, 403, "rotate");
        atStep(st, 404);
        cqUnion(c, st, 404, 398, {"toUnion": {"wp": 403}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 404, "union");
        atStep(st, 405);
        cqWorkplaneNew(c, st, 405, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 406);
        cqPolyline(c, st, 406, 405, {"listOfXYTuple": [[10, 21.52777777777778], [12, 21.52777777777778], [12, 22.22222222222222], [10, 22.22222222222222]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 407);
        cqClose(c, st, 407, 406, {});
        atStep(st, 408);
        cqExtrude(c, st, 408, 407, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 408, "extrude");
        atStep(st, 409);
        cqRotate(c, st, 409, 408, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 435});
        stepVol(c, st, 409, "rotate");
        atStep(st, 410);
        cqUnion(c, st, 410, 404, {"toUnion": {"wp": 409}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 410, "union");
        atStep(st, 411);
        cqWorkplaneNew(c, st, 411, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 412);
        cqPolyline(c, st, 412, 411, {"listOfXYTuple": [[10, 22.22222222222222], [12, 22.22222222222222], [12, 22.916666666666664], [10, 22.916666666666664]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 413);
        cqClose(c, st, 413, 412, {});
        atStep(st, 414);
        cqExtrude(c, st, 414, 413, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 414, "extrude");
        atStep(st, 415);
        cqRotate(c, st, 415, 414, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 445});
        stepVol(c, st, 415, "rotate");
        atStep(st, 416);
        cqUnion(c, st, 416, 410, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        atStep(st, 417);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 418);
        cqPolyline(c, st, 418, 417, {"listOfXYTuple": [[10, 22.916666666666664], [12, 22.916666666666664], [12, 23.61111111111111], [10, 23.61111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 419);
        cqClose(c, st, 419, 418, {});
        atStep(st, 420);
        cqExtrude(c, st, 420, 419, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 420, "extrude");
        atStep(st, 421);
        cqRotate(c, st, 421, 420, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 455});
        stepVol(c, st, 421, "rotate");
        atStep(st, 422);
        cqUnion(c, st, 422, 416, {"toUnion": {"wp": 421}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 422, "union");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 424);
        cqPolyline(c, st, 424, 423, {"listOfXYTuple": [[10, 23.61111111111111], [12, 23.61111111111111], [12, 24.305555555555554], [10, 24.305555555555554]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 425);
        cqClose(c, st, 425, 424, {});
        atStep(st, 426);
        cqExtrude(c, st, 426, 425, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 427);
        cqRotate(c, st, 427, 426, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 465});
        stepVol(c, st, 427, "rotate");
        atStep(st, 428);
        cqUnion(c, st, 428, 422, {"toUnion": {"wp": 427}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 428, "union");
        atStep(st, 429);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 430);
        cqPolyline(c, st, 430, 429, {"listOfXYTuple": [[10, 24.305555555555554], [12, 24.305555555555554], [12, 25], [10, 25]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 431);
        cqClose(c, st, 431, 430, {});
        atStep(st, 432);
        cqExtrude(c, st, 432, 431, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 432, "extrude");
        atStep(st, 433);
        cqRotate(c, st, 433, 432, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 475});
        stepVol(c, st, 433, "rotate");
        atStep(st, 434);
        cqUnion(c, st, 434, 428, {"toUnion": {"wp": 433}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 434, "union");
        atStep(st, 435);
        cqUnion(c, st, 435, 219, {"toUnion": {"wp": 434}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 435, "union");
        atStep(st, 436);
        cqWorkplaneNew(c, st, 436, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 437);
        cqPolyline(c, st, 437, 436, {"listOfXYTuple": [[10, 0], [12, 0], [12, 0.6944444444444444], [10, 0.6944444444444444]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 438);
        cqClose(c, st, 438, 437, {});
        atStep(st, 439);
        cqExtrude(c, st, 439, 438, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 439, "extrude");
        atStep(st, 440);
        cqRotate(c, st, 440, 439, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 245});
        stepVol(c, st, 440, "rotate");
        atStep(st, 441);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 442);
        cqPolyline(c, st, 442, 441, {"listOfXYTuple": [[10, 0.6944444444444444], [12, 0.6944444444444444], [12, 1.3888888888888888], [10, 1.3888888888888888]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 443);
        cqClose(c, st, 443, 442, {});
        atStep(st, 444);
        cqExtrude(c, st, 444, 443, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 444, "extrude");
        atStep(st, 445);
        cqRotate(c, st, 445, 444, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 255});
        stepVol(c, st, 445, "rotate");
        atStep(st, 446);
        cqUnion(c, st, 446, 440, {"toUnion": {"wp": 445}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 446, "union");
        atStep(st, 447);
        cqWorkplaneNew(c, st, 447, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 448);
        cqPolyline(c, st, 448, 447, {"listOfXYTuple": [[10, 1.3888888888888888], [12, 1.3888888888888888], [12, 2.083333333333333], [10, 2.083333333333333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 449);
        cqClose(c, st, 449, 448, {});
        atStep(st, 450);
        cqExtrude(c, st, 450, 449, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 450, "extrude");
        atStep(st, 451);
        cqRotate(c, st, 451, 450, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 265});
        stepVol(c, st, 451, "rotate");
        atStep(st, 452);
        cqUnion(c, st, 452, 446, {"toUnion": {"wp": 451}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 452, "union");
        atStep(st, 453);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 454);
        cqPolyline(c, st, 454, 453, {"listOfXYTuple": [[10, 2.083333333333333], [12, 2.083333333333333], [12, 2.7777777777777777], [10, 2.7777777777777777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 455);
        cqClose(c, st, 455, 454, {});
        atStep(st, 456);
        cqExtrude(c, st, 456, 455, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 456, "extrude");
        atStep(st, 457);
        cqRotate(c, st, 457, 456, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 275});
        stepVol(c, st, 457, "rotate");
        atStep(st, 458);
        cqUnion(c, st, 458, 452, {"toUnion": {"wp": 457}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 458, "union");
        atStep(st, 459);
        cqWorkplaneNew(c, st, 459, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 460);
        cqPolyline(c, st, 460, 459, {"listOfXYTuple": [[10, 2.7777777777777777], [12, 2.7777777777777777], [12, 3.4722222222222223], [10, 3.4722222222222223]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 461);
        cqClose(c, st, 461, 460, {});
        atStep(st, 462);
        cqExtrude(c, st, 462, 461, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 462, "extrude");
        atStep(st, 463);
        cqRotate(c, st, 463, 462, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 285});
        stepVol(c, st, 463, "rotate");
        atStep(st, 464);
        cqUnion(c, st, 464, 458, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 464, "union");
        atStep(st, 465);
        cqWorkplaneNew(c, st, 465, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 466);
        cqPolyline(c, st, 466, 465, {"listOfXYTuple": [[10, 3.4722222222222223], [12, 3.4722222222222223], [12, 4.166666666666666], [10, 4.166666666666666]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 467);
        cqClose(c, st, 467, 466, {});
        atStep(st, 468);
        cqExtrude(c, st, 468, 467, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 468, "extrude");
        atStep(st, 469);
        cqRotate(c, st, 469, 468, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 295});
        stepVol(c, st, 469, "rotate");
        atStep(st, 470);
        cqUnion(c, st, 470, 464, {"toUnion": {"wp": 469}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 470, "union");
        atStep(st, 471);
        cqWorkplaneNew(c, st, 471, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 472);
        cqPolyline(c, st, 472, 471, {"listOfXYTuple": [[10, 4.166666666666666], [12, 4.166666666666666], [12, 4.861111111111112], [10, 4.861111111111112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 473);
        cqClose(c, st, 473, 472, {});
        atStep(st, 474);
        cqExtrude(c, st, 474, 473, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 474, "extrude");
        atStep(st, 475);
        cqRotate(c, st, 475, 474, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 305});
        stepVol(c, st, 475, "rotate");
        atStep(st, 476);
        cqUnion(c, st, 476, 470, {"toUnion": {"wp": 475}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 476, "union");
        atStep(st, 477);
        cqWorkplaneNew(c, st, 477, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 478);
        cqPolyline(c, st, 478, 477, {"listOfXYTuple": [[10, 4.861111111111112], [12, 4.861111111111112], [12, 5.555555555555555], [10, 5.555555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 479);
        cqClose(c, st, 479, 478, {});
        atStep(st, 480);
        cqExtrude(c, st, 480, 479, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 480, "extrude");
        atStep(st, 481);
        cqRotate(c, st, 481, 480, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 315});
        stepVol(c, st, 481, "rotate");
        atStep(st, 482);
        cqUnion(c, st, 482, 476, {"toUnion": {"wp": 481}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 482, "union");
        atStep(st, 483);
        cqWorkplaneNew(c, st, 483, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 484);
        cqPolyline(c, st, 484, 483, {"listOfXYTuple": [[10, 5.555555555555555], [12, 5.555555555555555], [12, 6.25], [10, 6.25]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 485);
        cqClose(c, st, 485, 484, {});
        atStep(st, 486);
        cqExtrude(c, st, 486, 485, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 486, "extrude");
        atStep(st, 487);
        cqRotate(c, st, 487, 486, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 325});
        stepVol(c, st, 487, "rotate");
        atStep(st, 488);
        cqUnion(c, st, 488, 482, {"toUnion": {"wp": 487}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 488, "union");
        atStep(st, 489);
        cqWorkplaneNew(c, st, 489, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 490);
        cqPolyline(c, st, 490, 489, {"listOfXYTuple": [[10, 6.25], [12, 6.25], [12, 6.944444444444445], [10, 6.944444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 491);
        cqClose(c, st, 491, 490, {});
        atStep(st, 492);
        cqExtrude(c, st, 492, 491, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 492, "extrude");
        atStep(st, 493);
        cqRotate(c, st, 493, 492, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 335});
        stepVol(c, st, 493, "rotate");
        atStep(st, 494);
        cqUnion(c, st, 494, 488, {"toUnion": {"wp": 493}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 494, "union");
        atStep(st, 495);
        cqWorkplaneNew(c, st, 495, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 496);
        cqPolyline(c, st, 496, 495, {"listOfXYTuple": [[10, 6.944444444444445], [12, 6.944444444444445], [12, 7.638888888888889], [10, 7.638888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 497);
        cqClose(c, st, 497, 496, {});
        atStep(st, 498);
        cqExtrude(c, st, 498, 497, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 498, "extrude");
        atStep(st, 499);
        cqRotate(c, st, 499, 498, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 345});
        stepVol(c, st, 499, "rotate");
        atStep(st, 500);
        cqUnion(c, st, 500, 494, {"toUnion": {"wp": 499}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 500, "union");
        atStep(st, 501);
        cqWorkplaneNew(c, st, 501, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 502);
        cqPolyline(c, st, 502, 501, {"listOfXYTuple": [[10, 7.638888888888889], [12, 7.638888888888889], [12, 8.333333333333332], [10, 8.333333333333332]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 503);
        cqClose(c, st, 503, 502, {});
        atStep(st, 504);
        cqExtrude(c, st, 504, 503, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 504, "extrude");
        atStep(st, 505);
        cqRotate(c, st, 505, 504, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 355});
        stepVol(c, st, 505, "rotate");
        atStep(st, 506);
        cqUnion(c, st, 506, 500, {"toUnion": {"wp": 505}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 506, "union");
        atStep(st, 507);
        cqWorkplaneNew(c, st, 507, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 508);
        cqPolyline(c, st, 508, 507, {"listOfXYTuple": [[10, 8.333333333333332], [12, 8.333333333333332], [12, 9.027777777777777], [10, 9.027777777777777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 509);
        cqClose(c, st, 509, 508, {});
        atStep(st, 510);
        cqExtrude(c, st, 510, 509, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 510, "extrude");
        atStep(st, 511);
        cqRotate(c, st, 511, 510, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 365});
        stepVol(c, st, 511, "rotate");
        atStep(st, 512);
        cqUnion(c, st, 512, 506, {"toUnion": {"wp": 511}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 512, "union");
        atStep(st, 513);
        cqWorkplaneNew(c, st, 513, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 514);
        cqPolyline(c, st, 514, 513, {"listOfXYTuple": [[10, 9.027777777777777], [12, 9.027777777777777], [12, 9.722222222222223], [10, 9.722222222222223]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 515);
        cqClose(c, st, 515, 514, {});
        atStep(st, 516);
        cqExtrude(c, st, 516, 515, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 516, "extrude");
        atStep(st, 517);
        cqRotate(c, st, 517, 516, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 375});
        stepVol(c, st, 517, "rotate");
        atStep(st, 518);
        cqUnion(c, st, 518, 512, {"toUnion": {"wp": 517}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 518, "union");
        atStep(st, 519);
        cqWorkplaneNew(c, st, 519, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 520);
        cqPolyline(c, st, 520, 519, {"listOfXYTuple": [[10, 9.722222222222223], [12, 9.722222222222223], [12, 10.416666666666668], [10, 10.416666666666668]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 521);
        cqClose(c, st, 521, 520, {});
        atStep(st, 522);
        cqExtrude(c, st, 522, 521, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 522, "extrude");
        atStep(st, 523);
        cqRotate(c, st, 523, 522, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 385});
        stepVol(c, st, 523, "rotate");
        atStep(st, 524);
        cqUnion(c, st, 524, 518, {"toUnion": {"wp": 523}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 524, "union");
        atStep(st, 525);
        cqWorkplaneNew(c, st, 525, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 526);
        cqPolyline(c, st, 526, 525, {"listOfXYTuple": [[10, 10.416666666666668], [12, 10.416666666666668], [12, 11.11111111111111], [10, 11.11111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 527);
        cqClose(c, st, 527, 526, {});
        atStep(st, 528);
        cqExtrude(c, st, 528, 527, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 528, "extrude");
        atStep(st, 529);
        cqRotate(c, st, 529, 528, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 395});
        stepVol(c, st, 529, "rotate");
        atStep(st, 530);
        cqUnion(c, st, 530, 524, {"toUnion": {"wp": 529}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 530, "union");
        atStep(st, 531);
        cqWorkplaneNew(c, st, 531, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 532);
        cqPolyline(c, st, 532, 531, {"listOfXYTuple": [[10, 11.11111111111111], [12, 11.11111111111111], [12, 11.805555555555555], [10, 11.805555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 533);
        cqClose(c, st, 533, 532, {});
        atStep(st, 534);
        cqExtrude(c, st, 534, 533, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 534, "extrude");
        atStep(st, 535);
        cqRotate(c, st, 535, 534, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 405});
        stepVol(c, st, 535, "rotate");
        atStep(st, 536);
        cqUnion(c, st, 536, 530, {"toUnion": {"wp": 535}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 536, "union");
        atStep(st, 537);
        cqWorkplaneNew(c, st, 537, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 538);
        cqPolyline(c, st, 538, 537, {"listOfXYTuple": [[10, 11.805555555555555], [12, 11.805555555555555], [12, 12.5], [10, 12.5]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 539);
        cqClose(c, st, 539, 538, {});
        atStep(st, 540);
        cqExtrude(c, st, 540, 539, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 540, "extrude");
        atStep(st, 541);
        cqRotate(c, st, 541, 540, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 415});
        stepVol(c, st, 541, "rotate");
        atStep(st, 542);
        cqUnion(c, st, 542, 536, {"toUnion": {"wp": 541}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 542, "union");
        atStep(st, 543);
        cqWorkplaneNew(c, st, 543, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 544);
        cqPolyline(c, st, 544, 543, {"listOfXYTuple": [[10, 12.5], [12, 12.5], [12, 13.194444444444445], [10, 13.194444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 545);
        cqClose(c, st, 545, 544, {});
        atStep(st, 546);
        cqExtrude(c, st, 546, 545, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 546, "extrude");
        atStep(st, 547);
        cqRotate(c, st, 547, 546, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 425});
        stepVol(c, st, 547, "rotate");
        atStep(st, 548);
        cqUnion(c, st, 548, 542, {"toUnion": {"wp": 547}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 548, "union");
        atStep(st, 549);
        cqWorkplaneNew(c, st, 549, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 550);
        cqPolyline(c, st, 550, 549, {"listOfXYTuple": [[10, 13.194444444444445], [12, 13.194444444444445], [12, 13.88888888888889], [10, 13.88888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 551);
        cqClose(c, st, 551, 550, {});
        atStep(st, 552);
        cqExtrude(c, st, 552, 551, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 552, "extrude");
        atStep(st, 553);
        cqRotate(c, st, 553, 552, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 435});
        stepVol(c, st, 553, "rotate");
        atStep(st, 554);
        cqUnion(c, st, 554, 548, {"toUnion": {"wp": 553}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 554, "union");
        atStep(st, 555);
        cqWorkplaneNew(c, st, 555, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 556);
        cqPolyline(c, st, 556, 555, {"listOfXYTuple": [[10, 13.88888888888889], [12, 13.88888888888889], [12, 14.583333333333334], [10, 14.583333333333334]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 557);
        cqClose(c, st, 557, 556, {});
        atStep(st, 558);
        cqExtrude(c, st, 558, 557, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 558, "extrude");
        atStep(st, 559);
        cqRotate(c, st, 559, 558, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 445});
        stepVol(c, st, 559, "rotate");
        atStep(st, 560);
        cqUnion(c, st, 560, 554, {"toUnion": {"wp": 559}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 560, "union");
        atStep(st, 561);
        cqWorkplaneNew(c, st, 561, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 562);
        cqPolyline(c, st, 562, 561, {"listOfXYTuple": [[10, 14.583333333333334], [12, 14.583333333333334], [12, 15.27777777777778], [10, 15.27777777777778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 563);
        cqClose(c, st, 563, 562, {});
        atStep(st, 564);
        cqExtrude(c, st, 564, 563, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 564, "extrude");
        atStep(st, 565);
        cqRotate(c, st, 565, 564, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 455});
        stepVol(c, st, 565, "rotate");
        atStep(st, 566);
        cqUnion(c, st, 566, 560, {"toUnion": {"wp": 565}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 566, "union");
        atStep(st, 567);
        cqWorkplaneNew(c, st, 567, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 568);
        cqPolyline(c, st, 568, 567, {"listOfXYTuple": [[10, 15.27777777777778], [12, 15.27777777777778], [12, 15.97222222222222], [10, 15.97222222222222]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 569);
        cqClose(c, st, 569, 568, {});
        atStep(st, 570);
        cqExtrude(c, st, 570, 569, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 570, "extrude");
        atStep(st, 571);
        cqRotate(c, st, 571, 570, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 465});
        stepVol(c, st, 571, "rotate");
        atStep(st, 572);
        cqUnion(c, st, 572, 566, {"toUnion": {"wp": 571}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 572, "union");
        atStep(st, 573);
        cqWorkplaneNew(c, st, 573, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 574);
        cqPolyline(c, st, 574, 573, {"listOfXYTuple": [[10, 15.97222222222222], [12, 15.97222222222222], [12, 16.666666666666664], [10, 16.666666666666664]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 575);
        cqClose(c, st, 575, 574, {});
        atStep(st, 576);
        cqExtrude(c, st, 576, 575, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 576, "extrude");
        atStep(st, 577);
        cqRotate(c, st, 577, 576, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 475});
        stepVol(c, st, 577, "rotate");
        atStep(st, 578);
        cqUnion(c, st, 578, 572, {"toUnion": {"wp": 577}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 578, "union");
        atStep(st, 579);
        cqWorkplaneNew(c, st, 579, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 580);
        cqPolyline(c, st, 580, 579, {"listOfXYTuple": [[10, 16.666666666666664], [12, 16.666666666666664], [12, 17.36111111111111], [10, 17.36111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 581);
        cqClose(c, st, 581, 580, {});
        atStep(st, 582);
        cqExtrude(c, st, 582, 581, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 582, "extrude");
        atStep(st, 583);
        cqRotate(c, st, 583, 582, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 485});
        stepVol(c, st, 583, "rotate");
        atStep(st, 584);
        cqUnion(c, st, 584, 578, {"toUnion": {"wp": 583}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 584, "union");
        atStep(st, 585);
        cqWorkplaneNew(c, st, 585, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 586);
        cqPolyline(c, st, 586, 585, {"listOfXYTuple": [[10, 17.36111111111111], [12, 17.36111111111111], [12, 18.055555555555554], [10, 18.055555555555554]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 587);
        cqClose(c, st, 587, 586, {});
        atStep(st, 588);
        cqExtrude(c, st, 588, 587, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 588, "extrude");
        atStep(st, 589);
        cqRotate(c, st, 589, 588, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 495});
        stepVol(c, st, 589, "rotate");
        atStep(st, 590);
        cqUnion(c, st, 590, 584, {"toUnion": {"wp": 589}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 590, "union");
        atStep(st, 591);
        cqWorkplaneNew(c, st, 591, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 592);
        cqPolyline(c, st, 592, 591, {"listOfXYTuple": [[10, 18.055555555555554], [12, 18.055555555555554], [12, 18.75], [10, 18.75]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 593);
        cqClose(c, st, 593, 592, {});
        atStep(st, 594);
        cqExtrude(c, st, 594, 593, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 594, "extrude");
        atStep(st, 595);
        cqRotate(c, st, 595, 594, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 505});
        stepVol(c, st, 595, "rotate");
        atStep(st, 596);
        cqUnion(c, st, 596, 590, {"toUnion": {"wp": 595}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 596, "union");
        atStep(st, 597);
        cqWorkplaneNew(c, st, 597, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 598);
        cqPolyline(c, st, 598, 597, {"listOfXYTuple": [[10, 18.75], [12, 18.75], [12, 19.444444444444446], [10, 19.444444444444446]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 599);
        cqClose(c, st, 599, 598, {});
        atStep(st, 600);
        cqExtrude(c, st, 600, 599, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 600, "extrude");
        atStep(st, 601);
        cqRotate(c, st, 601, 600, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 515});
        stepVol(c, st, 601, "rotate");
        atStep(st, 602);
        cqUnion(c, st, 602, 596, {"toUnion": {"wp": 601}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 602, "union");
        atStep(st, 603);
        cqWorkplaneNew(c, st, 603, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 604);
        cqPolyline(c, st, 604, 603, {"listOfXYTuple": [[10, 19.444444444444446], [12, 19.444444444444446], [12, 20.13888888888889], [10, 20.13888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 605);
        cqClose(c, st, 605, 604, {});
        atStep(st, 606);
        cqExtrude(c, st, 606, 605, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 606, "extrude");
        atStep(st, 607);
        cqRotate(c, st, 607, 606, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 525});
        stepVol(c, st, 607, "rotate");
        atStep(st, 608);
        cqUnion(c, st, 608, 602, {"toUnion": {"wp": 607}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 608, "union");
        atStep(st, 609);
        cqWorkplaneNew(c, st, 609, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 610);
        cqPolyline(c, st, 610, 609, {"listOfXYTuple": [[10, 20.13888888888889], [12, 20.13888888888889], [12, 20.833333333333336], [10, 20.833333333333336]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 611);
        cqClose(c, st, 611, 610, {});
        atStep(st, 612);
        cqExtrude(c, st, 612, 611, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 612, "extrude");
        atStep(st, 613);
        cqRotate(c, st, 613, 612, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 535});
        stepVol(c, st, 613, "rotate");
        atStep(st, 614);
        cqUnion(c, st, 614, 608, {"toUnion": {"wp": 613}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 614, "union");
        atStep(st, 615);
        cqWorkplaneNew(c, st, 615, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 616);
        cqPolyline(c, st, 616, 615, {"listOfXYTuple": [[10, 20.833333333333336], [12, 20.833333333333336], [12, 21.52777777777778], [10, 21.52777777777778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 617);
        cqClose(c, st, 617, 616, {});
        atStep(st, 618);
        cqExtrude(c, st, 618, 617, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 618, "extrude");
        atStep(st, 619);
        cqRotate(c, st, 619, 618, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 545});
        stepVol(c, st, 619, "rotate");
        atStep(st, 620);
        cqUnion(c, st, 620, 614, {"toUnion": {"wp": 619}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 620, "union");
        atStep(st, 621);
        cqWorkplaneNew(c, st, 621, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 622);
        cqPolyline(c, st, 622, 621, {"listOfXYTuple": [[10, 21.52777777777778], [12, 21.52777777777778], [12, 22.22222222222222], [10, 22.22222222222222]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 623);
        cqClose(c, st, 623, 622, {});
        atStep(st, 624);
        cqExtrude(c, st, 624, 623, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 624, "extrude");
        atStep(st, 625);
        cqRotate(c, st, 625, 624, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 555});
        stepVol(c, st, 625, "rotate");
        atStep(st, 626);
        cqUnion(c, st, 626, 620, {"toUnion": {"wp": 625}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 626, "union");
        atStep(st, 627);
        cqWorkplaneNew(c, st, 627, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 628);
        cqPolyline(c, st, 628, 627, {"listOfXYTuple": [[10, 22.22222222222222], [12, 22.22222222222222], [12, 22.916666666666664], [10, 22.916666666666664]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 629);
        cqClose(c, st, 629, 628, {});
        atStep(st, 630);
        cqExtrude(c, st, 630, 629, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 630, "extrude");
        atStep(st, 631);
        cqRotate(c, st, 631, 630, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 565});
        stepVol(c, st, 631, "rotate");
        atStep(st, 632);
        cqUnion(c, st, 632, 626, {"toUnion": {"wp": 631}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 632, "union");
        atStep(st, 633);
        cqWorkplaneNew(c, st, 633, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 634);
        cqPolyline(c, st, 634, 633, {"listOfXYTuple": [[10, 22.916666666666664], [12, 22.916666666666664], [12, 23.61111111111111], [10, 23.61111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 635);
        cqClose(c, st, 635, 634, {});
        atStep(st, 636);
        cqExtrude(c, st, 636, 635, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 636, "extrude");
        atStep(st, 637);
        cqRotate(c, st, 637, 636, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 575});
        stepVol(c, st, 637, "rotate");
        atStep(st, 638);
        cqUnion(c, st, 638, 632, {"toUnion": {"wp": 637}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 638, "union");
        atStep(st, 639);
        cqWorkplaneNew(c, st, 639, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 640);
        cqPolyline(c, st, 640, 639, {"listOfXYTuple": [[10, 23.61111111111111], [12, 23.61111111111111], [12, 24.305555555555554], [10, 24.305555555555554]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 641);
        cqClose(c, st, 641, 640, {});
        atStep(st, 642);
        cqExtrude(c, st, 642, 641, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 642, "extrude");
        atStep(st, 643);
        cqRotate(c, st, 643, 642, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 585});
        stepVol(c, st, 643, "rotate");
        atStep(st, 644);
        cqUnion(c, st, 644, 638, {"toUnion": {"wp": 643}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 644, "union");
        atStep(st, 645);
        cqWorkplaneNew(c, st, 645, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 646);
        cqPolyline(c, st, 646, 645, {"listOfXYTuple": [[10, 24.305555555555554], [12, 24.305555555555554], [12, 25], [10, 25]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 647);
        cqClose(c, st, 647, 646, {});
        atStep(st, 648);
        cqExtrude(c, st, 648, 647, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 648, "extrude");
        atStep(st, 649);
        cqRotate(c, st, 649, 648, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 595});
        stepVol(c, st, 649, "rotate");
        atStep(st, 650);
        cqUnion(c, st, 650, 644, {"toUnion": {"wp": 649}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 650, "union");
        atStep(st, 651);
        cqUnion(c, st, 651, 435, {"toUnion": {"wp": 650}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 651, "union");
        // step 652: guard_begin {"before":{"wp":651},"block":1,"g":1,"op":"guard_begin","skippable":true}
        atStep(st, 653);
        cqSelect("Edges")(c, st, 652, 651, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 654);
        cqFillet(c, st, 653, 652, {"radius": 0.1});
        stepVol(c, st, 653, "fillet");
        // step 655: guard_end {"after":{"wp":653},"block":1,"g":1,"op":"guard_end"}
        return mergeMaps(cqMeasure(c, st, {"wp": 653}), {"i": 2351});
    } catch (e) {
        return {"i": 2351, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c2351"] = case_2351();
    return out;
}
