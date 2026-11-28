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

const case_4313 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqBox(c, st, 2, 1, {"length": 100, "width": 60, "height": 40, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "box");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqBox(c, st, 4, 3, {"length": 94, "width": 54, "height": 34, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 4, "box");
        atStep(st, 5);
        cqCut(c, st, 5, 2, {"toCut": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 5, "cut");
        atStep(st, 6);
        cqWorkplaneNew(c, st, 6, {"plane": {"origin": [-32, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 7);
        cqRect(c, st, 7, 6, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 8);
        cqExtrude(c, st, 8, 7, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 8, "extrude");
        // step 9: guard_begin {"before":{"wp":8},"block":1,"g":1,"op":"guard_begin","skippable":true}
        atStep(st, 10);
        cqSelect("Edges")(c, st, 9, 8, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 11);
        cqFillet(c, st, 10, 9, {"radius": 0.499999});
        stepVol(c, st, 10, "fillet");
        // step 12: guard_end {"after":{"wp":10},"block":1,"g":1,"op":"guard_end"}
        atStep(st, 13);
        cqUnion(c, st, 11, 5, {"toUnion": {"wp": 10}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 11, "union");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 12, {"plane": {"origin": [-24, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 15);
        cqRect(c, st, 13, 12, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 16);
        cqExtrude(c, st, 14, 13, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 14, "extrude");
        // step 17: guard_begin {"before":{"wp":14},"block":1,"g":2,"op":"guard_begin","skippable":true}
        atStep(st, 18);
        cqSelect("Edges")(c, st, 15, 14, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 19);
        cqFillet(c, st, 16, 15, {"radius": 0.499999});
        stepVol(c, st, 16, "fillet");
        // step 20: guard_end {"after":{"wp":16},"block":1,"g":2,"op":"guard_end"}
        atStep(st, 21);
        cqUnion(c, st, 17, 11, {"toUnion": {"wp": 16}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 17, "union");
        atStep(st, 22);
        cqWorkplaneNew(c, st, 18, {"plane": {"origin": [-16, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 23);
        cqRect(c, st, 19, 18, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 24);
        cqExtrude(c, st, 20, 19, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 20, "extrude");
        // step 25: guard_begin {"before":{"wp":20},"block":1,"g":3,"op":"guard_begin","skippable":true}
        atStep(st, 26);
        cqSelect("Edges")(c, st, 21, 20, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 27);
        cqFillet(c, st, 22, 21, {"radius": 0.499999});
        stepVol(c, st, 22, "fillet");
        // step 28: guard_end {"after":{"wp":22},"block":1,"g":3,"op":"guard_end"}
        atStep(st, 29);
        cqUnion(c, st, 23, 17, {"toUnion": {"wp": 22}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 23, "union");
        atStep(st, 30);
        cqWorkplaneNew(c, st, 24, {"plane": {"origin": [-8, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 31);
        cqRect(c, st, 25, 24, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 32);
        cqExtrude(c, st, 26, 25, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 26, "extrude");
        // step 33: guard_begin {"before":{"wp":26},"block":1,"g":4,"op":"guard_begin","skippable":true}
        atStep(st, 34);
        cqSelect("Edges")(c, st, 27, 26, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 35);
        cqFillet(c, st, 28, 27, {"radius": 0.499999});
        stepVol(c, st, 28, "fillet");
        // step 36: guard_end {"after":{"wp":28},"block":1,"g":4,"op":"guard_end"}
        atStep(st, 37);
        cqUnion(c, st, 29, 23, {"toUnion": {"wp": 28}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 29, "union");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 30, {"plane": {"origin": [0, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 39);
        cqRect(c, st, 31, 30, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 40);
        cqExtrude(c, st, 32, 31, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 32, "extrude");
        // step 41: guard_begin {"before":{"wp":32},"block":1,"g":5,"op":"guard_begin","skippable":true}
        atStep(st, 42);
        cqSelect("Edges")(c, st, 33, 32, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 43);
        cqFillet(c, st, 34, 33, {"radius": 0.499999});
        stepVol(c, st, 34, "fillet");
        // step 44: guard_end {"after":{"wp":34},"block":1,"g":5,"op":"guard_end"}
        atStep(st, 45);
        cqUnion(c, st, 35, 29, {"toUnion": {"wp": 34}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 35, "union");
        atStep(st, 46);
        cqWorkplaneNew(c, st, 36, {"plane": {"origin": [8, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 47);
        cqRect(c, st, 37, 36, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 48);
        cqExtrude(c, st, 38, 37, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 38, "extrude");
        // step 49: guard_begin {"before":{"wp":38},"block":1,"g":6,"op":"guard_begin","skippable":true}
        atStep(st, 50);
        cqSelect("Edges")(c, st, 39, 38, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 51);
        cqFillet(c, st, 40, 39, {"radius": 0.499999});
        stepVol(c, st, 40, "fillet");
        // step 52: guard_end {"after":{"wp":40},"block":1,"g":6,"op":"guard_end"}
        atStep(st, 53);
        cqUnion(c, st, 41, 35, {"toUnion": {"wp": 40}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 41, "union");
        atStep(st, 54);
        cqWorkplaneNew(c, st, 42, {"plane": {"origin": [16, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 55);
        cqRect(c, st, 43, 42, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 56);
        cqExtrude(c, st, 44, 43, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 44, "extrude");
        // step 57: guard_begin {"before":{"wp":44},"block":1,"g":7,"op":"guard_begin","skippable":true}
        atStep(st, 58);
        cqSelect("Edges")(c, st, 45, 44, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 59);
        cqFillet(c, st, 46, 45, {"radius": 0.499999});
        stepVol(c, st, 46, "fillet");
        // step 60: guard_end {"after":{"wp":46},"block":1,"g":7,"op":"guard_end"}
        atStep(st, 61);
        cqUnion(c, st, 47, 41, {"toUnion": {"wp": 46}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 47, "union");
        atStep(st, 62);
        cqWorkplaneNew(c, st, 48, {"plane": {"origin": [24, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 63);
        cqRect(c, st, 49, 48, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 64);
        cqExtrude(c, st, 50, 49, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 50, "extrude");
        // step 65: guard_begin {"before":{"wp":50},"block":1,"g":8,"op":"guard_begin","skippable":true}
        atStep(st, 66);
        cqSelect("Edges")(c, st, 51, 50, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 67);
        cqFillet(c, st, 52, 51, {"radius": 0.499999});
        stepVol(c, st, 52, "fillet");
        // step 68: guard_end {"after":{"wp":52},"block":1,"g":8,"op":"guard_end"}
        atStep(st, 69);
        cqUnion(c, st, 53, 47, {"toUnion": {"wp": 52}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 53, "union");
        atStep(st, 70);
        cqWorkplaneNew(c, st, 54, {"plane": {"origin": [32, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 71);
        cqRect(c, st, 55, 54, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 72);
        cqExtrude(c, st, 56, 55, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 56, "extrude");
        // step 73: guard_begin {"before":{"wp":56},"block":1,"g":9,"op":"guard_begin","skippable":true}
        atStep(st, 74);
        cqSelect("Edges")(c, st, 57, 56, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 75);
        cqFillet(c, st, 58, 57, {"radius": 0.499999});
        stepVol(c, st, 58, "fillet");
        // step 76: guard_end {"after":{"wp":58},"block":1,"g":9,"op":"guard_end"}
        atStep(st, 77);
        cqUnion(c, st, 59, 53, {"toUnion": {"wp": 58}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 59, "union");
        atStep(st, 78);
        cqWorkplaneNew(c, st, 60, {"plane": {"origin": [40, -20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 79);
        cqRect(c, st, 61, 60, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 80);
        cqExtrude(c, st, 62, 61, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 62, "extrude");
        // step 81: guard_begin {"before":{"wp":62},"block":1,"g":10,"op":"guard_begin","skippable":true}
        atStep(st, 82);
        cqSelect("Edges")(c, st, 63, 62, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 83);
        cqFillet(c, st, 64, 63, {"radius": 0.499999});
        stepVol(c, st, 64, "fillet");
        // step 84: guard_end {"after":{"wp":64},"block":1,"g":10,"op":"guard_end"}
        atStep(st, 85);
        cqUnion(c, st, 65, 59, {"toUnion": {"wp": 64}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 65, "union");
        atStep(st, 86);
        cqWorkplaneNew(c, st, 66, {"plane": {"origin": [-36, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 87);
        cqRect(c, st, 67, 66, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 88);
        cqExtrude(c, st, 68, 67, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 68, "extrude");
        // step 89: guard_begin {"before":{"wp":68},"block":1,"g":11,"op":"guard_begin","skippable":true}
        atStep(st, 90);
        cqSelect("Edges")(c, st, 69, 68, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 91);
        cqFillet(c, st, 70, 69, {"radius": 0.499999});
        stepVol(c, st, 70, "fillet");
        // step 92: guard_end {"after":{"wp":70},"block":1,"g":11,"op":"guard_end"}
        atStep(st, 93);
        cqUnion(c, st, 71, 65, {"toUnion": {"wp": 70}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 71, "union");
        atStep(st, 94);
        cqWorkplaneNew(c, st, 72, {"plane": {"origin": [-38, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 95);
        cqRect(c, st, 73, 72, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 96);
        cqExtrude(c, st, 74, 73, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 74, "extrude");
        // step 97: guard_begin {"before":{"wp":74},"block":1,"g":12,"op":"guard_begin","skippable":true}
        atStep(st, 98);
        cqSelect("Edges")(c, st, 75, 74, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 99);
        cqFillet(c, st, 76, 75, {"radius": 0.499999});
        stepVol(c, st, 76, "fillet");
        // step 100: guard_end {"after":{"wp":76},"block":1,"g":12,"op":"guard_end"}
        atStep(st, 101);
        cqUnion(c, st, 77, 71, {"toUnion": {"wp": 76}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 77, "union");
        atStep(st, 102);
        cqWorkplaneNew(c, st, 78, {"plane": {"origin": [-28, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 103);
        cqRect(c, st, 79, 78, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 104);
        cqExtrude(c, st, 80, 79, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 80, "extrude");
        // step 105: guard_begin {"before":{"wp":80},"block":1,"g":13,"op":"guard_begin","skippable":true}
        atStep(st, 106);
        cqSelect("Edges")(c, st, 81, 80, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 107);
        cqFillet(c, st, 82, 81, {"radius": 0.499999});
        stepVol(c, st, 82, "fillet");
        // step 108: guard_end {"after":{"wp":82},"block":1,"g":13,"op":"guard_end"}
        atStep(st, 109);
        cqUnion(c, st, 83, 77, {"toUnion": {"wp": 82}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 83, "union");
        atStep(st, 110);
        cqWorkplaneNew(c, st, 84, {"plane": {"origin": [-30, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 111);
        cqRect(c, st, 85, 84, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 112);
        cqExtrude(c, st, 86, 85, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 86, "extrude");
        // step 113: guard_begin {"before":{"wp":86},"block":1,"g":14,"op":"guard_begin","skippable":true}
        atStep(st, 114);
        cqSelect("Edges")(c, st, 87, 86, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 115);
        cqFillet(c, st, 88, 87, {"radius": 0.499999});
        stepVol(c, st, 88, "fillet");
        // step 116: guard_end {"after":{"wp":88},"block":1,"g":14,"op":"guard_end"}
        atStep(st, 117);
        cqUnion(c, st, 89, 83, {"toUnion": {"wp": 88}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 89, "union");
        atStep(st, 118);
        cqWorkplaneNew(c, st, 90, {"plane": {"origin": [-34, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 119);
        cqRect(c, st, 91, 90, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 120);
        cqExtrude(c, st, 92, 91, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 92, "extrude");
        // step 121: guard_begin {"before":{"wp":92},"block":1,"g":15,"op":"guard_begin","skippable":true}
        atStep(st, 122);
        cqSelect("Edges")(c, st, 93, 92, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 123);
        cqFillet(c, st, 94, 93, {"radius": 0.499999});
        stepVol(c, st, 94, "fillet");
        // step 124: guard_end {"after":{"wp":94},"block":1,"g":15,"op":"guard_end"}
        atStep(st, 125);
        cqUnion(c, st, 95, 89, {"toUnion": {"wp": 94}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 95, "union");
        atStep(st, 126);
        cqWorkplaneNew(c, st, 96, {"plane": {"origin": [-20, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 127);
        cqRect(c, st, 97, 96, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 128);
        cqExtrude(c, st, 98, 97, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 98, "extrude");
        // step 129: guard_begin {"before":{"wp":98},"block":1,"g":16,"op":"guard_begin","skippable":true}
        atStep(st, 130);
        cqSelect("Edges")(c, st, 99, 98, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 131);
        cqFillet(c, st, 100, 99, {"radius": 0.499999});
        stepVol(c, st, 100, "fillet");
        // step 132: guard_end {"after":{"wp":100},"block":1,"g":16,"op":"guard_end"}
        atStep(st, 133);
        cqUnion(c, st, 101, 95, {"toUnion": {"wp": 100}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 101, "union");
        atStep(st, 134);
        cqWorkplaneNew(c, st, 102, {"plane": {"origin": [-22, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 135);
        cqRect(c, st, 103, 102, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 136);
        cqExtrude(c, st, 104, 103, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 104, "extrude");
        // step 137: guard_begin {"before":{"wp":104},"block":1,"g":17,"op":"guard_begin","skippable":true}
        atStep(st, 138);
        cqSelect("Edges")(c, st, 105, 104, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 139);
        cqFillet(c, st, 106, 105, {"radius": 0.499999});
        stepVol(c, st, 106, "fillet");
        // step 140: guard_end {"after":{"wp":106},"block":1,"g":17,"op":"guard_end"}
        atStep(st, 141);
        cqUnion(c, st, 107, 101, {"toUnion": {"wp": 106}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 107, "union");
        atStep(st, 142);
        cqWorkplaneNew(c, st, 108, {"plane": {"origin": [-26, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 143);
        cqRect(c, st, 109, 108, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 144);
        cqExtrude(c, st, 110, 109, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 110, "extrude");
        // step 145: guard_begin {"before":{"wp":110},"block":1,"g":18,"op":"guard_begin","skippable":true}
        atStep(st, 146);
        cqSelect("Edges")(c, st, 111, 110, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 147);
        cqFillet(c, st, 112, 111, {"radius": 0.499999});
        stepVol(c, st, 112, "fillet");
        // step 148: guard_end {"after":{"wp":112},"block":1,"g":18,"op":"guard_end"}
        atStep(st, 149);
        cqUnion(c, st, 113, 107, {"toUnion": {"wp": 112}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 113, "union");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 114, {"plane": {"origin": [-12, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 151);
        cqRect(c, st, 115, 114, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 152);
        cqExtrude(c, st, 116, 115, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 116, "extrude");
        // step 153: guard_begin {"before":{"wp":116},"block":1,"g":19,"op":"guard_begin","skippable":true}
        atStep(st, 154);
        cqSelect("Edges")(c, st, 117, 116, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 155);
        cqFillet(c, st, 118, 117, {"radius": 0.499999});
        stepVol(c, st, 118, "fillet");
        // step 156: guard_end {"after":{"wp":118},"block":1,"g":19,"op":"guard_end"}
        atStep(st, 157);
        cqUnion(c, st, 119, 113, {"toUnion": {"wp": 118}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 119, "union");
        atStep(st, 158);
        cqWorkplaneNew(c, st, 120, {"plane": {"origin": [-14, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 159);
        cqRect(c, st, 121, 120, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 160);
        cqExtrude(c, st, 122, 121, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 122, "extrude");
        // step 161: guard_begin {"before":{"wp":122},"block":1,"g":20,"op":"guard_begin","skippable":true}
        atStep(st, 162);
        cqSelect("Edges")(c, st, 123, 122, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 163);
        cqFillet(c, st, 124, 123, {"radius": 0.499999});
        stepVol(c, st, 124, "fillet");
        // step 164: guard_end {"after":{"wp":124},"block":1,"g":20,"op":"guard_end"}
        atStep(st, 165);
        cqUnion(c, st, 125, 119, {"toUnion": {"wp": 124}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 125, "union");
        atStep(st, 166);
        cqWorkplaneNew(c, st, 126, {"plane": {"origin": [-18, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 167);
        cqRect(c, st, 127, 126, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 168);
        cqExtrude(c, st, 128, 127, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 128, "extrude");
        // step 169: guard_begin {"before":{"wp":128},"block":1,"g":21,"op":"guard_begin","skippable":true}
        atStep(st, 170);
        cqSelect("Edges")(c, st, 129, 128, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 171);
        cqFillet(c, st, 130, 129, {"radius": 0.499999});
        stepVol(c, st, 130, "fillet");
        // step 172: guard_end {"after":{"wp":130},"block":1,"g":21,"op":"guard_end"}
        atStep(st, 173);
        cqUnion(c, st, 131, 125, {"toUnion": {"wp": 130}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 131, "union");
        atStep(st, 174);
        cqWorkplaneNew(c, st, 132, {"plane": {"origin": [-4, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 175);
        cqRect(c, st, 133, 132, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 176);
        cqExtrude(c, st, 134, 133, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 134, "extrude");
        // step 177: guard_begin {"before":{"wp":134},"block":1,"g":22,"op":"guard_begin","skippable":true}
        atStep(st, 178);
        cqSelect("Edges")(c, st, 135, 134, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 179);
        cqFillet(c, st, 136, 135, {"radius": 0.499999});
        stepVol(c, st, 136, "fillet");
        // step 180: guard_end {"after":{"wp":136},"block":1,"g":22,"op":"guard_end"}
        atStep(st, 181);
        cqUnion(c, st, 137, 131, {"toUnion": {"wp": 136}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 137, "union");
        atStep(st, 182);
        cqWorkplaneNew(c, st, 138, {"plane": {"origin": [-6, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 183);
        cqRect(c, st, 139, 138, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 184);
        cqExtrude(c, st, 140, 139, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 140, "extrude");
        // step 185: guard_begin {"before":{"wp":140},"block":1,"g":23,"op":"guard_begin","skippable":true}
        atStep(st, 186);
        cqSelect("Edges")(c, st, 141, 140, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 187);
        cqFillet(c, st, 142, 141, {"radius": 0.499999});
        stepVol(c, st, 142, "fillet");
        // step 188: guard_end {"after":{"wp":142},"block":1,"g":23,"op":"guard_end"}
        atStep(st, 189);
        cqUnion(c, st, 143, 137, {"toUnion": {"wp": 142}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 143, "union");
        atStep(st, 190);
        cqWorkplaneNew(c, st, 144, {"plane": {"origin": [-10, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 191);
        cqRect(c, st, 145, 144, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 192);
        cqExtrude(c, st, 146, 145, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 146, "extrude");
        // step 193: guard_begin {"before":{"wp":146},"block":1,"g":24,"op":"guard_begin","skippable":true}
        atStep(st, 194);
        cqSelect("Edges")(c, st, 147, 146, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 195);
        cqFillet(c, st, 148, 147, {"radius": 0.499999});
        stepVol(c, st, 148, "fillet");
        // step 196: guard_end {"after":{"wp":148},"block":1,"g":24,"op":"guard_end"}
        atStep(st, 197);
        cqUnion(c, st, 149, 143, {"toUnion": {"wp": 148}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 149, "union");
        atStep(st, 198);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [4, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 199);
        cqRect(c, st, 151, 150, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 200);
        cqExtrude(c, st, 152, 151, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 152, "extrude");
        // step 201: guard_begin {"before":{"wp":152},"block":1,"g":25,"op":"guard_begin","skippable":true}
        atStep(st, 202);
        cqSelect("Edges")(c, st, 153, 152, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 203);
        cqFillet(c, st, 154, 153, {"radius": 0.499999});
        stepVol(c, st, 154, "fillet");
        // step 204: guard_end {"after":{"wp":154},"block":1,"g":25,"op":"guard_end"}
        atStep(st, 205);
        cqUnion(c, st, 155, 149, {"toUnion": {"wp": 154}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 155, "union");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 156, {"plane": {"origin": [2, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 207);
        cqRect(c, st, 157, 156, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 208);
        cqExtrude(c, st, 158, 157, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 158, "extrude");
        // step 209: guard_begin {"before":{"wp":158},"block":1,"g":26,"op":"guard_begin","skippable":true}
        atStep(st, 210);
        cqSelect("Edges")(c, st, 159, 158, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 211);
        cqFillet(c, st, 160, 159, {"radius": 0.499999});
        stepVol(c, st, 160, "fillet");
        // step 212: guard_end {"after":{"wp":160},"block":1,"g":26,"op":"guard_end"}
        atStep(st, 213);
        cqUnion(c, st, 161, 155, {"toUnion": {"wp": 160}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 161, "union");
        atStep(st, 214);
        cqWorkplaneNew(c, st, 162, {"plane": {"origin": [-2, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 215);
        cqRect(c, st, 163, 162, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 216);
        cqExtrude(c, st, 164, 163, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 164, "extrude");
        // step 217: guard_begin {"before":{"wp":164},"block":1,"g":27,"op":"guard_begin","skippable":true}
        atStep(st, 218);
        cqSelect("Edges")(c, st, 165, 164, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 219);
        cqFillet(c, st, 166, 165, {"radius": 0.499999});
        stepVol(c, st, 166, "fillet");
        // step 220: guard_end {"after":{"wp":166},"block":1,"g":27,"op":"guard_end"}
        atStep(st, 221);
        cqUnion(c, st, 167, 161, {"toUnion": {"wp": 166}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 167, "union");
        atStep(st, 222);
        cqWorkplaneNew(c, st, 168, {"plane": {"origin": [12, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 223);
        cqRect(c, st, 169, 168, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 224);
        cqExtrude(c, st, 170, 169, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 170, "extrude");
        // step 225: guard_begin {"before":{"wp":170},"block":1,"g":28,"op":"guard_begin","skippable":true}
        atStep(st, 226);
        cqSelect("Edges")(c, st, 171, 170, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 227);
        cqFillet(c, st, 172, 171, {"radius": 0.499999});
        stepVol(c, st, 172, "fillet");
        // step 228: guard_end {"after":{"wp":172},"block":1,"g":28,"op":"guard_end"}
        atStep(st, 229);
        cqUnion(c, st, 173, 167, {"toUnion": {"wp": 172}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 173, "union");
        atStep(st, 230);
        cqWorkplaneNew(c, st, 174, {"plane": {"origin": [10, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 231);
        cqRect(c, st, 175, 174, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 232);
        cqExtrude(c, st, 176, 175, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 176, "extrude");
        // step 233: guard_begin {"before":{"wp":176},"block":1,"g":29,"op":"guard_begin","skippable":true}
        atStep(st, 234);
        cqSelect("Edges")(c, st, 177, 176, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 235);
        cqFillet(c, st, 178, 177, {"radius": 0.499999});
        stepVol(c, st, 178, "fillet");
        // step 236: guard_end {"after":{"wp":178},"block":1,"g":29,"op":"guard_end"}
        atStep(st, 237);
        cqUnion(c, st, 179, 173, {"toUnion": {"wp": 178}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 179, "union");
        atStep(st, 238);
        cqWorkplaneNew(c, st, 180, {"plane": {"origin": [6, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 239);
        cqRect(c, st, 181, 180, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 240);
        cqExtrude(c, st, 182, 181, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 182, "extrude");
        // step 241: guard_begin {"before":{"wp":182},"block":1,"g":30,"op":"guard_begin","skippable":true}
        atStep(st, 242);
        cqSelect("Edges")(c, st, 183, 182, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 243);
        cqFillet(c, st, 184, 183, {"radius": 0.499999});
        stepVol(c, st, 184, "fillet");
        // step 244: guard_end {"after":{"wp":184},"block":1,"g":30,"op":"guard_end"}
        atStep(st, 245);
        cqUnion(c, st, 185, 179, {"toUnion": {"wp": 184}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 185, "union");
        atStep(st, 246);
        cqWorkplaneNew(c, st, 186, {"plane": {"origin": [20, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 247);
        cqRect(c, st, 187, 186, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 248);
        cqExtrude(c, st, 188, 187, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 188, "extrude");
        // step 249: guard_begin {"before":{"wp":188},"block":1,"g":31,"op":"guard_begin","skippable":true}
        atStep(st, 250);
        cqSelect("Edges")(c, st, 189, 188, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 251);
        cqFillet(c, st, 190, 189, {"radius": 0.499999});
        stepVol(c, st, 190, "fillet");
        // step 252: guard_end {"after":{"wp":190},"block":1,"g":31,"op":"guard_end"}
        atStep(st, 253);
        cqUnion(c, st, 191, 185, {"toUnion": {"wp": 190}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 191, "union");
        atStep(st, 254);
        cqWorkplaneNew(c, st, 192, {"plane": {"origin": [18, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 255);
        cqRect(c, st, 193, 192, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 256);
        cqExtrude(c, st, 194, 193, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 194, "extrude");
        // step 257: guard_begin {"before":{"wp":194},"block":1,"g":32,"op":"guard_begin","skippable":true}
        atStep(st, 258);
        cqSelect("Edges")(c, st, 195, 194, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 259);
        cqFillet(c, st, 196, 195, {"radius": 0.499999});
        stepVol(c, st, 196, "fillet");
        // step 260: guard_end {"after":{"wp":196},"block":1,"g":32,"op":"guard_end"}
        atStep(st, 261);
        cqUnion(c, st, 197, 191, {"toUnion": {"wp": 196}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 197, "union");
        atStep(st, 262);
        cqWorkplaneNew(c, st, 198, {"plane": {"origin": [14, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 263);
        cqRect(c, st, 199, 198, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 264);
        cqExtrude(c, st, 200, 199, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 200, "extrude");
        // step 265: guard_begin {"before":{"wp":200},"block":1,"g":33,"op":"guard_begin","skippable":true}
        atStep(st, 266);
        cqSelect("Edges")(c, st, 201, 200, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 267);
        cqFillet(c, st, 202, 201, {"radius": 0.499999});
        stepVol(c, st, 202, "fillet");
        // step 268: guard_end {"after":{"wp":202},"block":1,"g":33,"op":"guard_end"}
        atStep(st, 269);
        cqUnion(c, st, 203, 197, {"toUnion": {"wp": 202}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 203, "union");
        atStep(st, 270);
        cqWorkplaneNew(c, st, 204, {"plane": {"origin": [28, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 271);
        cqRect(c, st, 205, 204, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 272);
        cqExtrude(c, st, 206, 205, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 206, "extrude");
        // step 273: guard_begin {"before":{"wp":206},"block":1,"g":34,"op":"guard_begin","skippable":true}
        atStep(st, 274);
        cqSelect("Edges")(c, st, 207, 206, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 275);
        cqFillet(c, st, 208, 207, {"radius": 0.499999});
        stepVol(c, st, 208, "fillet");
        // step 276: guard_end {"after":{"wp":208},"block":1,"g":34,"op":"guard_end"}
        atStep(st, 277);
        cqUnion(c, st, 209, 203, {"toUnion": {"wp": 208}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 209, "union");
        atStep(st, 278);
        cqWorkplaneNew(c, st, 210, {"plane": {"origin": [26, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 279);
        cqRect(c, st, 211, 210, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 280);
        cqExtrude(c, st, 212, 211, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 212, "extrude");
        // step 281: guard_begin {"before":{"wp":212},"block":1,"g":35,"op":"guard_begin","skippable":true}
        atStep(st, 282);
        cqSelect("Edges")(c, st, 213, 212, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 283);
        cqFillet(c, st, 214, 213, {"radius": 0.499999});
        stepVol(c, st, 214, "fillet");
        // step 284: guard_end {"after":{"wp":214},"block":1,"g":35,"op":"guard_end"}
        atStep(st, 285);
        cqUnion(c, st, 215, 209, {"toUnion": {"wp": 214}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 215, "union");
        atStep(st, 286);
        cqWorkplaneNew(c, st, 216, {"plane": {"origin": [22, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 287);
        cqRect(c, st, 217, 216, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 288);
        cqExtrude(c, st, 218, 217, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 218, "extrude");
        // step 289: guard_begin {"before":{"wp":218},"block":1,"g":36,"op":"guard_begin","skippable":true}
        atStep(st, 290);
        cqSelect("Edges")(c, st, 219, 218, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 291);
        cqFillet(c, st, 220, 219, {"radius": 0.499999});
        stepVol(c, st, 220, "fillet");
        // step 292: guard_end {"after":{"wp":220},"block":1,"g":36,"op":"guard_end"}
        atStep(st, 293);
        cqUnion(c, st, 221, 215, {"toUnion": {"wp": 220}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 221, "union");
        atStep(st, 294);
        cqWorkplaneNew(c, st, 222, {"plane": {"origin": [36, -13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 295);
        cqRect(c, st, 223, 222, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 296);
        cqExtrude(c, st, 224, 223, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 224, "extrude");
        // step 297: guard_begin {"before":{"wp":224},"block":1,"g":37,"op":"guard_begin","skippable":true}
        atStep(st, 298);
        cqSelect("Edges")(c, st, 225, 224, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 299);
        cqFillet(c, st, 226, 225, {"radius": 0.499999});
        stepVol(c, st, 226, "fillet");
        // step 300: guard_end {"after":{"wp":226},"block":1,"g":37,"op":"guard_end"}
        atStep(st, 301);
        cqUnion(c, st, 227, 221, {"toUnion": {"wp": 226}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 227, "union");
        atStep(st, 302);
        cqWorkplaneNew(c, st, 228, {"plane": {"origin": [34, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 303);
        cqRect(c, st, 229, 228, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 304);
        cqExtrude(c, st, 230, 229, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 230, "extrude");
        // step 305: guard_begin {"before":{"wp":230},"block":1,"g":38,"op":"guard_begin","skippable":true}
        atStep(st, 306);
        cqSelect("Edges")(c, st, 231, 230, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 307);
        cqFillet(c, st, 232, 231, {"radius": 0.499999});
        stepVol(c, st, 232, "fillet");
        // step 308: guard_end {"after":{"wp":232},"block":1,"g":38,"op":"guard_end"}
        atStep(st, 309);
        cqUnion(c, st, 233, 227, {"toUnion": {"wp": 232}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 233, "union");
        atStep(st, 310);
        cqWorkplaneNew(c, st, 234, {"plane": {"origin": [30, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 311);
        cqRect(c, st, 235, 234, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 312);
        cqExtrude(c, st, 236, 235, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 236, "extrude");
        // step 313: guard_begin {"before":{"wp":236},"block":1,"g":39,"op":"guard_begin","skippable":true}
        atStep(st, 314);
        cqSelect("Edges")(c, st, 237, 236, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 315);
        cqFillet(c, st, 238, 237, {"radius": 0.499999});
        stepVol(c, st, 238, "fillet");
        // step 316: guard_end {"after":{"wp":238},"block":1,"g":39,"op":"guard_end"}
        atStep(st, 317);
        cqUnion(c, st, 239, 233, {"toUnion": {"wp": 238}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 239, "union");
        atStep(st, 318);
        cqWorkplaneNew(c, st, 240, {"plane": {"origin": [42, -10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 319);
        cqRect(c, st, 241, 240, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 320);
        cqExtrude(c, st, 242, 241, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 242, "extrude");
        // step 321: guard_begin {"before":{"wp":242},"block":1,"g":40,"op":"guard_begin","skippable":true}
        atStep(st, 322);
        cqSelect("Edges")(c, st, 243, 242, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 323);
        cqFillet(c, st, 244, 243, {"radius": 0.499999});
        stepVol(c, st, 244, "fillet");
        // step 324: guard_end {"after":{"wp":244},"block":1,"g":40,"op":"guard_end"}
        atStep(st, 325);
        cqUnion(c, st, 245, 239, {"toUnion": {"wp": 244}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 245, "union");
        atStep(st, 326);
        cqWorkplaneNew(c, st, 246, {"plane": {"origin": [38, -10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 327);
        cqRect(c, st, 247, 246, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 328);
        cqExtrude(c, st, 248, 247, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 248, "extrude");
        // step 329: guard_begin {"before":{"wp":248},"block":1,"g":41,"op":"guard_begin","skippable":true}
        atStep(st, 330);
        cqSelect("Edges")(c, st, 249, 248, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 331);
        cqFillet(c, st, 250, 249, {"radius": 0.499999});
        stepVol(c, st, 250, "fillet");
        // step 332: guard_end {"after":{"wp":250},"block":1,"g":41,"op":"guard_end"}
        atStep(st, 333);
        cqUnion(c, st, 251, 245, {"toUnion": {"wp": 250}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 251, "union");
        atStep(st, 334);
        cqWorkplaneNew(c, st, 252, {"plane": {"origin": [-32, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 335);
        cqRect(c, st, 253, 252, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 336);
        cqExtrude(c, st, 254, 253, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        // step 337: guard_begin {"before":{"wp":254},"block":1,"g":42,"op":"guard_begin","skippable":true}
        atStep(st, 338);
        cqSelect("Edges")(c, st, 255, 254, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 339);
        cqFillet(c, st, 256, 255, {"radius": 0.499999});
        stepVol(c, st, 256, "fillet");
        // step 340: guard_end {"after":{"wp":256},"block":1,"g":42,"op":"guard_end"}
        atStep(st, 341);
        cqUnion(c, st, 257, 251, {"toUnion": {"wp": 256}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 257, "union");
        atStep(st, 342);
        cqWorkplaneNew(c, st, 258, {"plane": {"origin": [-34, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 343);
        cqRect(c, st, 259, 258, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 344);
        cqExtrude(c, st, 260, 259, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 260, "extrude");
        // step 345: guard_begin {"before":{"wp":260},"block":1,"g":43,"op":"guard_begin","skippable":true}
        atStep(st, 346);
        cqSelect("Edges")(c, st, 261, 260, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 347);
        cqFillet(c, st, 262, 261, {"radius": 0.499999});
        stepVol(c, st, 262, "fillet");
        // step 348: guard_end {"after":{"wp":262},"block":1,"g":43,"op":"guard_end"}
        atStep(st, 349);
        cqUnion(c, st, 263, 257, {"toUnion": {"wp": 262}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 263, "union");
        atStep(st, 350);
        cqWorkplaneNew(c, st, 264, {"plane": {"origin": [-38, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 351);
        cqRect(c, st, 265, 264, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 352);
        cqExtrude(c, st, 266, 265, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 266, "extrude");
        // step 353: guard_begin {"before":{"wp":266},"block":1,"g":44,"op":"guard_begin","skippable":true}
        atStep(st, 354);
        cqSelect("Edges")(c, st, 267, 266, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 355);
        cqFillet(c, st, 268, 267, {"radius": 0.499999});
        stepVol(c, st, 268, "fillet");
        // step 356: guard_end {"after":{"wp":268},"block":1,"g":44,"op":"guard_end"}
        atStep(st, 357);
        cqUnion(c, st, 269, 263, {"toUnion": {"wp": 268}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 269, "union");
        atStep(st, 358);
        cqWorkplaneNew(c, st, 270, {"plane": {"origin": [-24, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 359);
        cqRect(c, st, 271, 270, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 360);
        cqExtrude(c, st, 272, 271, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 272, "extrude");
        // step 361: guard_begin {"before":{"wp":272},"block":1,"g":45,"op":"guard_begin","skippable":true}
        atStep(st, 362);
        cqSelect("Edges")(c, st, 273, 272, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 363);
        cqFillet(c, st, 274, 273, {"radius": 0.499999});
        stepVol(c, st, 274, "fillet");
        // step 364: guard_end {"after":{"wp":274},"block":1,"g":45,"op":"guard_end"}
        atStep(st, 365);
        cqUnion(c, st, 275, 269, {"toUnion": {"wp": 274}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 275, "union");
        atStep(st, 366);
        cqWorkplaneNew(c, st, 276, {"plane": {"origin": [-26, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 367);
        cqRect(c, st, 277, 276, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 368);
        cqExtrude(c, st, 278, 277, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 278, "extrude");
        // step 369: guard_begin {"before":{"wp":278},"block":1,"g":46,"op":"guard_begin","skippable":true}
        atStep(st, 370);
        cqSelect("Edges")(c, st, 279, 278, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 371);
        cqFillet(c, st, 280, 279, {"radius": 0.499999});
        stepVol(c, st, 280, "fillet");
        // step 372: guard_end {"after":{"wp":280},"block":1,"g":46,"op":"guard_end"}
        atStep(st, 373);
        cqUnion(c, st, 281, 275, {"toUnion": {"wp": 280}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 281, "union");
        atStep(st, 374);
        cqWorkplaneNew(c, st, 282, {"plane": {"origin": [-30, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 375);
        cqRect(c, st, 283, 282, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 376);
        cqExtrude(c, st, 284, 283, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 284, "extrude");
        // step 377: guard_begin {"before":{"wp":284},"block":1,"g":47,"op":"guard_begin","skippable":true}
        atStep(st, 378);
        cqSelect("Edges")(c, st, 285, 284, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 379);
        cqFillet(c, st, 286, 285, {"radius": 0.499999});
        stepVol(c, st, 286, "fillet");
        // step 380: guard_end {"after":{"wp":286},"block":1,"g":47,"op":"guard_end"}
        atStep(st, 381);
        cqUnion(c, st, 287, 281, {"toUnion": {"wp": 286}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 287, "union");
        atStep(st, 382);
        cqWorkplaneNew(c, st, 288, {"plane": {"origin": [-16, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 383);
        cqRect(c, st, 289, 288, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 384);
        cqExtrude(c, st, 290, 289, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 290, "extrude");
        // step 385: guard_begin {"before":{"wp":290},"block":1,"g":48,"op":"guard_begin","skippable":true}
        atStep(st, 386);
        cqSelect("Edges")(c, st, 291, 290, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 387);
        cqFillet(c, st, 292, 291, {"radius": 0.499999});
        stepVol(c, st, 292, "fillet");
        // step 388: guard_end {"after":{"wp":292},"block":1,"g":48,"op":"guard_end"}
        atStep(st, 389);
        cqUnion(c, st, 293, 287, {"toUnion": {"wp": 292}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 293, "union");
        atStep(st, 390);
        cqWorkplaneNew(c, st, 294, {"plane": {"origin": [-18, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 391);
        cqRect(c, st, 295, 294, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 392);
        cqExtrude(c, st, 296, 295, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 296, "extrude");
        // step 393: guard_begin {"before":{"wp":296},"block":1,"g":49,"op":"guard_begin","skippable":true}
        atStep(st, 394);
        cqSelect("Edges")(c, st, 297, 296, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 395);
        cqFillet(c, st, 298, 297, {"radius": 0.499999});
        stepVol(c, st, 298, "fillet");
        // step 396: guard_end {"after":{"wp":298},"block":1,"g":49,"op":"guard_end"}
        atStep(st, 397);
        cqUnion(c, st, 299, 293, {"toUnion": {"wp": 298}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 299, "union");
        atStep(st, 398);
        cqWorkplaneNew(c, st, 300, {"plane": {"origin": [-22, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 399);
        cqRect(c, st, 301, 300, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 400);
        cqExtrude(c, st, 302, 301, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 302, "extrude");
        // step 401: guard_begin {"before":{"wp":302},"block":1,"g":50,"op":"guard_begin","skippable":true}
        atStep(st, 402);
        cqSelect("Edges")(c, st, 303, 302, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 403);
        cqFillet(c, st, 304, 303, {"radius": 0.499999});
        stepVol(c, st, 304, "fillet");
        // step 404: guard_end {"after":{"wp":304},"block":1,"g":50,"op":"guard_end"}
        atStep(st, 405);
        cqUnion(c, st, 305, 299, {"toUnion": {"wp": 304}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 305, "union");
        atStep(st, 406);
        cqWorkplaneNew(c, st, 306, {"plane": {"origin": [-8, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 407);
        cqRect(c, st, 307, 306, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 408);
        cqExtrude(c, st, 308, 307, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 308, "extrude");
        // step 409: guard_begin {"before":{"wp":308},"block":1,"g":51,"op":"guard_begin","skippable":true}
        atStep(st, 410);
        cqSelect("Edges")(c, st, 309, 308, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 411);
        cqFillet(c, st, 310, 309, {"radius": 0.499999});
        stepVol(c, st, 310, "fillet");
        // step 412: guard_end {"after":{"wp":310},"block":1,"g":51,"op":"guard_end"}
        atStep(st, 413);
        cqUnion(c, st, 311, 305, {"toUnion": {"wp": 310}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 311, "union");
        atStep(st, 414);
        cqWorkplaneNew(c, st, 312, {"plane": {"origin": [-10, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 415);
        cqRect(c, st, 313, 312, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 416);
        cqExtrude(c, st, 314, 313, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 314, "extrude");
        // step 417: guard_begin {"before":{"wp":314},"block":1,"g":52,"op":"guard_begin","skippable":true}
        atStep(st, 418);
        cqSelect("Edges")(c, st, 315, 314, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 419);
        cqFillet(c, st, 316, 315, {"radius": 0.499999});
        stepVol(c, st, 316, "fillet");
        // step 420: guard_end {"after":{"wp":316},"block":1,"g":52,"op":"guard_end"}
        atStep(st, 421);
        cqUnion(c, st, 317, 311, {"toUnion": {"wp": 316}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 317, "union");
        atStep(st, 422);
        cqWorkplaneNew(c, st, 318, {"plane": {"origin": [-14, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 423);
        cqRect(c, st, 319, 318, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 424);
        cqExtrude(c, st, 320, 319, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 320, "extrude");
        // step 425: guard_begin {"before":{"wp":320},"block":1,"g":53,"op":"guard_begin","skippable":true}
        atStep(st, 426);
        cqSelect("Edges")(c, st, 321, 320, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 427);
        cqFillet(c, st, 322, 321, {"radius": 0.499999});
        stepVol(c, st, 322, "fillet");
        // step 428: guard_end {"after":{"wp":322},"block":1,"g":53,"op":"guard_end"}
        atStep(st, 429);
        cqUnion(c, st, 323, 317, {"toUnion": {"wp": 322}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 323, "union");
        atStep(st, 430);
        cqWorkplaneNew(c, st, 324, {"plane": {"origin": [0, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 431);
        cqRect(c, st, 325, 324, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 432);
        cqExtrude(c, st, 326, 325, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 326, "extrude");
        // step 433: guard_begin {"before":{"wp":326},"block":1,"g":54,"op":"guard_begin","skippable":true}
        atStep(st, 434);
        cqSelect("Edges")(c, st, 327, 326, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 435);
        cqFillet(c, st, 328, 327, {"radius": 0.499999});
        stepVol(c, st, 328, "fillet");
        // step 436: guard_end {"after":{"wp":328},"block":1,"g":54,"op":"guard_end"}
        atStep(st, 437);
        cqUnion(c, st, 329, 323, {"toUnion": {"wp": 328}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 329, "union");
        atStep(st, 438);
        cqWorkplaneNew(c, st, 330, {"plane": {"origin": [-2, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 439);
        cqRect(c, st, 331, 330, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 440);
        cqExtrude(c, st, 332, 331, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 332, "extrude");
        // step 441: guard_begin {"before":{"wp":332},"block":1,"g":55,"op":"guard_begin","skippable":true}
        atStep(st, 442);
        cqSelect("Edges")(c, st, 333, 332, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 443);
        cqFillet(c, st, 334, 333, {"radius": 0.499999});
        stepVol(c, st, 334, "fillet");
        // step 444: guard_end {"after":{"wp":334},"block":1,"g":55,"op":"guard_end"}
        atStep(st, 445);
        cqUnion(c, st, 335, 329, {"toUnion": {"wp": 334}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 335, "union");
        atStep(st, 446);
        cqWorkplaneNew(c, st, 336, {"plane": {"origin": [-6, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 447);
        cqRect(c, st, 337, 336, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 448);
        cqExtrude(c, st, 338, 337, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 338, "extrude");
        // step 449: guard_begin {"before":{"wp":338},"block":1,"g":56,"op":"guard_begin","skippable":true}
        atStep(st, 450);
        cqSelect("Edges")(c, st, 339, 338, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 451);
        cqFillet(c, st, 340, 339, {"radius": 0.499999});
        stepVol(c, st, 340, "fillet");
        // step 452: guard_end {"after":{"wp":340},"block":1,"g":56,"op":"guard_end"}
        atStep(st, 453);
        cqUnion(c, st, 341, 335, {"toUnion": {"wp": 340}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 341, "union");
        atStep(st, 454);
        cqWorkplaneNew(c, st, 342, {"plane": {"origin": [8, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 455);
        cqRect(c, st, 343, 342, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 456);
        cqExtrude(c, st, 344, 343, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 344, "extrude");
        // step 457: guard_begin {"before":{"wp":344},"block":1,"g":57,"op":"guard_begin","skippable":true}
        atStep(st, 458);
        cqSelect("Edges")(c, st, 345, 344, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 459);
        cqFillet(c, st, 346, 345, {"radius": 0.499999});
        stepVol(c, st, 346, "fillet");
        // step 460: guard_end {"after":{"wp":346},"block":1,"g":57,"op":"guard_end"}
        atStep(st, 461);
        cqUnion(c, st, 347, 341, {"toUnion": {"wp": 346}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 347, "union");
        atStep(st, 462);
        cqWorkplaneNew(c, st, 348, {"plane": {"origin": [6, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 463);
        cqRect(c, st, 349, 348, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 464);
        cqExtrude(c, st, 350, 349, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 350, "extrude");
        // step 465: guard_begin {"before":{"wp":350},"block":1,"g":58,"op":"guard_begin","skippable":true}
        atStep(st, 466);
        cqSelect("Edges")(c, st, 351, 350, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 467);
        cqFillet(c, st, 352, 351, {"radius": 0.499999});
        stepVol(c, st, 352, "fillet");
        // step 468: guard_end {"after":{"wp":352},"block":1,"g":58,"op":"guard_end"}
        atStep(st, 469);
        cqUnion(c, st, 353, 347, {"toUnion": {"wp": 352}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 353, "union");
        atStep(st, 470);
        cqWorkplaneNew(c, st, 354, {"plane": {"origin": [2, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 471);
        cqRect(c, st, 355, 354, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 472);
        cqExtrude(c, st, 356, 355, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 356, "extrude");
        // step 473: guard_begin {"before":{"wp":356},"block":1,"g":59,"op":"guard_begin","skippable":true}
        atStep(st, 474);
        cqSelect("Edges")(c, st, 357, 356, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 475);
        cqFillet(c, st, 358, 357, {"radius": 0.499999});
        stepVol(c, st, 358, "fillet");
        // step 476: guard_end {"after":{"wp":358},"block":1,"g":59,"op":"guard_end"}
        atStep(st, 477);
        cqUnion(c, st, 359, 353, {"toUnion": {"wp": 358}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 359, "union");
        atStep(st, 478);
        cqWorkplaneNew(c, st, 360, {"plane": {"origin": [16, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 479);
        cqRect(c, st, 361, 360, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 480);
        cqExtrude(c, st, 362, 361, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        // step 481: guard_begin {"before":{"wp":362},"block":1,"g":60,"op":"guard_begin","skippable":true}
        atStep(st, 482);
        cqSelect("Edges")(c, st, 363, 362, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 483);
        cqFillet(c, st, 364, 363, {"radius": 0.499999});
        stepVol(c, st, 364, "fillet");
        // step 484: guard_end {"after":{"wp":364},"block":1,"g":60,"op":"guard_end"}
        atStep(st, 485);
        cqUnion(c, st, 365, 359, {"toUnion": {"wp": 364}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 365, "union");
        atStep(st, 486);
        cqWorkplaneNew(c, st, 366, {"plane": {"origin": [14, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 487);
        cqRect(c, st, 367, 366, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 488);
        cqExtrude(c, st, 368, 367, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 368, "extrude");
        // step 489: guard_begin {"before":{"wp":368},"block":1,"g":61,"op":"guard_begin","skippable":true}
        atStep(st, 490);
        cqSelect("Edges")(c, st, 369, 368, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 491);
        cqFillet(c, st, 370, 369, {"radius": 0.499999});
        stepVol(c, st, 370, "fillet");
        // step 492: guard_end {"after":{"wp":370},"block":1,"g":61,"op":"guard_end"}
        atStep(st, 493);
        cqUnion(c, st, 371, 365, {"toUnion": {"wp": 370}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 371, "union");
        atStep(st, 494);
        cqWorkplaneNew(c, st, 372, {"plane": {"origin": [10, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 495);
        cqRect(c, st, 373, 372, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 496);
        cqExtrude(c, st, 374, 373, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 374, "extrude");
        // step 497: guard_begin {"before":{"wp":374},"block":1,"g":62,"op":"guard_begin","skippable":true}
        atStep(st, 498);
        cqSelect("Edges")(c, st, 375, 374, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 499);
        cqFillet(c, st, 376, 375, {"radius": 0.499999});
        stepVol(c, st, 376, "fillet");
        // step 500: guard_end {"after":{"wp":376},"block":1,"g":62,"op":"guard_end"}
        atStep(st, 501);
        cqUnion(c, st, 377, 371, {"toUnion": {"wp": 376}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 377, "union");
        atStep(st, 502);
        cqWorkplaneNew(c, st, 378, {"plane": {"origin": [24, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 503);
        cqRect(c, st, 379, 378, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 504);
        cqExtrude(c, st, 380, 379, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 380, "extrude");
        // step 505: guard_begin {"before":{"wp":380},"block":1,"g":63,"op":"guard_begin","skippable":true}
        atStep(st, 506);
        cqSelect("Edges")(c, st, 381, 380, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 507);
        cqFillet(c, st, 382, 381, {"radius": 0.499999});
        stepVol(c, st, 382, "fillet");
        // step 508: guard_end {"after":{"wp":382},"block":1,"g":63,"op":"guard_end"}
        atStep(st, 509);
        cqUnion(c, st, 383, 377, {"toUnion": {"wp": 382}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 383, "union");
        atStep(st, 510);
        cqWorkplaneNew(c, st, 384, {"plane": {"origin": [22, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 511);
        cqRect(c, st, 385, 384, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 512);
        cqExtrude(c, st, 386, 385, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 386, "extrude");
        // step 513: guard_begin {"before":{"wp":386},"block":1,"g":64,"op":"guard_begin","skippable":true}
        atStep(st, 514);
        cqSelect("Edges")(c, st, 387, 386, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 515);
        cqFillet(c, st, 388, 387, {"radius": 0.499999});
        stepVol(c, st, 388, "fillet");
        // step 516: guard_end {"after":{"wp":388},"block":1,"g":64,"op":"guard_end"}
        atStep(st, 517);
        cqUnion(c, st, 389, 383, {"toUnion": {"wp": 388}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 389, "union");
        atStep(st, 518);
        cqWorkplaneNew(c, st, 390, {"plane": {"origin": [18, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 519);
        cqRect(c, st, 391, 390, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 520);
        cqExtrude(c, st, 392, 391, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 392, "extrude");
        // step 521: guard_begin {"before":{"wp":392},"block":1,"g":65,"op":"guard_begin","skippable":true}
        atStep(st, 522);
        cqSelect("Edges")(c, st, 393, 392, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 523);
        cqFillet(c, st, 394, 393, {"radius": 0.499999});
        stepVol(c, st, 394, "fillet");
        // step 524: guard_end {"after":{"wp":394},"block":1,"g":65,"op":"guard_end"}
        atStep(st, 525);
        cqUnion(c, st, 395, 389, {"toUnion": {"wp": 394}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 395, "union");
        atStep(st, 526);
        cqWorkplaneNew(c, st, 396, {"plane": {"origin": [32, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 527);
        cqRect(c, st, 397, 396, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 528);
        cqExtrude(c, st, 398, 397, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 398, "extrude");
        // step 529: guard_begin {"before":{"wp":398},"block":1,"g":66,"op":"guard_begin","skippable":true}
        atStep(st, 530);
        cqSelect("Edges")(c, st, 399, 398, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 531);
        cqFillet(c, st, 400, 399, {"radius": 0.499999});
        stepVol(c, st, 400, "fillet");
        // step 532: guard_end {"after":{"wp":400},"block":1,"g":66,"op":"guard_end"}
        atStep(st, 533);
        cqUnion(c, st, 401, 395, {"toUnion": {"wp": 400}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 401, "union");
        atStep(st, 534);
        cqWorkplaneNew(c, st, 402, {"plane": {"origin": [30, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 535);
        cqRect(c, st, 403, 402, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 536);
        cqExtrude(c, st, 404, 403, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 404, "extrude");
        // step 537: guard_begin {"before":{"wp":404},"block":1,"g":67,"op":"guard_begin","skippable":true}
        atStep(st, 538);
        cqSelect("Edges")(c, st, 405, 404, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 539);
        cqFillet(c, st, 406, 405, {"radius": 0.499999});
        stepVol(c, st, 406, "fillet");
        // step 540: guard_end {"after":{"wp":406},"block":1,"g":67,"op":"guard_end"}
        atStep(st, 541);
        cqUnion(c, st, 407, 401, {"toUnion": {"wp": 406}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 407, "union");
        atStep(st, 542);
        cqWorkplaneNew(c, st, 408, {"plane": {"origin": [26, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 543);
        cqRect(c, st, 409, 408, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 544);
        cqExtrude(c, st, 410, 409, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 410, "extrude");
        // step 545: guard_begin {"before":{"wp":410},"block":1,"g":68,"op":"guard_begin","skippable":true}
        atStep(st, 546);
        cqSelect("Edges")(c, st, 411, 410, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 547);
        cqFillet(c, st, 412, 411, {"radius": 0.499999});
        stepVol(c, st, 412, "fillet");
        // step 548: guard_end {"after":{"wp":412},"block":1,"g":68,"op":"guard_end"}
        atStep(st, 549);
        cqUnion(c, st, 413, 407, {"toUnion": {"wp": 412}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 413, "union");
        atStep(st, 550);
        cqWorkplaneNew(c, st, 414, {"plane": {"origin": [40, -6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 551);
        cqRect(c, st, 415, 414, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 552);
        cqExtrude(c, st, 416, 415, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 416, "extrude");
        // step 553: guard_begin {"before":{"wp":416},"block":1,"g":69,"op":"guard_begin","skippable":true}
        atStep(st, 554);
        cqSelect("Edges")(c, st, 417, 416, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 555);
        cqFillet(c, st, 418, 417, {"radius": 0.499999});
        stepVol(c, st, 418, "fillet");
        // step 556: guard_end {"after":{"wp":418},"block":1,"g":69,"op":"guard_end"}
        atStep(st, 557);
        cqUnion(c, st, 419, 413, {"toUnion": {"wp": 418}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 419, "union");
        atStep(st, 558);
        cqWorkplaneNew(c, st, 420, {"plane": {"origin": [38, -3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 559);
        cqRect(c, st, 421, 420, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 560);
        cqExtrude(c, st, 422, 421, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 422, "extrude");
        // step 561: guard_begin {"before":{"wp":422},"block":1,"g":70,"op":"guard_begin","skippable":true}
        atStep(st, 562);
        cqSelect("Edges")(c, st, 423, 422, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 563);
        cqFillet(c, st, 424, 423, {"radius": 0.499999});
        stepVol(c, st, 424, "fillet");
        // step 564: guard_end {"after":{"wp":424},"block":1,"g":70,"op":"guard_end"}
        atStep(st, 565);
        cqUnion(c, st, 425, 419, {"toUnion": {"wp": 424}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 425, "union");
        atStep(st, 566);
        cqWorkplaneNew(c, st, 426, {"plane": {"origin": [34, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 567);
        cqRect(c, st, 427, 426, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 568);
        cqExtrude(c, st, 428, 427, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 428, "extrude");
        // step 569: guard_begin {"before":{"wp":428},"block":1,"g":71,"op":"guard_begin","skippable":true}
        atStep(st, 570);
        cqSelect("Edges")(c, st, 429, 428, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 571);
        cqFillet(c, st, 430, 429, {"radius": 0.499999});
        stepVol(c, st, 430, "fillet");
        // step 572: guard_end {"after":{"wp":430},"block":1,"g":71,"op":"guard_end"}
        atStep(st, 573);
        cqUnion(c, st, 431, 425, {"toUnion": {"wp": 430}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 431, "union");
        atStep(st, 574);
        cqWorkplaneNew(c, st, 432, {"plane": {"origin": [42, -3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 575);
        cqRect(c, st, 433, 432, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 576);
        cqExtrude(c, st, 434, 433, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 434, "extrude");
        // step 577: guard_begin {"before":{"wp":434},"block":1,"g":72,"op":"guard_begin","skippable":true}
        atStep(st, 578);
        cqSelect("Edges")(c, st, 435, 434, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 579);
        cqFillet(c, st, 436, 435, {"radius": 0.499999});
        stepVol(c, st, 436, "fillet");
        // step 580: guard_end {"after":{"wp":436},"block":1,"g":72,"op":"guard_end"}
        atStep(st, 581);
        cqUnion(c, st, 437, 431, {"toUnion": {"wp": 436}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 437, "union");
        atStep(st, 582);
        cqWorkplaneNew(c, st, 438, {"plane": {"origin": [-36, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 583);
        cqRect(c, st, 439, 438, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 584);
        cqExtrude(c, st, 440, 439, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 440, "extrude");
        // step 585: guard_begin {"before":{"wp":440},"block":1,"g":73,"op":"guard_begin","skippable":true}
        atStep(st, 586);
        cqSelect("Edges")(c, st, 441, 440, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 587);
        cqFillet(c, st, 442, 441, {"radius": 0.499999});
        stepVol(c, st, 442, "fillet");
        // step 588: guard_end {"after":{"wp":442},"block":1,"g":73,"op":"guard_end"}
        atStep(st, 589);
        cqUnion(c, st, 443, 437, {"toUnion": {"wp": 442}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 443, "union");
        atStep(st, 590);
        cqWorkplaneNew(c, st, 444, {"plane": {"origin": [-38, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 591);
        cqRect(c, st, 445, 444, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 592);
        cqExtrude(c, st, 446, 445, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 446, "extrude");
        // step 593: guard_begin {"before":{"wp":446},"block":1,"g":74,"op":"guard_begin","skippable":true}
        atStep(st, 594);
        cqSelect("Edges")(c, st, 447, 446, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 595);
        cqFillet(c, st, 448, 447, {"radius": 0.499999});
        stepVol(c, st, 448, "fillet");
        // step 596: guard_end {"after":{"wp":448},"block":1,"g":74,"op":"guard_end"}
        atStep(st, 597);
        cqUnion(c, st, 449, 443, {"toUnion": {"wp": 448}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 449, "union");
        atStep(st, 598);
        cqWorkplaneNew(c, st, 450, {"plane": {"origin": [-28, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 599);
        cqRect(c, st, 451, 450, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 600);
        cqExtrude(c, st, 452, 451, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 452, "extrude");
        // step 601: guard_begin {"before":{"wp":452},"block":1,"g":75,"op":"guard_begin","skippable":true}
        atStep(st, 602);
        cqSelect("Edges")(c, st, 453, 452, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 603);
        cqFillet(c, st, 454, 453, {"radius": 0.499999});
        stepVol(c, st, 454, "fillet");
        // step 604: guard_end {"after":{"wp":454},"block":1,"g":75,"op":"guard_end"}
        atStep(st, 605);
        cqUnion(c, st, 455, 449, {"toUnion": {"wp": 454}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 455, "union");
        atStep(st, 606);
        cqWorkplaneNew(c, st, 456, {"plane": {"origin": [-30, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 607);
        cqRect(c, st, 457, 456, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 608);
        cqExtrude(c, st, 458, 457, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 458, "extrude");
        // step 609: guard_begin {"before":{"wp":458},"block":1,"g":76,"op":"guard_begin","skippable":true}
        atStep(st, 610);
        cqSelect("Edges")(c, st, 459, 458, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 611);
        cqFillet(c, st, 460, 459, {"radius": 0.499999});
        stepVol(c, st, 460, "fillet");
        // step 612: guard_end {"after":{"wp":460},"block":1,"g":76,"op":"guard_end"}
        atStep(st, 613);
        cqUnion(c, st, 461, 455, {"toUnion": {"wp": 460}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 461, "union");
        atStep(st, 614);
        cqWorkplaneNew(c, st, 462, {"plane": {"origin": [-34, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 615);
        cqRect(c, st, 463, 462, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 616);
        cqExtrude(c, st, 464, 463, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 464, "extrude");
        // step 617: guard_begin {"before":{"wp":464},"block":1,"g":77,"op":"guard_begin","skippable":true}
        atStep(st, 618);
        cqSelect("Edges")(c, st, 465, 464, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 619);
        cqFillet(c, st, 466, 465, {"radius": 0.499999});
        stepVol(c, st, 466, "fillet");
        // step 620: guard_end {"after":{"wp":466},"block":1,"g":77,"op":"guard_end"}
        atStep(st, 621);
        cqUnion(c, st, 467, 461, {"toUnion": {"wp": 466}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 467, "union");
        atStep(st, 622);
        cqWorkplaneNew(c, st, 468, {"plane": {"origin": [-20, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 623);
        cqRect(c, st, 469, 468, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 624);
        cqExtrude(c, st, 470, 469, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 470, "extrude");
        // step 625: guard_begin {"before":{"wp":470},"block":1,"g":78,"op":"guard_begin","skippable":true}
        atStep(st, 626);
        cqSelect("Edges")(c, st, 471, 470, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 627);
        cqFillet(c, st, 472, 471, {"radius": 0.499999});
        stepVol(c, st, 472, "fillet");
        // step 628: guard_end {"after":{"wp":472},"block":1,"g":78,"op":"guard_end"}
        atStep(st, 629);
        cqUnion(c, st, 473, 467, {"toUnion": {"wp": 472}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 473, "union");
        atStep(st, 630);
        cqWorkplaneNew(c, st, 474, {"plane": {"origin": [-22, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 631);
        cqRect(c, st, 475, 474, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 632);
        cqExtrude(c, st, 476, 475, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 476, "extrude");
        // step 633: guard_begin {"before":{"wp":476},"block":1,"g":79,"op":"guard_begin","skippable":true}
        atStep(st, 634);
        cqSelect("Edges")(c, st, 477, 476, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 635);
        cqFillet(c, st, 478, 477, {"radius": 0.499999});
        stepVol(c, st, 478, "fillet");
        // step 636: guard_end {"after":{"wp":478},"block":1,"g":79,"op":"guard_end"}
        atStep(st, 637);
        cqUnion(c, st, 479, 473, {"toUnion": {"wp": 478}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 479, "union");
        atStep(st, 638);
        cqWorkplaneNew(c, st, 480, {"plane": {"origin": [-26, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 639);
        cqRect(c, st, 481, 480, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 640);
        cqExtrude(c, st, 482, 481, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 482, "extrude");
        // step 641: guard_begin {"before":{"wp":482},"block":1,"g":80,"op":"guard_begin","skippable":true}
        atStep(st, 642);
        cqSelect("Edges")(c, st, 483, 482, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 643);
        cqFillet(c, st, 484, 483, {"radius": 0.499999});
        stepVol(c, st, 484, "fillet");
        // step 644: guard_end {"after":{"wp":484},"block":1,"g":80,"op":"guard_end"}
        atStep(st, 645);
        cqUnion(c, st, 485, 479, {"toUnion": {"wp": 484}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 485, "union");
        atStep(st, 646);
        cqWorkplaneNew(c, st, 486, {"plane": {"origin": [-12, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 647);
        cqRect(c, st, 487, 486, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 648);
        cqExtrude(c, st, 488, 487, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 488, "extrude");
        // step 649: guard_begin {"before":{"wp":488},"block":1,"g":81,"op":"guard_begin","skippable":true}
        atStep(st, 650);
        cqSelect("Edges")(c, st, 489, 488, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 651);
        cqFillet(c, st, 490, 489, {"radius": 0.499999});
        stepVol(c, st, 490, "fillet");
        // step 652: guard_end {"after":{"wp":490},"block":1,"g":81,"op":"guard_end"}
        atStep(st, 653);
        cqUnion(c, st, 491, 485, {"toUnion": {"wp": 490}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 491, "union");
        atStep(st, 654);
        cqWorkplaneNew(c, st, 492, {"plane": {"origin": [-14, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 655);
        cqRect(c, st, 493, 492, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 656);
        cqExtrude(c, st, 494, 493, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 494, "extrude");
        // step 657: guard_begin {"before":{"wp":494},"block":1,"g":82,"op":"guard_begin","skippable":true}
        atStep(st, 658);
        cqSelect("Edges")(c, st, 495, 494, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 659);
        cqFillet(c, st, 496, 495, {"radius": 0.499999});
        stepVol(c, st, 496, "fillet");
        // step 660: guard_end {"after":{"wp":496},"block":1,"g":82,"op":"guard_end"}
        atStep(st, 661);
        cqUnion(c, st, 497, 491, {"toUnion": {"wp": 496}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 497, "union");
        atStep(st, 662);
        cqWorkplaneNew(c, st, 498, {"plane": {"origin": [-18, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 663);
        cqRect(c, st, 499, 498, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 664);
        cqExtrude(c, st, 500, 499, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 500, "extrude");
        // step 665: guard_begin {"before":{"wp":500},"block":1,"g":83,"op":"guard_begin","skippable":true}
        atStep(st, 666);
        cqSelect("Edges")(c, st, 501, 500, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 667);
        cqFillet(c, st, 502, 501, {"radius": 0.499999});
        stepVol(c, st, 502, "fillet");
        // step 668: guard_end {"after":{"wp":502},"block":1,"g":83,"op":"guard_end"}
        atStep(st, 669);
        cqUnion(c, st, 503, 497, {"toUnion": {"wp": 502}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 503, "union");
        atStep(st, 670);
        cqWorkplaneNew(c, st, 504, {"plane": {"origin": [-4, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 671);
        cqRect(c, st, 505, 504, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 672);
        cqExtrude(c, st, 506, 505, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 506, "extrude");
        // step 673: guard_begin {"before":{"wp":506},"block":1,"g":84,"op":"guard_begin","skippable":true}
        atStep(st, 674);
        cqSelect("Edges")(c, st, 507, 506, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 675);
        cqFillet(c, st, 508, 507, {"radius": 0.499999});
        stepVol(c, st, 508, "fillet");
        // step 676: guard_end {"after":{"wp":508},"block":1,"g":84,"op":"guard_end"}
        atStep(st, 677);
        cqUnion(c, st, 509, 503, {"toUnion": {"wp": 508}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 509, "union");
        atStep(st, 678);
        cqWorkplaneNew(c, st, 510, {"plane": {"origin": [-6, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 679);
        cqRect(c, st, 511, 510, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 680);
        cqExtrude(c, st, 512, 511, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 512, "extrude");
        // step 681: guard_begin {"before":{"wp":512},"block":1,"g":85,"op":"guard_begin","skippable":true}
        atStep(st, 682);
        cqSelect("Edges")(c, st, 513, 512, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 683);
        cqFillet(c, st, 514, 513, {"radius": 0.499999});
        stepVol(c, st, 514, "fillet");
        // step 684: guard_end {"after":{"wp":514},"block":1,"g":85,"op":"guard_end"}
        atStep(st, 685);
        cqUnion(c, st, 515, 509, {"toUnion": {"wp": 514}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 515, "union");
        atStep(st, 686);
        cqWorkplaneNew(c, st, 516, {"plane": {"origin": [-10, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 687);
        cqRect(c, st, 517, 516, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 688);
        cqExtrude(c, st, 518, 517, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 518, "extrude");
        // step 689: guard_begin {"before":{"wp":518},"block":1,"g":86,"op":"guard_begin","skippable":true}
        atStep(st, 690);
        cqSelect("Edges")(c, st, 519, 518, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 691);
        cqFillet(c, st, 520, 519, {"radius": 0.499999});
        stepVol(c, st, 520, "fillet");
        // step 692: guard_end {"after":{"wp":520},"block":1,"g":86,"op":"guard_end"}
        atStep(st, 693);
        cqUnion(c, st, 521, 515, {"toUnion": {"wp": 520}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 521, "union");
        atStep(st, 694);
        cqWorkplaneNew(c, st, 522, {"plane": {"origin": [4, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 695);
        cqRect(c, st, 523, 522, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 696);
        cqExtrude(c, st, 524, 523, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 524, "extrude");
        // step 697: guard_begin {"before":{"wp":524},"block":1,"g":87,"op":"guard_begin","skippable":true}
        atStep(st, 698);
        cqSelect("Edges")(c, st, 525, 524, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 699);
        cqFillet(c, st, 526, 525, {"radius": 0.499999});
        stepVol(c, st, 526, "fillet");
        // step 700: guard_end {"after":{"wp":526},"block":1,"g":87,"op":"guard_end"}
        atStep(st, 701);
        cqUnion(c, st, 527, 521, {"toUnion": {"wp": 526}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 527, "union");
        atStep(st, 702);
        cqWorkplaneNew(c, st, 528, {"plane": {"origin": [2, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 703);
        cqRect(c, st, 529, 528, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 704);
        cqExtrude(c, st, 530, 529, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 530, "extrude");
        // step 705: guard_begin {"before":{"wp":530},"block":1,"g":88,"op":"guard_begin","skippable":true}
        atStep(st, 706);
        cqSelect("Edges")(c, st, 531, 530, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 707);
        cqFillet(c, st, 532, 531, {"radius": 0.499999});
        stepVol(c, st, 532, "fillet");
        // step 708: guard_end {"after":{"wp":532},"block":1,"g":88,"op":"guard_end"}
        atStep(st, 709);
        cqUnion(c, st, 533, 527, {"toUnion": {"wp": 532}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 533, "union");
        atStep(st, 710);
        cqWorkplaneNew(c, st, 534, {"plane": {"origin": [-2, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 711);
        cqRect(c, st, 535, 534, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 712);
        cqExtrude(c, st, 536, 535, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 536, "extrude");
        // step 713: guard_begin {"before":{"wp":536},"block":1,"g":89,"op":"guard_begin","skippable":true}
        atStep(st, 714);
        cqSelect("Edges")(c, st, 537, 536, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 715);
        cqFillet(c, st, 538, 537, {"radius": 0.499999});
        stepVol(c, st, 538, "fillet");
        // step 716: guard_end {"after":{"wp":538},"block":1,"g":89,"op":"guard_end"}
        atStep(st, 717);
        cqUnion(c, st, 539, 533, {"toUnion": {"wp": 538}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 539, "union");
        atStep(st, 718);
        cqWorkplaneNew(c, st, 540, {"plane": {"origin": [12, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 719);
        cqRect(c, st, 541, 540, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 720);
        cqExtrude(c, st, 542, 541, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 542, "extrude");
        // step 721: guard_begin {"before":{"wp":542},"block":1,"g":90,"op":"guard_begin","skippable":true}
        atStep(st, 722);
        cqSelect("Edges")(c, st, 543, 542, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 723);
        cqFillet(c, st, 544, 543, {"radius": 0.499999});
        stepVol(c, st, 544, "fillet");
        // step 724: guard_end {"after":{"wp":544},"block":1,"g":90,"op":"guard_end"}
        atStep(st, 725);
        cqUnion(c, st, 545, 539, {"toUnion": {"wp": 544}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 545, "union");
        atStep(st, 726);
        cqWorkplaneNew(c, st, 546, {"plane": {"origin": [10, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 727);
        cqRect(c, st, 547, 546, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 728);
        cqExtrude(c, st, 548, 547, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 548, "extrude");
        // step 729: guard_begin {"before":{"wp":548},"block":1,"g":91,"op":"guard_begin","skippable":true}
        atStep(st, 730);
        cqSelect("Edges")(c, st, 549, 548, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 731);
        cqFillet(c, st, 550, 549, {"radius": 0.499999});
        stepVol(c, st, 550, "fillet");
        // step 732: guard_end {"after":{"wp":550},"block":1,"g":91,"op":"guard_end"}
        atStep(st, 733);
        cqUnion(c, st, 551, 545, {"toUnion": {"wp": 550}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 551, "union");
        atStep(st, 734);
        cqWorkplaneNew(c, st, 552, {"plane": {"origin": [6, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 735);
        cqRect(c, st, 553, 552, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 736);
        cqExtrude(c, st, 554, 553, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 554, "extrude");
        // step 737: guard_begin {"before":{"wp":554},"block":1,"g":92,"op":"guard_begin","skippable":true}
        atStep(st, 738);
        cqSelect("Edges")(c, st, 555, 554, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 739);
        cqFillet(c, st, 556, 555, {"radius": 0.499999});
        stepVol(c, st, 556, "fillet");
        // step 740: guard_end {"after":{"wp":556},"block":1,"g":92,"op":"guard_end"}
        atStep(st, 741);
        cqUnion(c, st, 557, 551, {"toUnion": {"wp": 556}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 557, "union");
        atStep(st, 742);
        cqWorkplaneNew(c, st, 558, {"plane": {"origin": [20, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 743);
        cqRect(c, st, 559, 558, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 744);
        cqExtrude(c, st, 560, 559, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 560, "extrude");
        // step 745: guard_begin {"before":{"wp":560},"block":1,"g":93,"op":"guard_begin","skippable":true}
        atStep(st, 746);
        cqSelect("Edges")(c, st, 561, 560, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 747);
        cqFillet(c, st, 562, 561, {"radius": 0.499999});
        stepVol(c, st, 562, "fillet");
        // step 748: guard_end {"after":{"wp":562},"block":1,"g":93,"op":"guard_end"}
        atStep(st, 749);
        cqUnion(c, st, 563, 557, {"toUnion": {"wp": 562}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 563, "union");
        atStep(st, 750);
        cqWorkplaneNew(c, st, 564, {"plane": {"origin": [18, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 751);
        cqRect(c, st, 565, 564, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 752);
        cqExtrude(c, st, 566, 565, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 566, "extrude");
        // step 753: guard_begin {"before":{"wp":566},"block":1,"g":94,"op":"guard_begin","skippable":true}
        atStep(st, 754);
        cqSelect("Edges")(c, st, 567, 566, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 755);
        cqFillet(c, st, 568, 567, {"radius": 0.499999});
        stepVol(c, st, 568, "fillet");
        // step 756: guard_end {"after":{"wp":568},"block":1,"g":94,"op":"guard_end"}
        atStep(st, 757);
        cqUnion(c, st, 569, 563, {"toUnion": {"wp": 568}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 569, "union");
        atStep(st, 758);
        cqWorkplaneNew(c, st, 570, {"plane": {"origin": [14, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 759);
        cqRect(c, st, 571, 570, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 760);
        cqExtrude(c, st, 572, 571, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 572, "extrude");
        // step 761: guard_begin {"before":{"wp":572},"block":1,"g":95,"op":"guard_begin","skippable":true}
        atStep(st, 762);
        cqSelect("Edges")(c, st, 573, 572, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 763);
        cqFillet(c, st, 574, 573, {"radius": 0.499999});
        stepVol(c, st, 574, "fillet");
        // step 764: guard_end {"after":{"wp":574},"block":1,"g":95,"op":"guard_end"}
        atStep(st, 765);
        cqUnion(c, st, 575, 569, {"toUnion": {"wp": 574}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 575, "union");
        atStep(st, 766);
        cqWorkplaneNew(c, st, 576, {"plane": {"origin": [28, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 767);
        cqRect(c, st, 577, 576, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 768);
        cqExtrude(c, st, 578, 577, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 578, "extrude");
        // step 769: guard_begin {"before":{"wp":578},"block":1,"g":96,"op":"guard_begin","skippable":true}
        atStep(st, 770);
        cqSelect("Edges")(c, st, 579, 578, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 771);
        cqFillet(c, st, 580, 579, {"radius": 0.499999});
        stepVol(c, st, 580, "fillet");
        // step 772: guard_end {"after":{"wp":580},"block":1,"g":96,"op":"guard_end"}
        atStep(st, 773);
        cqUnion(c, st, 581, 575, {"toUnion": {"wp": 580}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 581, "union");
        atStep(st, 774);
        cqWorkplaneNew(c, st, 582, {"plane": {"origin": [26, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 775);
        cqRect(c, st, 583, 582, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 776);
        cqExtrude(c, st, 584, 583, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 584, "extrude");
        // step 777: guard_begin {"before":{"wp":584},"block":1,"g":97,"op":"guard_begin","skippable":true}
        atStep(st, 778);
        cqSelect("Edges")(c, st, 585, 584, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 779);
        cqFillet(c, st, 586, 585, {"radius": 0.499999});
        stepVol(c, st, 586, "fillet");
        // step 780: guard_end {"after":{"wp":586},"block":1,"g":97,"op":"guard_end"}
        atStep(st, 781);
        cqUnion(c, st, 587, 581, {"toUnion": {"wp": 586}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 587, "union");
        atStep(st, 782);
        cqWorkplaneNew(c, st, 588, {"plane": {"origin": [22, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 783);
        cqRect(c, st, 589, 588, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 784);
        cqExtrude(c, st, 590, 589, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 590, "extrude");
        // step 785: guard_begin {"before":{"wp":590},"block":1,"g":98,"op":"guard_begin","skippable":true}
        atStep(st, 786);
        cqSelect("Edges")(c, st, 591, 590, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 787);
        cqFillet(c, st, 592, 591, {"radius": 0.499999});
        stepVol(c, st, 592, "fillet");
        // step 788: guard_end {"after":{"wp":592},"block":1,"g":98,"op":"guard_end"}
        atStep(st, 789);
        cqUnion(c, st, 593, 587, {"toUnion": {"wp": 592}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 593, "union");
        atStep(st, 790);
        cqWorkplaneNew(c, st, 594, {"plane": {"origin": [36, 0, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 791);
        cqRect(c, st, 595, 594, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 792);
        cqExtrude(c, st, 596, 595, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 596, "extrude");
        // step 793: guard_begin {"before":{"wp":596},"block":1,"g":99,"op":"guard_begin","skippable":true}
        atStep(st, 794);
        cqSelect("Edges")(c, st, 597, 596, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 795);
        cqFillet(c, st, 598, 597, {"radius": 0.499999});
        stepVol(c, st, 598, "fillet");
        // step 796: guard_end {"after":{"wp":598},"block":1,"g":99,"op":"guard_end"}
        atStep(st, 797);
        cqUnion(c, st, 599, 593, {"toUnion": {"wp": 598}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 599, "union");
        atStep(st, 798);
        cqWorkplaneNew(c, st, 600, {"plane": {"origin": [34, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 799);
        cqRect(c, st, 601, 600, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 800);
        cqExtrude(c, st, 602, 601, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 602, "extrude");
        // step 801: guard_begin {"before":{"wp":602},"block":1,"g":100,"op":"guard_begin","skippable":true}
        atStep(st, 802);
        cqSelect("Edges")(c, st, 603, 602, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 803);
        cqFillet(c, st, 604, 603, {"radius": 0.499999});
        stepVol(c, st, 604, "fillet");
        // step 804: guard_end {"after":{"wp":604},"block":1,"g":100,"op":"guard_end"}
        atStep(st, 805);
        cqUnion(c, st, 605, 599, {"toUnion": {"wp": 604}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 605, "union");
        atStep(st, 806);
        cqWorkplaneNew(c, st, 606, {"plane": {"origin": [30, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 807);
        cqRect(c, st, 607, 606, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 808);
        cqExtrude(c, st, 608, 607, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 608, "extrude");
        // step 809: guard_begin {"before":{"wp":608},"block":1,"g":101,"op":"guard_begin","skippable":true}
        atStep(st, 810);
        cqSelect("Edges")(c, st, 609, 608, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 811);
        cqFillet(c, st, 610, 609, {"radius": 0.499999});
        stepVol(c, st, 610, "fillet");
        // step 812: guard_end {"after":{"wp":610},"block":1,"g":101,"op":"guard_end"}
        atStep(st, 813);
        cqUnion(c, st, 611, 605, {"toUnion": {"wp": 610}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 611, "union");
        atStep(st, 814);
        cqWorkplaneNew(c, st, 612, {"plane": {"origin": [42, 3.4641015, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 815);
        cqRect(c, st, 613, 612, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 816);
        cqExtrude(c, st, 614, 613, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 614, "extrude");
        // step 817: guard_begin {"before":{"wp":614},"block":1,"g":102,"op":"guard_begin","skippable":true}
        atStep(st, 818);
        cqSelect("Edges")(c, st, 615, 614, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 819);
        cqFillet(c, st, 616, 615, {"radius": 0.499999});
        stepVol(c, st, 616, "fillet");
        // step 820: guard_end {"after":{"wp":616},"block":1,"g":102,"op":"guard_end"}
        atStep(st, 821);
        cqUnion(c, st, 617, 611, {"toUnion": {"wp": 616}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 617, "union");
        atStep(st, 822);
        cqWorkplaneNew(c, st, 618, {"plane": {"origin": [38, 3.4641015, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 823);
        cqRect(c, st, 619, 618, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 824);
        cqExtrude(c, st, 620, 619, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 620, "extrude");
        // step 825: guard_begin {"before":{"wp":620},"block":1,"g":103,"op":"guard_begin","skippable":true}
        atStep(st, 826);
        cqSelect("Edges")(c, st, 621, 620, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 827);
        cqFillet(c, st, 622, 621, {"radius": 0.499999});
        stepVol(c, st, 622, "fillet");
        // step 828: guard_end {"after":{"wp":622},"block":1,"g":103,"op":"guard_end"}
        atStep(st, 829);
        cqUnion(c, st, 623, 617, {"toUnion": {"wp": 622}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 623, "union");
        atStep(st, 830);
        cqWorkplaneNew(c, st, 624, {"plane": {"origin": [-32, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 831);
        cqRect(c, st, 625, 624, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 832);
        cqExtrude(c, st, 626, 625, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 626, "extrude");
        // step 833: guard_begin {"before":{"wp":626},"block":1,"g":104,"op":"guard_begin","skippable":true}
        atStep(st, 834);
        cqSelect("Edges")(c, st, 627, 626, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 835);
        cqFillet(c, st, 628, 627, {"radius": 0.499999});
        stepVol(c, st, 628, "fillet");
        // step 836: guard_end {"after":{"wp":628},"block":1,"g":104,"op":"guard_end"}
        atStep(st, 837);
        cqUnion(c, st, 629, 623, {"toUnion": {"wp": 628}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 629, "union");
        atStep(st, 838);
        cqWorkplaneNew(c, st, 630, {"plane": {"origin": [-34, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 839);
        cqRect(c, st, 631, 630, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 840);
        cqExtrude(c, st, 632, 631, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 632, "extrude");
        // step 841: guard_begin {"before":{"wp":632},"block":1,"g":105,"op":"guard_begin","skippable":true}
        atStep(st, 842);
        cqSelect("Edges")(c, st, 633, 632, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 843);
        cqFillet(c, st, 634, 633, {"radius": 0.499999});
        stepVol(c, st, 634, "fillet");
        // step 844: guard_end {"after":{"wp":634},"block":1,"g":105,"op":"guard_end"}
        atStep(st, 845);
        cqUnion(c, st, 635, 629, {"toUnion": {"wp": 634}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 635, "union");
        atStep(st, 846);
        cqWorkplaneNew(c, st, 636, {"plane": {"origin": [-38, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 847);
        cqRect(c, st, 637, 636, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 848);
        cqExtrude(c, st, 638, 637, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 638, "extrude");
        // step 849: guard_begin {"before":{"wp":638},"block":1,"g":106,"op":"guard_begin","skippable":true}
        atStep(st, 850);
        cqSelect("Edges")(c, st, 639, 638, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 851);
        cqFillet(c, st, 640, 639, {"radius": 0.499999});
        stepVol(c, st, 640, "fillet");
        // step 852: guard_end {"after":{"wp":640},"block":1,"g":106,"op":"guard_end"}
        atStep(st, 853);
        cqUnion(c, st, 641, 635, {"toUnion": {"wp": 640}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 641, "union");
        atStep(st, 854);
        cqWorkplaneNew(c, st, 642, {"plane": {"origin": [-24, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 855);
        cqRect(c, st, 643, 642, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 856);
        cqExtrude(c, st, 644, 643, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 644, "extrude");
        // step 857: guard_begin {"before":{"wp":644},"block":1,"g":107,"op":"guard_begin","skippable":true}
        atStep(st, 858);
        cqSelect("Edges")(c, st, 645, 644, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 859);
        cqFillet(c, st, 646, 645, {"radius": 0.499999});
        stepVol(c, st, 646, "fillet");
        // step 860: guard_end {"after":{"wp":646},"block":1,"g":107,"op":"guard_end"}
        atStep(st, 861);
        cqUnion(c, st, 647, 641, {"toUnion": {"wp": 646}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 647, "union");
        atStep(st, 862);
        cqWorkplaneNew(c, st, 648, {"plane": {"origin": [-26, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 863);
        cqRect(c, st, 649, 648, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 864);
        cqExtrude(c, st, 650, 649, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 650, "extrude");
        // step 865: guard_begin {"before":{"wp":650},"block":1,"g":108,"op":"guard_begin","skippable":true}
        atStep(st, 866);
        cqSelect("Edges")(c, st, 651, 650, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 867);
        cqFillet(c, st, 652, 651, {"radius": 0.499999});
        stepVol(c, st, 652, "fillet");
        // step 868: guard_end {"after":{"wp":652},"block":1,"g":108,"op":"guard_end"}
        atStep(st, 869);
        cqUnion(c, st, 653, 647, {"toUnion": {"wp": 652}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 653, "union");
        atStep(st, 870);
        cqWorkplaneNew(c, st, 654, {"plane": {"origin": [-30, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 871);
        cqRect(c, st, 655, 654, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 872);
        cqExtrude(c, st, 656, 655, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 656, "extrude");
        // step 873: guard_begin {"before":{"wp":656},"block":1,"g":109,"op":"guard_begin","skippable":true}
        atStep(st, 874);
        cqSelect("Edges")(c, st, 657, 656, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 875);
        cqFillet(c, st, 658, 657, {"radius": 0.499999});
        stepVol(c, st, 658, "fillet");
        // step 876: guard_end {"after":{"wp":658},"block":1,"g":109,"op":"guard_end"}
        atStep(st, 877);
        cqUnion(c, st, 659, 653, {"toUnion": {"wp": 658}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 659, "union");
        atStep(st, 878);
        cqWorkplaneNew(c, st, 660, {"plane": {"origin": [-16, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 879);
        cqRect(c, st, 661, 660, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 880);
        cqExtrude(c, st, 662, 661, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 662, "extrude");
        // step 881: guard_begin {"before":{"wp":662},"block":1,"g":110,"op":"guard_begin","skippable":true}
        atStep(st, 882);
        cqSelect("Edges")(c, st, 663, 662, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 883);
        cqFillet(c, st, 664, 663, {"radius": 0.499999});
        stepVol(c, st, 664, "fillet");
        // step 884: guard_end {"after":{"wp":664},"block":1,"g":110,"op":"guard_end"}
        atStep(st, 885);
        cqUnion(c, st, 665, 659, {"toUnion": {"wp": 664}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 665, "union");
        atStep(st, 886);
        cqWorkplaneNew(c, st, 666, {"plane": {"origin": [-18, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 887);
        cqRect(c, st, 667, 666, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 888);
        cqExtrude(c, st, 668, 667, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 668, "extrude");
        // step 889: guard_begin {"before":{"wp":668},"block":1,"g":111,"op":"guard_begin","skippable":true}
        atStep(st, 890);
        cqSelect("Edges")(c, st, 669, 668, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 891);
        cqFillet(c, st, 670, 669, {"radius": 0.499999});
        stepVol(c, st, 670, "fillet");
        // step 892: guard_end {"after":{"wp":670},"block":1,"g":111,"op":"guard_end"}
        atStep(st, 893);
        cqUnion(c, st, 671, 665, {"toUnion": {"wp": 670}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 671, "union");
        atStep(st, 894);
        cqWorkplaneNew(c, st, 672, {"plane": {"origin": [-22, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 895);
        cqRect(c, st, 673, 672, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 896);
        cqExtrude(c, st, 674, 673, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 674, "extrude");
        // step 897: guard_begin {"before":{"wp":674},"block":1,"g":112,"op":"guard_begin","skippable":true}
        atStep(st, 898);
        cqSelect("Edges")(c, st, 675, 674, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 899);
        cqFillet(c, st, 676, 675, {"radius": 0.499999});
        stepVol(c, st, 676, "fillet");
        // step 900: guard_end {"after":{"wp":676},"block":1,"g":112,"op":"guard_end"}
        atStep(st, 901);
        cqUnion(c, st, 677, 671, {"toUnion": {"wp": 676}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 677, "union");
        atStep(st, 902);
        cqWorkplaneNew(c, st, 678, {"plane": {"origin": [-8, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 903);
        cqRect(c, st, 679, 678, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 904);
        cqExtrude(c, st, 680, 679, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 680, "extrude");
        // step 905: guard_begin {"before":{"wp":680},"block":1,"g":113,"op":"guard_begin","skippable":true}
        atStep(st, 906);
        cqSelect("Edges")(c, st, 681, 680, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 907);
        cqFillet(c, st, 682, 681, {"radius": 0.499999});
        stepVol(c, st, 682, "fillet");
        // step 908: guard_end {"after":{"wp":682},"block":1,"g":113,"op":"guard_end"}
        atStep(st, 909);
        cqUnion(c, st, 683, 677, {"toUnion": {"wp": 682}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 683, "union");
        atStep(st, 910);
        cqWorkplaneNew(c, st, 684, {"plane": {"origin": [-10, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 911);
        cqRect(c, st, 685, 684, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 912);
        cqExtrude(c, st, 686, 685, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 686, "extrude");
        // step 913: guard_begin {"before":{"wp":686},"block":1,"g":114,"op":"guard_begin","skippable":true}
        atStep(st, 914);
        cqSelect("Edges")(c, st, 687, 686, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 915);
        cqFillet(c, st, 688, 687, {"radius": 0.499999});
        stepVol(c, st, 688, "fillet");
        // step 916: guard_end {"after":{"wp":688},"block":1,"g":114,"op":"guard_end"}
        atStep(st, 917);
        cqUnion(c, st, 689, 683, {"toUnion": {"wp": 688}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 689, "union");
        atStep(st, 918);
        cqWorkplaneNew(c, st, 690, {"plane": {"origin": [-14, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 919);
        cqRect(c, st, 691, 690, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 920);
        cqExtrude(c, st, 692, 691, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 692, "extrude");
        // step 921: guard_begin {"before":{"wp":692},"block":1,"g":115,"op":"guard_begin","skippable":true}
        atStep(st, 922);
        cqSelect("Edges")(c, st, 693, 692, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 923);
        cqFillet(c, st, 694, 693, {"radius": 0.499999});
        stepVol(c, st, 694, "fillet");
        // step 924: guard_end {"after":{"wp":694},"block":1,"g":115,"op":"guard_end"}
        atStep(st, 925);
        cqUnion(c, st, 695, 689, {"toUnion": {"wp": 694}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 695, "union");
        atStep(st, 926);
        cqWorkplaneNew(c, st, 696, {"plane": {"origin": [0, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 927);
        cqRect(c, st, 697, 696, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 928);
        cqExtrude(c, st, 698, 697, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 698, "extrude");
        // step 929: guard_begin {"before":{"wp":698},"block":1,"g":116,"op":"guard_begin","skippable":true}
        atStep(st, 930);
        cqSelect("Edges")(c, st, 699, 698, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 931);
        cqFillet(c, st, 700, 699, {"radius": 0.499999});
        stepVol(c, st, 700, "fillet");
        // step 932: guard_end {"after":{"wp":700},"block":1,"g":116,"op":"guard_end"}
        atStep(st, 933);
        cqUnion(c, st, 701, 695, {"toUnion": {"wp": 700}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 701, "union");
        atStep(st, 934);
        cqWorkplaneNew(c, st, 702, {"plane": {"origin": [-2, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 935);
        cqRect(c, st, 703, 702, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 936);
        cqExtrude(c, st, 704, 703, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 704, "extrude");
        // step 937: guard_begin {"before":{"wp":704},"block":1,"g":117,"op":"guard_begin","skippable":true}
        atStep(st, 938);
        cqSelect("Edges")(c, st, 705, 704, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 939);
        cqFillet(c, st, 706, 705, {"radius": 0.499999});
        stepVol(c, st, 706, "fillet");
        // step 940: guard_end {"after":{"wp":706},"block":1,"g":117,"op":"guard_end"}
        atStep(st, 941);
        cqUnion(c, st, 707, 701, {"toUnion": {"wp": 706}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 707, "union");
        atStep(st, 942);
        cqWorkplaneNew(c, st, 708, {"plane": {"origin": [-6, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 943);
        cqRect(c, st, 709, 708, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 944);
        cqExtrude(c, st, 710, 709, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 710, "extrude");
        // step 945: guard_begin {"before":{"wp":710},"block":1,"g":118,"op":"guard_begin","skippable":true}
        atStep(st, 946);
        cqSelect("Edges")(c, st, 711, 710, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 947);
        cqFillet(c, st, 712, 711, {"radius": 0.499999});
        stepVol(c, st, 712, "fillet");
        // step 948: guard_end {"after":{"wp":712},"block":1,"g":118,"op":"guard_end"}
        atStep(st, 949);
        cqUnion(c, st, 713, 707, {"toUnion": {"wp": 712}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 713, "union");
        atStep(st, 950);
        cqWorkplaneNew(c, st, 714, {"plane": {"origin": [8, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 951);
        cqRect(c, st, 715, 714, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 952);
        cqExtrude(c, st, 716, 715, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 716, "extrude");
        // step 953: guard_begin {"before":{"wp":716},"block":1,"g":119,"op":"guard_begin","skippable":true}
        atStep(st, 954);
        cqSelect("Edges")(c, st, 717, 716, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 955);
        cqFillet(c, st, 718, 717, {"radius": 0.499999});
        stepVol(c, st, 718, "fillet");
        // step 956: guard_end {"after":{"wp":718},"block":1,"g":119,"op":"guard_end"}
        atStep(st, 957);
        cqUnion(c, st, 719, 713, {"toUnion": {"wp": 718}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 719, "union");
        atStep(st, 958);
        cqWorkplaneNew(c, st, 720, {"plane": {"origin": [6, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 959);
        cqRect(c, st, 721, 720, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 960);
        cqExtrude(c, st, 722, 721, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 722, "extrude");
        // step 961: guard_begin {"before":{"wp":722},"block":1,"g":120,"op":"guard_begin","skippable":true}
        atStep(st, 962);
        cqSelect("Edges")(c, st, 723, 722, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 963);
        cqFillet(c, st, 724, 723, {"radius": 0.499999});
        stepVol(c, st, 724, "fillet");
        // step 964: guard_end {"after":{"wp":724},"block":1,"g":120,"op":"guard_end"}
        atStep(st, 965);
        cqUnion(c, st, 725, 719, {"toUnion": {"wp": 724}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 725, "union");
        atStep(st, 966);
        cqWorkplaneNew(c, st, 726, {"plane": {"origin": [2, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 967);
        cqRect(c, st, 727, 726, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 968);
        cqExtrude(c, st, 728, 727, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 728, "extrude");
        // step 969: guard_begin {"before":{"wp":728},"block":1,"g":121,"op":"guard_begin","skippable":true}
        atStep(st, 970);
        cqSelect("Edges")(c, st, 729, 728, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 971);
        cqFillet(c, st, 730, 729, {"radius": 0.499999});
        stepVol(c, st, 730, "fillet");
        // step 972: guard_end {"after":{"wp":730},"block":1,"g":121,"op":"guard_end"}
        atStep(st, 973);
        cqUnion(c, st, 731, 725, {"toUnion": {"wp": 730}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 731, "union");
        atStep(st, 974);
        cqWorkplaneNew(c, st, 732, {"plane": {"origin": [16, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 975);
        cqRect(c, st, 733, 732, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 976);
        cqExtrude(c, st, 734, 733, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 734, "extrude");
        // step 977: guard_begin {"before":{"wp":734},"block":1,"g":122,"op":"guard_begin","skippable":true}
        atStep(st, 978);
        cqSelect("Edges")(c, st, 735, 734, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 979);
        cqFillet(c, st, 736, 735, {"radius": 0.499999});
        stepVol(c, st, 736, "fillet");
        // step 980: guard_end {"after":{"wp":736},"block":1,"g":122,"op":"guard_end"}
        atStep(st, 981);
        cqUnion(c, st, 737, 731, {"toUnion": {"wp": 736}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 737, "union");
        atStep(st, 982);
        cqWorkplaneNew(c, st, 738, {"plane": {"origin": [14, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 983);
        cqRect(c, st, 739, 738, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 984);
        cqExtrude(c, st, 740, 739, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 740, "extrude");
        // step 985: guard_begin {"before":{"wp":740},"block":1,"g":123,"op":"guard_begin","skippable":true}
        atStep(st, 986);
        cqSelect("Edges")(c, st, 741, 740, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 987);
        cqFillet(c, st, 742, 741, {"radius": 0.499999});
        stepVol(c, st, 742, "fillet");
        // step 988: guard_end {"after":{"wp":742},"block":1,"g":123,"op":"guard_end"}
        atStep(st, 989);
        cqUnion(c, st, 743, 737, {"toUnion": {"wp": 742}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 743, "union");
        atStep(st, 990);
        cqWorkplaneNew(c, st, 744, {"plane": {"origin": [10, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 991);
        cqRect(c, st, 745, 744, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 992);
        cqExtrude(c, st, 746, 745, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 746, "extrude");
        // step 993: guard_begin {"before":{"wp":746},"block":1,"g":124,"op":"guard_begin","skippable":true}
        atStep(st, 994);
        cqSelect("Edges")(c, st, 747, 746, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 995);
        cqFillet(c, st, 748, 747, {"radius": 0.499999});
        stepVol(c, st, 748, "fillet");
        // step 996: guard_end {"after":{"wp":748},"block":1,"g":124,"op":"guard_end"}
        atStep(st, 997);
        cqUnion(c, st, 749, 743, {"toUnion": {"wp": 748}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 749, "union");
        atStep(st, 998);
        cqWorkplaneNew(c, st, 750, {"plane": {"origin": [24, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 999);
        cqRect(c, st, 751, 750, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1000);
        cqExtrude(c, st, 752, 751, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 752, "extrude");
        // step 1001: guard_begin {"before":{"wp":752},"block":1,"g":125,"op":"guard_begin","skippable":true}
        atStep(st, 1002);
        cqSelect("Edges")(c, st, 753, 752, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1003);
        cqFillet(c, st, 754, 753, {"radius": 0.499999});
        stepVol(c, st, 754, "fillet");
        // step 1004: guard_end {"after":{"wp":754},"block":1,"g":125,"op":"guard_end"}
        atStep(st, 1005);
        cqUnion(c, st, 755, 749, {"toUnion": {"wp": 754}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 755, "union");
        atStep(st, 1006);
        cqWorkplaneNew(c, st, 756, {"plane": {"origin": [22, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1007);
        cqRect(c, st, 757, 756, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1008);
        cqExtrude(c, st, 758, 757, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 758, "extrude");
        // step 1009: guard_begin {"before":{"wp":758},"block":1,"g":126,"op":"guard_begin","skippable":true}
        atStep(st, 1010);
        cqSelect("Edges")(c, st, 759, 758, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1011);
        cqFillet(c, st, 760, 759, {"radius": 0.499999});
        stepVol(c, st, 760, "fillet");
        // step 1012: guard_end {"after":{"wp":760},"block":1,"g":126,"op":"guard_end"}
        atStep(st, 1013);
        cqUnion(c, st, 761, 755, {"toUnion": {"wp": 760}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 761, "union");
        atStep(st, 1014);
        cqWorkplaneNew(c, st, 762, {"plane": {"origin": [18, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1015);
        cqRect(c, st, 763, 762, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1016);
        cqExtrude(c, st, 764, 763, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 764, "extrude");
        // step 1017: guard_begin {"before":{"wp":764},"block":1,"g":127,"op":"guard_begin","skippable":true}
        atStep(st, 1018);
        cqSelect("Edges")(c, st, 765, 764, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1019);
        cqFillet(c, st, 766, 765, {"radius": 0.499999});
        stepVol(c, st, 766, "fillet");
        // step 1020: guard_end {"after":{"wp":766},"block":1,"g":127,"op":"guard_end"}
        atStep(st, 1021);
        cqUnion(c, st, 767, 761, {"toUnion": {"wp": 766}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 767, "union");
        atStep(st, 1022);
        cqWorkplaneNew(c, st, 768, {"plane": {"origin": [32, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1023);
        cqRect(c, st, 769, 768, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1024);
        cqExtrude(c, st, 770, 769, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 770, "extrude");
        // step 1025: guard_begin {"before":{"wp":770},"block":1,"g":128,"op":"guard_begin","skippable":true}
        atStep(st, 1026);
        cqSelect("Edges")(c, st, 771, 770, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1027);
        cqFillet(c, st, 772, 771, {"radius": 0.499999});
        stepVol(c, st, 772, "fillet");
        // step 1028: guard_end {"after":{"wp":772},"block":1,"g":128,"op":"guard_end"}
        atStep(st, 1029);
        cqUnion(c, st, 773, 767, {"toUnion": {"wp": 772}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 773, "union");
        atStep(st, 1030);
        cqWorkplaneNew(c, st, 774, {"plane": {"origin": [30, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1031);
        cqRect(c, st, 775, 774, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1032);
        cqExtrude(c, st, 776, 775, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 776, "extrude");
        // step 1033: guard_begin {"before":{"wp":776},"block":1,"g":129,"op":"guard_begin","skippable":true}
        atStep(st, 1034);
        cqSelect("Edges")(c, st, 777, 776, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1035);
        cqFillet(c, st, 778, 777, {"radius": 0.499999});
        stepVol(c, st, 778, "fillet");
        // step 1036: guard_end {"after":{"wp":778},"block":1,"g":129,"op":"guard_end"}
        atStep(st, 1037);
        cqUnion(c, st, 779, 773, {"toUnion": {"wp": 778}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 779, "union");
        atStep(st, 1038);
        cqWorkplaneNew(c, st, 780, {"plane": {"origin": [26, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1039);
        cqRect(c, st, 781, 780, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1040);
        cqExtrude(c, st, 782, 781, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 782, "extrude");
        // step 1041: guard_begin {"before":{"wp":782},"block":1,"g":130,"op":"guard_begin","skippable":true}
        atStep(st, 1042);
        cqSelect("Edges")(c, st, 783, 782, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1043);
        cqFillet(c, st, 784, 783, {"radius": 0.499999});
        stepVol(c, st, 784, "fillet");
        // step 1044: guard_end {"after":{"wp":784},"block":1,"g":130,"op":"guard_end"}
        atStep(st, 1045);
        cqUnion(c, st, 785, 779, {"toUnion": {"wp": 784}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 785, "union");
        atStep(st, 1046);
        cqWorkplaneNew(c, st, 786, {"plane": {"origin": [40, 6.928203, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1047);
        cqRect(c, st, 787, 786, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1048);
        cqExtrude(c, st, 788, 787, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 788, "extrude");
        // step 1049: guard_begin {"before":{"wp":788},"block":1,"g":131,"op":"guard_begin","skippable":true}
        atStep(st, 1050);
        cqSelect("Edges")(c, st, 789, 788, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1051);
        cqFillet(c, st, 790, 789, {"radius": 0.499999});
        stepVol(c, st, 790, "fillet");
        // step 1052: guard_end {"after":{"wp":790},"block":1,"g":131,"op":"guard_end"}
        atStep(st, 1053);
        cqUnion(c, st, 791, 785, {"toUnion": {"wp": 790}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 791, "union");
        atStep(st, 1054);
        cqWorkplaneNew(c, st, 792, {"plane": {"origin": [38, 10.3923045, -17.5], "xDir": [0.5000000000000001, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1055);
        cqRect(c, st, 793, 792, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1056);
        cqExtrude(c, st, 794, 793, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 794, "extrude");
        // step 1057: guard_begin {"before":{"wp":794},"block":1,"g":132,"op":"guard_begin","skippable":true}
        atStep(st, 1058);
        cqSelect("Edges")(c, st, 795, 794, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1059);
        cqFillet(c, st, 796, 795, {"radius": 0.499999});
        stepVol(c, st, 796, "fillet");
        // step 1060: guard_end {"after":{"wp":796},"block":1,"g":132,"op":"guard_end"}
        atStep(st, 1061);
        cqUnion(c, st, 797, 791, {"toUnion": {"wp": 796}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 797, "union");
        atStep(st, 1062);
        cqWorkplaneNew(c, st, 798, {"plane": {"origin": [34, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1063);
        cqRect(c, st, 799, 798, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1064);
        cqExtrude(c, st, 800, 799, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 800, "extrude");
        // step 1065: guard_begin {"before":{"wp":800},"block":1,"g":133,"op":"guard_begin","skippable":true}
        atStep(st, 1066);
        cqSelect("Edges")(c, st, 801, 800, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1067);
        cqFillet(c, st, 802, 801, {"radius": 0.499999});
        stepVol(c, st, 802, "fillet");
        // step 1068: guard_end {"after":{"wp":802},"block":1,"g":133,"op":"guard_end"}
        atStep(st, 1069);
        cqUnion(c, st, 803, 797, {"toUnion": {"wp": 802}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 803, "union");
        atStep(st, 1070);
        cqWorkplaneNew(c, st, 804, {"plane": {"origin": [42, 10.3923045, -17.5], "xDir": [-0.5000000000000003, 0.8660254037844386, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1071);
        cqRect(c, st, 805, 804, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1072);
        cqExtrude(c, st, 806, 805, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 806, "extrude");
        // step 1073: guard_begin {"before":{"wp":806},"block":1,"g":134,"op":"guard_begin","skippable":true}
        atStep(st, 1074);
        cqSelect("Edges")(c, st, 807, 806, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1075);
        cqFillet(c, st, 808, 807, {"radius": 0.499999});
        stepVol(c, st, 808, "fillet");
        // step 1076: guard_end {"after":{"wp":808},"block":1,"g":134,"op":"guard_end"}
        atStep(st, 1077);
        cqUnion(c, st, 809, 803, {"toUnion": {"wp": 808}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 809, "union");
        atStep(st, 1078);
        cqWorkplaneNew(c, st, 810, {"plane": {"origin": [-36, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1079);
        cqRect(c, st, 811, 810, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1080);
        cqExtrude(c, st, 812, 811, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 812, "extrude");
        // step 1081: guard_begin {"before":{"wp":812},"block":1,"g":135,"op":"guard_begin","skippable":true}
        atStep(st, 1082);
        cqSelect("Edges")(c, st, 813, 812, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1083);
        cqFillet(c, st, 814, 813, {"radius": 0.499999});
        stepVol(c, st, 814, "fillet");
        // step 1084: guard_end {"after":{"wp":814},"block":1,"g":135,"op":"guard_end"}
        atStep(st, 1085);
        cqUnion(c, st, 815, 809, {"toUnion": {"wp": 814}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 815, "union");
        atStep(st, 1086);
        cqWorkplaneNew(c, st, 816, {"plane": {"origin": [-28, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1087);
        cqRect(c, st, 817, 816, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1088);
        cqExtrude(c, st, 818, 817, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 818, "extrude");
        // step 1089: guard_begin {"before":{"wp":818},"block":1,"g":136,"op":"guard_begin","skippable":true}
        atStep(st, 1090);
        cqSelect("Edges")(c, st, 819, 818, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1091);
        cqFillet(c, st, 820, 819, {"radius": 0.499999});
        stepVol(c, st, 820, "fillet");
        // step 1092: guard_end {"after":{"wp":820},"block":1,"g":136,"op":"guard_end"}
        atStep(st, 1093);
        cqUnion(c, st, 821, 815, {"toUnion": {"wp": 820}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 821, "union");
        atStep(st, 1094);
        cqWorkplaneNew(c, st, 822, {"plane": {"origin": [-20, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1095);
        cqRect(c, st, 823, 822, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1096);
        cqExtrude(c, st, 824, 823, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 824, "extrude");
        // step 1097: guard_begin {"before":{"wp":824},"block":1,"g":137,"op":"guard_begin","skippable":true}
        atStep(st, 1098);
        cqSelect("Edges")(c, st, 825, 824, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1099);
        cqFillet(c, st, 826, 825, {"radius": 0.499999});
        stepVol(c, st, 826, "fillet");
        // step 1100: guard_end {"after":{"wp":826},"block":1,"g":137,"op":"guard_end"}
        atStep(st, 1101);
        cqUnion(c, st, 827, 821, {"toUnion": {"wp": 826}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 827, "union");
        atStep(st, 1102);
        cqWorkplaneNew(c, st, 828, {"plane": {"origin": [-12, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1103);
        cqRect(c, st, 829, 828, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1104);
        cqExtrude(c, st, 830, 829, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 830, "extrude");
        // step 1105: guard_begin {"before":{"wp":830},"block":1,"g":138,"op":"guard_begin","skippable":true}
        atStep(st, 1106);
        cqSelect("Edges")(c, st, 831, 830, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1107);
        cqFillet(c, st, 832, 831, {"radius": 0.499999});
        stepVol(c, st, 832, "fillet");
        // step 1108: guard_end {"after":{"wp":832},"block":1,"g":138,"op":"guard_end"}
        atStep(st, 1109);
        cqUnion(c, st, 833, 827, {"toUnion": {"wp": 832}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 833, "union");
        atStep(st, 1110);
        cqWorkplaneNew(c, st, 834, {"plane": {"origin": [-4, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1111);
        cqRect(c, st, 835, 834, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1112);
        cqExtrude(c, st, 836, 835, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 836, "extrude");
        // step 1113: guard_begin {"before":{"wp":836},"block":1,"g":139,"op":"guard_begin","skippable":true}
        atStep(st, 1114);
        cqSelect("Edges")(c, st, 837, 836, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1115);
        cqFillet(c, st, 838, 837, {"radius": 0.499999});
        stepVol(c, st, 838, "fillet");
        // step 1116: guard_end {"after":{"wp":838},"block":1,"g":139,"op":"guard_end"}
        atStep(st, 1117);
        cqUnion(c, st, 839, 833, {"toUnion": {"wp": 838}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 839, "union");
        atStep(st, 1118);
        cqWorkplaneNew(c, st, 840, {"plane": {"origin": [4, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1119);
        cqRect(c, st, 841, 840, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1120);
        cqExtrude(c, st, 842, 841, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 842, "extrude");
        // step 1121: guard_begin {"before":{"wp":842},"block":1,"g":140,"op":"guard_begin","skippable":true}
        atStep(st, 1122);
        cqSelect("Edges")(c, st, 843, 842, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1123);
        cqFillet(c, st, 844, 843, {"radius": 0.499999});
        stepVol(c, st, 844, "fillet");
        // step 1124: guard_end {"after":{"wp":844},"block":1,"g":140,"op":"guard_end"}
        atStep(st, 1125);
        cqUnion(c, st, 845, 839, {"toUnion": {"wp": 844}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 845, "union");
        atStep(st, 1126);
        cqWorkplaneNew(c, st, 846, {"plane": {"origin": [12, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1127);
        cqRect(c, st, 847, 846, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1128);
        cqExtrude(c, st, 848, 847, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 848, "extrude");
        // step 1129: guard_begin {"before":{"wp":848},"block":1,"g":141,"op":"guard_begin","skippable":true}
        atStep(st, 1130);
        cqSelect("Edges")(c, st, 849, 848, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1131);
        cqFillet(c, st, 850, 849, {"radius": 0.499999});
        stepVol(c, st, 850, "fillet");
        // step 1132: guard_end {"after":{"wp":850},"block":1,"g":141,"op":"guard_end"}
        atStep(st, 1133);
        cqUnion(c, st, 851, 845, {"toUnion": {"wp": 850}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 851, "union");
        atStep(st, 1134);
        cqWorkplaneNew(c, st, 852, {"plane": {"origin": [20, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1135);
        cqRect(c, st, 853, 852, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1136);
        cqExtrude(c, st, 854, 853, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 854, "extrude");
        // step 1137: guard_begin {"before":{"wp":854},"block":1,"g":142,"op":"guard_begin","skippable":true}
        atStep(st, 1138);
        cqSelect("Edges")(c, st, 855, 854, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1139);
        cqFillet(c, st, 856, 855, {"radius": 0.499999});
        stepVol(c, st, 856, "fillet");
        // step 1140: guard_end {"after":{"wp":856},"block":1,"g":142,"op":"guard_end"}
        atStep(st, 1141);
        cqUnion(c, st, 857, 851, {"toUnion": {"wp": 856}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 857, "union");
        atStep(st, 1142);
        cqWorkplaneNew(c, st, 858, {"plane": {"origin": [28, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1143);
        cqRect(c, st, 859, 858, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1144);
        cqExtrude(c, st, 860, 859, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 860, "extrude");
        // step 1145: guard_begin {"before":{"wp":860},"block":1,"g":143,"op":"guard_begin","skippable":true}
        atStep(st, 1146);
        cqSelect("Edges")(c, st, 861, 860, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1147);
        cqFillet(c, st, 862, 861, {"radius": 0.499999});
        stepVol(c, st, 862, "fillet");
        // step 1148: guard_end {"after":{"wp":862},"block":1,"g":143,"op":"guard_end"}
        atStep(st, 1149);
        cqUnion(c, st, 863, 857, {"toUnion": {"wp": 862}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 863, "union");
        atStep(st, 1150);
        cqWorkplaneNew(c, st, 864, {"plane": {"origin": [36, 13.856406, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1151);
        cqRect(c, st, 865, 864, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1152);
        cqExtrude(c, st, 866, 865, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 866, "extrude");
        // step 1153: guard_begin {"before":{"wp":866},"block":1,"g":144,"op":"guard_begin","skippable":true}
        atStep(st, 1154);
        cqSelect("Edges")(c, st, 867, 866, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1155);
        cqFillet(c, st, 868, 867, {"radius": 0.499999});
        stepVol(c, st, 868, "fillet");
        // step 1156: guard_end {"after":{"wp":868},"block":1,"g":144,"op":"guard_end"}
        atStep(st, 1157);
        cqUnion(c, st, 869, 863, {"toUnion": {"wp": 868}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 869, "union");
        atStep(st, 1158);
        cqWorkplaneNew(c, st, 870, {"plane": {"origin": [-32, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1159);
        cqRect(c, st, 871, 870, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1160);
        cqExtrude(c, st, 872, 871, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 872, "extrude");
        // step 1161: guard_begin {"before":{"wp":872},"block":1,"g":145,"op":"guard_begin","skippable":true}
        atStep(st, 1162);
        cqSelect("Edges")(c, st, 873, 872, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1163);
        cqFillet(c, st, 874, 873, {"radius": 0.499999});
        stepVol(c, st, 874, "fillet");
        // step 1164: guard_end {"after":{"wp":874},"block":1,"g":145,"op":"guard_end"}
        atStep(st, 1165);
        cqUnion(c, st, 875, 869, {"toUnion": {"wp": 874}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 875, "union");
        atStep(st, 1166);
        cqWorkplaneNew(c, st, 876, {"plane": {"origin": [-24, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1167);
        cqRect(c, st, 877, 876, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1168);
        cqExtrude(c, st, 878, 877, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 878, "extrude");
        // step 1169: guard_begin {"before":{"wp":878},"block":1,"g":146,"op":"guard_begin","skippable":true}
        atStep(st, 1170);
        cqSelect("Edges")(c, st, 879, 878, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1171);
        cqFillet(c, st, 880, 879, {"radius": 0.499999});
        stepVol(c, st, 880, "fillet");
        // step 1172: guard_end {"after":{"wp":880},"block":1,"g":146,"op":"guard_end"}
        atStep(st, 1173);
        cqUnion(c, st, 881, 875, {"toUnion": {"wp": 880}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 881, "union");
        atStep(st, 1174);
        cqWorkplaneNew(c, st, 882, {"plane": {"origin": [-16, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1175);
        cqRect(c, st, 883, 882, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1176);
        cqExtrude(c, st, 884, 883, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 884, "extrude");
        // step 1177: guard_begin {"before":{"wp":884},"block":1,"g":147,"op":"guard_begin","skippable":true}
        atStep(st, 1178);
        cqSelect("Edges")(c, st, 885, 884, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1179);
        cqFillet(c, st, 886, 885, {"radius": 0.499999});
        stepVol(c, st, 886, "fillet");
        // step 1180: guard_end {"after":{"wp":886},"block":1,"g":147,"op":"guard_end"}
        atStep(st, 1181);
        cqUnion(c, st, 887, 881, {"toUnion": {"wp": 886}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 887, "union");
        atStep(st, 1182);
        cqWorkplaneNew(c, st, 888, {"plane": {"origin": [-8, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1183);
        cqRect(c, st, 889, 888, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1184);
        cqExtrude(c, st, 890, 889, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 890, "extrude");
        // step 1185: guard_begin {"before":{"wp":890},"block":1,"g":148,"op":"guard_begin","skippable":true}
        atStep(st, 1186);
        cqSelect("Edges")(c, st, 891, 890, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1187);
        cqFillet(c, st, 892, 891, {"radius": 0.499999});
        stepVol(c, st, 892, "fillet");
        // step 1188: guard_end {"after":{"wp":892},"block":1,"g":148,"op":"guard_end"}
        atStep(st, 1189);
        cqUnion(c, st, 893, 887, {"toUnion": {"wp": 892}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 893, "union");
        atStep(st, 1190);
        cqWorkplaneNew(c, st, 894, {"plane": {"origin": [0, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1191);
        cqRect(c, st, 895, 894, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1192);
        cqExtrude(c, st, 896, 895, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 896, "extrude");
        // step 1193: guard_begin {"before":{"wp":896},"block":1,"g":149,"op":"guard_begin","skippable":true}
        atStep(st, 1194);
        cqSelect("Edges")(c, st, 897, 896, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1195);
        cqFillet(c, st, 898, 897, {"radius": 0.499999});
        stepVol(c, st, 898, "fillet");
        // step 1196: guard_end {"after":{"wp":898},"block":1,"g":149,"op":"guard_end"}
        atStep(st, 1197);
        cqUnion(c, st, 899, 893, {"toUnion": {"wp": 898}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 899, "union");
        atStep(st, 1198);
        cqWorkplaneNew(c, st, 900, {"plane": {"origin": [8, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1199);
        cqRect(c, st, 901, 900, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1200);
        cqExtrude(c, st, 902, 901, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 902, "extrude");
        // step 1201: guard_begin {"before":{"wp":902},"block":1,"g":150,"op":"guard_begin","skippable":true}
        atStep(st, 1202);
        cqSelect("Edges")(c, st, 903, 902, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1203);
        cqFillet(c, st, 904, 903, {"radius": 0.499999});
        stepVol(c, st, 904, "fillet");
        // step 1204: guard_end {"after":{"wp":904},"block":1,"g":150,"op":"guard_end"}
        atStep(st, 1205);
        cqUnion(c, st, 905, 899, {"toUnion": {"wp": 904}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 905, "union");
        atStep(st, 1206);
        cqWorkplaneNew(c, st, 906, {"plane": {"origin": [16, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1207);
        cqRect(c, st, 907, 906, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1208);
        cqExtrude(c, st, 908, 907, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 908, "extrude");
        // step 1209: guard_begin {"before":{"wp":908},"block":1,"g":151,"op":"guard_begin","skippable":true}
        atStep(st, 1210);
        cqSelect("Edges")(c, st, 909, 908, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1211);
        cqFillet(c, st, 910, 909, {"radius": 0.499999});
        stepVol(c, st, 910, "fillet");
        // step 1212: guard_end {"after":{"wp":910},"block":1,"g":151,"op":"guard_end"}
        atStep(st, 1213);
        cqUnion(c, st, 911, 905, {"toUnion": {"wp": 910}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 911, "union");
        atStep(st, 1214);
        cqWorkplaneNew(c, st, 912, {"plane": {"origin": [24, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1215);
        cqRect(c, st, 913, 912, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1216);
        cqExtrude(c, st, 914, 913, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 914, "extrude");
        // step 1217: guard_begin {"before":{"wp":914},"block":1,"g":152,"op":"guard_begin","skippable":true}
        atStep(st, 1218);
        cqSelect("Edges")(c, st, 915, 914, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1219);
        cqFillet(c, st, 916, 915, {"radius": 0.499999});
        stepVol(c, st, 916, "fillet");
        // step 1220: guard_end {"after":{"wp":916},"block":1,"g":152,"op":"guard_end"}
        atStep(st, 1221);
        cqUnion(c, st, 917, 911, {"toUnion": {"wp": 916}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 917, "union");
        atStep(st, 1222);
        cqWorkplaneNew(c, st, 918, {"plane": {"origin": [32, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1223);
        cqRect(c, st, 919, 918, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1224);
        cqExtrude(c, st, 920, 919, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 920, "extrude");
        // step 1225: guard_begin {"before":{"wp":920},"block":1,"g":153,"op":"guard_begin","skippable":true}
        atStep(st, 1226);
        cqSelect("Edges")(c, st, 921, 920, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1227);
        cqFillet(c, st, 922, 921, {"radius": 0.499999});
        stepVol(c, st, 922, "fillet");
        // step 1228: guard_end {"after":{"wp":922},"block":1,"g":153,"op":"guard_end"}
        atStep(st, 1229);
        cqUnion(c, st, 923, 917, {"toUnion": {"wp": 922}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 923, "union");
        atStep(st, 1230);
        cqWorkplaneNew(c, st, 924, {"plane": {"origin": [40, 20.78461, -17.5], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 1231);
        cqRect(c, st, 925, 924, {"xLen": 8, "yLen": 1, "centered": true, "forConstruction": false});
        atStep(st, 1232);
        cqExtrude(c, st, 926, 925, {"until": 35, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 926, "extrude");
        // step 1233: guard_begin {"before":{"wp":926},"block":1,"g":154,"op":"guard_begin","skippable":true}
        atStep(st, 1234);
        cqSelect("Edges")(c, st, 927, 926, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 1235);
        cqFillet(c, st, 928, 927, {"radius": 0.499999});
        stepVol(c, st, 928, "fillet");
        // step 1236: guard_end {"after":{"wp":928},"block":1,"g":154,"op":"guard_end"}
        atStep(st, 1237);
        cqUnion(c, st, 929, 923, {"toUnion": {"wp": 928}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 929, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 929}), {"i": 4313});
    } catch (e) {
        return {"i": 4313, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c4313"] = case_4313();
    return out;
}
