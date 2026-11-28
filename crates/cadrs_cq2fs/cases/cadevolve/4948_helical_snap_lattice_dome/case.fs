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

const case_4948 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 2);
        cqMoveTo(c, st, 2, 1, {"x": 11.958260743101398, "y": 1});
        atStep(st, 3);
        cqThreePointArc(c, st, 3, 2, {"point1": [0, 12], "point2": [-11.958260743101398, 1], "forConstruction": false});
        atStep(st, 4);
        cqLineTo(c, st, 4, 3, {"x": -10.954451150103322, "y": 1, "forConstruction": false});
        atStep(st, 5);
        cqThreePointArc(c, st, 5, 4, {"point1": [0, 11], "point2": [10.954451150103322, 1], "forConstruction": false});
        atStep(st, 6);
        cqClose(c, st, 6, 5, {});
        atStep(st, 7);
        cqRevolve(c, st, 7, 6, {"angleDegrees": 360, "axisStart": [0, 0, 0], "axisEnd": [0, 0, 1], "combine": true, "clean": true});
        stepVol(c, st, 7, "revolve");
        atStep(st, 8);
        cqWorkplaneNew(c, st, 8, {"plane": {"origin": [2.29017990487849, 1.2019791374290816, 11.717948717948715], "xDir": [-0.4647231720437685, 0.88545602565321, 0], "zDir": [0.19084832540654084, 0.10016492811909011, 0.9764957264957264]}, "obj": undefined});
        atStep(st, 9);
        cqCircle(c, st, 9, 8, {"radius": 2, "forConstruction": false});
        atStep(st, 10);
        cqExtrude(c, st, 10, 9, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 10, "extrude");
        atStep(st, 11);
        cqCut(c, st, 11, 7, {"toCut": {"wp": 10}, "clean": true, "tol": undefined});
        stepVol(c, st, 11, "cut");
        atStep(st, 12);
        cqWorkplaneNew(c, st, 12, {"plane": {"origin": [2.0654641894735417, 2.992341477444852, 11.435897435897436], "xDir": [-0.8229838658936564, 0.568064746731156, 0], "zDir": [0.17212201578946182, 0.249361789787071, 0.952991452991453]}, "obj": undefined});
        atStep(st, 13);
        cqCircle(c, st, 13, 12, {"radius": 2, "forConstruction": false});
        atStep(st, 14);
        cqExtrude(c, st, 14, 13, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 14, "extrude");
        atStep(st, 15);
        cqCut(c, st, 15, 11, {"toCut": {"wp": 14}, "clean": true, "tol": undefined});
        stepVol(c, st, 15, "cut");
        atStep(st, 16);
        cqWorkplaneNew(c, st, 16, {"plane": {"origin": [0.5335258474418876, 4.393980672060671, 11.153846153846157], "xDir": [-0.992708874098054, 0.120536680255323, 0], "zDir": [0.04446048728682397, 0.36616505600505594, 0.9294871794871796]}, "obj": undefined});
        atStep(st, 17);
        cqCircle(c, st, 17, 16, {"radius": 2, "forConstruction": false});
        atStep(st, 18);
        cqExtrude(c, st, 18, 17, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 18, "extrude");
        atStep(st, 19);
        cqCut(c, st, 19, 15, {"toCut": {"wp": 18}, "clean": true, "tol": undefined});
        stepVol(c, st, 19, "cut");
        atStep(st, 20);
        cqWorkplaneNew(c, st, 20, {"plane": {"origin": [-1.801311721243062, 4.7496686567093365, 10.871794871794872], "xDir": [-0.9350162426854148, -0.35460488704253545, 0], "zDir": [-0.15010931010358852, 0.39580572139244474, 0.905982905982906]}, "obj": undefined});
        atStep(st, 21);
        cqCircle(c, st, 21, 20, {"radius": 2, "forConstruction": false});
        atStep(st, 22);
        cqExtrude(c, st, 22, 21, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 22, "extrude");
        atStep(st, 23);
        cqCut(c, st, 23, 19, {"toCut": {"wp": 22}, "clean": true, "tol": undefined});
        stepVol(c, st, 23, "cut");
        atStep(st, 24);
        cqWorkplaneNew(c, st, 24, {"plane": {"origin": [-4.22476671475944, 3.7428167079547574, 10.58974358974359], "xDir": [-0.6631226582407955, -0.748510748171101, 0], "zDir": [-0.35206389289662, 0.3119013923295631, 0.8824786324786325]}, "obj": undefined});
        atStep(st, 25);
        cqCircle(c, st, 25, 24, {"radius": 2, "forConstruction": false});
        atStep(st, 26);
        cqExtrude(c, st, 26, 25, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 26, "extrude");
        atStep(st, 27);
        cqCut(c, st, 27, 23, {"toCut": {"wp": 26}, "clean": true, "tol": undefined});
        stepVol(c, st, 27, "cut");
        atStep(st, 28);
        cqWorkplaneNew(c, st, 28, {"plane": {"origin": [-5.965683284609621, 1.4704088675161673, 10.307692307692308], "xDir": [-0.2393156642875577, -0.970941817426052, -1.387778780781446e-17], "zDir": [-0.4971402737174684, 0.12253407229301394, 0.858974358974359]}, "obj": undefined});
        atStep(st, 29);
        cqCircle(c, st, 29, 28, {"radius": 2, "forConstruction": false});
        atStep(st, 30);
        cqExtrude(c, st, 30, 29, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 30, "extrude");
        atStep(st, 31);
        cqCut(c, st, 31, 27, {"toCut": {"wp": 30}, "clean": true, "tol": undefined});
        stepVol(c, st, 31, "cut");
        atStep(st, 32);
        cqWorkplaneNew(c, st, 32, {"plane": {"origin": [-6.402808953369854, -1.5781506682286115, 10.025641025641026], "xDir": [0.23931566428755743, -0.9709418174260522, 0], "zDir": [-0.5335674127808211, -0.13151255568571765, 0.8354700854700855]}, "obj": undefined});
        atStep(st, 33);
        cqCircle(c, st, 33, 32, {"radius": 2, "forConstruction": false});
        atStep(st, 34);
        cqExtrude(c, st, 34, 33, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 34, "extrude");
        atStep(st, 35);
        cqCut(c, st, 35, 31, {"toCut": {"wp": 34}, "clean": true, "tol": undefined});
        stepVol(c, st, 35, "cut");
        atStep(st, 36);
        cqWorkplaneNew(c, st, 36, {"plane": {"origin": [-5.24291354261255, -4.644816088210885, 9.743589743589745], "xDir": [0.663122658240795, -0.7485107481711013, 0], "zDir": [-0.4369094618843792, -0.38706800735090713, 0.8119658119658121]}, "obj": undefined});
        atStep(st, 37);
        cqCircle(c, st, 37, 36, {"radius": 2, "forConstruction": false});
        atStep(st, 38);
        cqExtrude(c, st, 38, 37, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 38, "extrude");
        atStep(st, 39);
        cqCut(c, st, 39, 35, {"toCut": {"wp": 38}, "clean": true, "tol": undefined});
        stepVol(c, st, 39, "cut");
        atStep(st, 40);
        cqWorkplaneNew(c, st, 40, {"plane": {"origin": [-2.617341768606903, -6.901363054291096, 9.46153846153846], "xDir": [0.9350162426854148, -0.3546048870425359, 0], "zDir": [-0.21811181405057525, -0.5751135878575914, 0.7884615384615383]}, "obj": undefined});
        atStep(st, 41);
        cqCircle(c, st, 41, 40, {"radius": 2, "forConstruction": false});
        atStep(st, 42);
        cqExtrude(c, st, 42, 41, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 42, "extrude");
        atStep(st, 43);
        cqCut(c, st, 43, 39, {"toCut": {"wp": 42}, "clean": true, "tol": undefined});
        stepVol(c, st, 43, "cut");
        atStep(st, 44);
        cqWorkplaneNew(c, st, 44, {"plane": {"origin": [0.9316246808202888, -7.672619538053286, 9.17948717948718], "xDir": [0.992708874098054, 0.12053668025532233, 0], "zDir": [0.07763539006835739, -0.6393849615044405, 0.7649572649572649]}, "obj": undefined});
        atStep(st, 45);
        cqCircle(c, st, 45, 44, {"radius": 2, "forConstruction": false});
        atStep(st, 46);
        cqExtrude(c, st, 46, 45, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 46, "extrude");
        atStep(st, 47);
        cqCut(c, st, 47, 43, {"toCut": {"wp": 46}, "clean": true, "tol": undefined});
        stepVol(c, st, 47, "cut");
        atStep(st, 48);
        cqWorkplaneNew(c, st, 48, {"plane": {"origin": [4.5740901619985745, -6.626713638065774, 8.897435897435898], "xDir": [0.8229838658936566, 0.5680647467311557, 0], "zDir": [0.38117418016654786, -0.5522261365054811, 0.7414529914529915]}, "obj": undefined});
        atStep(st, 49);
        cqCircle(c, st, 49, 48, {"radius": 2, "forConstruction": false});
        atStep(st, 50);
        cqExtrude(c, st, 50, 49, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 50, "extrude");
        atStep(st, 51);
        cqCut(c, st, 51, 47, {"toCut": {"wp": 50}, "clean": true, "tol": undefined});
        stepVol(c, st, 51, "cut");
        atStep(st, 52);
        cqWorkplaneNew(c, st, 52, {"plane": {"origin": [7.3963487500728045, -3.8819032827068938, 8.615384615384615], "xDir": [0.46472317204376834, 0.88545602565321, 0], "zDir": [0.6163623958394003, -0.3234919402255744, 0.717948717948718]}, "obj": undefined});
        atStep(st, 53);
        cqCircle(c, st, 53, 52, {"radius": 2, "forConstruction": false});
        atStep(st, 54);
        cqExtrude(c, st, 54, 53, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 54, "extrude");
        atStep(st, 55);
        cqCut(c, st, 55, 51, {"toCut": {"wp": 54}, "clean": true, "tol": undefined});
        stepVol(c, st, 55, "cut");
        atStep(st, 56);
        cqWorkplaneNew(c, st, 56, {"plane": {"origin": [8.634555897992412, -2.1148562485070615e-15, 8.333333333333334], "xDir": [2.449293598294707e-16, 1, 2.465190328815662e-32], "zDir": [0.719546324832701, -1.762380207089218e-16, 0.6944444444444445]}, "obj": undefined});
        atStep(st, 57);
        cqCircle(c, st, 57, 56, {"radius": 2, "forConstruction": false});
        atStep(st, 58);
        cqExtrude(c, st, 58, 57, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 58, "extrude");
        atStep(st, 59);
        cqCut(c, st, 59, 55, {"toCut": {"wp": 58}, "clean": true, "tol": undefined});
        stepVol(c, st, 59, "cut");
        atStep(st, 60);
        cqWorkplaneNew(c, st, 60, {"plane": {"origin": [7.8789097829978525, 4.135170850410444, 8.051282051282051], "xDir": [-0.4647231720437679, 0.8854560256532102, 0], "zDir": [0.656575815249821, 0.344597570867537, 0.6709401709401709]}, "obj": undefined});
        atStep(st, 61);
        cqCircle(c, st, 61, 60, {"radius": 2, "forConstruction": false});
        atStep(st, 62);
        cqExtrude(c, st, 62, 61, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 62, "extrude");
        atStep(st, 63);
        cqCut(c, st, 63, 59, {"toCut": {"wp": 62}, "clean": true, "tol": undefined});
        stepVol(c, st, 63, "cut");
        atStep(st, 64);
        cqWorkplaneNew(c, st, 64, {"plane": {"origin": [5.195201450511969, 7.526548687349082, 7.76923076923077], "xDir": [-0.8229838658936564, 0.568064746731156, 0], "zDir": [0.4329334542093308, 0.6272123906124235, 0.6474358974358975]}, "obj": undefined});
        atStep(st, 65);
        cqCircle(c, st, 65, 64, {"radius": 2, "forConstruction": false});
        atStep(st, 66);
        cqExtrude(c, st, 66, 65, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 66, "extrude");
        atStep(st, 67);
        cqCut(c, st, 67, 63, {"toCut": {"wp": 66}, "clean": true, "tol": undefined});
        stepVol(c, st, 67, "cut");
        atStep(st, 68);
        cqWorkplaneNew(c, st, 68, {"plane": {"origin": [1.13036252077227, 9.309372905754287, 7.487179487179487], "xDir": [-0.992708874098054, 0.12053668025532369, 6.93889390390723e-18], "zDir": [0.0941968767310225, 0.7757810754795241, 0.6239316239316239]}, "obj": undefined});
        atStep(st, 69);
        cqCircle(c, st, 69, 68, {"radius": 2, "forConstruction": false});
        atStep(st, 70);
        cqExtrude(c, st, 70, 69, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 70, "extrude");
        atStep(st, 71);
        cqCut(c, st, 71, 67, {"toCut": {"wp": 70}, "clean": true, "tol": undefined});
        stepVol(c, st, 71, "cut");
        atStep(st, 72);
        cqWorkplaneNew(c, st, 72, {"plane": {"origin": [-3.4028422914368996, 8.97255771155998, 7.205128205128205], "xDir": [-0.9350162426854148, -0.35460488704253545, 0], "zDir": [-0.28357019095307495, 0.7477131426299984, 0.6004273504273504]}, "obj": undefined});
        atStep(st, 73);
        cqCircle(c, st, 73, 72, {"radius": 2, "forConstruction": false});
        atStep(st, 74);
        cqExtrude(c, st, 74, 73, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 74, "extrude");
        atStep(st, 75);
        cqCut(c, st, 75, 71, {"toCut": {"wp": 74}, "clean": true, "tol": undefined});
        stepVol(c, st, 75, "cut");
        atStep(st, 76);
        cqWorkplaneNew(c, st, 76, {"plane": {"origin": [-7.336589331535587, 6.49965098274961, 6.9230769230769225], "xDir": [-0.6631226582407957, -0.7485107481711007, -2.7755575615628907e-17], "zDir": [-0.6113824442946323, 0.5416375818958009, 0.576923076923077]}, "obj": undefined});
        atStep(st, 77);
        cqCircle(c, st, 77, 76, {"radius": 2, "forConstruction": false});
        atStep(st, 78);
        cqExtrude(c, st, 78, 77, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 78, "extrude");
        atStep(st, 79);
        cqCut(c, st, 79, 75, {"toCut": {"wp": 78}, "clean": true, "tol": undefined});
        stepVol(c, st, 79, "cut");
        atStep(st, 80);
        cqWorkplaneNew(c, st, 80, {"plane": {"origin": [-9.704405773772724, 2.391921197114696, 6.6410256410256405], "xDir": [-0.2393156642875588, -0.970941817426052, 1.3877787807814457e-17], "zDir": [-0.8087004811477271, 0.19932676642622468, 0.5534188034188033]}, "obj": undefined});
        atStep(st, 81);
        cqCircle(c, st, 81, 80, {"radius": 2, "forConstruction": false});
        atStep(st, 82);
        cqExtrude(c, st, 82, 81, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 82, "extrude");
        atStep(st, 83);
        cqCut(c, st, 83, 79, {"toCut": {"wp": 82}, "clean": true, "tol": undefined});
        stepVol(c, st, 83, "cut");
        atStep(st, 84);
        cqWorkplaneNew(c, st, 84, {"plane": {"origin": [-9.88089878515096, -2.4354228173992563, 6.3589743589743595], "xDir": [0.2393156642875564, -0.9709418174260523, 1.3877787807814457e-17], "zDir": [-0.8234082320959133, -0.202951901449938, 0.5299145299145299]}, "obj": undefined});
        atStep(st, 85);
        cqCircle(c, st, 85, 84, {"radius": 2, "forConstruction": false});
        atStep(st, 86);
        cqExtrude(c, st, 86, 85, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 86, "extrude");
        atStep(st, 87);
        cqCut(c, st, 87, 83, {"toCut": {"wp": 86}, "clean": true, "tol": undefined});
        stepVol(c, st, 87, "cut");
        atStep(st, 88);
        cqWorkplaneNew(c, st, 88, {"plane": {"origin": [-7.745223996825795, -6.861669706139579, 6.0769230769230775], "xDir": [0.6631226582407952, -0.7485107481711012, 0], "zDir": [-0.6454353330688162, -0.5718058088449649, 0.5064102564102565]}, "obj": undefined});
        atStep(st, 89);
        cqCircle(c, st, 89, 88, {"radius": 2, "forConstruction": false});
        atStep(st, 90);
        cqExtrude(c, st, 90, 89, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 90, "extrude");
        atStep(st, 91);
        cqCut(c, st, 91, 87, {"toCut": {"wp": 90}, "clean": true, "tol": undefined});
        stepVol(c, st, 91, "cut");
        atStep(st, 92);
        cqWorkplaneNew(c, st, 92, {"plane": {"origin": [-3.7262117226690816, -9.825212826134752, 5.794871794871795], "xDir": [0.9350162426854146, -0.3546048870425361, 0], "zDir": [-0.3105176435557568, -0.8187677355112294, 0.4829059829059829]}, "obj": undefined});
        atStep(st, 93);
        cqCircle(c, st, 93, 92, {"radius": 2, "forConstruction": false});
        atStep(st, 94);
        cqExtrude(c, st, 94, 93, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 94, "extrude");
        atStep(st, 95);
        cqCut(c, st, 95, 91, {"toCut": {"wp": 94}, "clean": true, "tol": undefined});
        stepVol(c, st, 95, "cut");
        atStep(st, 96);
        cqWorkplaneNew(c, st, 96, {"plane": {"origin": [1.2847695401895756, -10.581029119231571, 5.512820512820513], "xDir": [0.992708874098054, 0.12053668025532208, -6.9388939039072276e-18], "zDir": [0.1070641283491313, -0.8817524266026311, 0.4594017094017094]}, "obj": undefined});
        atStep(st, 97);
        cqCircle(c, st, 97, 96, {"radius": 2, "forConstruction": false});
        atStep(st, 98);
        cqExtrude(c, st, 98, 97, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 98, "extrude");
        atStep(st, 99);
        cqCut(c, st, 99, 95, {"toCut": {"wp": 98}, "clean": true, "tol": undefined});
        stepVol(c, st, 99, "cut");
        atStep(st, 100);
        cqWorkplaneNew(c, st, 100, {"plane": {"origin": [6.135074365906242, -8.888189679526311, 5.23076923076923], "xDir": [0.8229838658936561, 0.5680647467311563, 0], "zDir": [0.5112561971588535, -0.7406824732938593, 0.43589743589743585]}, "obj": undefined});
        atStep(st, 101);
        cqCircle(c, st, 101, 100, {"radius": 2, "forConstruction": false});
        atStep(st, 102);
        cqExtrude(c, st, 102, 101, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 102, "extrude");
        atStep(st, 103);
        cqCut(c, st, 103, 99, {"toCut": {"wp": 102}, "clean": true, "tol": undefined});
        stepVol(c, st, 103, "cut");
        atStep(st, 104);
        cqWorkplaneNew(c, st, 104, {"plane": {"origin": [9.679868719548708, -5.080387016393467, 4.948717948717948], "xDir": [0.4647231720437687, 0.88545602565321, 0], "zDir": [0.8066557266290589, -0.4233655846994556, 0.4123931623931623]}, "obj": undefined});
        atStep(st, 105);
        cqCircle(c, st, 105, 104, {"radius": 2, "forConstruction": false});
        atStep(st, 106);
        cqExtrude(c, st, 106, 105, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 106, "extrude");
        atStep(st, 107);
        cqCut(c, st, 107, 103, {"toCut": {"wp": 106}, "clean": true, "tol": undefined});
        stepVol(c, st, 107, "cut");
        atStep(st, 108);
        cqWorkplaneNew(c, st, 108, {"plane": {"origin": [11.055415967851332, -5.4155919113086695e-15, 4.666666666666667], "xDir": [4.898587196589413e-16, 1, 0], "zDir": [0.9212846639876112, -4.512993259423891e-16, 0.3888888888888889]}, "obj": undefined});
        atStep(st, 109);
        cqCircle(c, st, 109, 108, {"radius": 2, "forConstruction": false});
        atStep(st, 110);
        cqExtrude(c, st, 110, 109, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 110, "extrude");
        atStep(st, 111);
        cqCut(c, st, 111, 107, {"toCut": {"wp": 110}, "clean": true, "tol": undefined});
        stepVol(c, st, 111, "cut");
        atStep(st, 112);
        cqWorkplaneNew(c, st, 112, {"plane": {"origin": [9.890791442943804, 5.19108780133641, 4.384615384615385], "xDir": [-0.4647231720437677, 0.8854560256532102, 0], "zDir": [0.8242326202453172, 0.43259065011136755, 0.3653846153846155]}, "obj": undefined});
        atStep(st, 113);
        cqCircle(c, st, 113, 112, {"radius": 2, "forConstruction": false});
        atStep(st, 114);
        cqExtrude(c, st, 114, 113, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 114, "extrude");
        atStep(st, 115);
        cqCut(c, st, 115, 111, {"toCut": {"wp": 114}, "clean": true, "tol": undefined});
        stepVol(c, st, 115, "cut");
        atStep(st, 116);
        cqWorkplaneNew(c, st, 116, {"plane": {"origin": [6.406021789217805, 9.280724789606634, 4.102564102564102], "xDir": [-0.8229838658936557, 0.568064746731157, 0], "zDir": [0.5338351491014839, 0.7733937324672195, 0.3418803418803419]}, "obj": undefined});
        atStep(st, 117);
        cqCircle(c, st, 117, 116, {"radius": 2, "forConstruction": false});
        atStep(st, 118);
        cqExtrude(c, st, 118, 117, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 118, "extrude");
        atStep(st, 119);
        cqCut(c, st, 119, 115, {"toCut": {"wp": 118}, "clean": true, "tol": undefined});
        stepVol(c, st, 119, "cut");
        atStep(st, 120);
        cqWorkplaneNew(c, st, 120, {"plane": {"origin": [1.3711739151445022, 11.29263316869562, 3.82051282051282], "xDir": [-0.992708874098054, 0.12053668025532303, -6.938893903907228e-18], "zDir": [0.11426449292870852, 0.9410527640579684, 0.31837606837606836]}, "obj": undefined});
        atStep(st, 121);
        cqCircle(c, st, 121, 120, {"radius": 2, "forConstruction": false});
        atStep(st, 122);
        cqExtrude(c, st, 122, 121, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 122, "extrude");
        atStep(st, 123);
        cqCut(c, st, 123, 119, {"toCut": {"wp": 122}, "clean": true, "tol": undefined});
        stepVol(c, st, 123, "cut");
        atStep(st, 124);
        cqWorkplaneNew(c, st, 124, {"plane": {"origin": [-4.066056336194429, 10.721309426171858, 3.538461538461538], "xDir": [-0.9350162426854148, -0.35460488704253523, 2.7755575615628914e-17], "zDir": [-0.33883802801620244, 0.8934424521809882, 0.2948717948717948]}, "obj": undefined});
        atStep(st, 125);
        cqCircle(c, st, 125, 124, {"radius": 2, "forConstruction": false});
        atStep(st, 126);
        cqExtrude(c, st, 126, 125, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 126, "extrude");
        atStep(st, 127);
        cqCut(c, st, 127, 123, {"toCut": {"wp": 126}, "clean": true, "tol": undefined});
        stepVol(c, st, 127, "cut");
        atStep(st, 128);
        cqWorkplaneNew(c, st, 128, {"plane": {"origin": [-8.645081786739174, 7.658874143275673, 3.256410256410257], "xDir": [-0.6631226582407959, -0.7485107481711005, 0], "zDir": [-0.7204234822282646, 0.6382395119396395, 0.2713675213675214]}, "obj": undefined});
        atStep(st, 129);
        cqCircle(c, st, 129, 128, {"radius": 2, "forConstruction": false});
        atStep(st, 130);
        cqExtrude(c, st, 130, 129, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 130, "extrude");
        atStep(st, 131);
        cqCut(c, st, 131, 127, {"toCut": {"wp": 130}, "clean": true, "tol": undefined});
        stepVol(c, st, 131, "cut");
        atStep(st, 132);
        cqWorkplaneNew(c, st, 132, {"plane": {"origin": [-11.28772328693694, 2.7821739142605217, 2.9743589743589745], "xDir": [-0.23931566428755907, -0.9709418174260518, -6.938893903907228e-18], "zDir": [-0.9406436072447452, 0.23184782618837688, 0.24786324786324793]}, "obj": undefined});
        atStep(st, 133);
        cqCircle(c, st, 133, 132, {"radius": 2, "forConstruction": false});
        atStep(st, 134);
        cqExtrude(c, st, 134, 133, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 134, "extrude");
        atStep(st, 135);
        cqCut(c, st, 135, 131, {"toCut": {"wp": 134}, "clean": true, "tol": undefined});
        stepVol(c, st, 135, "cut");
        atStep(st, 136);
        cqWorkplaneNew(c, st, 136, {"plane": {"origin": [-11.354270136037126, -2.798576239418232, 2.6923076923076925], "xDir": [0.23931566428755785, -0.970941817426052, -6.938893903907228e-18], "zDir": [-0.946189178003094, -0.233214686618186, 0.22435897435897437]}, "obj": undefined});
        atStep(st, 137);
        cqCircle(c, st, 137, 136, {"radius": 2, "forConstruction": false});
        atStep(st, 138);
        cqExtrude(c, st, 138, 137, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 138, "extrude");
        atStep(st, 139);
        cqCut(c, st, 139, 135, {"toCut": {"wp": 138}, "clean": true, "tol": undefined});
        stepVol(c, st, 139, "cut");
        atStep(st, 140);
        cqWorkplaneNew(c, st, 140, {"plane": {"origin": [-8.799082563599873, -7.795306926335265, 2.41025641025641], "xDir": [0.663122658240795, -0.7485107481711014, -1.3877787807814457e-17], "zDir": [-0.7332568802999895, -0.6496089105279388, 0.2008547008547009]}, "obj": undefined});
        atStep(st, 141);
        cqCircle(c, st, 141, 140, {"radius": 2, "forConstruction": false});
        atStep(st, 142);
        cqExtrude(c, st, 142, 141, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 142, "extrude");
        atStep(st, 143);
        cqCut(c, st, 143, 139, {"toCut": {"wp": 142}, "clean": true, "tol": undefined});
        stepVol(c, st, 143, "cut");
        atStep(st, 144);
        cqWorkplaneNew(c, st, 144, {"plane": {"origin": [-4.18780329003524, -11.042329760347288, 2.1282051282051277], "xDir": [0.935016242685415, -0.3546048870425348, 6.9388939039072276e-18], "zDir": [-0.3489836075029367, -0.9201941466956074, 0.17735042735042733]}, "obj": undefined});
        atStep(st, 145);
        cqCircle(c, st, 145, 144, {"radius": 2, "forConstruction": false});
        atStep(st, 146);
        cqExtrude(c, st, 146, 145, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 146, "extrude");
        atStep(st, 147);
        cqCut(c, st, 147, 143, {"toCut": {"wp": 146}, "clean": true, "tol": undefined});
        stepVol(c, st, 147, "cut");
        atStep(st, 148);
        cqWorkplaneNew(c, st, 148, {"plane": {"origin": [1.4292200235136634, -11.770685880640896, 1.8461538461538456], "xDir": [0.9927088740980542, 0.12053668025532184, 3.469446951953614e-18], "zDir": [0.11910166862613862, -0.980890490053408, 0.1538461538461538]}, "obj": undefined});
        atStep(st, 149);
        cqCircle(c, st, 149, 148, {"radius": 2, "forConstruction": false});
        atStep(st, 150);
        cqExtrude(c, st, 150, 149, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 150, "extrude");
        atStep(st, 151);
        cqCut(c, st, 151, 147, {"toCut": {"wp": 150}, "clean": true, "tol": undefined});
        stepVol(c, st, 151, "cut");
        atStep(st, 152);
        cqWorkplaneNew(c, st, 152, {"plane": {"origin": [6.758623779989157, -9.79155696350345, 1.5641025641025643], "xDir": [0.8229838658936564, 0.5680647467311559, 1.3877787807814457e-17], "zDir": [0.5632186483324297, -0.8159630802919541, 0.13034188034188038]}, "obj": undefined});
        atStep(st, 153);
        cqCircle(c, st, 153, 152, {"radius": 2, "forConstruction": false});
        atStep(st, 154);
        cqExtrude(c, st, 154, 153, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 154, "extrude");
        atStep(st, 155);
        cqCut(c, st, 155, 151, {"toCut": {"wp": 154}, "clean": true, "tol": undefined});
        stepVol(c, st, 155, "cut");
        atStep(st, 156);
        cqWorkplaneNew(c, st, 156, {"plane": {"origin": [10.564657242427336, -5.544759855955736, 1.2820512820512824], "xDir": [0.4647231720437705, 0.8854560256532089, 0], "zDir": [0.8803881035356115, -0.4620633213296446, 0.10683760683760686]}, "obj": undefined});
        atStep(st, 157);
        cqCircle(c, st, 157, 156, {"radius": 2, "forConstruction": false});
        atStep(st, 158);
        cqExtrude(c, st, 158, 157, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 158, "extrude");
        atStep(st, 159);
        cqCut(c, st, 159, 155, {"toCut": {"wp": 158}, "clean": true, "tol": undefined});
        stepVol(c, st, 159, "cut");
        atStep(st, 160);
        cqWorkplaneNew(c, st, 160, {"plane": {"origin": [0.1041454846067536, 2.5843435455759427, 11.717948717948715], "xDir": [-0.9991889981715696, 0.04026594010941525, 0], "zDir": [0.008678790383896132, 0.21536196213132855, 0.9764957264957264]}, "obj": undefined});
        atStep(st, 161);
        cqCircle(c, st, 161, 160, {"radius": 2, "forConstruction": false});
        atStep(st, 162);
        cqExtrude(c, st, 162, 161, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 162, "extrude");
        atStep(st, 163);
        cqCut(c, st, 163, 159, {"toCut": {"wp": 162}, "clean": true, "tol": undefined});
        stepVol(c, st, 163, "cut");
        atStep(st, 164);
        cqWorkplaneNew(c, st, 164, {"plane": {"origin": [-1.5587116415283302, 3.2849151974135484, 11.435897435897436], "xDir": [-0.9034504346103824, -0.4286925614030539, -1.387778780781446e-17], "zDir": [-0.12989263679402752, 0.2737429331177957, 0.952991452991453]}, "obj": undefined});
        atStep(st, 165);
        cqCircle(c, st, 165, 164, {"radius": 2, "forConstruction": false});
        atStep(st, 166);
        cqExtrude(c, st, 166, 165, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 166, "extrude");
        atStep(st, 167);
        cqCut(c, st, 167, 163, {"toCut": {"wp": 166}, "clean": true, "tol": undefined});
        stepVol(c, st, 167, "cut");
        atStep(st, 168);
        cqWorkplaneNew(c, st, 168, {"plane": {"origin": [-3.538535962021418, 2.6590372734906307, 11.153846153846157], "xDir": [-0.6007422642379788, -0.7994427634035012, -2.7755575615628907e-17], "zDir": [-0.2948779968351182, 0.22158643945755255, 0.9294871794871796]}, "obj": undefined});
        atStep(st, 169);
        cqCircle(c, st, 169, 168, {"radius": 2, "forConstruction": false});
        atStep(st, 170);
        cqExtrude(c, st, 170, 169, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 170, "extrude");
        atStep(st, 171);
        cqCut(c, st, 171, 167, {"toCut": {"wp": 170}, "clean": true, "tol": undefined});
        stepVol(c, st, 171, "cut");
        atStep(st, 172);
        cqWorkplaneNew(c, st, 172, {"plane": {"origin": [-5.013989576890526, 0.8148526176235037, 10.871794871794872], "xDir": [-0.16041128085776055, -0.987050262637913, -6.938893903907228e-18], "zDir": [-0.4178324647408772, 0.06790438480195865, 0.905982905982906]}, "obj": undefined});
        atStep(st, 173);
        cqCircle(c, st, 173, 172, {"radius": 2, "forConstruction": false});
        atStep(st, 174);
        cqExtrude(c, st, 174, 173, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 174, "extrude");
        atStep(st, 175);
        cqCut(c, st, 175, 171, {"toCut": {"wp": 174}, "clean": true, "tol": undefined});
        stepVol(c, st, 175, "cut");
        atStep(st, 176);
        cqWorkplaneNew(c, st, 176, {"plane": {"origin": [-5.353757708177382, -1.7873469460672191, 10.58974358974359], "xDir": [0.3166679938014718, -0.9485364419471458, 0], "zDir": [-0.44614647568144855, -0.14894557883893494, 0.8824786324786325]}, "obj": undefined});
        atStep(st, 177);
        cqCircle(c, st, 177, 176, {"radius": 2, "forConstruction": false});
        atStep(st, 178);
        cqExtrude(c, st, 178, 177, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 178, "extrude");
        atStep(st, 179);
        cqCut(c, st, 179, 175, {"toCut": {"wp": 178}, "clean": true, "tol": undefined});
        stepVol(c, st, 179, "cut");
        atStep(st, 180);
        cqWorkplaneNew(c, st, 180, {"plane": {"origin": [-4.25625307552372, -4.431228841646038, 10.307692307692308], "xDir": [0.7212024473438144, -0.6927243535095996, -2.775557561562892e-17], "zDir": [-0.3546877562936433, -0.36926907013716986, 0.858974358974359]}, "obj": undefined});
        atStep(st, 181);
        cqCircle(c, st, 181, 180, {"radius": 2, "forConstruction": false});
        atStep(st, 182);
        cqExtrude(c, st, 182, 181, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 182, "extrude");
        atStep(st, 183);
        cqCut(c, st, 183, 179, {"toCut": {"wp": 182}, "clean": true, "tol": undefined});
        stepVol(c, st, 183, "cut");
        atStep(st, 184);
        cqWorkplaneNew(c, st, 184, {"plane": {"origin": [-1.834685906999564, -6.334070543311052, 10.025641025641026], "xDir": [0.960518111631372, -0.2782174639164533, 0], "zDir": [-0.15289049224996365, -0.5278392119425876, 0.8354700854700855]}, "obj": undefined});
        atStep(st, 185);
        cqCircle(c, st, 185, 184, {"radius": 2, "forConstruction": false});
        atStep(st, 186);
        cqExtrude(c, st, 186, 185, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 186, "extrude");
        atStep(st, 187);
        cqCut(c, st, 187, 183, {"toCut": {"wp": 186}, "clean": true, "tol": undefined});
        stepVol(c, st, 187, "cut");
        atStep(st, 188);
        cqWorkplaneNew(c, st, 188, {"plane": {"origin": [1.401071956991014, -6.862904361853378, 9.743589743589745], "xDir": [0.9797906520422678, 0.20002569377604415, 0], "zDir": [0.11675599641591784, -0.5719086968211148, 0.8119658119658121]}, "obj": undefined});
        atStep(st, 189);
        cqCircle(c, st, 189, 188, {"radius": 2, "forConstruction": false});
        atStep(st, 190);
        cqExtrude(c, st, 190, 189, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 190, "extrude");
        atStep(st, 191);
        cqCut(c, st, 191, 187, {"toCut": {"wp": 190}, "clean": true, "tol": undefined});
        stepVol(c, st, 191, "cut");
        atStep(st, 192);
        cqWorkplaneNew(c, st, 192, {"plane": {"origin": [4.668084841451999, -5.71736598914522, 9.46153846153846], "xDir": [0.7746049618276551, 0.6324453755953767, 0], "zDir": [0.38900707012099994, -0.47644716576210167, 0.7884615384615383]}, "obj": undefined});
        atStep(st, 193);
        cqCircle(c, st, 193, 192, {"radius": 2, "forConstruction": false});
        atStep(st, 194);
        cqExtrude(c, st, 194, 193, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 194, "extrude");
        atStep(st, 195);
        cqCut(c, st, 195, 191, {"toCut": {"wp": 194}, "clean": true, "tol": undefined});
        stepVol(c, st, 195, "cut");
        atStep(st, 196);
        cqWorkplaneNew(c, st, 196, {"plane": {"origin": [7.110495773937113, -3.0294991286437063, 9.17948717948718], "xDir": [0.391966609860076, 0.9199794436588238, 2.7755575615628907e-17], "zDir": [0.5925413144947594, -0.25245826072030886, 0.7649572649572649]}, "obj": undefined});
        atStep(st, 197);
        cqCircle(c, st, 197, 196, {"radius": 2, "forConstruction": false});
        atStep(st, 198);
        cqExtrude(c, st, 198, 197, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 198, "extrude");
        atStep(st, 199);
        cqCut(c, st, 199, 195, {"toCut": {"wp": 198}, "clean": true, "tol": undefined});
        stepVol(c, st, 199, "cut");
        atStep(st, 200);
        cqWorkplaneNew(c, st, 200, {"plane": {"origin": [8.025947435169046, 0.647921460458354, 8.897435897435898], "xDir": [-0.08046656871672526, 0.99675730813421, -6.9388939039072276e-18], "zDir": [0.6688289529307538, 0.05399345503819616, 0.7414529914529915]}, "obj": undefined});
        atStep(st, 201);
        cqCircle(c, st, 201, 200, {"radius": 2, "forConstruction": false});
        atStep(st, 202);
        cqExtrude(c, st, 202, 201, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 202, "extrude");
        atStep(st, 203);
        cqCut(c, st, 203, 199, {"toCut": {"wp": 202}, "clean": true, "tol": undefined});
        stepVol(c, st, 203, "cut");
        atStep(st, 204);
        cqWorkplaneNew(c, st, 204, {"plane": {"origin": [7.060001232894779, 4.4644742714588785, 8.615384615384615], "xDir": [-0.5344658261278009, 0.8451900855437949, 2.7755575615628914e-17], "zDir": [0.5883334360745649, 0.3720395226215732, 0.717948717948718]}, "obj": undefined});
        atStep(st, 205);
        cqCircle(c, st, 205, 204, {"radius": 2, "forConstruction": false});
        atStep(st, 206);
        cqExtrude(c, st, 206, 205, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 206, "extrude");
        atStep(st, 207);
        cqCut(c, st, 207, 203, {"toCut": {"wp": 206}, "clean": true, "tol": undefined});
        stepVol(c, st, 207, "cut");
        atStep(st, 208);
        cqWorkplaneNew(c, st, 208, {"plane": {"origin": [4.31727794899621, 7.477744758058182, 8.333333333333334], "xDir": [-0.8660254037844384, 0.5000000000000006, 0], "zDir": [0.3597731624163509, 0.6231453965048485, 0.6944444444444445]}, "obj": undefined});
        atStep(st, 209);
        cqCircle(c, st, 209, 208, {"radius": 2, "forConstruction": false});
        atStep(st, 210);
        cqExtrude(c, st, 210, 209, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 210, "extrude");
        atStep(st, 211);
        cqCut(c, st, 211, 207, {"toCut": {"wp": 210}, "clean": true, "tol": undefined});
        stepVol(c, st, 211, "cut");
        atStep(st, 212);
        cqWorkplaneNew(c, st, 212, {"plane": {"origin": [0.3582918860545837, 8.8909214514071, 8.051282051282051], "xDir": [-0.9991889981715696, 0.04026594010941616, 0], "zDir": [0.029857657171215305, 0.7409101209505917, 0.6709401709401709]}, "obj": undefined});
        atStep(st, 213);
        cqCircle(c, st, 213, 212, {"radius": 2, "forConstruction": false});
        atStep(st, 214);
        cqExtrude(c, st, 214, 213, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 214, "extrude");
        atStep(st, 215);
        cqCut(c, st, 215, 211, {"toCut": {"wp": 214}, "clean": true, "tol": undefined});
        stepVol(c, st, 215, "cut");
        atStep(st, 216);
        cqWorkplaneNew(c, st, 216, {"plane": {"origin": [-3.920581640808738, 8.262450777595673, 7.76923076923077], "xDir": [-0.9034504346103828, -0.4286925614030537, 2.7755575615628914e-17], "zDir": [-0.3267151367340615, 0.6885375647996393, 0.6474358974358975]}, "obj": undefined});
        atStep(st, 217);
        cqCircle(c, st, 217, 216, {"radius": 2, "forConstruction": false});
        atStep(st, 218);
        cqExtrude(c, st, 218, 217, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 218, "extrude");
        atStep(st, 219);
        cqCut(c, st, 219, 215, {"toCut": {"wp": 218}, "clean": true, "tol": undefined});
        stepVol(c, st, 219, "cut");
        atStep(st, 220);
        cqWorkplaneNew(c, st, 220, {"plane": {"origin": [-7.496972169299633, 5.633609111351748, 7.487179487179487], "xDir": [-0.6007422642379798, -0.7994427634035006, 0], "zDir": [-0.6247476807749696, 0.46946742594597907, 0.623931623931624]}, "obj": undefined});
        atStep(st, 221);
        cqCircle(c, st, 221, 220, {"radius": 2, "forConstruction": false});
        atStep(st, 222);
        cqExtrude(c, st, 222, 221, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 222, "extrude");
        atStep(st, 223);
        cqCut(c, st, 223, 219, {"toCut": {"wp": 222}, "clean": true, "tol": undefined});
        stepVol(c, st, 223, "cut");
        atStep(st, 224);
        cqWorkplaneNew(c, st, 224, {"plane": {"origin": [-9.471884060851362, 1.5393309863235798, 7.205128205128205], "xDir": [-0.16041128085775985, -0.987050262637913, 1.3877787807814457e-17], "zDir": [-0.7893236717376135, 0.12827758219363164, 0.6004273504273504]}, "obj": undefined});
        atStep(st, 225);
        cqCircle(c, st, 225, 224, {"radius": 2, "forConstruction": false});
        atStep(st, 226);
        cqExtrude(c, st, 226, 225, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 226, "extrude");
        atStep(st, 227);
        cqCut(c, st, 227, 223, {"toCut": {"wp": 226}, "clean": true, "tol": undefined});
        stepVol(c, st, 227, "cut");
        atStep(st, 228);
        cqWorkplaneNew(c, st, 228, {"plane": {"origin": [-9.297157532561448, -3.1038472468689116, 6.9230769230769225], "xDir": [0.3166679938014725, -0.9485364419471456, -2.7755575615628914e-17], "zDir": [-0.774763127713454, -0.258653937239076, 0.5769230769230769]}, "obj": undefined});
        atStep(st, 229);
        cqCircle(c, st, 229, 228, {"radius": 2, "forConstruction": false});
        atStep(st, 230);
        cqExtrude(c, st, 230, 229, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 230, "extrude");
        atStep(st, 231);
        cqCut(c, st, 231, 227, {"toCut": {"wp": 230}, "clean": true, "tol": undefined});
        stepVol(c, st, 231, "cut");
        atStep(st, 232);
        cqWorkplaneNew(c, st, 232, {"plane": {"origin": [-6.92366740743817, -7.208301330162218, 6.6410256410256405], "xDir": [0.7212024473438143, -0.6927243535095998, -2.7755575615628914e-17], "zDir": [-0.5769722839531809, -0.6006917775135182, 0.5534188034188033]}, "obj": undefined});
        atStep(st, 233);
        cqCircle(c, st, 233, 232, {"radius": 2, "forConstruction": false});
        atStep(st, 234);
        cqExtrude(c, st, 234, 233, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 234, "extrude");
        atStep(st, 235);
        cqCut(c, st, 235, 231, {"toCut": {"wp": 234}, "clean": true, "tol": undefined});
        stepVol(c, st, 235, "cut");
        atStep(st, 236);
        cqWorkplaneNew(c, st, 236, {"plane": {"origin": [-2.831311363751449, -9.774820768863158, 6.3589743589743595], "xDir": [0.960518111631372, -0.2782174639164536, 2.775557561562892e-17], "zDir": [-0.2359426136459541, -0.8145683974052632, 0.5299145299145299]}, "obj": undefined});
        atStep(st, 237);
        cqCircle(c, st, 237, 236, {"radius": 2, "forConstruction": false});
        atStep(st, 238);
        cqExtrude(c, st, 238, 237, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 238, "extrude");
        atStep(st, 239);
        cqCut(c, st, 239, 235, {"toCut": {"wp": 238}, "clean": true, "tol": undefined});
        stepVol(c, st, 239, "cut");
        atStep(st, 240);
        cqWorkplaneNew(c, st, 240, {"plane": {"origin": [2.069768279482088, -10.13839559232177, 6.0769230769230775], "xDir": [0.9797906520422676, 0.2000256937760449, 0], "zDir": [0.17248068995684068, -0.8448662993601476, 0.5064102564102566]}, "obj": undefined});
        atStep(st, 241);
        cqCircle(c, st, 241, 240, {"radius": 2, "forConstruction": false});
        atStep(st, 242);
        cqExtrude(c, st, 242, 241, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 242, "extrude");
        atStep(st, 243);
        cqCut(c, st, 243, 239, {"toCut": {"wp": 242}, "clean": true, "tol": undefined});
        stepVol(c, st, 243, "cut");
        atStep(st, 244);
        cqWorkplaneNew(c, st, 244, {"plane": {"origin": [6.6457780436868585, -8.139600424778173, 5.794871794871795], "xDir": [0.7746049618276546, 0.6324453755953773, -2.7755575615628914e-17], "zDir": [0.5538148369739049, -0.6783000353981811, 0.4829059829059829]}, "obj": undefined});
        atStep(st, 245);
        cqCircle(c, st, 245, 244, {"radius": 2, "forConstruction": false});
        atStep(st, 246);
        cqExtrude(c, st, 246, 245, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 246, "extrude");
        atStep(st, 247);
        cqCut(c, st, 247, 243, {"toCut": {"wp": 246}, "clean": true, "tol": undefined});
        stepVol(c, st, 247, "cut");
        atStep(st, 248);
        cqWorkplaneNew(c, st, 248, {"plane": {"origin": [9.805824785532216, -4.177871499803156, 5.512820512820513], "xDir": [0.39196660986007553, 0.919979443658824, 0], "zDir": [0.8171520654610179, -0.34815595831692964, 0.4594017094017094]}, "obj": undefined});
        atStep(st, 249);
        cqCircle(c, st, 249, 248, {"radius": 2, "forConstruction": false});
        atStep(st, 250);
        cqExtrude(c, st, 250, 249, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 250, "extrude");
        atStep(st, 251);
        cqCut(c, st, 251, 247, {"toCut": {"wp": 250}, "clean": true, "tol": undefined});
        stepVol(c, st, 251, "cut");
        atStep(st, 252);
        cqWorkplaneNew(c, st, 252, {"plane": {"origin": [10.764935239077577, 0.8690354152183615, 5.23076923076923], "xDir": [-0.08046656871672678, 0.99675730813421, 0], "zDir": [0.897077936589798, 0.07241961793486346, 0.43589743589743585]}, "obj": undefined});
        atStep(st, 253);
        cqCircle(c, st, 253, 252, {"radius": 2, "forConstruction": false});
        atStep(st, 254);
        cqExtrude(c, st, 254, 253, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        atStep(st, 255);
        cqCut(c, st, 255, 251, {"toCut": {"wp": 254}, "clean": true, "tol": undefined});
        stepVol(c, st, 255, "cut");
        atStep(st, 256);
        cqWorkplaneNew(c, st, 256, {"plane": {"origin": [9.23967857702772, 5.842818708230797, 4.948717948717948], "xDir": [-0.5344658261278016, 0.8451900855437946, 2.7755575615628914e-17], "zDir": [0.7699732147523102, 0.4869015590192331, 0.4123931623931624]}, "obj": undefined});
        atStep(st, 257);
        cqCircle(c, st, 257, 256, {"radius": 2, "forConstruction": false});
        atStep(st, 258);
        cqExtrude(c, st, 258, 257, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 258, "extrude");
        atStep(st, 259);
        cqCut(c, st, 259, 255, {"toCut": {"wp": 258}, "clean": true, "tol": undefined});
        stepVol(c, st, 259, "cut");
        atStep(st, 260);
        cqWorkplaneNew(c, st, 260, {"plane": {"origin": [5.527707983925666, 9.574271077563385, 4.666666666666667], "xDir": [-0.8660254037844387, 0.4999999999999999, 2.7755575615628914e-17], "zDir": [0.4606423319938055, 0.7978559231302819, 0.3888888888888889]}, "obj": undefined});
        atStep(st, 261);
        cqCircle(c, st, 261, 260, {"radius": 2, "forConstruction": false});
        atStep(st, 262);
        cqExtrude(c, st, 262, 261, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 262, "extrude");
        atStep(st, 263);
        cqCut(c, st, 263, 259, {"toCut": {"wp": 262}, "clean": true, "tol": undefined});
        stepVol(c, st, 263, "cut");
        atStep(st, 264);
        cqWorkplaneNew(c, st, 264, {"plane": {"origin": [0.4497818122390579, 11.161220553791283, 4.384615384615385], "xDir": [-0.9991889981715696, 0.040265940109415504, 0], "zDir": [0.03748181768658816, 0.9301017128159405, 0.3653846153846155]}, "obj": undefined});
        atStep(st, 265);
        cqCircle(c, st, 265, 264, {"radius": 2, "forConstruction": false});
        atStep(st, 266);
        cqExtrude(c, st, 266, 265, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 266, "extrude");
        atStep(st, 267);
        cqCut(c, st, 267, 263, {"toCut": {"wp": 266}, "clean": true, "tol": undefined});
        stepVol(c, st, 267, "cut");
        atStep(st, 268);
        cqWorkplaneNew(c, st, 268, {"plane": {"origin": [-4.834332538722437, 10.188140001462576, 4.102564102564102], "xDir": [-0.9034504346103828, -0.42869256140305345, 1.3877787807814457e-17], "zDir": [-0.40286104489353647, 0.8490116667885481, 0.3418803418803419]}, "obj": undefined});
        atStep(st, 269);
        cqCircle(c, st, 269, 268, {"radius": 2, "forConstruction": false});
        atStep(st, 270);
        cqExtrude(c, st, 270, 269, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 270, "extrude");
        atStep(st, 271);
        cqCut(c, st, 271, 267, {"toCut": {"wp": 270}, "clean": true, "tol": undefined});
        stepVol(c, st, 271, "cut");
        atStep(st, 272);
        cqWorkplaneNew(c, st, 272, {"plane": {"origin": [-9.094120242136922, 6.833788027869512, 3.82051282051282], "xDir": [-0.6007422642379785, -0.7994427634035015, -2.7755575615628914e-17], "zDir": [-0.7578433535114102, 0.5694823356557926, 0.31837606837606836]}, "obj": undefined});
        atStep(st, 273);
        cqCircle(c, st, 273, 272, {"radius": 2, "forConstruction": false});
        atStep(st, 274);
        cqExtrude(c, st, 274, 273, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 274, "extrude");
        atStep(st, 275);
        cqCut(c, st, 275, 271, {"toCut": {"wp": 274}, "clean": true, "tol": undefined});
        stepVol(c, st, 275, "cut");
        atStep(st, 276);
        cqWorkplaneNew(c, st, 276, {"plane": {"origin": [-11.317954492995607, 1.839346632722867, 3.538461538461538], "xDir": [-0.16041128085776013, -0.987050262637913, 0], "zDir": [-0.9431628744163006, 0.1532788860602389, 0.2948717948717948]}, "obj": undefined});
        atStep(st, 277);
        cqCircle(c, st, 277, 276, {"radius": 2, "forConstruction": false});
        atStep(st, 278);
        cqExtrude(c, st, 278, 277, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 278, "extrude");
        atStep(st, 279);
        cqCut(c, st, 279, 275, {"toCut": {"wp": 278}, "clean": true, "tol": undefined});
        stepVol(c, st, 279, "cut");
        atStep(st, 280);
        cqWorkplaneNew(c, st, 280, {"plane": {"origin": [-10.955320465834104, -3.6574233734724406, 3.256410256410257], "xDir": [0.3166679938014705, -0.9485364419471464, 0], "zDir": [-0.912943372152842, -0.30478528112270337, 0.2713675213675214]}, "obj": undefined});
        atStep(st, 281);
        cqCircle(c, st, 281, 280, {"radius": 2, "forConstruction": false});
        atStep(st, 282);
        cqExtrude(c, st, 282, 281, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 282, "extrude");
        atStep(st, 283);
        cqCut(c, st, 283, 279, {"toCut": {"wp": 282}, "clean": true, "tol": undefined});
        stepVol(c, st, 283, "cut");
        atStep(st, 284);
        cqWorkplaneNew(c, st, 284, {"plane": {"origin": [-8.053294930964466, -8.384368160246318, 2.9743589743589745], "xDir": [0.721202447343814, -0.6927243535095999, 0], "zDir": [-0.6711079109137056, -0.6986973466871933, 0.2478632478632479]}, "obj": undefined});
        atStep(st, 285);
        cqCircle(c, st, 285, 284, {"radius": 2, "forConstruction": false});
        atStep(st, 286);
        cqExtrude(c, st, 286, 285, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 286, "extrude");
        atStep(st, 287);
        cqCut(c, st, 287, 283, {"toCut": {"wp": 286}, "clean": true, "tol": undefined});
        stepVol(c, st, 287, "cut");
        atStep(st, 288);
        cqWorkplaneNew(c, st, 288, {"plane": {"origin": [-3.253496950254846, -11.232374498948262, 2.6923076923076925], "xDir": [0.9605181116313724, -0.2782174639164521, 6.9388939039072276e-18], "zDir": [-0.2711247458545706, -0.9360312082456886, 0.2243589743589744]}, "obj": undefined});
        atStep(st, 289);
        cqCircle(c, st, 289, 288, {"radius": 2, "forConstruction": false});
        atStep(st, 290);
        cqExtrude(c, st, 290, 289, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 290, "extrude");
        atStep(st, 291);
        cqCut(c, st, 291, 287, {"toCut": {"wp": 290}, "clean": true, "tol": undefined});
        stepVol(c, st, 291, "cut");
        atStep(st, 292);
        cqWorkplaneNew(c, st, 292, {"plane": {"origin": [2.3513925467031775, -11.51788249324183, 2.41025641025641], "xDir": [0.979790652042268, 0.20002569377604287, 0], "zDir": [0.19594937889193145, -0.9598235411034858, 0.20085470085470084]}, "obj": undefined});
        atStep(st, 293);
        cqCircle(c, st, 293, 292, {"radius": 2, "forConstruction": false});
        atStep(st, 294);
        cqExtrude(c, st, 294, 293, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 294, "extrude");
        atStep(st, 295);
        cqCut(c, st, 295, 291, {"toCut": {"wp": 294}, "clean": true, "tol": undefined});
        stepVol(c, st, 295, "cut");
        atStep(st, 296);
        cqWorkplaneNew(c, st, 296, {"plane": {"origin": [7.469036444408053, -9.147908915396222, 2.1282051282051277], "xDir": [0.7746049618276547, 0.6324453755953772, 0], "zDir": [0.6224197037006712, -0.7623257429496852, 0.17735042735042733]}, "obj": undefined});
        atStep(st, 297);
        cqCircle(c, st, 297, 296, {"radius": 2, "forConstruction": false});
        atStep(st, 298);
        cqExtrude(c, st, 298, 297, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 298, "extrude");
        atStep(st, 299);
        cqCut(c, st, 299, 295, {"toCut": {"wp": 298}, "clean": true, "tol": undefined});
        stepVol(c, st, 299, "cut");
        atStep(st, 300);
        cqWorkplaneNew(c, st, 300, {"plane": {"origin": [10.90832300435865, -4.647602092360237, 1.8461538461538456], "xDir": [0.39196660986007736, 0.9199794436588232, 0], "zDir": [0.9090269170298876, -0.38730017436335307, 0.1538461538461538]}, "obj": undefined});
        atStep(st, 301);
        cqCircle(c, st, 301, 300, {"radius": 2, "forConstruction": false});
        atStep(st, 302);
        cqExtrude(c, st, 302, 301, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 302, "extrude");
        atStep(st, 303);
        cqCut(c, st, 303, 299, {"toCut": {"wp": 302}, "clean": true, "tol": undefined});
        stepVol(c, st, 303, "cut");
        atStep(st, 304);
        cqWorkplaneNew(c, st, 304, {"plane": {"origin": [11.859048962990984, 0.9573614063404794, 1.5641025641025643], "xDir": [-0.0804665687167248, 0.99675730813421, -1.734723475976807e-18], "zDir": [0.9882540802492488, 0.07978011719503997, 0.1303418803418804]}, "obj": undefined});
        atStep(st, 305);
        cqCircle(c, st, 305, 304, {"radius": 2, "forConstruction": false});
        atStep(st, 306);
        cqExtrude(c, st, 306, 305, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 306, "extrude");
        atStep(st, 307);
        cqCut(c, st, 307, 303, {"toCut": {"wp": 306}, "clean": true, "tol": undefined});
        stepVol(c, st, 307, "cut");
        atStep(st, 308);
        cqWorkplaneNew(c, st, 308, {"plane": {"origin": [10.084231514355489, 6.376881626239449, 1.2820512820512824], "xDir": [-0.5344658261277984, 0.8451900855437966, -1.3877787807814457e-17], "zDir": [0.8403526261962908, 0.5314068021866208, 0.10683760683760687]}, "obj": undefined});
        atStep(st, 309);
        cqCircle(c, st, 309, 308, {"radius": 2, "forConstruction": false});
        atStep(st, 310);
        cqExtrude(c, st, 310, 309, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 310, "extrude");
        atStep(st, 311);
        cqCut(c, st, 311, 307, {"toCut": {"wp": 310}, "clean": true, "tol": undefined});
        stepVol(c, st, 311, "cut");
        atStep(st, 312);
        cqWorkplaneNew(c, st, 312, {"plane": {"origin": [-2.1860344202717363, 1.3823644081468611, 11.717948717948715], "xDir": [-0.5344658261278015, -0.8451900855437946, 0], "zDir": [-0.18216953502264469, 0.11519703401223844, 0.9764957264957264]}, "obj": undefined});
        atStep(st, 313);
        cqCircle(c, st, 313, 312, {"radius": 2, "forConstruction": false});
        atStep(st, 314);
        cqExtrude(c, st, 314, 313, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 314, "extrude");
        atStep(st, 315);
        cqCut(c, st, 315, 311, {"toCut": {"wp": 314}, "clean": true, "tol": undefined});
        stepVol(c, st, 315, "cut");
        atStep(st, 316);
        cqWorkplaneNew(c, st, 316, {"plane": {"origin": [-3.624175831001872, 0.2925737199686964, 11.435897435897436], "xDir": [-0.0804665687167261, -0.99675730813421, 0], "zDir": [-0.30201465258348936, 0.024381143330724706, 0.9529914529914532]}, "obj": undefined});
        atStep(st, 317);
        cqCircle(c, st, 317, 316, {"radius": 2, "forConstruction": false});
        atStep(st, 318);
        cqExtrude(c, st, 318, 317, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 318, "extrude");
        atStep(st, 319);
        cqCut(c, st, 319, 315, {"toCut": {"wp": 318}, "clean": true, "tol": undefined});
        stepVol(c, st, 319, "cut");
        atStep(st, 320);
        cqWorkplaneNew(c, st, 320, {"plane": {"origin": [-4.072061809463306, -1.734943398570039, 11.153846153846157], "xDir": [0.391966609860075, -0.9199794436588244, 1.387778780781446e-17], "zDir": [-0.3393384841219422, -0.14457861654750326, 0.9294871794871796]}, "obj": undefined});
        atStep(st, 321);
        cqCircle(c, st, 321, 320, {"radius": 2, "forConstruction": false});
        atStep(st, 322);
        cqExtrude(c, st, 322, 321, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 322, "extrude");
        atStep(st, 323);
        cqCut(c, st, 323, 319, {"toCut": {"wp": 322}, "clean": true, "tol": undefined});
        stepVol(c, st, 323, "cut");
        atStep(st, 324);
        cqWorkplaneNew(c, st, 324, {"plane": {"origin": [-3.212677855647466, -3.934816039085832, 10.871794871794872], "xDir": [0.774604961827654, -0.6324453755953778, -2.7755575615628907e-17], "zDir": [-0.2677231546372889, -0.32790133659048604, 0.905982905982906]}, "obj": undefined});
        atStep(st, 325);
        cqCircle(c, st, 325, 324, {"radius": 2, "forConstruction": false});
        atStep(st, 326);
        cqExtrude(c, st, 326, 325, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 326, "extrude");
        atStep(st, 327);
        cqCut(c, st, 327, 323, {"toCut": {"wp": 326}, "clean": true, "tol": undefined});
        stepVol(c, st, 327, "cut");
        atStep(st, 328);
        cqWorkplaneNew(c, st, 328, {"plane": {"origin": [-1.128990993417946, -5.530163654021978, 10.58974358974359], "xDir": [0.9797906520422676, -0.20002569377604548, 0], "zDir": [-0.09408258278482884, -0.46084697116849815, 0.8824786324786325]}, "obj": undefined});
        atStep(st, 329);
        cqCircle(c, st, 329, 328, {"radius": 2, "forConstruction": false});
        atStep(st, 330);
        cqExtrude(c, st, 330, 329, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 330, "extrude");
        atStep(st, 331);
        cqCut(c, st, 331, 327, {"toCut": {"wp": 330}, "clean": true, "tol": undefined});
        stepVol(c, st, 331, "cut");
        atStep(st, 332);
        cqWorkplaneNew(c, st, 332, {"plane": {"origin": [1.7094302090859037, -5.901637709162206, 10.307692307692308], "xDir": [0.9605181116313722, 0.27821746391645297, 1.387778780781446e-17], "zDir": [0.1424525174238253, -0.49180314243018386, 0.858974358974359]}, "obj": undefined});
        atStep(st, 333);
        cqCircle(c, st, 333, 332, {"radius": 2, "forConstruction": false});
        atStep(st, 334);
        cqExtrude(c, st, 334, 333, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 334, "extrude");
        atStep(st, 335);
        cqCut(c, st, 335, 331, {"toCut": {"wp": 334}, "clean": true, "tol": undefined});
        stepVol(c, st, 335, "cut");
        atStep(st, 336);
        cqWorkplaneNew(c, st, 336, {"plane": {"origin": [4.568123046370292, -4.75591987508244, 10.025641025641026], "xDir": [0.7212024473438147, 0.6927243535095993, -2.7755575615628907e-17], "zDir": [0.3806769205308577, -0.39632665625687, 0.8354700854700855]}, "obj": undefined});
        atStep(st, 337);
        cqCircle(c, st, 337, 336, {"radius": 2, "forConstruction": false});
        atStep(st, 338);
        cqExtrude(c, st, 338, 337, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 338, "extrude");
        atStep(st, 339);
        cqCut(c, st, 339, 335, {"toCut": {"wp": 338}, "clean": true, "tol": undefined});
        stepVol(c, st, 339, "cut");
        atStep(st, 340);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [6.643985499603563, -2.2180882736424943, 9.743589743589745], "xDir": [0.31666799380147304, 0.9485364419471454, 0], "zDir": [0.5536654583002969, -0.18484068947020785, 0.8119658119658121]}, "obj": undefined});
        atStep(st, 341);
        cqCircle(c, st, 341, 340, {"radius": 2, "forConstruction": false});
        atStep(st, 342);
        cqExtrude(c, st, 342, 341, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 342, "extrude");
        atStep(st, 343);
        cqCut(c, st, 343, 339, {"toCut": {"wp": 342}, "clean": true, "tol": undefined});
        stepVol(c, st, 343, "cut");
        atStep(st, 344);
        cqWorkplaneNew(c, st, 344, {"plane": {"origin": [7.285426610058906, 1.1839970651458729, 9.46153846153846], "xDir": [-0.1604112808577593, 0.9870502626379132, 0], "zDir": [0.6071188841715754, 0.0986664220954894, 0.7884615384615383]}, "obj": undefined});
        atStep(st, 345);
        cqCircle(c, st, 345, 344, {"radius": 2, "forConstruction": false});
        atStep(st, 346);
        cqExtrude(c, st, 346, 345, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 346, "extrude");
        atStep(st, 347);
        cqCut(c, st, 347, 343, {"toCut": {"wp": 346}, "clean": true, "tol": undefined});
        stepVol(c, st, 347, "cut");
        atStep(st, 348);
        cqWorkplaneNew(c, st, 348, {"plane": {"origin": [6.178871093116828, 4.643120409409578, 9.17948717948718], "xDir": [-0.6007422642379778, 0.7994427634035021, 0], "zDir": [0.5149059244264024, 0.3869267007841315, 0.7649572649572649]}, "obj": undefined});
        atStep(st, 349);
        cqCircle(c, st, 349, 348, {"radius": 2, "forConstruction": false});
        atStep(st, 350);
        cqExtrude(c, st, 350, 349, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 350, "extrude");
        atStep(st, 351);
        cqCut(c, st, 351, 347, {"toCut": {"wp": 350}, "clean": true, "tol": undefined});
        stepVol(c, st, 351, "cut");
        atStep(st, 352);
        cqWorkplaneNew(c, st, 352, {"plane": {"origin": [3.4518572731704698, 7.274635098524131, 8.897435897435898], "xDir": [-0.9034504346103822, 0.4286925614030543, 0], "zDir": [0.2876547727642058, 0.6062195915436775, 0.7414529914529915]}, "obj": undefined});
        atStep(st, 353);
        cqCircle(c, st, 353, 352, {"radius": 2, "forConstruction": false});
        atStep(st, 354);
        cqExtrude(c, st, 354, 353, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 354, "extrude");
        atStep(st, 355);
        cqCut(c, st, 355, 351, {"toCut": {"wp": 354}, "clean": true, "tol": undefined});
        stepVol(c, st, 355, "cut");
        atStep(st, 356);
        cqWorkplaneNew(c, st, 356, {"plane": {"origin": [-0.3363475171780212, 8.346377554165775, 8.615384615384615], "xDir": [-0.9991889981715698, -0.04026594010941466, 0], "zDir": [-0.0280289597648351, 0.695531462847148, 0.717948717948718]}, "obj": undefined});
        atStep(st, 357);
        cqCircle(c, st, 357, 356, {"radius": 2, "forConstruction": false});
        atStep(st, 358);
        cqExtrude(c, st, 358, 357, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 358, "extrude");
        atStep(st, 359);
        cqCut(c, st, 359, 355, {"toCut": {"wp": 358}, "clean": true, "tol": undefined});
        stepVol(c, st, 359, "cut");
        atStep(st, 360);
        cqWorkplaneNew(c, st, 360, {"plane": {"origin": [-4.317277948996199, 7.477744758058189, 8.333333333333334], "xDir": [-0.8660254037844392, -0.4999999999999992, 0], "zDir": [-0.3597731624163499, 0.6231453965048491, 0.6944444444444445]}, "obj": undefined});
        atStep(st, 361);
        cqCircle(c, st, 361, 360, {"radius": 2, "forConstruction": false});
        atStep(st, 362);
        cqExtrude(c, st, 362, 361, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        atStep(st, 363);
        cqCut(c, st, 363, 359, {"toCut": {"wp": 362}, "clean": true, "tol": undefined});
        stepVol(c, st, 363, "cut");
        atStep(st, 364);
        cqWorkplaneNew(c, st, 364, {"plane": {"origin": [-7.520617896943269, 4.755750600996662, 8.051282051282051], "xDir": [-0.5344658261278022, -0.8451900855437942, 0], "zDir": [-0.6267181580786058, 0.3963125500830552, 0.6709401709401709]}, "obj": undefined});
        atStep(st, 365);
        cqCircle(c, st, 365, 364, {"radius": 2, "forConstruction": false});
        atStep(st, 366);
        cqExtrude(c, st, 366, 365, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 366, "extrude");
        atStep(st, 367);
        cqCut(c, st, 367, 363, {"toCut": {"wp": 366}, "clean": true, "tol": undefined});
        stepVol(c, st, 367, "cut");
        atStep(st, 368);
        cqWorkplaneNew(c, st, 368, {"plane": {"origin": [-9.115783091320711, 0.7359020902465863, 7.76923076923077], "xDir": [-0.08046656871672589, -0.99675730813421, 0], "zDir": [-0.7596485909433927, 0.06132517418721552, 0.6474358974358975]}, "obj": undefined});
        atStep(st, 369);
        cqCircle(c, st, 369, 368, {"radius": 2, "forConstruction": false});
        atStep(st, 370);
        cqExtrude(c, st, 370, 369, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 370, "extrude");
        atStep(st, 371);
        cqCut(c, st, 371, 367, {"toCut": {"wp": 370}, "clean": true, "tol": undefined});
        stepVol(c, st, 371, "cut");
        atStep(st, 372);
        cqWorkplaneNew(c, st, 372, {"plane": {"origin": [-8.627334690071905, -3.6757637944025454, 7.487179487179487], "xDir": [0.3919666098600748, -0.9199794436588244, 2.7755575615628914e-17], "zDir": [-0.7189445575059922, -0.3063136495335455, 0.623931623931624]}, "obj": undefined});
        atStep(st, 373);
        cqCircle(c, st, 373, 372, {"radius": 2, "forConstruction": false});
        atStep(st, 374);
        cqExtrude(c, st, 374, 373, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 374, "extrude");
        atStep(st, 375);
        cqCut(c, st, 375, 371, {"toCut": {"wp": 374}, "clean": true, "tol": undefined});
        stepVol(c, st, 375, "cut");
        atStep(st, 376);
        cqWorkplaneNew(c, st, 376, {"plane": {"origin": [-6.069041769414467, -7.433226725236391, 7.205128205128205], "xDir": [0.774604961827654, -0.6324453755953781, 0], "zDir": [-0.505753480784539, -0.619435560436366, 0.6004273504273505]}, "obj": undefined});
        atStep(st, 377);
        cqCircle(c, st, 377, 376, {"radius": 2, "forConstruction": false});
        atStep(st, 378);
        cqExtrude(c, st, 378, 377, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 378, "extrude");
        atStep(st, 379);
        cqCut(c, st, 379, 375, {"toCut": {"wp": 378}, "clean": true, "tol": undefined});
        stepVol(c, st, 379, "cut");
        atStep(st, 380);
        cqWorkplaneNew(c, st, 380, {"plane": {"origin": [-1.9605682010258672, -9.603498229618516, 6.9230769230769225], "xDir": [0.9797906520422676, -0.2000256937760456, 1.3877787807814457e-17], "zDir": [-0.16338068341882228, -0.8002915191348764, 0.5769230769230769]}, "obj": undefined});
        atStep(st, 381);
        cqCircle(c, st, 381, 380, {"radius": 2, "forConstruction": false});
        atStep(st, 382);
        cqExtrude(c, st, 382, 381, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 382, "extrude");
        atStep(st, 383);
        cqCut(c, st, 383, 379, {"toCut": {"wp": 382}, "clean": true, "tol": undefined});
        stepVol(c, st, 383, "cut");
        atStep(st, 384);
        cqWorkplaneNew(c, st, 384, {"plane": {"origin": [2.780738366334543, -9.600222527276912, 6.6410256410256405], "xDir": [0.9605181116313728, 0.27821746391645097, 0], "zDir": [0.2317281971945453, -0.8000185439397427, 0.5534188034188033]}, "obj": undefined});
        atStep(st, 385);
        cqCircle(c, st, 385, 384, {"radius": 2, "forConstruction": false});
        atStep(st, 386);
        cqExtrude(c, st, 386, 385, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 386, "extrude");
        atStep(st, 387);
        cqCut(c, st, 387, 383, {"toCut": {"wp": 386}, "clean": true, "tol": undefined});
        stepVol(c, st, 387, "cut");
        atStep(st, 388);
        cqWorkplaneNew(c, st, 388, {"plane": {"origin": [7.049587421399501, -7.339397951463906, 6.3589743589743595], "xDir": [0.7212024473438161, 0.6927243535095979, 2.7755575615628914e-17], "zDir": [0.5874656184499584, -0.6116164959553255, 0.5299145299145299]}, "obj": undefined});
        atStep(st, 389);
        cqCircle(c, st, 389, 388, {"radius": 2, "forConstruction": false});
        atStep(st, 390);
        cqExtrude(c, st, 390, 389, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 390, "extrude");
        atStep(st, 391);
        cqCut(c, st, 391, 387, {"toCut": {"wp": 390}, "clean": true, "tol": undefined});
        stepVol(c, st, 391, "cut");
        atStep(st, 392);
        cqWorkplaneNew(c, st, 392, {"plane": {"origin": [9.814992276307876, -3.2767258861821995, 6.0769230769230775], "xDir": [0.3166679938014733, 0.9485364419471451, 0], "zDir": [0.8179160230256562, -0.2730604905151833, 0.5064102564102565]}, "obj": undefined});
        atStep(st, 393);
        cqCircle(c, st, 393, 392, {"radius": 2, "forConstruction": false});
        atStep(st, 394);
        cqExtrude(c, st, 394, 393, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 394, "extrude");
        atStep(st, 395);
        cqCut(c, st, 395, 391, {"toCut": {"wp": 394}, "clean": true, "tol": undefined});
        stepVol(c, st, 395, "cut");
        atStep(st, 396);
        cqWorkplaneNew(c, st, 396, {"plane": {"origin": [10.371989766355936, 1.6856124013565688, 5.794871794871795], "xDir": [-0.16041128085775902, 0.9870502626379132, -1.3877787807814457e-17], "zDir": [0.8643324805296614, 0.1404677001130474, 0.4829059829059829]}, "obj": undefined});
        atStep(st, 397);
        cqCircle(c, st, 397, 396, {"radius": 2, "forConstruction": false});
        atStep(st, 398);
        cqExtrude(c, st, 398, 397, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 398, "extrude");
        atStep(st, 399);
        cqCut(c, st, 399, 395, {"toCut": {"wp": 398}, "clean": true, "tol": undefined});
        stepVol(c, st, 399, "cut");
        atStep(st, 400);
        cqWorkplaneNew(c, st, 400, {"plane": {"origin": [8.521055245342643, 6.403157619428406, 5.512820512820513], "xDir": [-0.6007422642379777, 0.7994427634035022, 0], "zDir": [0.7100879371118869, 0.5335964682857005, 0.4594017094017094]}, "obj": undefined});
        atStep(st, 401);
        cqCircle(c, st, 401, 400, {"radius": 2, "forConstruction": false});
        atStep(st, 402);
        cqExtrude(c, st, 402, 401, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 402, "extrude");
        atStep(st, 403);
        cqCut(c, st, 403, 399, {"toCut": {"wp": 402}, "clean": true, "tol": undefined});
        stepVol(c, st, 403, "cut");
        atStep(st, 404);
        cqWorkplaneNew(c, st, 404, {"plane": {"origin": [4.62986087317134, 9.757225094744664, 5.23076923076923], "xDir": [-0.9034504346103822, 0.4286925614030545, 2.7755575615628907e-17], "zDir": [0.38582173943094505, 0.813102091228722, 0.43589743589743585]}, "obj": undefined});
        atStep(st, 405);
        cqCircle(c, st, 405, 404, {"radius": 2, "forConstruction": false});
        atStep(st, 406);
        cqExtrude(c, st, 406, 405, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 406, "extrude");
        atStep(st, 407);
        cqCut(c, st, 407, 403, {"toCut": {"wp": 406}, "clean": true, "tol": undefined});
        stepVol(c, st, 407, "cut");
        atStep(st, 408);
        cqWorkplaneNew(c, st, 408, {"plane": {"origin": [-0.44019014252097455, 10.92320572462426, 4.948717948717948], "xDir": [-0.9991889981715698, -0.04026594010941441, -1.734723475976807e-18], "zDir": [-0.03668251187674788, 0.9102671437186884, 0.4123931623931623]}, "obj": undefined});
        atStep(st, 409);
        cqCircle(c, st, 409, 408, {"radius": 2, "forConstruction": false});
        atStep(st, 410);
        cqExtrude(c, st, 410, 409, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 410, "extrude");
        atStep(st, 411);
        cqCut(c, st, 411, 407, {"toCut": {"wp": 410}, "clean": true, "tol": undefined});
        stepVol(c, st, 411, "cut");
        atStep(st, 412);
        cqWorkplaneNew(c, st, 412, {"plane": {"origin": [-5.527707983925656, 9.574271077563388, 4.666666666666667], "xDir": [-0.8660254037844393, -0.4999999999999991, 2.7755575615628914e-17], "zDir": [-0.46064233199380467, 0.7978559231302823, 0.3888888888888889]}, "obj": undefined});
        atStep(st, 413);
        cqCircle(c, st, 413, 412, {"radius": 2, "forConstruction": false});
        atStep(st, 414);
        cqExtrude(c, st, 414, 413, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 414, "extrude");
        atStep(st, 415);
        cqCut(c, st, 415, 411, {"toCut": {"wp": 414}, "clean": true, "tol": undefined});
        stepVol(c, st, 415, "cut");
        atStep(st, 416);
        cqWorkplaneNew(c, st, 416, {"plane": {"origin": [-9.441009630704738, 5.97013275245488, 4.384615384615385], "xDir": [-0.5344658261278024, -0.8451900855437939, 0], "zDir": [-0.7867508025587281, 0.49751106270457335, 0.3653846153846154]}, "obj": undefined});
        atStep(st, 417);
        cqCircle(c, st, 417, 416, {"radius": 2, "forConstruction": false});
        atStep(st, 418);
        cqExtrude(c, st, 418, 417, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 418, "extrude");
        atStep(st, 419);
        cqCut(c, st, 419, 415, {"toCut": {"wp": 418}, "clean": true, "tol": undefined});
        stepVol(c, st, 419, "cut");
        atStep(st, 420);
        cqWorkplaneNew(c, st, 420, {"plane": {"origin": [-11.240354327940237, 0.9074152118559524, 4.102564102564102], "xDir": [-0.0804665687167279, -0.9967573081342098, 0], "zDir": [-0.9366961939950198, 0.07561793432132938, 0.3418803418803419]}, "obj": undefined});
        atStep(st, 421);
        cqCircle(c, st, 421, 420, {"radius": 2, "forConstruction": false});
        atStep(st, 422);
        cqExtrude(c, st, 422, 421, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 422, "extrude");
        atStep(st, 423);
        cqCut(c, st, 423, 419, {"toCut": {"wp": 422}, "clean": true, "tol": undefined});
        stepVol(c, st, 423, "cut");
        atStep(st, 424);
        cqWorkplaneNew(c, st, 424, {"plane": {"origin": [-10.465294157281416, -4.458845140826114, 3.82051282051282], "xDir": [0.391966609860076, -0.9199794436588238, 1.3877787807814457e-17], "zDir": [-0.872107846440118, -0.3715704284021762, 0.3183760683760684]}, "obj": undefined});
        atStep(st, 425);
        cqCircle(c, st, 425, 424, {"radius": 2, "forConstruction": false});
        atStep(st, 426);
        cqExtrude(c, st, 426, 425, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 427);
        cqCut(c, st, 427, 423, {"toCut": {"wp": 426}, "clean": true, "tol": undefined});
        stepVol(c, st, 427, "cut");
        atStep(st, 428);
        cqWorkplaneNew(c, st, 428, {"plane": {"origin": [-7.251898156801182, -8.88196279344898, 3.538461538461538], "xDir": [0.7746049618276539, -0.6324453755953781, 0], "zDir": [-0.6043248464000985, -0.7401635661207484, 0.2948717948717948]}, "obj": undefined});
        atStep(st, 429);
        cqCircle(c, st, 429, 428, {"radius": 2, "forConstruction": false});
        atStep(st, 430);
        cqExtrude(c, st, 430, 429, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 430, "extrude");
        atStep(st, 431);
        cqCut(c, st, 431, 427, {"toCut": {"wp": 430}, "clean": true, "tol": undefined});
        stepVol(c, st, 431, "cut");
        atStep(st, 432);
        cqWorkplaneNew(c, st, 432, {"plane": {"origin": [-2.310238679094911, -11.316297516748133, 3.256410256410257], "xDir": [0.9797906520422678, -0.20002569377604415, 0], "zDir": [-0.19251988992457592, -0.9430247930623442, 0.2713675213675214]}, "obj": undefined});
        atStep(st, 433);
        cqCircle(c, st, 433, 432, {"radius": 2, "forConstruction": false});
        atStep(st, 434);
        cqExtrude(c, st, 434, 433, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 434, "extrude");
        atStep(st, 435);
        cqCut(c, st, 435, 431, {"toCut": {"wp": 434}, "clean": true, "tol": undefined});
        stepVol(c, st, 435, "cut");
        atStep(st, 436);
        cqWorkplaneNew(c, st, 436, {"plane": {"origin": [3.2344283559724625, -11.166542074506838, 2.9743589743589745], "xDir": [0.9605181116313728, 0.27821746391645075, 0], "zDir": [0.2695356963310386, -0.93054517287557, 0.24786324786324793]}, "obj": undefined});
        atStep(st, 437);
        cqCircle(c, st, 437, 436, {"radius": 2, "forConstruction": false});
        atStep(st, 438);
        cqExtrude(c, st, 438, 437, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 438, "extrude");
        atStep(st, 439);
        cqCut(c, st, 439, 435, {"toCut": {"wp": 438}, "clean": true, "tol": undefined});
        stepVol(c, st, 439, "cut");
        atStep(st, 440);
        cqWorkplaneNew(c, st, 440, {"plane": {"origin": [8.100773185782268, -8.433798259530034, 2.6923076923076925], "xDir": [0.721202447343815, 0.692724353509599, -2.7755575615628914e-17], "zDir": [0.6750644321485223, -0.7028165216275029, 0.22435897435897437]}, "obj": undefined});
        atStep(st, 441);
        cqCircle(c, st, 441, 440, {"radius": 2, "forConstruction": false});
        atStep(st, 442);
        cqExtrude(c, st, 442, 441, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 442, "extrude");
        atStep(st, 443);
        cqCut(c, st, 443, 439, {"toCut": {"wp": 442}, "clean": true, "tol": undefined});
        stepVol(c, st, 443, "cut");
        atStep(st, 444);
        cqWorkplaneNew(c, st, 444, {"plane": {"origin": [11.15047511030307, -3.72257556690655, 2.41025641025641], "xDir": [0.3166679938014718, 0.9485364419471456, -6.9388939039072276e-18], "zDir": [0.9292062591919226, -0.3102146305755459, 0.2008547008547009]}, "obj": undefined});
        atStep(st, 445);
        cqCircle(c, st, 445, 444, {"radius": 2, "forConstruction": false});
        atStep(st, 446);
        cqExtrude(c, st, 446, 445, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 446, "extrude");
        atStep(st, 447);
        cqCut(c, st, 447, 443, {"toCut": {"wp": 446}, "clean": true, "tol": undefined});
        stepVol(c, st, 447, "cut");
        atStep(st, 448);
        cqWorkplaneNew(c, st, 448, {"plane": {"origin": [11.656839734443302, 1.894420844951088, 2.1282051282051277], "xDir": [-0.1604112808577623, 0.9870502626379126, -3.469446951953614e-18], "zDir": [0.9714033112036086, 0.157868403745924, 0.1773504273504273]}, "obj": undefined});
        atStep(st, 449);
        cqCircle(c, st, 449, 448, {"radius": 2, "forConstruction": false});
        atStep(st, 450);
        cqExtrude(c, st, 450, 449, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 450, "extrude");
        atStep(st, 451);
        cqCut(c, st, 451, 447, {"toCut": {"wp": 450}, "clean": true, "tol": undefined});
        stepVol(c, st, 451, "cut");
        atStep(st, 452);
        cqWorkplaneNew(c, st, 452, {"plane": {"origin": [9.479102980844983, 7.123083788280685, 1.8461538461538456], "xDir": [-0.6007422642379789, 0.7994427634035011, 0], "zDir": [0.7899252484037487, 0.5935903156900572, 0.15384615384615383]}, "obj": undefined});
        atStep(st, 453);
        cqCircle(c, st, 453, 452, {"radius": 2, "forConstruction": false});
        atStep(st, 454);
        cqExtrude(c, st, 454, 453, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 454, "extrude");
        atStep(st, 455);
        cqCut(c, st, 455, 451, {"toCut": {"wp": 454}, "clean": true, "tol": undefined});
        stepVol(c, st, 455, "cut");
        atStep(st, 456);
        cqWorkplaneNew(c, st, 456, {"plane": {"origin": [5.100425183001816, 10.748918369843947, 1.5641025641025643], "xDir": [-0.9034504346103828, 0.42869256140305306, -6.93889390390723e-18], "zDir": [0.425035431916818, 0.8957431974869956, 0.13034188034188038]}, "obj": undefined});
        atStep(st, 457);
        cqCircle(c, st, 457, 456, {"radius": 2, "forConstruction": false});
        atStep(st, 458);
        cqExtrude(c, st, 458, 457, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 458, "extrude");
        atStep(st, 459);
        cqCut(c, st, 459, 455, {"toCut": {"wp": 458}, "clean": true, "tol": undefined});
        stepVol(c, st, 459, "cut");
        atStep(st, 460);
        cqWorkplaneNew(c, st, 460, {"plane": {"origin": [-0.48042572807187056, 11.921641482195197, 1.2820512820512824], "xDir": [-0.9991889981715696, -0.040265940109414165, 0], "zDir": [-0.04003547733932255, 0.9934701235162664, 0.10683760683760686]}, "obj": undefined});
        atStep(st, 461);
        cqCircle(c, st, 461, 460, {"radius": 2, "forConstruction": false});
        atStep(st, 462);
        cqExtrude(c, st, 462, 461, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 462, "extrude");
        atStep(st, 463);
        cqCut(c, st, 463, 459, {"toCut": {"wp": 462}, "clean": true, "tol": undefined});
        stepVol(c, st, 463, "cut");
        atStep(st, 464);
        cqWorkplaneNew(c, st, 464, {"plane": {"origin": [-2.29017990487849, -1.2019791374290816, 11.717948717948715], "xDir": [0.4647231720437685, -0.88545602565321, 0], "zDir": [-0.19084832540654084, -0.10016492811909011, 0.9764957264957264]}, "obj": undefined});
        atStep(st, 465);
        cqCircle(c, st, 465, 464, {"radius": 2, "forConstruction": false});
        atStep(st, 466);
        cqExtrude(c, st, 466, 465, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 466, "extrude");
        atStep(st, 467);
        cqCut(c, st, 467, 463, {"toCut": {"wp": 466}, "clean": true, "tol": undefined});
        stepVol(c, st, 467, "cut");
        atStep(st, 468);
        cqWorkplaneNew(c, st, 468, {"plane": {"origin": [-2.0654641894735417, -2.992341477444852, 11.435897435897436], "xDir": [0.8229838658936564, -0.568064746731156, 0], "zDir": [-0.17212201578946182, -0.249361789787071, 0.952991452991453]}, "obj": undefined});
        atStep(st, 469);
        cqCircle(c, st, 469, 468, {"radius": 2, "forConstruction": false});
        atStep(st, 470);
        cqExtrude(c, st, 470, 469, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 470, "extrude");
        atStep(st, 471);
        cqCut(c, st, 471, 467, {"toCut": {"wp": 470}, "clean": true, "tol": undefined});
        stepVol(c, st, 471, "cut");
        atStep(st, 472);
        cqWorkplaneNew(c, st, 472, {"plane": {"origin": [-0.5335258474418862, -4.393980672060671, 11.153846153846157], "xDir": [0.992708874098054, -0.12053668025532269, -6.9388939039072276e-18], "zDir": [-0.044460487286823856, -0.36616505600505594, 0.9294871794871796]}, "obj": undefined});
        atStep(st, 473);
        cqCircle(c, st, 473, 472, {"radius": 2, "forConstruction": false});
        atStep(st, 474);
        cqExtrude(c, st, 474, 473, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 474, "extrude");
        atStep(st, 475);
        cqCut(c, st, 475, 471, {"toCut": {"wp": 474}, "clean": true, "tol": undefined});
        stepVol(c, st, 475, "cut");
        atStep(st, 476);
        cqWorkplaneNew(c, st, 476, {"plane": {"origin": [1.8013117212430627, -4.7496686567093365, 10.871794871794872], "xDir": [0.9350162426854148, 0.3546048870425356, 0], "zDir": [0.15010931010358858, -0.39580572139244474, 0.905982905982906]}, "obj": undefined});
        atStep(st, 477);
        cqCircle(c, st, 477, 476, {"radius": 2, "forConstruction": false});
        atStep(st, 478);
        cqExtrude(c, st, 478, 477, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 478, "extrude");
        atStep(st, 479);
        cqCut(c, st, 479, 475, {"toCut": {"wp": 478}, "clean": true, "tol": undefined});
        stepVol(c, st, 479, "cut");
        atStep(st, 480);
        cqWorkplaneNew(c, st, 480, {"plane": {"origin": [4.224766714759439, -3.7428167079547574, 10.58974358974359], "xDir": [0.6631226582407955, 0.7485107481711009, 0], "zDir": [0.35206389289661993, -0.3119013923295631, 0.8824786324786325]}, "obj": undefined});
        atStep(st, 481);
        cqCircle(c, st, 481, 480, {"radius": 2, "forConstruction": false});
        atStep(st, 482);
        cqExtrude(c, st, 482, 481, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 482, "extrude");
        atStep(st, 483);
        cqCut(c, st, 483, 479, {"toCut": {"wp": 482}, "clean": true, "tol": undefined});
        stepVol(c, st, 483, "cut");
        atStep(st, 484);
        cqWorkplaneNew(c, st, 484, {"plane": {"origin": [5.965683284609621, -1.470408867516168, 10.307692307692308], "xDir": [0.2393156642875578, 0.970941817426052, -1.3877787807814457e-17], "zDir": [0.4971402737174684, -0.122534072293014, 0.858974358974359]}, "obj": undefined});
        atStep(st, 485);
        cqCircle(c, st, 485, 484, {"radius": 2, "forConstruction": false});
        atStep(st, 486);
        cqExtrude(c, st, 486, 485, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 486, "extrude");
        atStep(st, 487);
        cqCut(c, st, 487, 483, {"toCut": {"wp": 486}, "clean": true, "tol": undefined});
        stepVol(c, st, 487, "cut");
        atStep(st, 488);
        cqWorkplaneNew(c, st, 488, {"plane": {"origin": [6.402808953369854, 1.578150668228611, 10.025641025641026], "xDir": [-0.2393156642875574, 0.9709418174260522, 0], "zDir": [0.5335674127808211, 0.1315125556857176, 0.8354700854700855]}, "obj": undefined});
        atStep(st, 489);
        cqCircle(c, st, 489, 488, {"radius": 2, "forConstruction": false});
        atStep(st, 490);
        cqExtrude(c, st, 490, 489, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 490, "extrude");
        atStep(st, 491);
        cqCut(c, st, 491, 487, {"toCut": {"wp": 490}, "clean": true, "tol": undefined});
        stepVol(c, st, 491, "cut");
        atStep(st, 492);
        cqWorkplaneNew(c, st, 492, {"plane": {"origin": [5.242913542612548, 4.644816088210886, 9.743589743589745], "xDir": [-0.6631226582407952, 0.7485107481711013, -5.551115123125783e-17], "zDir": [0.436909461884379, 0.3870680073509072, 0.8119658119658121]}, "obj": undefined});
        atStep(st, 493);
        cqCircle(c, st, 493, 492, {"radius": 2, "forConstruction": false});
        atStep(st, 494);
        cqExtrude(c, st, 494, 493, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 494, "extrude");
        atStep(st, 495);
        cqCut(c, st, 495, 491, {"toCut": {"wp": 494}, "clean": true, "tol": undefined});
        stepVol(c, st, 495, "cut");
        atStep(st, 496);
        cqWorkplaneNew(c, st, 496, {"plane": {"origin": [2.6173417686069036, 6.901363054291096, 9.46153846153846], "xDir": [-0.9350162426854148, 0.35460488704253595, 2.7755575615628907e-17], "zDir": [0.2181118140505753, 0.5751135878575914, 0.7884615384615383]}, "obj": undefined});
        atStep(st, 497);
        cqCircle(c, st, 497, 496, {"radius": 2, "forConstruction": false});
        atStep(st, 498);
        cqExtrude(c, st, 498, 497, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 498, "extrude");
        atStep(st, 499);
        cqCut(c, st, 499, 495, {"toCut": {"wp": 498}, "clean": true, "tol": undefined});
        stepVol(c, st, 499, "cut");
        atStep(st, 500);
        cqWorkplaneNew(c, st, 500, {"plane": {"origin": [-0.9316246808202876, 7.672619538053286, 9.17948717948718], "xDir": [-0.992708874098054, -0.1205366802553222, 0], "zDir": [-0.0776353900683573, 0.6393849615044405, 0.7649572649572649]}, "obj": undefined});
        atStep(st, 501);
        cqCircle(c, st, 501, 500, {"radius": 2, "forConstruction": false});
        atStep(st, 502);
        cqExtrude(c, st, 502, 501, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 502, "extrude");
        atStep(st, 503);
        cqCut(c, st, 503, 499, {"toCut": {"wp": 502}, "clean": true, "tol": undefined});
        stepVol(c, st, 503, "cut");
        atStep(st, 504);
        cqWorkplaneNew(c, st, 504, {"plane": {"origin": [-4.57409016199858, 6.6267136380657705, 8.897435897435898], "xDir": [-0.8229838658936562, -0.5680647467311563, -2.775557561562892e-17], "zDir": [-0.3811741801665483, 0.5522261365054809, 0.7414529914529915]}, "obj": undefined});
        atStep(st, 505);
        cqCircle(c, st, 505, 504, {"radius": 2, "forConstruction": false});
        atStep(st, 506);
        cqExtrude(c, st, 506, 505, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 506, "extrude");
        atStep(st, 507);
        cqCut(c, st, 507, 503, {"toCut": {"wp": 506}, "clean": true, "tol": undefined});
        stepVol(c, st, 507, "cut");
        atStep(st, 508);
        cqWorkplaneNew(c, st, 508, {"plane": {"origin": [-7.396348750072804, 3.881903282706894, 8.615384615384615], "xDir": [-0.46472317204376856, -0.88545602565321, 0], "zDir": [-0.6163623958394003, 0.32349194022557454, 0.717948717948718]}, "obj": undefined});
        atStep(st, 509);
        cqCircle(c, st, 509, 508, {"radius": 2, "forConstruction": false});
        atStep(st, 510);
        cqExtrude(c, st, 510, 509, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 510, "extrude");
        atStep(st, 511);
        cqCut(c, st, 511, 507, {"toCut": {"wp": 510}, "clean": true, "tol": undefined});
        stepVol(c, st, 511, "cut");
        atStep(st, 512);
        cqWorkplaneNew(c, st, 512, {"plane": {"origin": [-8.634555897992412, 3.172284372760592e-15, 8.333333333333334], "xDir": [-3.6739403974420594e-16, -1, 2.465190328815662e-32], "zDir": [-0.719546324832701, 2.6435703106338264e-16, 0.6944444444444445]}, "obj": undefined});
        atStep(st, 513);
        cqCircle(c, st, 513, 512, {"radius": 2, "forConstruction": false});
        atStep(st, 514);
        cqExtrude(c, st, 514, 513, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 514, "extrude");
        atStep(st, 515);
        cqCut(c, st, 515, 511, {"toCut": {"wp": 514}, "clean": true, "tol": undefined});
        stepVol(c, st, 515, "cut");
        atStep(st, 516);
        cqWorkplaneNew(c, st, 516, {"plane": {"origin": [-7.8789097829978525, -4.135170850410443, 8.051282051282051], "xDir": [0.46472317204376784, -0.8854560256532104, -2.7755575615628907e-17], "zDir": [-0.6565758152498211, -0.34459757086753695, 0.670940170940171]}, "obj": undefined});
        atStep(st, 517);
        cqCircle(c, st, 517, 516, {"radius": 2, "forConstruction": false});
        atStep(st, 518);
        cqExtrude(c, st, 518, 517, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 518, "extrude");
        atStep(st, 519);
        cqCut(c, st, 519, 515, {"toCut": {"wp": 518}, "clean": true, "tol": undefined});
        stepVol(c, st, 519, "cut");
        atStep(st, 520);
        cqWorkplaneNew(c, st, 520, {"plane": {"origin": [-5.195201450511976, -7.526548687349076, 7.76923076923077], "xDir": [0.8229838658936558, -0.5680647467311568, 0], "zDir": [-0.4329334542093313, -0.6272123906124231, 0.6474358974358975]}, "obj": undefined});
        atStep(st, 521);
        cqCircle(c, st, 521, 520, {"radius": 2, "forConstruction": false});
        atStep(st, 522);
        cqExtrude(c, st, 522, 521, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 522, "extrude");
        atStep(st, 523);
        cqCut(c, st, 523, 519, {"toCut": {"wp": 522}, "clean": true, "tol": undefined});
        stepVol(c, st, 523, "cut");
        atStep(st, 524);
        cqWorkplaneNew(c, st, 524, {"plane": {"origin": [-1.130362520772263, -9.309372905754287, 7.487179487179487], "xDir": [0.992708874098054, -0.12053668025532294, 0], "zDir": [-0.09419687673102192, -0.7757810754795242, 0.623931623931624]}, "obj": undefined});
        atStep(st, 525);
        cqCircle(c, st, 525, 524, {"radius": 2, "forConstruction": false});
        atStep(st, 526);
        cqExtrude(c, st, 526, 525, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 526, "extrude");
        atStep(st, 527);
        cqCut(c, st, 527, 523, {"toCut": {"wp": 526}, "clean": true, "tol": undefined});
        stepVol(c, st, 527, "cut");
        atStep(st, 528);
        cqWorkplaneNew(c, st, 528, {"plane": {"origin": [3.4028422914368983, -8.97255771155998, 7.205128205128205], "xDir": [0.9350162426854148, 0.3546048870425353, 0], "zDir": [0.28357019095307484, -0.7477131426299984, 0.6004273504273504]}, "obj": undefined});
        atStep(st, 529);
        cqCircle(c, st, 529, 528, {"radius": 2, "forConstruction": false});
        atStep(st, 530);
        cqExtrude(c, st, 530, 529, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 530, "extrude");
        atStep(st, 531);
        cqCut(c, st, 531, 527, {"toCut": {"wp": 530}, "clean": true, "tol": undefined});
        stepVol(c, st, 531, "cut");
        atStep(st, 532);
        cqWorkplaneNew(c, st, 532, {"plane": {"origin": [7.336589331535587, -6.499650982749611, 6.9230769230769225], "xDir": [0.6631226582407957, 0.7485107481711006, -2.775557561562892e-17], "zDir": [0.6113824442946322, -0.5416375818958009, 0.5769230769230769]}, "obj": undefined});
        atStep(st, 533);
        cqCircle(c, st, 533, 532, {"radius": 2, "forConstruction": false});
        atStep(st, 534);
        cqExtrude(c, st, 534, 533, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 534, "extrude");
        atStep(st, 535);
        cqCut(c, st, 535, 531, {"toCut": {"wp": 534}, "clean": true, "tol": undefined});
        stepVol(c, st, 535, "cut");
        atStep(st, 536);
        cqWorkplaneNew(c, st, 536, {"plane": {"origin": [9.704405773772724, -2.3919211971146974, 6.6410256410256405], "xDir": [0.2393156642875589, 0.9709418174260518, -1.3877787807814457e-17], "zDir": [0.8087004811477271, -0.1993267664262248, 0.5534188034188033]}, "obj": undefined});
        atStep(st, 537);
        cqCircle(c, st, 537, 536, {"radius": 2, "forConstruction": false});
        atStep(st, 538);
        cqExtrude(c, st, 538, 537, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 538, "extrude");
        atStep(st, 539);
        cqCut(c, st, 539, 535, {"toCut": {"wp": 538}, "clean": true, "tol": undefined});
        stepVol(c, st, 539, "cut");
        atStep(st, 540);
        cqWorkplaneNew(c, st, 540, {"plane": {"origin": [9.88089878515096, 2.4354228173992545, 6.3589743589743595], "xDir": [-0.23931566428755616, 0.9709418174260523, 0], "zDir": [0.8234082320959134, 0.20295190144993788, 0.5299145299145299]}, "obj": undefined});
        atStep(st, 541);
        cqCircle(c, st, 541, 540, {"radius": 2, "forConstruction": false});
        atStep(st, 542);
        cqExtrude(c, st, 542, 541, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 542, "extrude");
        atStep(st, 543);
        cqCut(c, st, 543, 539, {"toCut": {"wp": 542}, "clean": true, "tol": undefined});
        stepVol(c, st, 543, "cut");
        atStep(st, 544);
        cqWorkplaneNew(c, st, 544, {"plane": {"origin": [7.745223996825795, 6.861669706139578, 6.0769230769230775], "xDir": [-0.663122658240795, 0.7485107481711012, 0], "zDir": [0.6454353330688163, 0.5718058088449648, 0.5064102564102565]}, "obj": undefined});
        atStep(st, 545);
        cqCircle(c, st, 545, 544, {"radius": 2, "forConstruction": false});
        atStep(st, 546);
        cqExtrude(c, st, 546, 545, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 546, "extrude");
        atStep(st, 547);
        cqCut(c, st, 547, 543, {"toCut": {"wp": 546}, "clean": true, "tol": undefined});
        stepVol(c, st, 547, "cut");
        atStep(st, 548);
        cqWorkplaneNew(c, st, 548, {"plane": {"origin": [3.7262117226690825, 9.825212826134752, 5.794871794871795], "xDir": [-0.9350162426854146, 0.3546048870425362, 0], "zDir": [0.3105176435557569, 0.8187677355112294, 0.4829059829059829]}, "obj": undefined});
        atStep(st, 549);
        cqCircle(c, st, 549, 548, {"radius": 2, "forConstruction": false});
        atStep(st, 550);
        cqExtrude(c, st, 550, 549, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 550, "extrude");
        atStep(st, 551);
        cqCut(c, st, 551, 547, {"toCut": {"wp": 550}, "clean": true, "tol": undefined});
        stepVol(c, st, 551, "cut");
        atStep(st, 552);
        cqWorkplaneNew(c, st, 552, {"plane": {"origin": [-1.2847695401895745, 10.581029119231571, 5.512820512820513], "xDir": [-0.992708874098054, -0.12053668025532197, 0], "zDir": [-0.1070641283491312, 0.8817524266026311, 0.4594017094017094]}, "obj": undefined});
        atStep(st, 553);
        cqCircle(c, st, 553, 552, {"radius": 2, "forConstruction": false});
        atStep(st, 554);
        cqExtrude(c, st, 554, 553, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 554, "extrude");
        atStep(st, 555);
        cqCut(c, st, 555, 551, {"toCut": {"wp": 554}, "clean": true, "tol": undefined});
        stepVol(c, st, 555, "cut");
        atStep(st, 556);
        cqWorkplaneNew(c, st, 556, {"plane": {"origin": [-6.13507436590624, 8.888189679526311, 5.23076923076923], "xDir": [-0.8229838658936564, -0.568064746731156, 2.7755575615628914e-17], "zDir": [-0.5112561971588533, 0.7406824732938593, 0.43589743589743585]}, "obj": undefined});
        atStep(st, 557);
        cqCircle(c, st, 557, 556, {"radius": 2, "forConstruction": false});
        atStep(st, 558);
        cqExtrude(c, st, 558, 557, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 558, "extrude");
        atStep(st, 559);
        cqCut(c, st, 559, 555, {"toCut": {"wp": 558}, "clean": true, "tol": undefined});
        stepVol(c, st, 559, "cut");
        atStep(st, 560);
        cqWorkplaneNew(c, st, 560, {"plane": {"origin": [-9.679868719548706, 5.080387016393468, 4.948717948717948], "xDir": [-0.46472317204376873, -0.8854560256532098, 2.7755575615628914e-17], "zDir": [-0.8066557266290588, 0.4233655846994557, 0.4123931623931623]}, "obj": undefined});
        atStep(st, 561);
        cqCircle(c, st, 561, 560, {"radius": 2, "forConstruction": false});
        atStep(st, 562);
        cqExtrude(c, st, 562, 561, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 562, "extrude");
        atStep(st, 563);
        cqCut(c, st, 563, 559, {"toCut": {"wp": 562}, "clean": true, "tol": undefined});
        stepVol(c, st, 563, "cut");
        atStep(st, 564);
        cqWorkplaneNew(c, st, 564, {"plane": {"origin": [-11.055415967851332, 6.769489889135837e-15, 4.666666666666667], "xDir": [-6.123233995736766e-16, -1, 0], "zDir": [-0.9212846639876112, 5.641241574279864e-16, 0.3888888888888889]}, "obj": undefined});
        atStep(st, 565);
        cqCircle(c, st, 565, 564, {"radius": 2, "forConstruction": false});
        atStep(st, 566);
        cqExtrude(c, st, 566, 565, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 566, "extrude");
        atStep(st, 567);
        cqCut(c, st, 567, 563, {"toCut": {"wp": 566}, "clean": true, "tol": undefined});
        stepVol(c, st, 567, "cut");
        atStep(st, 568);
        cqWorkplaneNew(c, st, 568, {"plane": {"origin": [-9.890791442943796, -5.191087801336426, 4.384615384615385], "xDir": [0.4647231720437692, -0.8854560256532096, 0], "zDir": [-0.8242326202453164, -0.4325906501113689, 0.3653846153846155]}, "obj": undefined});
        atStep(st, 569);
        cqCircle(c, st, 569, 568, {"radius": 2, "forConstruction": false});
        atStep(st, 570);
        cqExtrude(c, st, 570, 569, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 570, "extrude");
        atStep(st, 571);
        cqCut(c, st, 571, 567, {"toCut": {"wp": 570}, "clean": true, "tol": undefined});
        stepVol(c, st, 571, "cut");
        atStep(st, 572);
        cqWorkplaneNew(c, st, 572, {"plane": {"origin": [-6.406021789217807, -9.280724789606634, 4.102564102564102], "xDir": [0.8229838658936557, -0.568064746731157, -2.775557561562892e-17], "zDir": [-0.5338351491014839, -0.7733937324672194, 0.34188034188034183]}, "obj": undefined});
        atStep(st, 573);
        cqCircle(c, st, 573, 572, {"radius": 2, "forConstruction": false});
        atStep(st, 574);
        cqExtrude(c, st, 574, 573, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 574, "extrude");
        atStep(st, 575);
        cqCut(c, st, 575, 571, {"toCut": {"wp": 574}, "clean": true, "tol": undefined});
        stepVol(c, st, 575, "cut");
        atStep(st, 576);
        cqWorkplaneNew(c, st, 576, {"plane": {"origin": [-1.3711739151445037, -11.29263316869562, 3.82051282051282], "xDir": [0.992708874098054, -0.1205366802553232, -6.938893903907228e-18], "zDir": [-0.11426449292870865, -0.9410527640579684, 0.31837606837606836]}, "obj": undefined});
        atStep(st, 577);
        cqCircle(c, st, 577, 576, {"radius": 2, "forConstruction": false});
        atStep(st, 578);
        cqExtrude(c, st, 578, 577, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 578, "extrude");
        atStep(st, 579);
        cqCut(c, st, 579, 575, {"toCut": {"wp": 578}, "clean": true, "tol": undefined});
        stepVol(c, st, 579, "cut");
        atStep(st, 580);
        cqWorkplaneNew(c, st, 580, {"plane": {"origin": [4.066056336194447, -10.721309426171851, 3.538461538461538], "xDir": [0.9350162426854144, 0.35460488704253673, 0], "zDir": [0.33883802801620394, -0.8934424521809876, 0.2948717948717948]}, "obj": undefined});
        atStep(st, 581);
        cqCircle(c, st, 581, 580, {"radius": 2, "forConstruction": false});
        atStep(st, 582);
        cqExtrude(c, st, 582, 581, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 582, "extrude");
        atStep(st, 583);
        cqCut(c, st, 583, 579, {"toCut": {"wp": 582}, "clean": true, "tol": undefined});
        stepVol(c, st, 583, "cut");
        atStep(st, 584);
        cqWorkplaneNew(c, st, 584, {"plane": {"origin": [8.645081786739174, -7.658874143275675, 3.256410256410257], "xDir": [0.663122658240796, 0.7485107481711005, 2.7755575615628907e-17], "zDir": [0.7204234822282646, -0.6382395119396396, 0.2713675213675214]}, "obj": undefined});
        atStep(st, 585);
        cqCircle(c, st, 585, 584, {"radius": 2, "forConstruction": false});
        atStep(st, 586);
        cqExtrude(c, st, 586, 585, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 586, "extrude");
        atStep(st, 587);
        cqCut(c, st, 587, 583, {"toCut": {"wp": 586}, "clean": true, "tol": undefined});
        stepVol(c, st, 587, "cut");
        atStep(st, 588);
        cqWorkplaneNew(c, st, 588, {"plane": {"origin": [11.287723286936934, -2.782173914260543, 2.9743589743589745], "xDir": [0.2393156642875609, 0.9709418174260512, 0], "zDir": [0.9406436072447448, -0.23184782618837865, 0.24786324786324793]}, "obj": undefined});
        atStep(st, 589);
        cqCircle(c, st, 589, 588, {"radius": 2, "forConstruction": false});
        atStep(st, 590);
        cqExtrude(c, st, 590, 589, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 590, "extrude");
        atStep(st, 591);
        cqCut(c, st, 591, 587, {"toCut": {"wp": 590}, "clean": true, "tol": undefined});
        stepVol(c, st, 591, "cut");
        atStep(st, 592);
        cqWorkplaneNew(c, st, 592, {"plane": {"origin": [11.35427013603712, 2.7985762394182507, 2.6923076923076925], "xDir": [-0.23931566428755943, 0.9709418174260516, 0], "zDir": [0.9461891780030935, 0.2332146866181876, 0.2243589743589744]}, "obj": undefined});
        atStep(st, 593);
        cqCircle(c, st, 593, 592, {"radius": 2, "forConstruction": false});
        atStep(st, 594);
        cqExtrude(c, st, 594, 593, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 594, "extrude");
        atStep(st, 595);
        cqCut(c, st, 595, 591, {"toCut": {"wp": 594}, "clean": true, "tol": undefined});
        stepVol(c, st, 595, "cut");
        atStep(st, 596);
        cqWorkplaneNew(c, st, 596, {"plane": {"origin": [8.799082563599875, 7.795306926335264, 2.41025641025641], "xDir": [-0.6631226582407949, 0.7485107481711014, -1.3877787807814457e-17], "zDir": [0.7332568802999896, 0.6496089105279387, 0.20085470085470084]}, "obj": undefined});
        atStep(st, 597);
        cqCircle(c, st, 597, 596, {"radius": 2, "forConstruction": false});
        atStep(st, 598);
        cqExtrude(c, st, 598, 597, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 598, "extrude");
        atStep(st, 599);
        cqCut(c, st, 599, 595, {"toCut": {"wp": 598}, "clean": true, "tol": undefined});
        stepVol(c, st, 599, "cut");
        atStep(st, 600);
        cqWorkplaneNew(c, st, 600, {"plane": {"origin": [4.187803290035241, 11.042329760347288, 2.1282051282051277], "xDir": [-0.935016242685415, 0.35460488704253484, 0], "zDir": [0.3489836075029368, 0.9201941466956074, 0.17735042735042733]}, "obj": undefined});
        atStep(st, 601);
        cqCircle(c, st, 601, 600, {"radius": 2, "forConstruction": false});
        atStep(st, 602);
        cqExtrude(c, st, 602, 601, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 602, "extrude");
        atStep(st, 603);
        cqCut(c, st, 603, 599, {"toCut": {"wp": 602}, "clean": true, "tol": undefined});
        stepVol(c, st, 603, "cut");
        atStep(st, 604);
        cqWorkplaneNew(c, st, 604, {"plane": {"origin": [-1.4292200235136618, 11.770685880640896, 1.8461538461538456], "xDir": [-0.992708874098054, -0.12053668025532167, 3.4694469519536138e-18], "zDir": [-0.11910166862613848, 0.980890490053408, 0.1538461538461538]}, "obj": undefined});
        atStep(st, 605);
        cqCircle(c, st, 605, 604, {"radius": 2, "forConstruction": false});
        atStep(st, 606);
        cqExtrude(c, st, 606, 605, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 606, "extrude");
        atStep(st, 607);
        cqCut(c, st, 607, 603, {"toCut": {"wp": 606}, "clean": true, "tol": undefined});
        stepVol(c, st, 607, "cut");
        atStep(st, 608);
        cqWorkplaneNew(c, st, 608, {"plane": {"origin": [-6.758623779989155, 9.79155696350345, 1.5641025641025643], "xDir": [-0.8229838658936564, -0.5680647467311558, 6.938893903907228e-18], "zDir": [-0.5632186483324296, 0.8159630802919541, 0.13034188034188038]}, "obj": undefined});
        atStep(st, 609);
        cqCircle(c, st, 609, 608, {"radius": 2, "forConstruction": false});
        atStep(st, 610);
        cqExtrude(c, st, 610, 609, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 610, "extrude");
        atStep(st, 611);
        cqCut(c, st, 611, 607, {"toCut": {"wp": 610}, "clean": true, "tol": undefined});
        stepVol(c, st, 611, "cut");
        atStep(st, 612);
        cqWorkplaneNew(c, st, 612, {"plane": {"origin": [-10.564657242427336, 5.5447598559557365, 1.2820512820512824], "xDir": [-0.46472317204377056, -0.8854560256532089, 0], "zDir": [-0.8803881035356115, 0.4620633213296447, 0.10683760683760686]}, "obj": undefined});
        atStep(st, 613);
        cqCircle(c, st, 613, 612, {"radius": 2, "forConstruction": false});
        atStep(st, 614);
        cqExtrude(c, st, 614, 613, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 614, "extrude");
        atStep(st, 615);
        cqCut(c, st, 615, 611, {"toCut": {"wp": 614}, "clean": true, "tol": undefined});
        stepVol(c, st, 615, "cut");
        atStep(st, 616);
        cqWorkplaneNew(c, st, 616, {"plane": {"origin": [-0.10414548460675564, -2.5843435455759427, 11.717948717948715], "xDir": [0.9991889981715696, -0.04026594010941603, -1.734723475976807e-18], "zDir": [-0.008678790383896302, -0.21536196213132855, 0.9764957264957264]}, "obj": undefined});
        atStep(st, 617);
        cqCircle(c, st, 617, 616, {"radius": 2, "forConstruction": false});
        atStep(st, 618);
        cqExtrude(c, st, 618, 617, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 618, "extrude");
        atStep(st, 619);
        cqCut(c, st, 619, 615, {"toCut": {"wp": 618}, "clean": true, "tol": undefined});
        stepVol(c, st, 619, "cut");
        atStep(st, 620);
        cqWorkplaneNew(c, st, 620, {"plane": {"origin": [1.5587116415283295, -3.2849151974135484, 11.435897435897436], "xDir": [0.9034504346103824, 0.4286925614030539, -1.3877787807814457e-17], "zDir": [0.12989263679402752, -0.27374293311779574, 0.9529914529914532]}, "obj": undefined});
        atStep(st, 621);
        cqCircle(c, st, 621, 620, {"radius": 2, "forConstruction": false});
        atStep(st, 622);
        cqExtrude(c, st, 622, 621, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 622, "extrude");
        atStep(st, 623);
        cqCut(c, st, 623, 619, {"toCut": {"wp": 622}, "clean": true, "tol": undefined});
        stepVol(c, st, 623, "cut");
        atStep(st, 624);
        cqWorkplaneNew(c, st, 624, {"plane": {"origin": [3.5385359620214154, -2.6590372734906342, 11.153846153846157], "xDir": [0.6007422642379796, 0.7994427634035006, 0], "zDir": [0.29487799683511795, -0.22158643945755285, 0.9294871794871796]}, "obj": undefined});
        atStep(st, 625);
        cqCircle(c, st, 625, 624, {"radius": 2, "forConstruction": false});
        atStep(st, 626);
        cqExtrude(c, st, 626, 625, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 626, "extrude");
        atStep(st, 627);
        cqCut(c, st, 627, 623, {"toCut": {"wp": 626}, "clean": true, "tol": undefined});
        stepVol(c, st, 627, "cut");
        atStep(st, 628);
        cqWorkplaneNew(c, st, 628, {"plane": {"origin": [5.013989576890526, -0.8148526176235042, 10.871794871794872], "xDir": [0.16041128085776069, 0.987050262637913, 6.938893903907228e-18], "zDir": [0.4178324647408772, -0.0679043848019587, 0.905982905982906]}, "obj": undefined});
        atStep(st, 629);
        cqCircle(c, st, 629, 628, {"radius": 2, "forConstruction": false});
        atStep(st, 630);
        cqExtrude(c, st, 630, 629, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 630, "extrude");
        atStep(st, 631);
        cqCut(c, st, 631, 627, {"toCut": {"wp": 630}, "clean": true, "tol": undefined});
        stepVol(c, st, 631, "cut");
        atStep(st, 632);
        cqWorkplaneNew(c, st, 632, {"plane": {"origin": [5.353757708177382, 1.787346946067219, 10.58974358974359], "xDir": [-0.3166679938014717, 0.9485364419471458, 0], "zDir": [0.44614647568144855, 0.1489455788389349, 0.8824786324786325]}, "obj": undefined});
        atStep(st, 633);
        cqCircle(c, st, 633, 632, {"radius": 2, "forConstruction": false});
        atStep(st, 634);
        cqExtrude(c, st, 634, 633, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 634, "extrude");
        atStep(st, 635);
        cqCut(c, st, 635, 631, {"toCut": {"wp": 634}, "clean": true, "tol": undefined});
        stepVol(c, st, 635, "cut");
        atStep(st, 636);
        cqWorkplaneNew(c, st, 636, {"plane": {"origin": [4.256253075523721, 4.431228841646037, 10.307692307692308], "xDir": [-0.7212024473438143, 0.6927243535095997, 2.775557561562892e-17], "zDir": [0.3546877562936434, 0.36926907013716975, 0.858974358974359]}, "obj": undefined});
        atStep(st, 637);
        cqCircle(c, st, 637, 636, {"radius": 2, "forConstruction": false});
        atStep(st, 638);
        cqExtrude(c, st, 638, 637, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 638, "extrude");
        atStep(st, 639);
        cqCut(c, st, 639, 635, {"toCut": {"wp": 638}, "clean": true, "tol": undefined});
        stepVol(c, st, 639, "cut");
        atStep(st, 640);
        cqWorkplaneNew(c, st, 640, {"plane": {"origin": [1.8346859069995645, 6.334070543311052, 10.025641025641026], "xDir": [-0.960518111631372, 0.2782174639164534, -1.3877787807814457e-17], "zDir": [0.1528904922499637, 0.5278392119425876, 0.8354700854700855]}, "obj": undefined});
        atStep(st, 641);
        cqCircle(c, st, 641, 640, {"radius": 2, "forConstruction": false});
        atStep(st, 642);
        cqExtrude(c, st, 642, 641, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 642, "extrude");
        atStep(st, 643);
        cqCut(c, st, 643, 639, {"toCut": {"wp": 642}, "clean": true, "tol": undefined});
        stepVol(c, st, 643, "cut");
        atStep(st, 644);
        cqWorkplaneNew(c, st, 644, {"plane": {"origin": [-1.4010719569910073, 6.862904361853379, 9.743589743589745], "xDir": [-0.979790652042268, -0.2000256937760433, -1.3877787807814457e-17], "zDir": [-0.11675599641591727, 0.5719086968211149, 0.8119658119658121]}, "obj": undefined});
        atStep(st, 645);
        cqCircle(c, st, 645, 644, {"radius": 2, "forConstruction": false});
        atStep(st, 646);
        cqExtrude(c, st, 646, 645, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 646, "extrude");
        atStep(st, 647);
        cqCut(c, st, 647, 643, {"toCut": {"wp": 646}, "clean": true, "tol": undefined});
        stepVol(c, st, 647, "cut");
        atStep(st, 648);
        cqWorkplaneNew(c, st, 648, {"plane": {"origin": [-4.6680848414520035, 5.7173659891452155, 9.46153846153846], "xDir": [-0.7746049618276546, -0.6324453755953774, -2.7755575615628914e-17], "zDir": [-0.38900707012100033, 0.47644716576210133, 0.7884615384615384]}, "obj": undefined});
        atStep(st, 649);
        cqCircle(c, st, 649, 648, {"radius": 2, "forConstruction": false});
        atStep(st, 650);
        cqExtrude(c, st, 650, 649, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 650, "extrude");
        atStep(st, 651);
        cqCut(c, st, 651, 647, {"toCut": {"wp": 650}, "clean": true, "tol": undefined});
        stepVol(c, st, 651, "cut");
        atStep(st, 652);
        cqWorkplaneNew(c, st, 652, {"plane": {"origin": [-7.11049577393711, 3.029499128643713, 9.17948717948718], "xDir": [-0.3919666098600769, -0.9199794436588232, 0], "zDir": [-0.5925413144947592, 0.2524582607203094, 0.7649572649572649]}, "obj": undefined});
        atStep(st, 653);
        cqCircle(c, st, 653, 652, {"radius": 2, "forConstruction": false});
        atStep(st, 654);
        cqExtrude(c, st, 654, 653, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 654, "extrude");
        atStep(st, 655);
        cqCut(c, st, 655, 651, {"toCut": {"wp": 654}, "clean": true, "tol": undefined});
        stepVol(c, st, 655, "cut");
        atStep(st, 656);
        cqWorkplaneNew(c, st, 656, {"plane": {"origin": [-8.025947435169046, -0.6479214604583531, 8.897435897435898], "xDir": [0.08046656871672515, -0.99675730813421, 0], "zDir": [-0.6688289529307538, -0.05399345503819609, 0.7414529914529915]}, "obj": undefined});
        atStep(st, 657);
        cqCircle(c, st, 657, 656, {"radius": 2, "forConstruction": false});
        atStep(st, 658);
        cqExtrude(c, st, 658, 657, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 658, "extrude");
        atStep(st, 659);
        cqCut(c, st, 659, 655, {"toCut": {"wp": 658}, "clean": true, "tol": undefined});
        stepVol(c, st, 659, "cut");
        atStep(st, 660);
        cqWorkplaneNew(c, st, 660, {"plane": {"origin": [-7.060001232894775, -4.464474271458884, 8.615384615384615], "xDir": [0.5344658261278016, -0.8451900855437945, 0], "zDir": [-0.5883334360745647, -0.3720395226215737, 0.7179487179487181]}, "obj": undefined});
        atStep(st, 661);
        cqCircle(c, st, 661, 660, {"radius": 2, "forConstruction": false});
        atStep(st, 662);
        cqExtrude(c, st, 662, 661, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 662, "extrude");
        atStep(st, 663);
        cqCut(c, st, 663, 659, {"toCut": {"wp": 662}, "clean": true, "tol": undefined});
        stepVol(c, st, 663, "cut");
        atStep(st, 664);
        cqWorkplaneNew(c, st, 664, {"plane": {"origin": [-4.317277948996217, -7.4777447580581775, 8.333333333333334], "xDir": [0.8660254037844379, -0.5000000000000013, -2.7755575615628914e-17], "zDir": [-0.35977316241635143, -0.6231453965048481, 0.6944444444444445]}, "obj": undefined});
        atStep(st, 665);
        cqCircle(c, st, 665, 664, {"radius": 2, "forConstruction": false});
        atStep(st, 666);
        cqExtrude(c, st, 666, 665, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 666, "extrude");
        atStep(st, 667);
        cqCut(c, st, 667, 663, {"toCut": {"wp": 666}, "clean": true, "tol": undefined});
        stepVol(c, st, 667, "cut");
        atStep(st, 668);
        cqWorkplaneNew(c, st, 668, {"plane": {"origin": [-0.3582918860545769, -8.8909214514071, 8.051282051282051], "xDir": [0.9991889981715696, -0.04026594010941539, -3.469446951953613e-18], "zDir": [-0.029857657171214743, -0.7409101209505918, 0.670940170940171]}, "obj": undefined});
        atStep(st, 669);
        cqCircle(c, st, 669, 668, {"radius": 2, "forConstruction": false});
        atStep(st, 670);
        cqExtrude(c, st, 670, 669, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 670, "extrude");
        atStep(st, 671);
        cqCut(c, st, 671, 667, {"toCut": {"wp": 670}, "clean": true, "tol": undefined});
        stepVol(c, st, 671, "cut");
        atStep(st, 672);
        cqWorkplaneNew(c, st, 672, {"plane": {"origin": [3.920581640808737, -8.262450777595673, 7.76923076923077], "xDir": [0.9034504346103824, 0.4286925614030536, 0], "zDir": [0.32671513673406144, -0.6885375647996393, 0.6474358974358975]}, "obj": undefined});
        atStep(st, 673);
        cqCircle(c, st, 673, 672, {"radius": 2, "forConstruction": false});
        atStep(st, 674);
        cqExtrude(c, st, 674, 673, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 674, "extrude");
        atStep(st, 675);
        cqCut(c, st, 675, 671, {"toCut": {"wp": 674}, "clean": true, "tol": undefined});
        stepVol(c, st, 675, "cut");
        atStep(st, 676);
        cqWorkplaneNew(c, st, 676, {"plane": {"origin": [7.496972169299633, -5.63360911135175, 7.487179487179487], "xDir": [0.6007422642379798, 0.7994427634035004, 2.7755575615628914e-17], "zDir": [0.6247476807749694, -0.4694674259459792, 0.6239316239316239]}, "obj": undefined});
        atStep(st, 677);
        cqCircle(c, st, 677, 676, {"radius": 2, "forConstruction": false});
        atStep(st, 678);
        cqExtrude(c, st, 678, 677, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 678, "extrude");
        atStep(st, 679);
        cqCut(c, st, 679, 675, {"toCut": {"wp": 678}, "clean": true, "tol": undefined});
        stepVol(c, st, 679, "cut");
        atStep(st, 680);
        cqWorkplaneNew(c, st, 680, {"plane": {"origin": [9.471884060851362, -1.539330986323581, 7.205128205128205], "xDir": [0.16041128085776, 0.987050262637913, 1.3877787807814457e-17], "zDir": [0.7893236717376135, -0.12827758219363175, 0.6004273504273504]}, "obj": undefined});
        atStep(st, 681);
        cqCircle(c, st, 681, 680, {"radius": 2, "forConstruction": false});
        atStep(st, 682);
        cqExtrude(c, st, 682, 681, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 682, "extrude");
        atStep(st, 683);
        cqCut(c, st, 683, 679, {"toCut": {"wp": 682}, "clean": true, "tol": undefined});
        stepVol(c, st, 683, "cut");
        atStep(st, 684);
        cqWorkplaneNew(c, st, 684, {"plane": {"origin": [9.297157532561451, 3.1038472468688942, 6.9230769230769225], "xDir": [-0.3166679938014707, 0.948536441947146, 0], "zDir": [0.7747631277134545, 0.25865393723907454, 0.5769230769230769]}, "obj": undefined});
        atStep(st, 685);
        cqCircle(c, st, 685, 684, {"radius": 2, "forConstruction": false});
        atStep(st, 686);
        cqExtrude(c, st, 686, 685, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 686, "extrude");
        atStep(st, 687);
        cqCut(c, st, 687, 683, {"toCut": {"wp": 686}, "clean": true, "tol": undefined});
        stepVol(c, st, 687, "cut");
        atStep(st, 688);
        cqWorkplaneNew(c, st, 688, {"plane": {"origin": [6.923667407438172, 7.208301330162216, 6.6410256410256405], "xDir": [-0.7212024473438141, 0.6927243535095999, 2.7755575615628914e-17], "zDir": [0.576972283953181, 0.600691777513518, 0.5534188034188033]}, "obj": undefined});
        atStep(st, 689);
        cqCircle(c, st, 689, 688, {"radius": 2, "forConstruction": false});
        atStep(st, 690);
        cqExtrude(c, st, 690, 689, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 690, "extrude");
        atStep(st, 691);
        cqCut(c, st, 691, 687, {"toCut": {"wp": 690}, "clean": true, "tol": undefined});
        stepVol(c, st, 691, "cut");
        atStep(st, 692);
        cqWorkplaneNew(c, st, 692, {"plane": {"origin": [2.8313113637514675, 9.774820768863153, 6.3589743589743595], "xDir": [-0.9605181116313716, 0.2782174639164554, 0], "zDir": [0.2359426136459556, 0.8145683974052628, 0.5299145299145299]}, "obj": undefined});
        atStep(st, 693);
        cqCircle(c, st, 693, 692, {"radius": 2, "forConstruction": false});
        atStep(st, 694);
        cqExtrude(c, st, 694, 693, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 694, "extrude");
        atStep(st, 695);
        cqCut(c, st, 695, 691, {"toCut": {"wp": 694}, "clean": true, "tol": undefined});
        stepVol(c, st, 695, "cut");
        atStep(st, 696);
        cqWorkplaneNew(c, st, 696, {"plane": {"origin": [-2.0697682794820684, 10.138395592321777, 6.0769230769230775], "xDir": [-0.979790652042268, -0.20002569377604296, 1.387778780781446e-17], "zDir": [-0.17248068995683905, 0.8448662993601479, 0.5064102564102565]}, "obj": undefined});
        atStep(st, 697);
        cqCircle(c, st, 697, 696, {"radius": 2, "forConstruction": false});
        atStep(st, 698);
        cqExtrude(c, st, 698, 697, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 698, "extrude");
        atStep(st, 699);
        cqCut(c, st, 699, 695, {"toCut": {"wp": 698}, "clean": true, "tol": undefined});
        stepVol(c, st, 699, "cut");
        atStep(st, 700);
        cqWorkplaneNew(c, st, 700, {"plane": {"origin": [-6.645778043686857, 8.139600424778173, 5.794871794871795], "xDir": [-0.7746049618276545, -0.6324453755953773, 0], "zDir": [-0.5538148369739049, 0.6783000353981812, 0.48290598290598297]}, "obj": undefined});
        atStep(st, 701);
        cqCircle(c, st, 701, 700, {"radius": 2, "forConstruction": false});
        atStep(st, 702);
        cqExtrude(c, st, 702, 701, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 702, "extrude");
        atStep(st, 703);
        cqCut(c, st, 703, 699, {"toCut": {"wp": 702}, "clean": true, "tol": undefined});
        stepVol(c, st, 703, "cut");
        atStep(st, 704);
        cqWorkplaneNew(c, st, 704, {"plane": {"origin": [-9.805824785532208, 4.1778714998031745, 5.512820512820513], "xDir": [-0.3919666098600772, -0.9199794436588232, 0], "zDir": [-0.8171520654610173, 0.3481559583169312, 0.4594017094017094]}, "obj": undefined});
        atStep(st, 705);
        cqCircle(c, st, 705, 704, {"radius": 2, "forConstruction": false});
        atStep(st, 706);
        cqExtrude(c, st, 706, 705, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 706, "extrude");
        atStep(st, 707);
        cqCut(c, st, 707, 703, {"toCut": {"wp": 706}, "clean": true, "tol": undefined});
        stepVol(c, st, 707, "cut");
        atStep(st, 708);
        cqWorkplaneNew(c, st, 708, {"plane": {"origin": [-10.764935239077577, -0.869035415218341, 5.23076923076923], "xDir": [0.08046656871672489, -0.99675730813421, 0], "zDir": [-0.897077936589798, -0.07241961793486175, 0.43589743589743585]}, "obj": undefined});
        atStep(st, 709);
        cqCircle(c, st, 709, 708, {"radius": 2, "forConstruction": false});
        atStep(st, 710);
        cqExtrude(c, st, 710, 709, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 710, "extrude");
        atStep(st, 711);
        cqCut(c, st, 711, 707, {"toCut": {"wp": 710}, "clean": true, "tol": undefined});
        stepVol(c, st, 711, "cut");
        atStep(st, 712);
        cqWorkplaneNew(c, st, 712, {"plane": {"origin": [-9.239678577027725, -5.842818708230796, 4.948717948717948], "xDir": [0.5344658261278014, -0.8451900855437945, 2.7755575615628907e-17], "zDir": [-0.7699732147523103, -0.486901559019233, 0.4123931623931623]}, "obj": undefined});
        atStep(st, 713);
        cqCircle(c, st, 713, 712, {"radius": 2, "forConstruction": false});
        atStep(st, 714);
        cqExtrude(c, st, 714, 713, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 714, "extrude");
        atStep(st, 715);
        cqCut(c, st, 715, 711, {"toCut": {"wp": 714}, "clean": true, "tol": undefined});
        stepVol(c, st, 715, "cut");
        atStep(st, 716);
        cqWorkplaneNew(c, st, 716, {"plane": {"origin": [-5.5277079839256835, -9.57427107756337, 4.666666666666667], "xDir": [0.8660254037844378, -0.5000000000000017, -2.7755575615628914e-17], "zDir": [-0.460642331993807, -0.797855923130281, 0.38888888888888895]}, "obj": undefined});
        atStep(st, 717);
        cqCircle(c, st, 717, 716, {"radius": 2, "forConstruction": false});
        atStep(st, 718);
        cqExtrude(c, st, 718, 717, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 718, "extrude");
        atStep(st, 719);
        cqCut(c, st, 719, 715, {"toCut": {"wp": 718}, "clean": true, "tol": undefined});
        stepVol(c, st, 719, "cut");
        atStep(st, 720);
        cqWorkplaneNew(c, st, 720, {"plane": {"origin": [-0.4497818122390592, -11.161220553791283, 4.384615384615385], "xDir": [0.9991889981715696, -0.04026594010941562, 0], "zDir": [-0.03748181768658827, -0.9301017128159405, 0.3653846153846155]}, "obj": undefined});
        atStep(st, 721);
        cqCircle(c, st, 721, 720, {"radius": 2, "forConstruction": false});
        atStep(st, 722);
        cqExtrude(c, st, 722, 721, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 722, "extrude");
        atStep(st, 723);
        cqCut(c, st, 723, 719, {"toCut": {"wp": 722}, "clean": true, "tol": undefined});
        stepVol(c, st, 723, "cut");
        atStep(st, 724);
        cqWorkplaneNew(c, st, 724, {"plane": {"origin": [4.834332538722418, -10.188140001462584, 4.102564102564102], "xDir": [0.9034504346103835, 0.4286925614030518, 1.3877787807814457e-17], "zDir": [0.40286104489353486, -0.8490116667885489, 0.3418803418803419]}, "obj": undefined});
        atStep(st, 725);
        cqCircle(c, st, 725, 724, {"radius": 2, "forConstruction": false});
        atStep(st, 726);
        cqExtrude(c, st, 726, 725, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 726, "extrude");
        atStep(st, 727);
        cqCut(c, st, 727, 723, {"toCut": {"wp": 726}, "clean": true, "tol": undefined});
        stepVol(c, st, 727, "cut");
        atStep(st, 728);
        cqWorkplaneNew(c, st, 728, {"plane": {"origin": [9.094120242136908, -6.833788027869528, 3.82051282051282], "xDir": [0.6007422642379799, 0.7994427634035005, 2.7755575615628914e-17], "zDir": [0.7578433535114091, -0.5694823356557941, 0.3183760683760684]}, "obj": undefined});
        atStep(st, 729);
        cqCircle(c, st, 729, 728, {"radius": 2, "forConstruction": false});
        atStep(st, 730);
        cqExtrude(c, st, 730, 729, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 730, "extrude");
        atStep(st, 731);
        cqCut(c, st, 731, 727, {"toCut": {"wp": 730}, "clean": true, "tol": undefined});
        stepVol(c, st, 731, "cut");
        atStep(st, 732);
        cqWorkplaneNew(c, st, 732, {"plane": {"origin": [11.317954492995607, -1.839346632722868, 3.538461538461538], "xDir": [0.16041128085776027, 0.987050262637913, 0], "zDir": [0.9431628744163006, -0.153278886060239, 0.2948717948717948]}, "obj": undefined});
        atStep(st, 733);
        cqCircle(c, st, 733, 732, {"radius": 2, "forConstruction": false});
        atStep(st, 734);
        cqExtrude(c, st, 734, 733, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 734, "extrude");
        atStep(st, 735);
        cqCut(c, st, 735, 731, {"toCut": {"wp": 734}, "clean": true, "tol": undefined});
        stepVol(c, st, 735, "cut");
        atStep(st, 736);
        cqWorkplaneNew(c, st, 736, {"plane": {"origin": [10.955320465834104, 3.6574233734724393, 3.256410256410257], "xDir": [-0.3166679938014704, 0.9485364419471464, -1.3877787807814457e-17], "zDir": [0.912943372152842, 0.30478528112270326, 0.2713675213675214]}, "obj": undefined});
        atStep(st, 737);
        cqCircle(c, st, 737, 736, {"radius": 2, "forConstruction": false});
        atStep(st, 738);
        cqExtrude(c, st, 738, 737, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 738, "extrude");
        atStep(st, 739);
        cqCut(c, st, 739, 735, {"toCut": {"wp": 738}, "clean": true, "tol": undefined});
        stepVol(c, st, 739, "cut");
        atStep(st, 740);
        cqWorkplaneNew(c, st, 740, {"plane": {"origin": [8.053294930964467, 8.384368160246318, 2.9743589743589745], "xDir": [-0.721202447343814, 0.6927243535096, -1.3877787807814457e-17], "zDir": [0.6711079109137057, 0.6986973466871933, 0.2478632478632479]}, "obj": undefined});
        atStep(st, 741);
        cqCircle(c, st, 741, 740, {"radius": 2, "forConstruction": false});
        atStep(st, 742);
        cqExtrude(c, st, 742, 741, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 742, "extrude");
        atStep(st, 743);
        cqCut(c, st, 743, 739, {"toCut": {"wp": 742}, "clean": true, "tol": undefined});
        stepVol(c, st, 743, "cut");
        atStep(st, 744);
        cqWorkplaneNew(c, st, 744, {"plane": {"origin": [3.2534969502548483, 11.232374498948262, 2.6923076923076925], "xDir": [-0.9605181116313726, 0.27821746391645225, 0], "zDir": [0.2711247458545707, 0.9360312082456884, 0.22435897435897437]}, "obj": undefined});
        atStep(st, 745);
        cqCircle(c, st, 745, 744, {"radius": 2, "forConstruction": false});
        atStep(st, 746);
        cqExtrude(c, st, 746, 745, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 746, "extrude");
        atStep(st, 747);
        cqCut(c, st, 747, 743, {"toCut": {"wp": 746}, "clean": true, "tol": undefined});
        stepVol(c, st, 747, "cut");
        atStep(st, 748);
        cqWorkplaneNew(c, st, 748, {"plane": {"origin": [-2.351392546703176, 11.51788249324183, 2.41025641025641], "xDir": [-0.979790652042268, -0.20002569377604276, 0], "zDir": [-0.19594937889193137, 0.959823541103486, 0.2008547008547009]}, "obj": undefined});
        atStep(st, 749);
        cqCircle(c, st, 749, 748, {"radius": 2, "forConstruction": false});
        atStep(st, 750);
        cqExtrude(c, st, 750, 749, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 750, "extrude");
        atStep(st, 751);
        cqCut(c, st, 751, 747, {"toCut": {"wp": 750}, "clean": true, "tol": undefined});
        stepVol(c, st, 751, "cut");
        atStep(st, 752);
        cqWorkplaneNew(c, st, 752, {"plane": {"origin": [-7.469036444408052, 9.147908915396224, 2.1282051282051277], "xDir": [-0.7746049618276548, -0.632445375595377, 0], "zDir": [-0.622419703700671, 0.7623257429496854, 0.1773504273504273]}, "obj": undefined});
        atStep(st, 753);
        cqCircle(c, st, 753, 752, {"radius": 2, "forConstruction": false});
        atStep(st, 754);
        cqExtrude(c, st, 754, 753, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 754, "extrude");
        atStep(st, 755);
        cqCut(c, st, 755, 751, {"toCut": {"wp": 754}, "clean": true, "tol": undefined});
        stepVol(c, st, 755, "cut");
        atStep(st, 756);
        cqWorkplaneNew(c, st, 756, {"plane": {"origin": [-10.90832300435865, 4.647602092360238, 1.8461538461538456], "xDir": [-0.3919666098600774, -0.9199794436588232, -6.938893903907228e-18], "zDir": [-0.9090269170298876, 0.3873001743633531, 0.1538461538461538]}, "obj": undefined});
        atStep(st, 757);
        cqCircle(c, st, 757, 756, {"radius": 2, "forConstruction": false});
        atStep(st, 758);
        cqExtrude(c, st, 758, 757, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 758, "extrude");
        atStep(st, 759);
        cqCut(c, st, 759, 755, {"toCut": {"wp": 758}, "clean": true, "tol": undefined});
        stepVol(c, st, 759, "cut");
        atStep(st, 760);
        cqWorkplaneNew(c, st, 760, {"plane": {"origin": [-11.859048962990984, -0.9573614063404778, 1.5641025641025643], "xDir": [0.08046656871672468, -0.99675730813421, 0], "zDir": [-0.9882540802492488, -0.07978011719503983, 0.1303418803418804]}, "obj": undefined});
        atStep(st, 761);
        cqCircle(c, st, 761, 760, {"radius": 2, "forConstruction": false});
        atStep(st, 762);
        cqExtrude(c, st, 762, 761, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 762, "extrude");
        atStep(st, 763);
        cqCut(c, st, 763, 759, {"toCut": {"wp": 762}, "clean": true, "tol": undefined});
        stepVol(c, st, 763, "cut");
        atStep(st, 764);
        cqWorkplaneNew(c, st, 764, {"plane": {"origin": [-10.084231514355489, -6.376881626239448, 1.2820512820512824], "xDir": [0.5344658261277982, -0.8451900855437966, -6.938893903907228e-18], "zDir": [-0.8403526261962908, -0.5314068021866207, 0.10683760683760687]}, "obj": undefined});
        atStep(st, 765);
        cqCircle(c, st, 765, 764, {"radius": 2, "forConstruction": false});
        atStep(st, 766);
        cqExtrude(c, st, 766, 765, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 766, "extrude");
        atStep(st, 767);
        cqCut(c, st, 767, 763, {"toCut": {"wp": 766}, "clean": true, "tol": undefined});
        stepVol(c, st, 767, "cut");
        atStep(st, 768);
        cqWorkplaneNew(c, st, 768, {"plane": {"origin": [2.1860344202717363, -1.3823644081468616, 11.717948717948715], "xDir": [0.5344658261278015, 0.8451900855437946, 1.3877787807814457e-17], "zDir": [0.18216953502264469, -0.11519703401223846, 0.9764957264957264]}, "obj": undefined});
        atStep(st, 769);
        cqCircle(c, st, 769, 768, {"radius": 2, "forConstruction": false});
        atStep(st, 770);
        cqExtrude(c, st, 770, 769, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 770, "extrude");
        atStep(st, 771);
        cqCut(c, st, 771, 767, {"toCut": {"wp": 770}, "clean": true, "tol": undefined});
        stepVol(c, st, 771, "cut");
        atStep(st, 772);
        cqWorkplaneNew(c, st, 772, {"plane": {"origin": [3.624175831001872, -0.29257371996869524, 11.435897435897436], "xDir": [0.08046656871672576, 0.99675730813421, 3.469446951953615e-18], "zDir": [0.30201465258348936, -0.024381143330724605, 0.952991452991453]}, "obj": undefined});
        atStep(st, 773);
        cqCircle(c, st, 773, 772, {"radius": 2, "forConstruction": false});
        atStep(st, 774);
        cqExtrude(c, st, 774, 773, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 774, "extrude");
        atStep(st, 775);
        cqCut(c, st, 775, 771, {"toCut": {"wp": 774}, "clean": true, "tol": undefined});
        stepVol(c, st, 775, "cut");
        atStep(st, 776);
        cqWorkplaneNew(c, st, 776, {"plane": {"origin": [4.072061809463306, 1.7349433985700389, 11.153846153846157], "xDir": [-0.39196660986007487, 0.9199794436588244, 0], "zDir": [0.3393384841219422, 0.14457861654750323, 0.9294871794871796]}, "obj": undefined});
        atStep(st, 777);
        cqCircle(c, st, 777, 776, {"radius": 2, "forConstruction": false});
        atStep(st, 778);
        cqExtrude(c, st, 778, 777, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 778, "extrude");
        atStep(st, 779);
        cqCut(c, st, 779, 775, {"toCut": {"wp": 778}, "clean": true, "tol": undefined});
        stepVol(c, st, 779, "cut");
        atStep(st, 780);
        cqWorkplaneNew(c, st, 780, {"plane": {"origin": [3.2126778556474633, 3.934816039085834, 10.871794871794872], "xDir": [-0.7746049618276547, 0.6324453755953773, 0], "zDir": [0.2677231546372887, 0.32790133659048626, 0.905982905982906]}, "obj": undefined});
        atStep(st, 781);
        cqCircle(c, st, 781, 780, {"radius": 2, "forConstruction": false});
        atStep(st, 782);
        cqExtrude(c, st, 782, 781, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 782, "extrude");
        atStep(st, 783);
        cqCut(c, st, 783, 779, {"toCut": {"wp": 782}, "clean": true, "tol": undefined});
        stepVol(c, st, 783, "cut");
        atStep(st, 784);
        cqWorkplaneNew(c, st, 784, {"plane": {"origin": [1.1289909934179418, 5.530163654021979, 10.58974358974359], "xDir": [-0.9797906520422676, 0.20002569377604473, 0], "zDir": [0.09408258278482848, 0.4608469711684982, 0.8824786324786325]}, "obj": undefined});
        atStep(st, 785);
        cqCircle(c, st, 785, 784, {"radius": 2, "forConstruction": false});
        atStep(st, 786);
        cqExtrude(c, st, 786, 785, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 786, "extrude");
        atStep(st, 787);
        cqCut(c, st, 787, 783, {"toCut": {"wp": 786}, "clean": true, "tol": undefined});
        stepVol(c, st, 787, "cut");
        atStep(st, 788);
        cqWorkplaneNew(c, st, 788, {"plane": {"origin": [-1.709430209085903, 5.901637709162206, 10.307692307692308], "xDir": [-0.9605181116313722, -0.27821746391645286, 2.775557561562892e-17], "zDir": [-0.14245251742382525, 0.49180314243018386, 0.858974358974359]}, "obj": undefined});
        atStep(st, 789);
        cqCircle(c, st, 789, 788, {"radius": 2, "forConstruction": false});
        atStep(st, 790);
        cqExtrude(c, st, 790, 789, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 790, "extrude");
        atStep(st, 791);
        cqCut(c, st, 791, 787, {"toCut": {"wp": 790}, "clean": true, "tol": undefined});
        stepVol(c, st, 791, "cut");
        atStep(st, 792);
        cqWorkplaneNew(c, st, 792, {"plane": {"origin": [-4.568123046370292, 4.75591987508244, 10.025641025641026], "xDir": [-0.7212024473438147, -0.6927243535095993, -2.7755575615628907e-17], "zDir": [-0.3806769205308577, 0.39632665625687, 0.8354700854700855]}, "obj": undefined});
        atStep(st, 793);
        cqCircle(c, st, 793, 792, {"radius": 2, "forConstruction": false});
        atStep(st, 794);
        cqExtrude(c, st, 794, 793, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 794, "extrude");
        atStep(st, 795);
        cqCut(c, st, 795, 791, {"toCut": {"wp": 794}, "clean": true, "tol": undefined});
        stepVol(c, st, 795, "cut");
        atStep(st, 796);
        cqWorkplaneNew(c, st, 796, {"plane": {"origin": [-6.643985499603562, 2.218088273642495, 9.743589743589745], "xDir": [-0.3166679938014732, -0.9485364419471454, -2.775557561562892e-17], "zDir": [-0.5536654583002968, 0.18484068947020793, 0.8119658119658121]}, "obj": undefined});
        atStep(st, 797);
        cqCircle(c, st, 797, 796, {"radius": 2, "forConstruction": false});
        atStep(st, 798);
        cqExtrude(c, st, 798, 797, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 798, "extrude");
        atStep(st, 799);
        cqCut(c, st, 799, 795, {"toCut": {"wp": 798}, "clean": true, "tol": undefined});
        stepVol(c, st, 799, "cut");
        atStep(st, 800);
        cqWorkplaneNew(c, st, 800, {"plane": {"origin": [-7.285426610058906, -1.183997065145872, 9.46153846153846], "xDir": [0.16041128085775913, -0.9870502626379132, 0], "zDir": [-0.6071188841715754, -0.09866642209548931, 0.7884615384615383]}, "obj": undefined});
        atStep(st, 801);
        cqCircle(c, st, 801, 800, {"radius": 2, "forConstruction": false});
        atStep(st, 802);
        cqExtrude(c, st, 802, 801, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 802, "extrude");
        atStep(st, 803);
        cqCut(c, st, 803, 799, {"toCut": {"wp": 802}, "clean": true, "tol": undefined});
        stepVol(c, st, 803, "cut");
        atStep(st, 804);
        cqWorkplaneNew(c, st, 804, {"plane": {"origin": [-6.178871093116821, -4.643120409409589, 9.17948717948718], "xDir": [0.6007422642379792, -0.799442763403501, 0], "zDir": [-0.5149059244264017, -0.3869267007841324, 0.7649572649572649]}, "obj": undefined});
        atStep(st, 805);
        cqCircle(c, st, 805, 804, {"radius": 2, "forConstruction": false});
        atStep(st, 806);
        cqExtrude(c, st, 806, 805, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 806, "extrude");
        atStep(st, 807);
        cqCut(c, st, 807, 803, {"toCut": {"wp": 806}, "clean": true, "tol": undefined});
        stepVol(c, st, 807, "cut");
        atStep(st, 808);
        cqWorkplaneNew(c, st, 808, {"plane": {"origin": [-3.45185727317047, -7.274635098524131, 8.897435897435898], "xDir": [0.9034504346103822, -0.42869256140305434, 0], "zDir": [-0.28765477276420587, -0.6062195915436775, 0.7414529914529915]}, "obj": undefined});
        atStep(st, 809);
        cqCircle(c, st, 809, 808, {"radius": 2, "forConstruction": false});
        atStep(st, 810);
        cqExtrude(c, st, 810, 809, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 810, "extrude");
        atStep(st, 811);
        cqCut(c, st, 811, 807, {"toCut": {"wp": 810}, "clean": true, "tol": undefined});
        stepVol(c, st, 811, "cut");
        atStep(st, 812);
        cqWorkplaneNew(c, st, 812, {"plane": {"origin": [0.33634751717802025, -8.346377554165775, 8.615384615384615], "xDir": [0.9991889981715698, 0.04026594010941453, -3.469446951953614e-18], "zDir": [0.02802895976483502, -0.695531462847148, 0.717948717948718]}, "obj": undefined});
        atStep(st, 813);
        cqCircle(c, st, 813, 812, {"radius": 2, "forConstruction": false});
        atStep(st, 814);
        cqExtrude(c, st, 814, 813, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 814, "extrude");
        atStep(st, 815);
        cqCut(c, st, 815, 811, {"toCut": {"wp": 814}, "clean": true, "tol": undefined});
        stepVol(c, st, 815, "cut");
        atStep(st, 816);
        cqWorkplaneNew(c, st, 816, {"plane": {"origin": [4.317277948996211, -7.477744758058181, 8.333333333333334], "xDir": [0.8660254037844384, 0.5000000000000007, 2.7755575615628914e-17], "zDir": [0.35977316241635093, -0.6231453965048485, 0.6944444444444445]}, "obj": undefined});
        atStep(st, 817);
        cqCircle(c, st, 817, 816, {"radius": 2, "forConstruction": false});
        atStep(st, 818);
        cqExtrude(c, st, 818, 817, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 818, "extrude");
        atStep(st, 819);
        cqCut(c, st, 819, 815, {"toCut": {"wp": 818}, "clean": true, "tol": undefined});
        stepVol(c, st, 819, "cut");
        atStep(st, 820);
        cqWorkplaneNew(c, st, 820, {"plane": {"origin": [7.520617896943268, -4.755750600996663, 8.051282051282051], "xDir": [0.5344658261278024, 0.8451900855437939, 0], "zDir": [0.6267181580786056, -0.39631255008305527, 0.6709401709401709]}, "obj": undefined});
        atStep(st, 821);
        cqCircle(c, st, 821, 820, {"radius": 2, "forConstruction": false});
        atStep(st, 822);
        cqExtrude(c, st, 822, 821, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 822, "extrude");
        atStep(st, 823);
        cqCut(c, st, 823, 819, {"toCut": {"wp": 822}, "clean": true, "tol": undefined});
        stepVol(c, st, 823, "cut");
        atStep(st, 824);
        cqWorkplaneNew(c, st, 824, {"plane": {"origin": [9.11578309132071, -0.7359020902465874, 7.76923076923077], "xDir": [0.08046656871672603, 0.99675730813421, -6.938893903907228e-18], "zDir": [0.7596485909433925, -0.06132517418721562, 0.6474358974358976]}, "obj": undefined});
        atStep(st, 825);
        cqCircle(c, st, 825, 824, {"radius": 2, "forConstruction": false});
        atStep(st, 826);
        cqExtrude(c, st, 826, 825, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 826, "extrude");
        atStep(st, 827);
        cqCut(c, st, 827, 823, {"toCut": {"wp": 826}, "clean": true, "tol": undefined});
        stepVol(c, st, 827, "cut");
        atStep(st, 828);
        cqWorkplaneNew(c, st, 828, {"plane": {"origin": [8.627334690071907, 3.675763794402545, 7.487179487179487], "xDir": [-0.3919666098600746, 0.9199794436588244, -2.7755575615628907e-17], "zDir": [0.7189445575059923, 0.30631364953354545, 0.623931623931624]}, "obj": undefined});
        atStep(st, 829);
        cqCircle(c, st, 829, 828, {"radius": 2, "forConstruction": false});
        atStep(st, 830);
        cqExtrude(c, st, 830, 829, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 830, "extrude");
        atStep(st, 831);
        cqCut(c, st, 831, 827, {"toCut": {"wp": 830}, "clean": true, "tol": undefined});
        stepVol(c, st, 831, "cut");
        atStep(st, 832);
        cqWorkplaneNew(c, st, 832, {"plane": {"origin": [6.069041769414467, 7.43322672523639, 7.205128205128205], "xDir": [-0.774604961827654, 0.6324453755953781, 0], "zDir": [0.505753480784539, 0.6194355604363659, 0.6004273504273505]}, "obj": undefined});
        atStep(st, 833);
        cqCircle(c, st, 833, 832, {"radius": 2, "forConstruction": false});
        atStep(st, 834);
        cqExtrude(c, st, 834, 833, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 834, "extrude");
        atStep(st, 835);
        cqCut(c, st, 835, 831, {"toCut": {"wp": 834}, "clean": true, "tol": undefined});
        stepVol(c, st, 835, "cut");
        atStep(st, 836);
        cqWorkplaneNew(c, st, 836, {"plane": {"origin": [1.9605682010258516, 9.603498229618518, 6.9230769230769225], "xDir": [-0.979790652042268, 0.20002569377604407, 0], "zDir": [0.163380683418821, 0.8002915191348766, 0.576923076923077]}, "obj": undefined});
        atStep(st, 837);
        cqCircle(c, st, 837, 836, {"radius": 2, "forConstruction": false});
        atStep(st, 838);
        cqExtrude(c, st, 838, 837, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 838, "extrude");
        atStep(st, 839);
        cqCut(c, st, 839, 835, {"toCut": {"wp": 838}, "clean": true, "tol": undefined});
        stepVol(c, st, 839, "cut");
        atStep(st, 840);
        cqWorkplaneNew(c, st, 840, {"plane": {"origin": [-2.780738366334542, 9.600222527276912, 6.6410256410256405], "xDir": [-0.9605181116313728, -0.2782174639164509, 2.7755575615628907e-17], "zDir": [-0.2317281971945452, 0.8000185439397427, 0.5534188034188033]}, "obj": undefined});
        atStep(st, 841);
        cqCircle(c, st, 841, 840, {"radius": 2, "forConstruction": false});
        atStep(st, 842);
        cqExtrude(c, st, 842, 841, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 842, "extrude");
        atStep(st, 843);
        cqCut(c, st, 843, 839, {"toCut": {"wp": 842}, "clean": true, "tol": undefined});
        stepVol(c, st, 843, "cut");
        atStep(st, 844);
        cqWorkplaneNew(c, st, 844, {"plane": {"origin": [-7.049587421399513, 7.339397951463895, 6.3589743589743595], "xDir": [-0.721202447343815, -0.692724353509599, -2.7755575615628907e-17], "zDir": [-0.5874656184499595, 0.6116164959553246, 0.5299145299145299]}, "obj": undefined});
        atStep(st, 845);
        cqCircle(c, st, 845, 844, {"radius": 2, "forConstruction": false});
        atStep(st, 846);
        cqExtrude(c, st, 846, 845, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 846, "extrude");
        atStep(st, 847);
        cqCut(c, st, 847, 843, {"toCut": {"wp": 846}, "clean": true, "tol": undefined});
        stepVol(c, st, 847, "cut");
        atStep(st, 848);
        cqWorkplaneNew(c, st, 848, {"plane": {"origin": [-9.81499227630788, 3.276725886182183, 6.0769230769230775], "xDir": [-0.3166679938014717, -0.948536441947146, 2.7755575615628914e-17], "zDir": [-0.8179160230256567, 0.27306049051518194, 0.5064102564102565]}, "obj": undefined});
        atStep(st, 849);
        cqCircle(c, st, 849, 848, {"radius": 2, "forConstruction": false});
        atStep(st, 850);
        cqExtrude(c, st, 850, 849, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 850, "extrude");
        atStep(st, 851);
        cqCut(c, st, 851, 847, {"toCut": {"wp": 850}, "clean": true, "tol": undefined});
        stepVol(c, st, 851, "cut");
        atStep(st, 852);
        cqWorkplaneNew(c, st, 852, {"plane": {"origin": [-10.371989766355936, -1.6856124013565674, 5.794871794871795], "xDir": [0.1604112808577589, -0.9870502626379132, -1.3877787807814457e-17], "zDir": [-0.8643324805296615, -0.14046770011304732, 0.48290598290598297]}, "obj": undefined});
        atStep(st, 853);
        cqCircle(c, st, 853, 852, {"radius": 2, "forConstruction": false});
        atStep(st, 854);
        cqExtrude(c, st, 854, 853, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 854, "extrude");
        atStep(st, 855);
        cqCut(c, st, 855, 851, {"toCut": {"wp": 854}, "clean": true, "tol": undefined});
        stepVol(c, st, 855, "cut");
        atStep(st, 856);
        cqWorkplaneNew(c, st, 856, {"plane": {"origin": [-8.521055245342632, -6.40315761942842, 5.512820512820513], "xDir": [0.600742264237979, -0.7994427634035011, 0], "zDir": [-0.710087937111886, -0.5335964682857016, 0.4594017094017094]}, "obj": undefined});
        atStep(st, 857);
        cqCircle(c, st, 857, 856, {"radius": 2, "forConstruction": false});
        atStep(st, 858);
        cqExtrude(c, st, 858, 857, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 858, "extrude");
        atStep(st, 859);
        cqCut(c, st, 859, 855, {"toCut": {"wp": 858}, "clean": true, "tol": undefined});
        stepVol(c, st, 859, "cut");
        atStep(st, 860);
        cqWorkplaneNew(c, st, 860, {"plane": {"origin": [-4.6298608731713236, -9.757225094744673, 5.23076923076923], "xDir": [0.903450434610383, -0.42869256140305295, -2.7755575615628914e-17], "zDir": [-0.3858217394309436, -0.8131020912287227, 0.43589743589743585]}, "obj": undefined});
        atStep(st, 861);
        cqCircle(c, st, 861, 860, {"radius": 2, "forConstruction": false});
        atStep(st, 862);
        cqExtrude(c, st, 862, 861, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 862, "extrude");
        atStep(st, 863);
        cqCut(c, st, 863, 859, {"toCut": {"wp": 862}, "clean": true, "tol": undefined});
        stepVol(c, st, 863, "cut");
        atStep(st, 864);
        cqWorkplaneNew(c, st, 864, {"plane": {"origin": [0.44019014252097327, -10.92320572462426, 4.948717948717948], "xDir": [0.9991889981715698, 0.04026594010941429, 0], "zDir": [0.03668251187674777, -0.9102671437186884, 0.4123931623931623]}, "obj": undefined});
        atStep(st, 865);
        cqCircle(c, st, 865, 864, {"radius": 2, "forConstruction": false});
        atStep(st, 866);
        cqExtrude(c, st, 866, 865, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 866, "extrude");
        atStep(st, 867);
        cqCut(c, st, 867, 863, {"toCut": {"wp": 866}, "clean": true, "tol": undefined});
        stepVol(c, st, 867, "cut");
        atStep(st, 868);
        cqWorkplaneNew(c, st, 868, {"plane": {"origin": [5.527707983925672, -9.57427107756338, 4.666666666666667], "xDir": [0.8660254037844384, 0.5000000000000004, 0], "zDir": [0.460642331993806, -0.7978559231302816, 0.3888888888888889]}, "obj": undefined});
        atStep(st, 869);
        cqCircle(c, st, 869, 868, {"radius": 2, "forConstruction": false});
        atStep(st, 870);
        cqExtrude(c, st, 870, 869, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 870, "extrude");
        atStep(st, 871);
        cqCut(c, st, 871, 867, {"toCut": {"wp": 870}, "clean": true, "tol": undefined});
        stepVol(c, st, 871, "cut");
        atStep(st, 872);
        cqWorkplaneNew(c, st, 872, {"plane": {"origin": [9.441009630704736, -5.970132752454882, 4.384615384615385], "xDir": [0.5344658261278026, 0.8451900855437939, 2.7755575615628914e-17], "zDir": [0.786750802558728, -0.4975110627045735, 0.3653846153846154]}, "obj": undefined});
        atStep(st, 873);
        cqCircle(c, st, 873, 872, {"radius": 2, "forConstruction": false});
        atStep(st, 874);
        cqExtrude(c, st, 874, 873, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 874, "extrude");
        atStep(st, 875);
        cqCut(c, st, 875, 871, {"toCut": {"wp": 874}, "clean": true, "tol": undefined});
        stepVol(c, st, 875, "cut");
        atStep(st, 876);
        cqWorkplaneNew(c, st, 876, {"plane": {"origin": [11.240354327940238, -0.907415211855934, 4.102564102564102], "xDir": [0.08046656871672625, 0.99675730813421, 3.469446951953614e-18], "zDir": [0.93669619399502, -0.07561793432132782, 0.34188034188034183]}, "obj": undefined});
        atStep(st, 877);
        cqCircle(c, st, 877, 876, {"radius": 2, "forConstruction": false});
        atStep(st, 878);
        cqExtrude(c, st, 878, 877, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 878, "extrude");
        atStep(st, 879);
        cqCut(c, st, 879, 875, {"toCut": {"wp": 878}, "clean": true, "tol": undefined});
        stepVol(c, st, 879, "cut");
        atStep(st, 880);
        cqWorkplaneNew(c, st, 880, {"plane": {"origin": [10.465294157281416, 4.458845140826113, 3.82051282051282], "xDir": [-0.391966609860076, 0.9199794436588238, 1.3877787807814457e-17], "zDir": [0.872107846440118, 0.37157042840217613, 0.3183760683760684]}, "obj": undefined});
        atStep(st, 881);
        cqCircle(c, st, 881, 880, {"radius": 2, "forConstruction": false});
        atStep(st, 882);
        cqExtrude(c, st, 882, 881, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 882, "extrude");
        atStep(st, 883);
        cqCut(c, st, 883, 879, {"toCut": {"wp": 882}, "clean": true, "tol": undefined});
        stepVol(c, st, 883, "cut");
        atStep(st, 884);
        cqWorkplaneNew(c, st, 884, {"plane": {"origin": [7.251898156801183, 8.881962793448979, 3.538461538461538], "xDir": [-0.7746049618276538, 0.6324453755953782, 0], "zDir": [0.6043248464000986, 0.7401635661207483, 0.2948717948717948]}, "obj": undefined});
        atStep(st, 885);
        cqCircle(c, st, 885, 884, {"radius": 2, "forConstruction": false});
        atStep(st, 886);
        cqExtrude(c, st, 886, 885, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 886, "extrude");
        atStep(st, 887);
        cqCut(c, st, 887, 883, {"toCut": {"wp": 886}, "clean": true, "tol": undefined});
        stepVol(c, st, 887, "cut");
        atStep(st, 888);
        cqWorkplaneNew(c, st, 888, {"plane": {"origin": [2.3102386790949128, 11.31629751674813, 3.256410256410257], "xDir": [-0.9797906520422678, 0.20002569377604432, -6.938893903907228e-18], "zDir": [0.19251988992457603, 0.943024793062344, 0.2713675213675214]}, "obj": undefined});
        atStep(st, 889);
        cqCircle(c, st, 889, 888, {"radius": 2, "forConstruction": false});
        atStep(st, 890);
        cqExtrude(c, st, 890, 889, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 890, "extrude");
        atStep(st, 891);
        cqCut(c, st, 891, 887, {"toCut": {"wp": 890}, "clean": true, "tol": undefined});
        stepVol(c, st, 891, "cut");
        atStep(st, 892);
        cqWorkplaneNew(c, st, 892, {"plane": {"origin": [-3.234428355972461, 11.16654207450684, 2.9743589743589745], "xDir": [-0.960518111631373, -0.27821746391645064, 0], "zDir": [-0.26953569633103847, 0.93054517287557, 0.2478632478632479]}, "obj": undefined});
        atStep(st, 893);
        cqCircle(c, st, 893, 892, {"radius": 2, "forConstruction": false});
        atStep(st, 894);
        cqExtrude(c, st, 894, 893, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 894, "extrude");
        atStep(st, 895);
        cqCut(c, st, 895, 891, {"toCut": {"wp": 894}, "clean": true, "tol": undefined});
        stepVol(c, st, 895, "cut");
        atStep(st, 896);
        cqWorkplaneNew(c, st, 896, {"plane": {"origin": [-8.100773185782266, 8.433798259530034, 2.6923076923076925], "xDir": [-0.721202447343815, -0.6927243535095989, 1.3877787807814457e-17], "zDir": [-0.6750644321485223, 0.702816521627503, 0.2243589743589744]}, "obj": undefined});
        atStep(st, 897);
        cqCircle(c, st, 897, 896, {"radius": 2, "forConstruction": false});
        atStep(st, 898);
        cqExtrude(c, st, 898, 897, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 898, "extrude");
        atStep(st, 899);
        cqCut(c, st, 899, 895, {"toCut": {"wp": 898}, "clean": true, "tol": undefined});
        stepVol(c, st, 899, "cut");
        atStep(st, 900);
        cqWorkplaneNew(c, st, 900, {"plane": {"origin": [-11.150475110303068, 3.722575566906551, 2.41025641025641], "xDir": [-0.316667993801472, -0.9485364419471458, 0], "zDir": [-0.9292062591919223, 0.310214630575546, 0.2008547008547009]}, "obj": undefined});
        atStep(st, 901);
        cqCircle(c, st, 901, 900, {"radius": 2, "forConstruction": false});
        atStep(st, 902);
        cqExtrude(c, st, 902, 901, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 902, "extrude");
        atStep(st, 903);
        cqCut(c, st, 903, 899, {"toCut": {"wp": 902}, "clean": true, "tol": undefined});
        stepVol(c, st, 903, "cut");
        atStep(st, 904);
        cqWorkplaneNew(c, st, 904, {"plane": {"origin": [-11.656839734443302, -1.8944208449510869, 2.1282051282051277], "xDir": [0.16041128085776218, -0.9870502626379126, 0], "zDir": [-0.9714033112036086, -0.1578684037459239, 0.1773504273504273]}, "obj": undefined});
        atStep(st, 905);
        cqCircle(c, st, 905, 904, {"radius": 2, "forConstruction": false});
        atStep(st, 906);
        cqExtrude(c, st, 906, 905, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 906, "extrude");
        atStep(st, 907);
        cqCut(c, st, 907, 903, {"toCut": {"wp": 906}, "clean": true, "tol": undefined});
        stepVol(c, st, 907, "cut");
        atStep(st, 908);
        cqWorkplaneNew(c, st, 908, {"plane": {"origin": [-9.479102980844983, -7.123083788280683, 1.8461538461538456], "xDir": [0.6007422642379788, -0.7994427634035014, 0], "zDir": [-0.7899252484037488, -0.5935903156900569, 0.1538461538461538]}, "obj": undefined});
        atStep(st, 909);
        cqCircle(c, st, 909, 908, {"radius": 2, "forConstruction": false});
        atStep(st, 910);
        cqExtrude(c, st, 910, 909, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 910, "extrude");
        atStep(st, 911);
        cqCut(c, st, 911, 907, {"toCut": {"wp": 910}, "clean": true, "tol": undefined});
        stepVol(c, st, 911, "cut");
        atStep(st, 912);
        cqWorkplaneNew(c, st, 912, {"plane": {"origin": [-5.100425183001818, -10.748918369843947, 1.5641025641025643], "xDir": [0.9034504346103828, -0.4286925614030532, -6.938893903907228e-18], "zDir": [-0.42503543191681814, -0.8957431974869956, 0.13034188034188038]}, "obj": undefined});
        atStep(st, 913);
        cqCircle(c, st, 913, 912, {"radius": 2, "forConstruction": false});
        atStep(st, 914);
        cqExtrude(c, st, 914, 913, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 914, "extrude");
        atStep(st, 915);
        cqCut(c, st, 915, 911, {"toCut": {"wp": 914}, "clean": true, "tol": undefined});
        stepVol(c, st, 915, "cut");
        atStep(st, 916);
        cqWorkplaneNew(c, st, 916, {"plane": {"origin": [0.4804257280718691, -11.921641482195197, 1.2820512820512824], "xDir": [0.9991889981715696, 0.04026594010941404, 8.673617379884035e-19], "zDir": [0.040035477339322424, -0.9934701235162664, 0.10683760683760686]}, "obj": undefined});
        atStep(st, 917);
        cqCircle(c, st, 917, 916, {"radius": 2, "forConstruction": false});
        atStep(st, 918);
        cqExtrude(c, st, 918, 917, {"until": 2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 918, "extrude");
        atStep(st, 919);
        cqCut(c, st, 919, 915, {"toCut": {"wp": 918}, "clean": true, "tol": undefined});
        stepVol(c, st, 919, "cut");
        atStep(st, 920);
        cqWorkplaneNew(c, st, 920, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 921);
        cqMoveTo(c, st, 921, 920, {"x": 11, "y": 0});
        atStep(st, 922);
        cqLineTo(c, st, 922, 921, {"x": 11, "y": 1, "forConstruction": false});
        atStep(st, 923);
        cqLineTo(c, st, 923, 922, {"x": 12, "y": 1, "forConstruction": false});
        atStep(st, 924);
        cqLineTo(c, st, 924, 923, {"x": 12, "y": 0, "forConstruction": false});
        atStep(st, 925);
        cqClose(c, st, 925, 924, {});
        atStep(st, 926);
        cqRevolve(c, st, 926, 925, {"angleDegrees": 360, "axisStart": [0, 0, 0], "axisEnd": [0, 0, 1], "combine": true, "clean": true});
        stepVol(c, st, 926, "revolve");
        atStep(st, 927);
        cqUnion(c, st, 927, 919, {"toUnion": {"wp": 926}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 927, "union");
        atStep(st, 928);
        cqWorkplaneNew(c, st, 928, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 929);
        cqTransformed(c, st, 929, 928, {"rotate": [0, 0, 0], "offset": [12, 0, 0]});
        atStep(st, 930);
        cqBox(c, st, 930, 929, {"length": 1, "width": 2, "height": 1, "centered": [false, true, false], "combine": true, "clean": true});
        stepVol(c, st, 930, "box");
        atStep(st, 931);
        cqSelect("Edges")(c, st, 931, 930, {"selector": undefined, "tag": undefined});
        atStep(st, 932);
        cqFillet(c, st, 932, 931, {"radius": 0.2});
        stepVol(c, st, 932, "fillet");
        atStep(st, 933);
        cqUnion(c, st, 933, 927, {"toUnion": {"wp": 932}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 933, "union");
        atStep(st, 934);
        cqWorkplaneNew(c, st, 934, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 935);
        cqTransformed(c, st, 935, 934, {"rotate": [0, 0, 60], "offset": [12, 0, 0]});
        atStep(st, 936);
        cqBox(c, st, 936, 935, {"length": 1, "width": 2, "height": 1, "centered": [false, true, false], "combine": true, "clean": true});
        stepVol(c, st, 936, "box");
        atStep(st, 937);
        cqSelect("Edges")(c, st, 937, 936, {"selector": undefined, "tag": undefined});
        atStep(st, 938);
        cqFillet(c, st, 938, 937, {"radius": 0.2});
        stepVol(c, st, 938, "fillet");
        atStep(st, 939);
        cqUnion(c, st, 939, 933, {"toUnion": {"wp": 938}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 939, "union");
        atStep(st, 940);
        cqWorkplaneNew(c, st, 940, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 941);
        cqTransformed(c, st, 941, 940, {"rotate": [0, 0, 120], "offset": [12, 0, 0]});
        atStep(st, 942);
        cqBox(c, st, 942, 941, {"length": 1, "width": 2, "height": 1, "centered": [false, true, false], "combine": true, "clean": true});
        stepVol(c, st, 942, "box");
        atStep(st, 943);
        cqSelect("Edges")(c, st, 943, 942, {"selector": undefined, "tag": undefined});
        atStep(st, 944);
        cqFillet(c, st, 944, 943, {"radius": 0.2});
        stepVol(c, st, 944, "fillet");
        atStep(st, 945);
        cqUnion(c, st, 945, 939, {"toUnion": {"wp": 944}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 945, "union");
        atStep(st, 946);
        cqWorkplaneNew(c, st, 946, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 947);
        cqTransformed(c, st, 947, 946, {"rotate": [0, 0, 180], "offset": [12, 0, 0]});
        atStep(st, 948);
        cqBox(c, st, 948, 947, {"length": 1, "width": 2, "height": 1, "centered": [false, true, false], "combine": true, "clean": true});
        stepVol(c, st, 948, "box");
        atStep(st, 949);
        cqSelect("Edges")(c, st, 949, 948, {"selector": undefined, "tag": undefined});
        atStep(st, 950);
        cqFillet(c, st, 950, 949, {"radius": 0.2});
        stepVol(c, st, 950, "fillet");
        atStep(st, 951);
        cqUnion(c, st, 951, 945, {"toUnion": {"wp": 950}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 951, "union");
        atStep(st, 952);
        cqWorkplaneNew(c, st, 952, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 953);
        cqTransformed(c, st, 953, 952, {"rotate": [0, 0, 240], "offset": [12, 0, 0]});
        atStep(st, 954);
        cqBox(c, st, 954, 953, {"length": 1, "width": 2, "height": 1, "centered": [false, true, false], "combine": true, "clean": true});
        stepVol(c, st, 954, "box");
        atStep(st, 955);
        cqSelect("Edges")(c, st, 955, 954, {"selector": undefined, "tag": undefined});
        atStep(st, 956);
        cqFillet(c, st, 956, 955, {"radius": 0.2});
        stepVol(c, st, 956, "fillet");
        atStep(st, 957);
        cqUnion(c, st, 957, 951, {"toUnion": {"wp": 956}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 957, "union");
        atStep(st, 958);
        cqWorkplaneNew(c, st, 958, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 959);
        cqTransformed(c, st, 959, 958, {"rotate": [0, 0, 300], "offset": [12, 0, 0]});
        atStep(st, 960);
        cqBox(c, st, 960, 959, {"length": 1, "width": 2, "height": 1, "centered": [false, true, false], "combine": true, "clean": true});
        stepVol(c, st, 960, "box");
        atStep(st, 961);
        cqSelect("Edges")(c, st, 961, 960, {"selector": undefined, "tag": undefined});
        atStep(st, 962);
        cqFillet(c, st, 962, 961, {"radius": 0.2});
        stepVol(c, st, 962, "fillet");
        atStep(st, 963);
        cqUnion(c, st, 963, 957, {"toUnion": {"wp": 962}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 963, "union");
        atStep(st, 964);
        cqCombine(c, st, 964, 963, {"clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 964, "combine");
        return mergeMaps(cqMeasure(c, st, {"wp": 964}), {"i": 4948});
    } catch (e) {
        return {"i": 4948, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c4948"] = case_4948();
    return out;
}
