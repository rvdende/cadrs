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

const case_297 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqSphere(c, st, 2, 1, {"radius": 10, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "sphere");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqSphere(c, st, 4, 3, {"radius": 9, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 4, "sphere");
        atStep(st, 5);
        cqCut(c, st, 5, 2, {"toCut": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 5, "cut");
        atStep(st, 6);
        cqWorkplaneNew(c, st, 6, {"plane": {"origin": [3.0190348995769716, 0.6005233790766634, 8.457233587073176], "xDir": [0.9417914822982228, 0, -0.3361975667230747], "zDir": [0.3354483221752191, 0.06672481989740704, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 7);
        cqPolygon(c, st, 7, 6, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 8);
        cqExtrude(c, st, 8, 7, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 8, "extrude");
        atStep(st, 9);
        cqUnion(c, st, 9, 5, {"toUnion": {"wp": 8}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 9, "union");
        atStep(st, 10);
        cqWorkplaneNew(c, st, 10, {"plane": {"origin": [2.5594142037358925, 1.7101458965235556, 8.457233587073176], "xDir": [0.9571306287011128, 6.938893903907228e-18, -0.2896566236118431], "zDir": [0.2843793559706547, 0.1900162107248395, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 11);
        cqPolygon(c, st, 11, 10, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 12);
        cqExtrude(c, st, 12, 11, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 12, "extrude");
        atStep(st, 13);
        cqUnion(c, st, 13, 9, {"toUnion": {"wp": 12}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 13, "union");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 14, {"plane": {"origin": [1.710145896523556, 2.5594142037358925, 8.457233587073176], "xDir": [6.938893903907228e-18, 0.9571306287011128, -0.2896566236118431], "zDir": [0.1900162107248395, 0.2843793559706547, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 15);
        cqPolygon(c, st, 15, 14, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 16);
        cqExtrude(c, st, 16, 15, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 16, "extrude");
        atStep(st, 17);
        cqUnion(c, st, 17, 13, {"toUnion": {"wp": 16}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 17, "union");
        atStep(st, 18);
        cqWorkplaneNew(c, st, 18, {"plane": {"origin": [0.6005233790766638, 3.0190348995769716, 8.457233587073176], "xDir": [3.469446951953614e-18, 0.9417914822982228, -0.33619756672307477], "zDir": [0.06672481989740708, 0.3354483221752191, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 19);
        cqPolygon(c, st, 19, 18, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 20);
        cqExtrude(c, st, 20, 19, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 20, "extrude");
        atStep(st, 21);
        cqUnion(c, st, 21, 17, {"toUnion": {"wp": 20}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 21, "union");
        atStep(st, 22);
        cqWorkplaneNew(c, st, 22, {"plane": {"origin": [-0.6005233790766633, 3.0190348995769716, 8.457233587073176], "xDir": [3.469446951953614e-18, 0.9417914822982228, -0.3361975667230747], "zDir": [-0.06672481989740703, 0.3354483221752191, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 23);
        cqPolygon(c, st, 23, 22, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 24);
        cqExtrude(c, st, 24, 23, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 24, "extrude");
        atStep(st, 25);
        cqUnion(c, st, 25, 21, {"toUnion": {"wp": 24}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 25, "union");
        atStep(st, 26);
        cqWorkplaneNew(c, st, 26, {"plane": {"origin": [-1.710145896523555, 2.5594142037358933, 8.457233587073176], "xDir": [-6.938893903907228e-18, 0.9571306287011124, -0.2896566236118432], "zDir": [-0.19001621072483943, 0.28437935597065483, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 27);
        cqPolygon(c, st, 27, 26, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 28);
        cqExtrude(c, st, 28, 27, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 28, "extrude");
        atStep(st, 29);
        cqUnion(c, st, 29, 25, {"toUnion": {"wp": 28}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 29, "union");
        atStep(st, 30);
        cqWorkplaneNew(c, st, 30, {"plane": {"origin": [-2.559414203735893, 1.7101458965235556, 8.457233587073176], "xDir": [0.9571306287011124, -6.938893903907228e-18, 0.28965662361184313], "zDir": [-0.2843793559706548, 0.1900162107248395, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 31);
        cqPolygon(c, st, 31, 30, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 32);
        cqExtrude(c, st, 32, 31, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 32, "extrude");
        atStep(st, 33);
        cqUnion(c, st, 33, 29, {"toUnion": {"wp": 32}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 33, "union");
        atStep(st, 34);
        cqWorkplaneNew(c, st, 34, {"plane": {"origin": [-3.0190348995769716, 0.6005233790766645, 8.457233587073176], "xDir": [0.9417914822982228, 0, 0.33619756672307477], "zDir": [-0.3354483221752191, 0.06672481989740717, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 35);
        cqPolygon(c, st, 35, 34, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 36);
        cqExtrude(c, st, 36, 35, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 36, "extrude");
        atStep(st, 37);
        cqUnion(c, st, 37, 33, {"toUnion": {"wp": 36}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 37, "union");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 38, {"plane": {"origin": [-3.0190348995769716, -0.6005233790766638, 8.457233587073176], "xDir": [0.9417914822982228, 3.469446951953614e-18, 0.33619756672307477], "zDir": [-0.3354483221752191, -0.06672481989740708, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 39);
        cqPolygon(c, st, 39, 38, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 40);
        cqExtrude(c, st, 40, 39, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 40, "extrude");
        atStep(st, 41);
        cqUnion(c, st, 41, 37, {"toUnion": {"wp": 40}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 41, "union");
        atStep(st, 42);
        cqWorkplaneNew(c, st, 42, {"plane": {"origin": [-2.5594142037358933, -1.710145896523555, 8.457233587073176], "xDir": [0.9571306287011124, 6.938893903907228e-18, 0.2896566236118432], "zDir": [-0.28437935597065483, -0.19001621072483943, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 43);
        cqPolygon(c, st, 43, 42, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 44);
        cqExtrude(c, st, 44, 43, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 44, "extrude");
        atStep(st, 45);
        cqUnion(c, st, 45, 41, {"toUnion": {"wp": 44}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 45, "union");
        atStep(st, 46);
        cqWorkplaneNew(c, st, 46, {"plane": {"origin": [-1.7101458965235556, -2.5594142037358925, 8.457233587073176], "xDir": [6.938893903907228e-18, 0.9571306287011128, 0.2896566236118431], "zDir": [-0.1900162107248395, -0.2843793559706547, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 47);
        cqPolygon(c, st, 47, 46, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 48);
        cqExtrude(c, st, 48, 47, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 48, "extrude");
        atStep(st, 49);
        cqUnion(c, st, 49, 45, {"toUnion": {"wp": 48}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 49, "union");
        atStep(st, 50);
        cqWorkplaneNew(c, st, 50, {"plane": {"origin": [-0.6005233790766648, -3.019034899576971, 8.457233587073176], "xDir": [-3.469446951953614e-18, 0.9417914822982228, 0.3361975667230747], "zDir": [-0.0667248198974072, -0.335448322175219, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 51);
        cqPolygon(c, st, 51, 50, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 52);
        cqExtrude(c, st, 52, 51, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 52, "extrude");
        atStep(st, 53);
        cqUnion(c, st, 53, 49, {"toUnion": {"wp": 52}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 53, "union");
        atStep(st, 54);
        cqWorkplaneNew(c, st, 54, {"plane": {"origin": [0.6005233790766636, -3.0190348995769716, 8.457233587073176], "xDir": [0, 0.9417914822982228, 0.33619756672307477], "zDir": [0.06672481989740707, -0.3354483221752191, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 55);
        cqPolygon(c, st, 55, 54, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 56);
        cqExtrude(c, st, 56, 55, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 56, "extrude");
        atStep(st, 57);
        cqUnion(c, st, 57, 53, {"toUnion": {"wp": 56}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 57, "union");
        atStep(st, 58);
        cqWorkplaneNew(c, st, 58, {"plane": {"origin": [1.710145896523557, -2.559414203735892, 8.457233587073176], "xDir": [0, 0.9571306287011128, 0.289656623611843], "zDir": [0.19001621072483965, -0.28437935597065467, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 59);
        cqPolygon(c, st, 59, 58, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 60);
        cqExtrude(c, st, 60, 59, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 60, "extrude");
        atStep(st, 61);
        cqUnion(c, st, 61, 57, {"toUnion": {"wp": 60}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 61, "union");
        atStep(st, 62);
        cqWorkplaneNew(c, st, 62, {"plane": {"origin": [2.5594142037358925, -1.7101458965235556, 8.457233587073176], "xDir": [0.9571306287011128, -6.938893903907228e-18, -0.2896566236118431], "zDir": [0.2843793559706547, -0.1900162107248395, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 63);
        cqPolygon(c, st, 63, 62, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 64);
        cqExtrude(c, st, 64, 63, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 64, "extrude");
        atStep(st, 65);
        cqUnion(c, st, 65, 61, {"toUnion": {"wp": 64}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 65, "union");
        atStep(st, 66);
        cqWorkplaneNew(c, st, 66, {"plane": {"origin": [3.019034899576971, -0.6005233790766649, 8.457233587073176], "xDir": [0.9417914822982228, 0, -0.3361975667230747], "zDir": [0.335448322175219, -0.06672481989740721, 0.9396926207859084]}, "obj": undefined});
        atStep(st, 67);
        cqPolygon(c, st, 67, 66, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 68);
        cqExtrude(c, st, 68, 67, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 68, "extrude");
        atStep(st, 69);
        cqUnion(c, st, 69, 65, {"toUnion": {"wp": 68}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 69, "union");
        atStep(st, 70);
        cqWorkplaneNew(c, st, 70, {"plane": {"origin": [5.785088487178854, 0, 6.894399988070802], "xDir": [0.7660444431189781, 0, -0.6427876096865394], "zDir": [0.6427876096865393, 0, 0.766044443118978]}, "obj": undefined});
        atStep(st, 71);
        cqPolygon(c, st, 71, 70, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 72);
        cqExtrude(c, st, 72, 71, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 72, "extrude");
        atStep(st, 73);
        cqUnion(c, st, 73, 69, {"toUnion": {"wp": 72}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 73, "union");
        atStep(st, 74);
        cqWorkplaneNew(c, st, 74, {"plane": {"origin": [5.344724847071227, 2.2138575188093683, 6.894399988070802], "xDir": [0.7903281886809359, 1.387778780781446e-17, -0.6126837309544877], "zDir": [0.5938583163412474, 0.24598416875659648, 0.766044443118978]}, "obj": undefined});
        atStep(st, 75);
        cqPolygon(c, st, 75, 74, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 76);
        cqExtrude(c, st, 76, 75, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 76, "extrude");
        atStep(st, 77);
        cqUnion(c, st, 77, 73, {"toUnion": {"wp": 76}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 77, "union");
        atStep(st, 78);
        cqWorkplaneNew(c, st, 78, {"plane": {"origin": [4.090675299048393, 4.090675299048392, 6.894399988070802], "xDir": [0.8600121206459054, -2.775557561562892e-17, -0.510273605374737], "zDir": [0.4545194776720437, 0.4545194776720436, 0.766044443118978]}, "obj": undefined});
        atStep(st, 79);
        cqPolygon(c, st, 79, 78, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 80);
        cqExtrude(c, st, 80, 79, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 80, "extrude");
        atStep(st, 81);
        cqUnion(c, st, 81, 77, {"toUnion": {"wp": 80}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 81, "union");
        atStep(st, 82);
        cqWorkplaneNew(c, st, 82, {"plane": {"origin": [2.2138575188093688, 5.344724847071227, 6.894399988070802], "xDir": [1.387778780781446e-17, 0.7903281886809358, -0.6126837309544878], "zDir": [0.24598416875659657, 0.5938583163412474, 0.766044443118978]}, "obj": undefined});
        atStep(st, 83);
        cqPolygon(c, st, 83, 82, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 84);
        cqExtrude(c, st, 84, 83, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 84, "extrude");
        atStep(st, 85);
        cqUnion(c, st, 85, 81, {"toUnion": {"wp": 84}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 85, "union");
        atStep(st, 86);
        cqWorkplaneNew(c, st, 86, {"plane": {"origin": [3.5423450493038935e-16, 5.785088487178854, 6.894399988070802], "xDir": [0, 0.7660444431189781, -0.6427876096865394], "zDir": [3.9359389436709925e-17, 0.6427876096865393, 0.766044443118978]}, "obj": undefined});
        atStep(st, 87);
        cqPolygon(c, st, 87, 86, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 88);
        cqExtrude(c, st, 88, 87, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 88, "extrude");
        atStep(st, 89);
        cqUnion(c, st, 89, 85, {"toUnion": {"wp": 88}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 89, "union");
        atStep(st, 90);
        cqWorkplaneNew(c, st, 90, {"plane": {"origin": [-2.2138575188093683, 5.344724847071227, 6.894399988070802], "xDir": [-1.387778780781446e-17, 0.7903281886809359, -0.6126837309544877], "zDir": [-0.24598416875659648, 0.5938583163412474, 0.766044443118978]}, "obj": undefined});
        atStep(st, 91);
        cqPolygon(c, st, 91, 90, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 92);
        cqExtrude(c, st, 92, 91, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 92, "extrude");
        atStep(st, 93);
        cqUnion(c, st, 93, 89, {"toUnion": {"wp": 92}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 93, "union");
        atStep(st, 94);
        cqWorkplaneNew(c, st, 94, {"plane": {"origin": [-4.090675299048392, 4.090675299048393, 6.894399988070802], "xDir": [2.775557561562892e-17, 0.8600121206459054, -0.510273605374737], "zDir": [-0.4545194776720436, 0.4545194776720437, 0.766044443118978]}, "obj": undefined});
        atStep(st, 95);
        cqPolygon(c, st, 95, 94, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 96);
        cqExtrude(c, st, 96, 95, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 96, "extrude");
        atStep(st, 97);
        cqUnion(c, st, 97, 93, {"toUnion": {"wp": 96}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 97, "union");
        atStep(st, 98);
        cqWorkplaneNew(c, st, 98, {"plane": {"origin": [-5.344724847071227, 2.213857518809369, 6.894399988070802], "xDir": [0.7903281886809358, 0, 0.6126837309544877], "zDir": [-0.5938583163412474, 0.24598416875659657, 0.766044443118978]}, "obj": undefined});
        atStep(st, 99);
        cqPolygon(c, st, 99, 98, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 100);
        cqExtrude(c, st, 100, 99, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 100, "extrude");
        atStep(st, 101);
        cqUnion(c, st, 101, 97, {"toUnion": {"wp": 100}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 101, "union");
        atStep(st, 102);
        cqWorkplaneNew(c, st, 102, {"plane": {"origin": [-5.785088487178854, 7.084690098607787e-16, 6.894399988070802], "xDir": [0.7660444431189781, 0, 0.6427876096865394], "zDir": [-0.6427876096865393, 7.871877887341985e-17, 0.766044443118978]}, "obj": undefined});
        atStep(st, 103);
        cqPolygon(c, st, 103, 102, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 104);
        cqExtrude(c, st, 104, 103, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 104, "extrude");
        atStep(st, 105);
        cqUnion(c, st, 105, 101, {"toUnion": {"wp": 104}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 105, "union");
        atStep(st, 106);
        cqWorkplaneNew(c, st, 106, {"plane": {"origin": [-5.344724847071227, -2.213857518809368, 6.894399988070802], "xDir": [0.7903281886809359, 0, 0.6126837309544877], "zDir": [-0.5938583163412474, -0.24598416875659643, 0.766044443118978]}, "obj": undefined});
        atStep(st, 107);
        cqPolygon(c, st, 107, 106, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 108);
        cqExtrude(c, st, 108, 107, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 108, "extrude");
        atStep(st, 109);
        cqUnion(c, st, 109, 105, {"toUnion": {"wp": 108}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 109, "union");
        atStep(st, 110);
        cqWorkplaneNew(c, st, 110, {"plane": {"origin": [-4.090675299048394, -4.090675299048392, 6.894399988070802], "xDir": [0.8600121206459054, 2.7755575615628914e-17, 0.5102736053747371], "zDir": [-0.4545194776720438, -0.4545194776720436, 0.766044443118978]}, "obj": undefined});
        atStep(st, 111);
        cqPolygon(c, st, 111, 110, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 112);
        cqExtrude(c, st, 112, 111, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 112, "extrude");
        atStep(st, 113);
        cqUnion(c, st, 113, 109, {"toUnion": {"wp": 112}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 113, "union");
        atStep(st, 114);
        cqWorkplaneNew(c, st, 114, {"plane": {"origin": [-2.213857518809371, -5.344724847071225, 6.894399988070802], "xDir": [0, 0.790328188680936, 0.6126837309544876], "zDir": [-0.2459841687565968, -0.5938583163412472, 0.766044443118978]}, "obj": undefined});
        atStep(st, 115);
        cqPolygon(c, st, 115, 114, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 116);
        cqExtrude(c, st, 116, 115, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 116, "extrude");
        atStep(st, 117);
        cqUnion(c, st, 117, 113, {"toUnion": {"wp": 116}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 117, "union");
        atStep(st, 118);
        cqWorkplaneNew(c, st, 118, {"plane": {"origin": [-1.062703514791168e-15, -5.785088487178854, 6.894399988070802], "xDir": [0, 0.7660444431189781, 0.6427876096865394], "zDir": [-1.1807816831012978e-16, -0.6427876096865393, 0.766044443118978]}, "obj": undefined});
        atStep(st, 119);
        cqPolygon(c, st, 119, 118, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 120);
        cqExtrude(c, st, 120, 119, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 120, "extrude");
        atStep(st, 121);
        cqUnion(c, st, 121, 117, {"toUnion": {"wp": 120}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 121, "union");
        atStep(st, 122);
        cqWorkplaneNew(c, st, 122, {"plane": {"origin": [2.2138575188093697, -5.344724847071226, 6.894399988070802], "xDir": [-1.3877787807814457e-17, 0.7903281886809359, 0.6126837309544877], "zDir": [0.24598416875659665, -0.5938583163412473, 0.766044443118978]}, "obj": undefined});
        atStep(st, 123);
        cqPolygon(c, st, 123, 122, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 124);
        cqExtrude(c, st, 124, 123, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 124, "extrude");
        atStep(st, 125);
        cqUnion(c, st, 125, 121, {"toUnion": {"wp": 124}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 125, "union");
        atStep(st, 126);
        cqWorkplaneNew(c, st, 126, {"plane": {"origin": [4.090675299048392, -4.090675299048394, 6.894399988070802], "xDir": [-2.7755575615628914e-17, 0.8600121206459054, 0.5102736053747371], "zDir": [0.4545194776720436, -0.4545194776720438, 0.766044443118978]}, "obj": undefined});
        atStep(st, 127);
        cqPolygon(c, st, 127, 126, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 128);
        cqExtrude(c, st, 128, 127, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 128, "extrude");
        atStep(st, 129);
        cqUnion(c, st, 129, 125, {"toUnion": {"wp": 128}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 129, "union");
        atStep(st, 130);
        cqWorkplaneNew(c, st, 130, {"plane": {"origin": [5.344724847071225, -2.213857518809372, 6.894399988070802], "xDir": [0.790328188680936, 1.387778780781446e-17, -0.6126837309544876], "zDir": [0.5938583163412472, -0.24598416875659687, 0.766044443118978]}, "obj": undefined});
        atStep(st, 131);
        cqPolygon(c, st, 131, 130, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 132);
        cqExtrude(c, st, 132, 131, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 132, "extrude");
        atStep(st, 133);
        cqUnion(c, st, 133, 129, {"toUnion": {"wp": 132}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 133, "union");
        atStep(st, 134);
        cqWorkplaneNew(c, st, 134, {"plane": {"origin": [7.644464716383373, 1.5205785740860824, 4.500000000000001], "xDir": [-0.5072928013422138, 1.3877787807814457e-17, 0.8617737601635184], "zDir": [0.8493849684870415, 0.1689531748984536, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 135);
        cqPolygon(c, st, 135, 134, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 136);
        cqExtrude(c, st, 136, 135, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 136, "extrude");
        atStep(st, 137);
        cqUnion(c, st, 137, 133, {"toUnion": {"wp": 136}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 137, "union");
        atStep(st, 138);
        cqWorkplaneNew(c, st, 138, {"plane": {"origin": [6.480664260559221, 4.33024141843274, 4.500000000000001], "xDir": [-0.5703563642527718, 2.7755575615628914e-17, 0.8213973567989854], "zDir": [0.7200738067288024, 0.4811379353814156, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 139);
        cqPolygon(c, st, 139, 138, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 140);
        cqExtrude(c, st, 140, 139, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 140, "extrude");
        atStep(st, 141);
        cqUnion(c, st, 141, 137, {"toUnion": {"wp": 140}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 141, "union");
        atStep(st, 142);
        cqWorkplaneNew(c, st, 142, {"plane": {"origin": [4.330241418432741, 6.480664260559221, 4.500000000000001], "xDir": [0, -0.5703563642527717, 0.8213973567989853], "zDir": [0.4811379353814157, 0.7200738067288024, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 143);
        cqPolygon(c, st, 143, 142, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 144);
        cqExtrude(c, st, 144, 143, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 144, "extrude");
        atStep(st, 145);
        cqUnion(c, st, 145, 141, {"toUnion": {"wp": 144}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 145, "union");
        atStep(st, 146);
        cqWorkplaneNew(c, st, 146, {"plane": {"origin": [1.520578574086083, 7.644464716383373, 4.500000000000001], "xDir": [1.3877787807814457e-17, -0.5072928013422139, 0.8617737601635185], "zDir": [0.16895317489845368, 0.8493849684870415, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 147);
        cqPolygon(c, st, 147, 146, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 148);
        cqExtrude(c, st, 148, 147, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 148, "extrude");
        atStep(st, 149);
        cqUnion(c, st, 149, 145, {"toUnion": {"wp": 148}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 149, "union");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [-1.520578574086082, 7.644464716383373, 4.500000000000001], "xDir": [-1.3877787807814457e-17, -0.5072928013422138, 0.8617737601635184], "zDir": [-0.16895317489845357, 0.8493849684870415, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 151);
        cqPolygon(c, st, 151, 150, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 152);
        cqExtrude(c, st, 152, 151, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 152, "extrude");
        atStep(st, 153);
        cqUnion(c, st, 153, 149, {"toUnion": {"wp": 152}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 153, "union");
        atStep(st, 154);
        cqWorkplaneNew(c, st, 154, {"plane": {"origin": [-4.330241418432739, 6.480664260559223, 4.500000000000001], "xDir": [2.7755575615628907e-17, -0.5703563642527716, 0.8213973567989854], "zDir": [-0.4811379353814154, 0.7200738067288026, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 155);
        cqPolygon(c, st, 155, 154, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 156);
        cqExtrude(c, st, 156, 155, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 156, "extrude");
        atStep(st, 157);
        cqUnion(c, st, 157, 153, {"toUnion": {"wp": 156}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 157, "union");
        atStep(st, 158);
        cqWorkplaneNew(c, st, 158, {"plane": {"origin": [-6.480664260559222, 4.33024141843274, 4.500000000000001], "xDir": [-0.5703563642527717, 0, -0.8213973567989853], "zDir": [-0.7200738067288025, 0.4811379353814156, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 159);
        cqPolygon(c, st, 159, 158, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 160);
        cqExtrude(c, st, 160, 159, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 160, "extrude");
        atStep(st, 161);
        cqUnion(c, st, 161, 157, {"toUnion": {"wp": 160}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 161, "union");
        atStep(st, 162);
        cqWorkplaneNew(c, st, 162, {"plane": {"origin": [-7.644464716383373, 1.520578574086085, 4.500000000000001], "xDir": [-0.5072928013422139, -1.3877787807814457e-17, -0.8617737601635185], "zDir": [-0.8493849684870415, 0.1689531748984539, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 163);
        cqPolygon(c, st, 163, 162, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 164);
        cqExtrude(c, st, 164, 163, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 164, "extrude");
        atStep(st, 165);
        cqUnion(c, st, 165, 161, {"toUnion": {"wp": 164}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 165, "union");
        atStep(st, 166);
        cqWorkplaneNew(c, st, 166, {"plane": {"origin": [-7.644464716383373, -1.520578574086083, 4.500000000000001], "xDir": [-0.5072928013422139, 1.3877787807814457e-17, -0.8617737601635185], "zDir": [-0.8493849684870415, -0.1689531748984537, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 167);
        cqPolygon(c, st, 167, 166, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 168);
        cqExtrude(c, st, 168, 167, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 168, "extrude");
        atStep(st, 169);
        cqUnion(c, st, 169, 165, {"toUnion": {"wp": 168}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 169, "union");
        atStep(st, 170);
        cqWorkplaneNew(c, st, 170, {"plane": {"origin": [-6.480664260559223, -4.330241418432739, 4.500000000000001], "xDir": [-0.5703563642527716, -2.7755575615628907e-17, -0.8213973567989854], "zDir": [-0.7200738067288026, -0.4811379353814154, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 171);
        cqPolygon(c, st, 171, 170, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 172);
        cqExtrude(c, st, 172, 171, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 172, "extrude");
        atStep(st, 173);
        cqUnion(c, st, 173, 169, {"toUnion": {"wp": 172}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 173, "union");
        atStep(st, 174);
        cqWorkplaneNew(c, st, 174, {"plane": {"origin": [-4.33024141843274, -6.480664260559221, 4.500000000000001], "xDir": [2.7755575615628914e-17, -0.5703563642527718, -0.8213973567989854], "zDir": [-0.4811379353814156, -0.7200738067288024, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 175);
        cqPolygon(c, st, 175, 174, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 176);
        cqExtrude(c, st, 176, 175, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 176, "extrude");
        atStep(st, 177);
        cqUnion(c, st, 177, 173, {"toUnion": {"wp": 176}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 177, "union");
        atStep(st, 178);
        cqWorkplaneNew(c, st, 178, {"plane": {"origin": [-1.5205785740860858, -7.644464716383372, 4.500000000000001], "xDir": [0, -0.5072928013422139, -0.8617737601635183], "zDir": [-0.16895317489845396, -0.8493849684870414, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 179);
        cqPolygon(c, st, 179, 178, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 180);
        cqExtrude(c, st, 180, 179, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 180, "extrude");
        atStep(st, 181);
        cqUnion(c, st, 181, 177, {"toUnion": {"wp": 180}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 181, "union");
        atStep(st, 182);
        cqWorkplaneNew(c, st, 182, {"plane": {"origin": [1.5205785740860829, -7.644464716383373, 4.500000000000001], "xDir": [-1.3877787807814457e-17, -0.5072928013422139, -0.8617737601635184], "zDir": [0.16895317489845366, -0.8493849684870415, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 183);
        cqPolygon(c, st, 183, 182, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 184);
        cqExtrude(c, st, 184, 183, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 184, "extrude");
        atStep(st, 185);
        cqUnion(c, st, 185, 181, {"toUnion": {"wp": 184}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 185, "union");
        atStep(st, 186);
        cqWorkplaneNew(c, st, 186, {"plane": {"origin": [4.330241418432744, -6.4806642605592195, 4.500000000000001], "xDir": [-2.7755575615628907e-17, -0.5703563642527718, -0.8213973567989852], "zDir": [0.481137935381416, -0.7200738067288022, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 187);
        cqPolygon(c, st, 187, 186, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 188);
        cqExtrude(c, st, 188, 187, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 188, "extrude");
        atStep(st, 189);
        cqUnion(c, st, 189, 185, {"toUnion": {"wp": 188}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 189, "union");
        atStep(st, 190);
        cqWorkplaneNew(c, st, 190, {"plane": {"origin": [6.480664260559221, -4.33024141843274, 4.500000000000001], "xDir": [-0.5703563642527718, -2.7755575615628914e-17, 0.8213973567989854], "zDir": [0.7200738067288024, -0.4811379353814156, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 191);
        cqPolygon(c, st, 191, 190, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 192);
        cqExtrude(c, st, 192, 191, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 192, "extrude");
        atStep(st, 193);
        cqUnion(c, st, 193, 189, {"toUnion": {"wp": 192}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 193, "union");
        atStep(st, 194);
        cqWorkplaneNew(c, st, 194, {"plane": {"origin": [7.644464716383372, -1.5205785740860862, 4.500000000000001], "xDir": [-0.5072928013422139, -1.3877787807814457e-17, 0.8617737601635183], "zDir": [0.8493849684870414, -0.16895317489845402, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 195);
        cqPolygon(c, st, 195, 194, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 196);
        cqExtrude(c, st, 196, 195, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 196, "extrude");
        atStep(st, 197);
        cqUnion(c, st, 197, 193, {"toUnion": {"wp": 196}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 197, "union");
        atStep(st, 198);
        cqWorkplaneNew(c, st, 198, {"plane": {"origin": [8.863269777109872, 0, 1.5628335990023738], "xDir": [-0.17364817766693044, 0, 0.984807753012208], "zDir": [0.984807753012208, 0, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 199);
        cqPolygon(c, st, 199, 198, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 200);
        cqExtrude(c, st, 200, 199, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 200, "extrude");
        atStep(st, 201);
        cqUnion(c, st, 201, 197, {"toUnion": {"wp": 200}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 201, "union");
        atStep(st, 202);
        cqWorkplaneNew(c, st, 202, {"plane": {"origin": [8.188593538197685, 3.3918265002821704, 1.5628335990023738], "xDir": [-0.38268343236508984, 0.9238795325112868, -6.93889390390723e-18], "zDir": [0.9098437264664094, 0.37686961114246337, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 203);
        cqPolygon(c, st, 203, 202, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 204);
        cqExtrude(c, st, 204, 203, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 204, "extrude");
        atStep(st, 205);
        cqUnion(c, st, 205, 201, {"toUnion": {"wp": 204}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 205, "union");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 206, {"plane": {"origin": [6.267278162880171, 6.26727816288017, 1.5628335990023738], "xDir": [-0.7071067811865475, 0.7071067811865476, 0], "zDir": [0.696364240320019, 0.6963642403200189, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 207);
        cqPolygon(c, st, 207, 206, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 208);
        cqExtrude(c, st, 208, 207, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 208, "extrude");
        atStep(st, 209);
        cqUnion(c, st, 209, 205, {"toUnion": {"wp": 208}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 209, "union");
        atStep(st, 210);
        cqWorkplaneNew(c, st, 210, {"plane": {"origin": [3.391826500282171, 8.188593538197685, 1.5628335990023738], "xDir": [0.9238795325112868, -0.3826834323650899, -6.938893903907228e-18], "zDir": [0.3768696111424634, 0.9098437264664094, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 211);
        cqPolygon(c, st, 211, 210, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 212);
        cqExtrude(c, st, 212, 211, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 212, "extrude");
        atStep(st, 213);
        cqUnion(c, st, 213, 209, {"toUnion": {"wp": 212}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 213, "union");
        atStep(st, 214);
        cqWorkplaneNew(c, st, 214, {"plane": {"origin": [5.427187481258539e-16, 8.863269777109872, 1.5628335990023738], "xDir": [1.540743955509789e-33, -0.17364817766693044, 0.984807753012208], "zDir": [6.030208312509488e-17, 0.984807753012208, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 215);
        cqPolygon(c, st, 215, 214, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 216);
        cqExtrude(c, st, 216, 215, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 216, "extrude");
        atStep(st, 217);
        cqUnion(c, st, 217, 213, {"toUnion": {"wp": 216}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 217, "union");
        atStep(st, 218);
        cqWorkplaneNew(c, st, 218, {"plane": {"origin": [-3.39182650028217, 8.188593538197685, 1.5628335990023738], "xDir": [0.9238795325112868, 0.3826834323650898, -6.93889390390723e-18], "zDir": [-0.3768696111424633, 0.9098437264664094, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 219);
        cqPolygon(c, st, 219, 218, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 220);
        cqExtrude(c, st, 220, 219, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 220, "extrude");
        atStep(st, 221);
        cqUnion(c, st, 221, 217, {"toUnion": {"wp": 220}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 221, "union");
        atStep(st, 222);
        cqWorkplaneNew(c, st, 222, {"plane": {"origin": [-6.26727816288017, 6.267278162880171, 1.5628335990023738], "xDir": [0.7071067811865476, 0.7071067811865475, 0], "zDir": [-0.6963642403200189, 0.696364240320019, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 223);
        cqPolygon(c, st, 223, 222, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 224);
        cqExtrude(c, st, 224, 223, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 224, "extrude");
        atStep(st, 225);
        cqUnion(c, st, 225, 221, {"toUnion": {"wp": 224}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 225, "union");
        atStep(st, 226);
        cqWorkplaneNew(c, st, 226, {"plane": {"origin": [-8.188593538197685, 3.3918265002821713, 1.5628335990023738], "xDir": [-0.3826834323650899, -0.9238795325112868, 0], "zDir": [-0.9098437264664094, 0.3768696111424635, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 227);
        cqPolygon(c, st, 227, 226, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 228);
        cqExtrude(c, st, 228, 227, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 228, "extrude");
        atStep(st, 229);
        cqUnion(c, st, 229, 225, {"toUnion": {"wp": 228}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 229, "union");
        atStep(st, 230);
        cqWorkplaneNew(c, st, 230, {"plane": {"origin": [-8.863269777109872, 1.0854374962517079e-15, 1.5628335990023738], "xDir": [-0.17364817766693044, -3.081487911019578e-33, -0.984807753012208], "zDir": [-0.984807753012208, 1.2060416625018976e-16, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 231);
        cqPolygon(c, st, 231, 230, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 232);
        cqExtrude(c, st, 232, 231, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 232, "extrude");
        atStep(st, 233);
        cqUnion(c, st, 233, 229, {"toUnion": {"wp": 232}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 233, "union");
        atStep(st, 234);
        cqWorkplaneNew(c, st, 234, {"plane": {"origin": [-8.188593538197686, -3.391826500282169, 1.5628335990023738], "xDir": [0.3826834323650896, -0.9238795325112868, 0], "zDir": [-0.9098437264664097, -0.3768696111424632, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 235);
        cqPolygon(c, st, 235, 234, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 236);
        cqExtrude(c, st, 236, 235, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 236, "extrude");
        atStep(st, 237);
        cqUnion(c, st, 237, 233, {"toUnion": {"wp": 236}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 237, "union");
        atStep(st, 238);
        cqWorkplaneNew(c, st, 238, {"plane": {"origin": [-6.267278162880172, -6.26727816288017, 1.5628335990023738], "xDir": [0.7071067811865475, -0.7071067811865477, 1.3877787807814457e-17], "zDir": [-0.6963642403200191, -0.6963642403200189, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 239);
        cqPolygon(c, st, 239, 238, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 240);
        cqExtrude(c, st, 240, 239, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 240, "extrude");
        atStep(st, 241);
        cqUnion(c, st, 241, 237, {"toUnion": {"wp": 240}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 241, "union");
        atStep(st, 242);
        cqWorkplaneNew(c, st, 242, {"plane": {"origin": [-3.3918265002821752, -8.188593538197683, 1.5628335990023738], "xDir": [-0.9238795325112864, 0.3826834323650904, -6.93889390390723e-18], "zDir": [-0.3768696111424639, -0.9098437264664092, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 243);
        cqPolygon(c, st, 243, 242, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 244);
        cqExtrude(c, st, 244, 243, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 244, "extrude");
        atStep(st, 245);
        cqUnion(c, st, 245, 241, {"toUnion": {"wp": 244}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 245, "union");
        atStep(st, 246);
        cqWorkplaneNew(c, st, 246, {"plane": {"origin": [-1.6281562443775618e-15, -8.863269777109872, 1.5628335990023738], "xDir": [0, -0.17364817766693044, -0.984807753012208], "zDir": [-1.8090624937528463e-16, -0.984807753012208, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 247);
        cqPolygon(c, st, 247, 246, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 248);
        cqExtrude(c, st, 248, 247, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 248, "extrude");
        atStep(st, 249);
        cqUnion(c, st, 249, 245, {"toUnion": {"wp": 248}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 249, "union");
        atStep(st, 250);
        cqWorkplaneNew(c, st, 250, {"plane": {"origin": [3.391826500282172, -8.188593538197685, 1.5628335990023738], "xDir": [-0.9238795325112868, -0.38268343236509006, 6.938893903907228e-18], "zDir": [0.3768696111424636, -0.9098437264664094, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 251);
        cqPolygon(c, st, 251, 250, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 252);
        cqExtrude(c, st, 252, 251, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 252, "extrude");
        atStep(st, 253);
        cqUnion(c, st, 253, 249, {"toUnion": {"wp": 252}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 253, "union");
        atStep(st, 254);
        cqWorkplaneNew(c, st, 254, {"plane": {"origin": [6.267278162880169, -6.267278162880172, 1.5628335990023738], "xDir": [-0.7071067811865477, -0.7071067811865474, 0], "zDir": [0.6963642403200188, -0.6963642403200191, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 255);
        cqPolygon(c, st, 255, 254, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 256);
        cqExtrude(c, st, 256, 255, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 256, "extrude");
        atStep(st, 257);
        cqUnion(c, st, 257, 253, {"toUnion": {"wp": 256}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 257, "union");
        atStep(st, 258);
        cqWorkplaneNew(c, st, 258, {"plane": {"origin": [8.188593538197683, -3.3918265002821757, 1.5628335990023738], "xDir": [0.3826834323650905, 0.9238795325112864, 1.387778780781446e-17], "zDir": [0.9098437264664092, -0.376869611142464, 0.1736481776669304]}, "obj": undefined});
        atStep(st, 259);
        cqPolygon(c, st, 259, 258, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 260);
        cqExtrude(c, st, 260, 259, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 260, "extrude");
        atStep(st, 261);
        cqUnion(c, st, 261, 257, {"toUnion": {"wp": 260}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 261, "union");
        atStep(st, 262);
        cqWorkplaneNew(c, st, 262, {"plane": {"origin": [8.692964533632184, 1.7291381549321825, -1.5628335990023727], "xDir": [-0.19509032201612828, 0.9807852804032304, 0], "zDir": [0.9658849481813536, 0.19212646165913136, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 263);
        cqPolygon(c, st, 263, 262, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 264);
        cqExtrude(c, st, 264, 263, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 264, "extrude");
        atStep(st, 265);
        cqUnion(c, st, 265, 261, {"toUnion": {"wp": 264}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 265, "union");
        atStep(st, 266);
        cqWorkplaneNew(c, st, 266, {"plane": {"origin": [7.3695394853064125, 4.924168855384529, -1.5628335990023727], "xDir": [-0.5555702330196022, 0.8314696123025452, -1.3877787807814457e-17], "zDir": [0.8188377205896014, 0.5471298728205033, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 267);
        cqPolygon(c, st, 267, 266, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 268);
        cqExtrude(c, st, 268, 267, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 268, "extrude");
        atStep(st, 269);
        cqUnion(c, st, 269, 265, {"toUnion": {"wp": 268}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 269, "union");
        atStep(st, 270);
        cqWorkplaneNew(c, st, 270, {"plane": {"origin": [4.92416885538453, 7.3695394853064125, -1.5628335990023727], "xDir": [0.8314696123025451, -0.5555702330196023, -1.3877787807814457e-17], "zDir": [0.5471298728205034, 0.8188377205896014, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 271);
        cqPolygon(c, st, 271, 270, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 272);
        cqExtrude(c, st, 272, 271, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 272, "extrude");
        atStep(st, 273);
        cqUnion(c, st, 273, 269, {"toUnion": {"wp": 272}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 273, "union");
        atStep(st, 274);
        cqWorkplaneNew(c, st, 274, {"plane": {"origin": [1.729138154932183, 8.692964533632184, -1.5628335990023727], "xDir": [0.9807852804032304, -0.19509032201612836, 0], "zDir": [0.19212646165913144, 0.9658849481813536, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 275);
        cqPolygon(c, st, 275, 274, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 276);
        cqExtrude(c, st, 276, 275, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 276, "extrude");
        atStep(st, 277);
        cqUnion(c, st, 277, 273, {"toUnion": {"wp": 276}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 277, "union");
        atStep(st, 278);
        cqWorkplaneNew(c, st, 278, {"plane": {"origin": [-1.7291381549321818, 8.692964533632184, -1.5628335990023727], "xDir": [0.9807852804032304, 0.19509032201612825, -6.93889390390723e-18], "zDir": [-0.1921264616591313, 0.9658849481813536, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 279);
        cqPolygon(c, st, 279, 278, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 280);
        cqExtrude(c, st, 280, 279, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 280, "extrude");
        atStep(st, 281);
        cqUnion(c, st, 281, 277, {"toUnion": {"wp": 280}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 281, "union");
        atStep(st, 282);
        cqWorkplaneNew(c, st, 282, {"plane": {"origin": [-4.9241688553845275, 7.369539485306414, -1.5628335990023727], "xDir": [0.8314696123025453, 0.555570233019602, 0], "zDir": [-0.5471298728205031, 0.8188377205896016, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 283);
        cqPolygon(c, st, 283, 282, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 284);
        cqExtrude(c, st, 284, 283, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 284, "extrude");
        atStep(st, 285);
        cqUnion(c, st, 285, 281, {"toUnion": {"wp": 284}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 285, "union");
        atStep(st, 286);
        cqWorkplaneNew(c, st, 286, {"plane": {"origin": [-7.369539485306413, 4.924168855384529, -1.5628335990023727], "xDir": [-0.5555702330196022, -0.8314696123025452, 0], "zDir": [-0.8188377205896015, 0.5471298728205033, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 287);
        cqPolygon(c, st, 287, 286, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 288);
        cqExtrude(c, st, 288, 287, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 288, "extrude");
        atStep(st, 289);
        cqUnion(c, st, 289, 285, {"toUnion": {"wp": 288}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 289, "union");
        atStep(st, 290);
        cqWorkplaneNew(c, st, 290, {"plane": {"origin": [-8.692964533632184, 1.7291381549321854, -1.5628335990023727], "xDir": [-0.1950903220161286, -0.9807852804032304, 0], "zDir": [-0.9658849481813536, 0.19212646165913172, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 291);
        cqPolygon(c, st, 291, 290, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 292);
        cqExtrude(c, st, 292, 291, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 292, "extrude");
        atStep(st, 293);
        cqUnion(c, st, 293, 289, {"toUnion": {"wp": 292}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 293, "union");
        atStep(st, 294);
        cqWorkplaneNew(c, st, 294, {"plane": {"origin": [-8.692964533632184, -1.7291381549321831, -1.5628335990023727], "xDir": [0.1950903220161284, -0.9807852804032304, 0], "zDir": [-0.9658849481813536, -0.19212646165913147, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 295);
        cqPolygon(c, st, 295, 294, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 296);
        cqExtrude(c, st, 296, 295, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 296, "extrude");
        atStep(st, 297);
        cqUnion(c, st, 297, 293, {"toUnion": {"wp": 296}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 297, "union");
        atStep(st, 298);
        cqWorkplaneNew(c, st, 298, {"plane": {"origin": [-7.369539485306414, -4.9241688553845275, -1.5628335990023727], "xDir": [0.555570233019602, -0.8314696123025453, 0], "zDir": [-0.8188377205896016, -0.5471298728205031, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 299);
        cqPolygon(c, st, 299, 298, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 300);
        cqExtrude(c, st, 300, 299, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 300, "extrude");
        atStep(st, 301);
        cqUnion(c, st, 301, 297, {"toUnion": {"wp": 300}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 301, "union");
        atStep(st, 302);
        cqWorkplaneNew(c, st, 302, {"plane": {"origin": [-4.924168855384529, -7.3695394853064125, -1.5628335990023727], "xDir": [-0.8314696123025452, 0.5555702330196022, -1.3877787807814457e-17], "zDir": [-0.5471298728205033, -0.8188377205896014, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 303);
        cqPolygon(c, st, 303, 302, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 304);
        cqExtrude(c, st, 304, 303, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 304, "extrude");
        atStep(st, 305);
        cqUnion(c, st, 305, 301, {"toUnion": {"wp": 304}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 305, "union");
        atStep(st, 306);
        cqWorkplaneNew(c, st, 306, {"plane": {"origin": [-1.7291381549321858, -8.692964533632182, -1.5628335990023727], "xDir": [-0.9807852804032304, 0.19509032201612872, 0], "zDir": [-0.1921264616591318, -0.9658849481813536, -0.17364817766693033]}, "obj": undefined});
        atStep(st, 307);
        cqPolygon(c, st, 307, 306, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 308);
        cqExtrude(c, st, 308, 307, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 308, "extrude");
        atStep(st, 309);
        cqUnion(c, st, 309, 305, {"toUnion": {"wp": 308}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 309, "union");
        atStep(st, 310);
        cqWorkplaneNew(c, st, 310, {"plane": {"origin": [1.7291381549321827, -8.692964533632184, -1.5628335990023727], "xDir": [-0.9807852804032304, -0.19509032201612833, -6.93889390390723e-18], "zDir": [0.1921264616591314, -0.9658849481813536, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 311);
        cqPolygon(c, st, 311, 310, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 312);
        cqExtrude(c, st, 312, 311, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 312, "extrude");
        atStep(st, 313);
        cqUnion(c, st, 313, 309, {"toUnion": {"wp": 312}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 313, "union");
        atStep(st, 314);
        cqWorkplaneNew(c, st, 314, {"plane": {"origin": [4.924168855384533, -7.36953948530641, -1.5628335990023727], "xDir": [-0.8314696123025451, -0.5555702330196026, 1.3877787807814457e-17], "zDir": [0.5471298728205036, -0.8188377205896011, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 315);
        cqPolygon(c, st, 315, 314, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 316);
        cqExtrude(c, st, 316, 315, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 316, "extrude");
        atStep(st, 317);
        cqUnion(c, st, 317, 313, {"toUnion": {"wp": 316}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 317, "union");
        atStep(st, 318);
        cqWorkplaneNew(c, st, 318, {"plane": {"origin": [7.3695394853064125, -4.924168855384529, -1.5628335990023727], "xDir": [0.5555702330196022, 0.8314696123025452, 1.3877787807814457e-17], "zDir": [0.8188377205896014, -0.5471298728205033, -0.1736481776669303]}, "obj": undefined});
        atStep(st, 319);
        cqPolygon(c, st, 319, 318, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 320);
        cqExtrude(c, st, 320, 319, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 320, "extrude");
        atStep(st, 321);
        cqUnion(c, st, 321, 317, {"toUnion": {"wp": 320}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 321, "union");
        atStep(st, 322);
        cqWorkplaneNew(c, st, 322, {"plane": {"origin": [8.692964533632182, -1.7291381549321865, -1.5628335990023727], "xDir": [0.19509032201612875, 0.9807852804032304, 0], "zDir": [0.9658849481813536, -0.19212646165913183, -0.17364817766693033]}, "obj": undefined});
        atStep(st, 323);
        cqPolygon(c, st, 323, 322, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 324);
        cqExtrude(c, st, 324, 323, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 324, "extrude");
        atStep(st, 325);
        cqUnion(c, st, 325, 321, {"toUnion": {"wp": 324}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 325, "union");
        atStep(st, 326);
        cqWorkplaneNew(c, st, 326, {"plane": {"origin": [7.794228634059948, 0, -4.499999999999998], "xDir": [0.49999999999999983, 0, 0.8660254037844388], "zDir": [0.8660254037844387, 0, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 327);
        cqPolygon(c, st, 327, 326, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 328);
        cqExtrude(c, st, 328, 327, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 328, "extrude");
        atStep(st, 329);
        cqUnion(c, st, 329, 325, {"toUnion": {"wp": 328}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 329, "union");
        atStep(st, 330);
        cqWorkplaneNew(c, st, 330, {"plane": {"origin": [7.20092830672139, 2.9827221663203263, -4.499999999999998], "xDir": [0.5299498049346547, 0, 0.8480290114434302], "zDir": [0.8001031451912656, 0.3314135740355918, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 331);
        cqPolygon(c, st, 331, 330, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 332);
        cqExtrude(c, st, 332, 331, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 332, "extrude");
        atStep(st, 333);
        cqUnion(c, st, 333, 329, {"toUnion": {"wp": 332}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 333, "union");
        atStep(st, 334);
        cqWorkplaneNew(c, st, 334, {"plane": {"origin": [5.511351921262151, 5.5113519212621505, -4.499999999999998], "xDir": [-0.7071067811865474, 0.7071067811865476, 2.775557561562892e-17], "zDir": [0.6123724356957946, 0.6123724356957945, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 335);
        cqPolygon(c, st, 335, 334, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 336);
        cqExtrude(c, st, 336, 335, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 336, "extrude");
        atStep(st, 337);
        cqUnion(c, st, 337, 333, {"toUnion": {"wp": 336}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 337, "union");
        atStep(st, 338);
        cqWorkplaneNew(c, st, 338, {"plane": {"origin": [2.9827221663203267, 7.20092830672139, -4.499999999999998], "xDir": [2.775557561562892e-17, 0.5299498049346548, 0.8480290114434302], "zDir": [0.33141357403559185, 0.8001031451912656, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 339);
        cqPolygon(c, st, 339, 338, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 340);
        cqExtrude(c, st, 340, 339, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 340, "extrude");
        atStep(st, 341);
        cqUnion(c, st, 341, 337, {"toUnion": {"wp": 340}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 341, "union");
        atStep(st, 342);
        cqWorkplaneNew(c, st, 342, {"plane": {"origin": [4.772588574262081e-16, 7.794228634059948, -4.499999999999998], "xDir": [0, 0.49999999999999983, 0.8660254037844388], "zDir": [5.302876193624534e-17, 0.8660254037844387, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 343);
        cqPolygon(c, st, 343, 342, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 344);
        cqExtrude(c, st, 344, 343, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 344, "extrude");
        atStep(st, 345);
        cqUnion(c, st, 345, 341, {"toUnion": {"wp": 344}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 345, "union");
        atStep(st, 346);
        cqWorkplaneNew(c, st, 346, {"plane": {"origin": [-2.982722166320326, 7.20092830672139, -4.499999999999998], "xDir": [0, 0.5299498049346547, 0.8480290114434302], "zDir": [-0.33141357403559174, 0.8001031451912656, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 347);
        cqPolygon(c, st, 347, 346, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 348);
        cqExtrude(c, st, 348, 347, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 348, "extrude");
        atStep(st, 349);
        cqUnion(c, st, 349, 345, {"toUnion": {"wp": 348}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 349, "union");
        atStep(st, 350);
        cqWorkplaneNew(c, st, 350, {"plane": {"origin": [-5.5113519212621505, 5.511351921262151, -4.499999999999998], "xDir": [0.7071067811865476, 0.7071067811865474, -2.775557561562892e-17], "zDir": [-0.6123724356957945, 0.6123724356957946, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 351);
        cqPolygon(c, st, 351, 350, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 352);
        cqExtrude(c, st, 352, 351, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 352, "extrude");
        atStep(st, 353);
        cqUnion(c, st, 353, 349, {"toUnion": {"wp": 352}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 353, "union");
        atStep(st, 354);
        cqWorkplaneNew(c, st, 354, {"plane": {"origin": [-7.20092830672139, 2.982722166320327, -4.499999999999998], "xDir": [0.5299498049346547, -2.7755575615628914e-17, -0.8480290114434302], "zDir": [-0.8001031451912656, 0.3314135740355919, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 355);
        cqPolygon(c, st, 355, 354, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 356);
        cqExtrude(c, st, 356, 355, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 356, "extrude");
        atStep(st, 357);
        cqUnion(c, st, 357, 353, {"toUnion": {"wp": 356}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 357, "union");
        atStep(st, 358);
        cqWorkplaneNew(c, st, 358, {"plane": {"origin": [-7.794228634059948, 9.54517714852416e-16, -4.499999999999998], "xDir": [0.49999999999999983, 0, -0.8660254037844388], "zDir": [-0.8660254037844387, 1.0605752387249068e-16, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 359);
        cqPolygon(c, st, 359, 358, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 360);
        cqExtrude(c, st, 360, 359, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 360, "extrude");
        atStep(st, 361);
        cqUnion(c, st, 361, 357, {"toUnion": {"wp": 360}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 361, "union");
        atStep(st, 362);
        cqWorkplaneNew(c, st, 362, {"plane": {"origin": [-7.200928306721391, -2.9827221663203254, -4.499999999999998], "xDir": [0.5299498049346546, 0, -0.8480290114434301], "zDir": [-0.8001031451912657, -0.3314135740355917, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 363);
        cqPolygon(c, st, 363, 362, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 364);
        cqExtrude(c, st, 364, 363, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 364, "extrude");
        atStep(st, 365);
        cqUnion(c, st, 365, 361, {"toUnion": {"wp": 364}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 365, "union");
        atStep(st, 366);
        cqWorkplaneNew(c, st, 366, {"plane": {"origin": [-5.511351921262152, -5.5113519212621505, -4.499999999999998], "xDir": [0.7071067811865475, -0.7071067811865477, 0], "zDir": [-0.6123724356957947, -0.6123724356957945, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 367);
        cqPolygon(c, st, 367, 366, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 368);
        cqExtrude(c, st, 368, 367, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 368, "extrude");
        atStep(st, 369);
        cqUnion(c, st, 369, 365, {"toUnion": {"wp": 368}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 369, "union");
        atStep(st, 370);
        cqWorkplaneNew(c, st, 370, {"plane": {"origin": [-2.9827221663203307, -7.200928306721388, -4.499999999999998], "xDir": [0, 0.5299498049346548, -0.8480290114434301], "zDir": [-0.3314135740355923, -0.8001031451912654, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 371);
        cqPolygon(c, st, 371, 370, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 372);
        cqExtrude(c, st, 372, 371, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 372, "extrude");
        atStep(st, 373);
        cqUnion(c, st, 373, 369, {"toUnion": {"wp": 372}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 373, "union");
        atStep(st, 374);
        cqWorkplaneNew(c, st, 374, {"plane": {"origin": [-1.4317765722786245e-15, -7.794228634059948, -4.499999999999998], "xDir": [1.2325951644078312e-32, 0.49999999999999983, -0.8660254037844388], "zDir": [-1.59086285808736e-16, -0.8660254037844387, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 375);
        cqPolygon(c, st, 375, 374, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 376);
        cqExtrude(c, st, 376, 375, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 376, "extrude");
        atStep(st, 377);
        cqUnion(c, st, 377, 373, {"toUnion": {"wp": 376}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 377, "union");
        atStep(st, 378);
        cqWorkplaneNew(c, st, 378, {"plane": {"origin": [2.982722166320328, -7.200928306721389, -4.499999999999998], "xDir": [0, 0.5299498049346548, -0.8480290114434301], "zDir": [0.331413574035592, -0.8001031451912655, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 379);
        cqPolygon(c, st, 379, 378, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 380);
        cqExtrude(c, st, 380, 379, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 380, "extrude");
        atStep(st, 381);
        cqUnion(c, st, 381, 377, {"toUnion": {"wp": 380}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 381, "union");
        atStep(st, 382);
        cqWorkplaneNew(c, st, 382, {"plane": {"origin": [5.51135192126215, -5.511351921262152, -4.499999999999998], "xDir": [-0.7071067811865477, -0.7071067811865472, -2.775557561562892e-17], "zDir": [0.6123724356957944, -0.6123724356957947, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 383);
        cqPolygon(c, st, 383, 382, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 384);
        cqExtrude(c, st, 384, 383, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 384, "extrude");
        atStep(st, 385);
        cqUnion(c, st, 385, 381, {"toUnion": {"wp": 384}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 385, "union");
        atStep(st, 386);
        cqWorkplaneNew(c, st, 386, {"plane": {"origin": [7.200928306721388, -2.982722166320331, -4.499999999999998], "xDir": [0.5299498049346548, 0, 0.8480290114434301], "zDir": [0.8001031451912654, -0.33141357403559235, -0.4999999999999998]}, "obj": undefined});
        atStep(st, 387);
        cqPolygon(c, st, 387, 386, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 388);
        cqExtrude(c, st, 388, 387, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 388, "extrude");
        atStep(st, 389);
        cqUnion(c, st, 389, 385, {"toUnion": {"wp": 388}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 389, "union");
        atStep(st, 390);
        cqWorkplaneNew(c, st, 390, {"plane": {"origin": [5.673929634055214, 1.1286147758555192, -6.894399988070801], "xDir": [-0.7721396696956376, 0, -0.635452854649589], "zDir": [0.6304366260061349, 0.12540164176172436, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 391);
        cqPolygon(c, st, 391, 390, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 392);
        cqExtrude(c, st, 392, 391, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 392, "extrude");
        atStep(st, 393);
        cqUnion(c, st, 393, 389, {"toUnion": {"wp": 392}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 393, "union");
        atStep(st, 394);
        cqWorkplaneNew(c, st, 394, {"plane": {"origin": [4.810125281570521, 3.2140229588609746, -6.894399988070801], "xDir": [-0.8201225567990689, -2.7755575615628914e-17, -0.5721878990588304], "zDir": [0.5344583646189468, 0.35711366209566386, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 395);
        cqPolygon(c, st, 395, 394, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 396);
        cqExtrude(c, st, 396, 395, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 396, "extrude");
        atStep(st, 397);
        cqUnion(c, st, 397, 393, {"toUnion": {"wp": 396}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 397, "union");
        atStep(st, 398);
        cqWorkplaneNew(c, st, 398, {"plane": {"origin": [3.214022958860975, 4.810125281570521, -6.894399988070801], "xDir": [-2.7755575615628907e-17, -0.8201225567990688, -0.5721878990588304], "zDir": [0.3571136620956639, 0.5344583646189468, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 399);
        cqPolygon(c, st, 399, 398, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 400);
        cqExtrude(c, st, 400, 399, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 400, "extrude");
        atStep(st, 401);
        cqUnion(c, st, 401, 397, {"toUnion": {"wp": 400}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 401, "union");
        atStep(st, 402);
        cqWorkplaneNew(c, st, 402, {"plane": {"origin": [1.1286147758555196, 5.673929634055214, -6.894399988070801], "xDir": [6.938893903907228e-18, -0.7721396696956376, -0.635452854649589], "zDir": [0.1254016417617244, 0.6304366260061349, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 403);
        cqPolygon(c, st, 403, 402, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 404);
        cqExtrude(c, st, 404, 403, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 404, "extrude");
        atStep(st, 405);
        cqUnion(c, st, 405, 401, {"toUnion": {"wp": 404}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 405, "union");
        atStep(st, 406);
        cqWorkplaneNew(c, st, 406, {"plane": {"origin": [-1.1286147758555187, 5.673929634055214, -6.894399988070801], "xDir": [-1.387778780781446e-17, -0.7721396696956376, -0.6354528546495891], "zDir": [-0.1254016417617243, 0.6304366260061349, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 407);
        cqPolygon(c, st, 407, 406, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 408);
        cqExtrude(c, st, 408, 407, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 408, "extrude");
        atStep(st, 409);
        cqUnion(c, st, 409, 405, {"toUnion": {"wp": 408}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 409, "union");
        atStep(st, 410);
        cqWorkplaneNew(c, st, 410, {"plane": {"origin": [-3.2140229588609732, 4.810125281570522, -6.894399988070801], "xDir": [0, -0.8201225567990686, -0.5721878990588304], "zDir": [-0.3571136620956637, 0.5344583646189469, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 411);
        cqPolygon(c, st, 411, 410, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 412);
        cqExtrude(c, st, 412, 411, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 412, "extrude");
        atStep(st, 413);
        cqUnion(c, st, 413, 409, {"toUnion": {"wp": 412}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 413, "union");
        atStep(st, 414);
        cqWorkplaneNew(c, st, 414, {"plane": {"origin": [-4.810125281570522, 3.2140229588609746, -6.894399988070801], "xDir": [-0.8201225567990686, 2.7755575615628907e-17, 0.5721878990588305], "zDir": [-0.5344583646189469, 0.35711366209566386, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 415);
        cqPolygon(c, st, 415, 414, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 416);
        cqExtrude(c, st, 416, 415, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 416, "extrude");
        atStep(st, 417);
        cqUnion(c, st, 417, 413, {"toUnion": {"wp": 416}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 417, "union");
        atStep(st, 418);
        cqWorkplaneNew(c, st, 418, {"plane": {"origin": [-5.673929634055214, 1.1286147758555212, -6.894399988070801], "xDir": [-0.7721396696956376, -6.938893903907228e-18, 0.635452854649589], "zDir": [-0.6304366260061349, 0.12540164176172458, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 419);
        cqPolygon(c, st, 419, 418, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 420);
        cqExtrude(c, st, 420, 419, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 420, "extrude");
        atStep(st, 421);
        cqUnion(c, st, 421, 417, {"toUnion": {"wp": 420}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 421, "union");
        atStep(st, 422);
        cqWorkplaneNew(c, st, 422, {"plane": {"origin": [-5.673929634055214, -1.1286147758555198, -6.894399988070801], "xDir": [-0.7721396696956376, 6.938893903907228e-18, 0.635452854649589], "zDir": [-0.6304366260061349, -0.1254016417617244, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 423);
        cqPolygon(c, st, 423, 422, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 424);
        cqExtrude(c, st, 424, 423, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 424, "extrude");
        atStep(st, 425);
        cqUnion(c, st, 425, 421, {"toUnion": {"wp": 424}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 425, "union");
        atStep(st, 426);
        cqWorkplaneNew(c, st, 426, {"plane": {"origin": [-4.810125281570522, -3.2140229588609732, -6.894399988070801], "xDir": [-0.8201225567990686, 0, 0.5721878990588304], "zDir": [-0.5344583646189469, -0.3571136620956637, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 427);
        cqPolygon(c, st, 427, 426, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 428);
        cqExtrude(c, st, 428, 427, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 428, "extrude");
        atStep(st, 429);
        cqUnion(c, st, 429, 425, {"toUnion": {"wp": 428}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 429, "union");
        atStep(st, 430);
        cqWorkplaneNew(c, st, 430, {"plane": {"origin": [-3.2140229588609746, -4.810125281570521, -6.894399988070801], "xDir": [-2.7755575615628914e-17, -0.8201225567990689, 0.5721878990588304], "zDir": [-0.35711366209566386, -0.5344583646189468, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 431);
        cqPolygon(c, st, 431, 430, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 432);
        cqExtrude(c, st, 432, 431, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 432, "extrude");
        atStep(st, 433);
        cqUnion(c, st, 433, 429, {"toUnion": {"wp": 432}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 433, "union");
        atStep(st, 434);
        cqWorkplaneNew(c, st, 434, {"plane": {"origin": [-1.1286147758555216, -5.673929634055213, -6.894399988070801], "xDir": [-1.3877787807814457e-17, -0.7721396696956376, 0.6354528546495889], "zDir": [-0.12540164176172464, -0.6304366260061348, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 435);
        cqPolygon(c, st, 435, 434, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 436);
        cqExtrude(c, st, 436, 435, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 436, "extrude");
        atStep(st, 437);
        cqUnion(c, st, 437, 433, {"toUnion": {"wp": 436}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 437, "union");
        atStep(st, 438);
        cqWorkplaneNew(c, st, 438, {"plane": {"origin": [1.1286147758555194, -5.673929634055214, -6.894399988070801], "xDir": [-6.938893903907228e-18, -0.7721396696956376, 0.635452854649589], "zDir": [0.1254016417617244, -0.6304366260061349, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 439);
        cqPolygon(c, st, 439, 438, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 440);
        cqExtrude(c, st, 440, 439, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 440, "extrude");
        atStep(st, 441);
        cqUnion(c, st, 441, 437, {"toUnion": {"wp": 440}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 441, "union");
        atStep(st, 442);
        cqWorkplaneNew(c, st, 442, {"plane": {"origin": [3.214022958860977, -4.81012528157052, -6.894399988070801], "xDir": [0, -0.8201225567990688, 0.5721878990588303], "zDir": [0.35711366209566414, -0.5344583646189467, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 443);
        cqPolygon(c, st, 443, 442, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 444);
        cqExtrude(c, st, 444, 443, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 444, "extrude");
        atStep(st, 445);
        cqUnion(c, st, 445, 441, {"toUnion": {"wp": 444}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 445, "union");
        atStep(st, 446);
        cqWorkplaneNew(c, st, 446, {"plane": {"origin": [4.810125281570521, -3.2140229588609746, -6.894399988070801], "xDir": [-0.8201225567990689, 2.7755575615628914e-17, -0.5721878990588304], "zDir": [0.5344583646189468, -0.35711366209566386, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 447);
        cqPolygon(c, st, 447, 446, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 448);
        cqExtrude(c, st, 448, 447, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 448, "extrude");
        atStep(st, 449);
        cqUnion(c, st, 449, 445, {"toUnion": {"wp": 448}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 449, "union");
        atStep(st, 450);
        cqWorkplaneNew(c, st, 450, {"plane": {"origin": [5.673929634055213, -1.1286147758555218, -6.894399988070801], "xDir": [-0.7721396696956376, 1.3877787807814457e-17, -0.6354528546495889], "zDir": [0.6304366260061348, -0.12540164176172464, -0.7660444431189779]}, "obj": undefined});
        atStep(st, 451);
        cqPolygon(c, st, 451, 450, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 452);
        cqExtrude(c, st, 452, 451, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 452, "extrude");
        atStep(st, 453);
        cqUnion(c, st, 453, 449, {"toUnion": {"wp": 452}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 453, "union");
        atStep(st, 454);
        cqWorkplaneNew(c, st, 454, {"plane": {"origin": [3.07818128993102, 0, -8.457233587073175], "xDir": [-0.9396926207859084, 0, -0.34202014332566893], "zDir": [0.3420201433256689, 0, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 455);
        cqPolygon(c, st, 455, 454, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 456);
        cqExtrude(c, st, 456, 455, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 456, "extrude");
        atStep(st, 457);
        cqUnion(c, st, 457, 453, {"toUnion": {"wp": 456}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 457, "union");
        atStep(st, 458);
        cqWorkplaneNew(c, st, 458, {"plane": {"origin": [2.84386869112646, 1.1779689814728025, -8.457233587073175], "xDir": [-0.94784647102561, 0, -0.3187272617212048], "zDir": [0.31598541012516224, 0.1308854423858669, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 459);
        cqPolygon(c, st, 459, 458, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 460);
        cqExtrude(c, st, 460, 459, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 460, "extrude");
        atStep(st, 461);
        cqUnion(c, st, 461, 457, {"toUnion": {"wp": 460}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 461, "union");
        atStep(st, 462);
        cqWorkplaneNew(c, st, 462, {"plane": {"origin": [2.1766028638317785, 2.176602863831778, -8.457233587073175], "xDir": [-0.9684408309910916, 0, -0.2492435693639543], "zDir": [0.2418447626479754, 0.24184476264797533, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 463);
        cqPolygon(c, st, 463, 462, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 464);
        cqExtrude(c, st, 464, 463, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 464, "extrude");
        atStep(st, 465);
        cqUnion(c, st, 465, 461, {"toUnion": {"wp": 464}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 465, "union");
        atStep(st, 466);
        cqWorkplaneNew(c, st, 466, {"plane": {"origin": [1.1779689814728025, 2.84386869112646, -8.457233587073175], "xDir": [-6.938893903907228e-18, -0.94784647102561, -0.3187272617212048], "zDir": [0.13088544238586694, 0.31598541012516224, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 467);
        cqPolygon(c, st, 467, 466, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 468);
        cqExtrude(c, st, 468, 467, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 468, "extrude");
        atStep(st, 469);
        cqUnion(c, st, 469, 465, {"toUnion": {"wp": 468}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 469, "union");
        atStep(st, 470);
        cqWorkplaneNew(c, st, 470, {"plane": {"origin": [1.8848424319546471e-16, 3.07818128993102, -8.457233587073175], "xDir": [0, -0.9396926207859084, -0.34202014332566893], "zDir": [2.0942693688384968e-17, 0.3420201433256689, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 471);
        cqPolygon(c, st, 471, 470, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 472);
        cqExtrude(c, st, 472, 471, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 472, "extrude");
        atStep(st, 473);
        cqUnion(c, st, 473, 469, {"toUnion": {"wp": 472}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 473, "union");
        atStep(st, 474);
        cqWorkplaneNew(c, st, 474, {"plane": {"origin": [-1.177968981472802, 2.84386869112646, -8.457233587073175], "xDir": [0, -0.94784647102561, -0.3187272617212048], "zDir": [-0.13088544238586688, 0.31598541012516224, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 475);
        cqPolygon(c, st, 475, 474, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 476);
        cqExtrude(c, st, 476, 475, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 476, "extrude");
        atStep(st, 477);
        cqUnion(c, st, 477, 473, {"toUnion": {"wp": 476}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 477, "union");
        atStep(st, 478);
        cqWorkplaneNew(c, st, 478, {"plane": {"origin": [-2.176602863831778, 2.1766028638317785, -8.457233587073175], "xDir": [0, -0.9684408309910916, -0.2492435693639543], "zDir": [-0.24184476264797533, 0.2418447626479754, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 479);
        cqPolygon(c, st, 479, 478, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 480);
        cqExtrude(c, st, 480, 479, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 480, "extrude");
        atStep(st, 481);
        cqUnion(c, st, 481, 477, {"toUnion": {"wp": 480}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 481, "union");
        atStep(st, 482);
        cqWorkplaneNew(c, st, 482, {"plane": {"origin": [-2.84386869112646, 1.1779689814728027, -8.457233587073175], "xDir": [-0.94784647102561, 6.938893903907228e-18, 0.3187272617212048], "zDir": [-0.31598541012516224, 0.13088544238586697, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 483);
        cqPolygon(c, st, 483, 482, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 484);
        cqExtrude(c, st, 484, 483, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 484, "extrude");
        atStep(st, 485);
        cqUnion(c, st, 485, 481, {"toUnion": {"wp": 484}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 485, "union");
        atStep(st, 486);
        cqWorkplaneNew(c, st, 486, {"plane": {"origin": [-3.07818128993102, 3.7696848639092943e-16, -8.457233587073175], "xDir": [-0.9396926207859084, 0, 0.34202014332566893], "zDir": [-0.3420201433256689, 4.1885387376769936e-17, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 487);
        cqPolygon(c, st, 487, 486, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 488);
        cqExtrude(c, st, 488, 487, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 488, "extrude");
        atStep(st, 489);
        cqUnion(c, st, 489, 485, {"toUnion": {"wp": 488}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 489, "union");
        atStep(st, 490);
        cqWorkplaneNew(c, st, 490, {"plane": {"origin": [-2.843868691126461, -1.177968981472802, -8.457233587073175], "xDir": [-0.94784647102561, 0, 0.3187272617212049], "zDir": [-0.3159854101251623, -0.13088544238586688, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 491);
        cqPolygon(c, st, 491, 490, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 492);
        cqExtrude(c, st, 492, 491, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 492, "extrude");
        atStep(st, 493);
        cqUnion(c, st, 493, 489, {"toUnion": {"wp": 492}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 493, "union");
        atStep(st, 494);
        cqWorkplaneNew(c, st, 494, {"plane": {"origin": [-2.176602863831779, -2.176602863831778, -8.457233587073175], "xDir": [-0.9684408309910916, -6.938893903907228e-18, 0.24924356936395436], "zDir": [-0.24184476264797544, -0.24184476264797533, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 495);
        cqPolygon(c, st, 495, 494, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 496);
        cqExtrude(c, st, 496, 495, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 496, "extrude");
        atStep(st, 497);
        cqUnion(c, st, 497, 493, {"toUnion": {"wp": 496}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 497, "union");
        atStep(st, 498);
        cqWorkplaneNew(c, st, 498, {"plane": {"origin": [-1.177968981472804, -2.8438686911264597, -8.457233587073175], "xDir": [0, -0.94784647102561, 0.31872726172120475], "zDir": [-0.1308854423858671, -0.3159854101251622, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 499);
        cqPolygon(c, st, 499, 498, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 500);
        cqExtrude(c, st, 500, 499, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 500, "extrude");
        atStep(st, 501);
        cqUnion(c, st, 501, 497, {"toUnion": {"wp": 500}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 501, "union");
        atStep(st, 502);
        cqWorkplaneNew(c, st, 502, {"plane": {"origin": [-5.654527295863944e-16, -3.07818128993102, -8.457233587073175], "xDir": [0, -0.9396926207859084, 0.34202014332566893], "zDir": [-6.28280810651549e-17, -0.3420201433256689, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 503);
        cqPolygon(c, st, 503, 502, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 504);
        cqExtrude(c, st, 504, 503, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 504, "extrude");
        atStep(st, 505);
        cqUnion(c, st, 505, 501, {"toUnion": {"wp": 504}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 505, "union");
        atStep(st, 506);
        cqWorkplaneNew(c, st, 506, {"plane": {"origin": [1.177968981472803, -2.84386869112646, -8.457233587073175], "xDir": [-6.938893903907228e-18, -0.94784647102561, 0.3187272617212048], "zDir": [0.130885442385867, -0.31598541012516224, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 507);
        cqPolygon(c, st, 507, 506, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 508);
        cqExtrude(c, st, 508, 507, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 508, "extrude");
        atStep(st, 509);
        cqUnion(c, st, 509, 505, {"toUnion": {"wp": 508}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 509, "union");
        atStep(st, 510);
        cqWorkplaneNew(c, st, 510, {"plane": {"origin": [2.1766028638317776, -2.176602863831779, -8.457233587073175], "xDir": [0, -0.9684408309910916, 0.2492435693639544], "zDir": [0.24184476264797528, -0.24184476264797544, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 511);
        cqPolygon(c, st, 511, 510, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 512);
        cqExtrude(c, st, 512, 511, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 512, "extrude");
        atStep(st, 513);
        cqUnion(c, st, 513, 509, {"toUnion": {"wp": 512}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 513, "union");
        atStep(st, 514);
        cqWorkplaneNew(c, st, 514, {"plane": {"origin": [2.8438686911264597, -1.1779689814728045, -8.457233587073175], "xDir": [-0.94784647102561, 6.938893903907228e-18, -0.31872726172120475], "zDir": [0.3159854101251622, -0.1308854423858671, -0.9396926207859084]}, "obj": undefined});
        atStep(st, 515);
        cqPolygon(c, st, 515, 514, {"nSides": 6, "diameter": 2, "forConstruction": false, "circumscribed": false});
        atStep(st, 516);
        cqExtrude(c, st, 516, 515, {"until": -1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 516, "extrude");
        atStep(st, 517);
        cqUnion(c, st, 517, 513, {"toUnion": {"wp": 516}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 517, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 517}), {"i": 297});
    } catch (e) {
        return {"i": 297, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c297"] = case_297();
    return out;
}
