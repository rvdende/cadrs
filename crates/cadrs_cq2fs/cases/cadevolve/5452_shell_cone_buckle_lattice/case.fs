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

const case_5452 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 2);
        cqPolyline(c, st, 2, 1, {"listOfXYTuple": [[0, 0], [10, 0], [0, 20]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 3);
        cqClose(c, st, 3, 2, {});
        atStep(st, 4);
        cqRevolve(c, st, 4, 3, {"angleDegrees": 360, "axisStart": [0, 0, 0], "axisEnd": [0, 0, 1], "combine": true, "clean": true});
        stepVol(c, st, 4, "revolve");
        atStep(st, 5);
        cqWorkplaneNew(c, st, 5, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 6);
        cqPolyline(c, st, 6, 5, {"listOfXYTuple": [[0, 0], [8.5, 0], [0, 20]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 7);
        cqClose(c, st, 7, 6, {});
        atStep(st, 8);
        cqRevolve(c, st, 8, 7, {"angleDegrees": 360, "axisStart": [0, 0, 0], "axisEnd": [0, 0, 1], "combine": true, "clean": true});
        stepVol(c, st, 8, "revolve");
        atStep(st, 9);
        cqCut(c, st, 9, 4, {"toCut": {"wp": 8}, "clean": true, "tol": undefined});
        stepVol(c, st, 9, "cut");
        atStep(st, 10);
        cqWorkplaneNew(c, st, 10, {"plane": {"origin": [8.81633499376349, 2.362329841781174, 1.2909944487358056], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.23149479148832816, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 11);
        cqPolyline(c, st, 11, 10, {"listOfXYTuple": [[-2.588190451025207, -1.3925808938442112], [2.775557561562891e-16, 2.7851617876884225], [2.588190451025207, -1.3925808938442117]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 12);
        cqClose(c, st, 12, 11, {});
        atStep(st, 13);
        cqExtrude(c, st, 13, 12, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 13, "extrude");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 14, {"plane": {"origin": [7.383588799797677, 4.262916981148694, 2.581988897471611], "xDir": [-0.49999999999999994, 0.8660254037844388, 0], "zDir": [0.7745966692414834, 0.4472135954999578, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 15);
        cqPolyline(c, st, 15, 14, {"listOfXYTuple": [[-6.661338147750939e-16, -2.968668170843414], [-2.086989525343523, 1.4843340854217075], [2.086989525343523, 1.4843340854217073]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 16);
        cqClose(c, st, 16, 15, {});
        atStep(st, 17);
        cqExtrude(c, st, 17, 16, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 17, "extrude");
        atStep(st, 18);
        cqWorkplaneNew(c, st, 18, {"plane": {"origin": [6.454005151982316, 6.454005151982315, 1.2909944487358056], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 19);
        cqPolyline(c, st, 19, 18, {"listOfXYTuple": [[-2.5881904510252083, -1.392580893844211], [-1.1102230246251563e-16, 2.785161787688422], [2.588190451025207, -1.3925808938442108]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 20);
        cqClose(c, st, 20, 19, {});
        atStep(st, 21);
        cqExtrude(c, st, 21, 20, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 21, "extrude");
        atStep(st, 22);
        cqWorkplaneNew(c, st, 22, {"plane": {"origin": [4.262916981148695, 7.383588799797676, 2.581988897471611], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 23);
        cqPolyline(c, st, 23, 22, {"listOfXYTuple": [[1.1102230246251563e-16, -2.968668170843414], [-2.0869895253435224, 1.4843340854217066], [2.0869895253435224, 1.4843340854217069]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 24);
        cqClose(c, st, 24, 23, {});
        atStep(st, 25);
        cqExtrude(c, st, 25, 24, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 25, "extrude");
        atStep(st, 26);
        cqWorkplaneNew(c, st, 26, {"plane": {"origin": [2.3623298417811753, 8.816334993763489, 1.2909944487358056], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 27);
        cqPolyline(c, st, 27, 26, {"listOfXYTuple": [[-2.588190451025207, -1.3925808938442117], [-7.2164496600635185e-16, 2.7851617876884216], [2.5881904510252087, -1.3925808938442117]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 28);
        cqClose(c, st, 28, 27, {});
        atStep(st, 29);
        cqExtrude(c, st, 29, 28, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 29, "extrude");
        atStep(st, 30);
        cqWorkplaneNew(c, st, 30, {"plane": {"origin": [1.0922862195580171e-15, 8.525833962297389, 2.581988897471611], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 31);
        cqPolyline(c, st, 31, 30, {"listOfXYTuple": [[5.702294559585527e-16, -2.968668170843414], [-2.086989525343524, 1.484334085421707], [2.086989525343523, 1.4843340854217069]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 32);
        cqClose(c, st, 32, 31, {});
        atStep(st, 33);
        cqExtrude(c, st, 33, 32, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 33, "extrude");
        atStep(st, 34);
        cqWorkplaneNew(c, st, 34, {"plane": {"origin": [-2.362329841781173, 8.81633499376349, 1.2909944487358056], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 35);
        cqPolyline(c, st, 35, 34, {"listOfXYTuple": [[-2.5881904510252065, -1.3925808938442112], [1.6653345369377348e-16, 2.7851617876884225], [2.588190451025207, -1.3925808938442112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 36);
        cqClose(c, st, 36, 35, {});
        atStep(st, 37);
        cqExtrude(c, st, 37, 36, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 37, "extrude");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 38, {"plane": {"origin": [-4.2629169811486936, 7.383588799797677, 2.581988897471611], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 39);
        cqPolyline(c, st, 39, 38, {"listOfXYTuple": [[-4.440892098500626e-16, -2.9686681708434133], [-2.086989525343523, 1.484334085421707], [2.0869895253435233, 1.484334085421707]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 40);
        cqClose(c, st, 40, 39, {});
        atStep(st, 41);
        cqExtrude(c, st, 41, 40, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 41, "extrude");
        atStep(st, 42);
        cqWorkplaneNew(c, st, 42, {"plane": {"origin": [-6.454005151982315, 6.454005151982318, 1.2909944487358056], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 43);
        cqPolyline(c, st, 43, 42, {"listOfXYTuple": [[-2.5881904510252074, -1.39258089384421], [1.2212453270876722e-15, 2.7851617876884225], [2.5881904510252065, -1.3925808938442112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 44);
        cqClose(c, st, 44, 43, {});
        atStep(st, 45);
        cqExtrude(c, st, 45, 44, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 45, "extrude");
        atStep(st, 46);
        cqWorkplaneNew(c, st, 46, {"plane": {"origin": [-7.383588799797676, 4.262916981148696, 2.581988897471611], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 47);
        cqPolyline(c, st, 47, 46, {"listOfXYTuple": [[-9.992007221626409e-16, -2.9686681708434146], [-2.086989525343522, 1.4843340854217064], [2.086989525343523, 1.484334085421707]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 48);
        cqClose(c, st, 48, 47, {});
        atStep(st, 49);
        cqExtrude(c, st, 49, 48, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 49, "extrude");
        atStep(st, 50);
        cqWorkplaneNew(c, st, 50, {"plane": {"origin": [-8.816334993763489, 2.3623298417811767, 1.2909944487358056], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 51);
        cqPolyline(c, st, 51, 50, {"listOfXYTuple": [[-2.5881904510252083, -1.3925808938442117], [1.1102230246251563e-16, 2.785161787688422], [2.588190451025209, -1.3925808938442112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 52);
        cqClose(c, st, 52, 51, {});
        atStep(st, 53);
        cqExtrude(c, st, 53, 52, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 53, "extrude");
        atStep(st, 54);
        cqWorkplaneNew(c, st, 54, {"plane": {"origin": [-8.525833962297389, 2.1845724391160347e-15, 2.581988897471611], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 55);
        cqPolyline(c, st, 55, 54, {"listOfXYTuple": [[1.1404589119171052e-15, -2.968668170843414], [-2.086989525343523, 1.484334085421707], [2.086989525343522, 1.4843340854217066]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 56);
        cqClose(c, st, 56, 55, {});
        atStep(st, 57);
        cqExtrude(c, st, 57, 56, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 57, "extrude");
        atStep(st, 58);
        cqWorkplaneNew(c, st, 58, {"plane": {"origin": [-8.81633499376349, -2.362329841781172, 1.2909944487358056], "xDir": [0.2588190451025206, -0.9659258262890684, 0], "zDir": [-0.8639503235220042, -0.231494791488328, 0.447213595499958]}, "obj": undefined});
        atStep(st, 59);
        cqPolyline(c, st, 59, 58, {"listOfXYTuple": [[-2.5881904510252065, -1.3925808938442112], [-1.4432899320127037e-15, 2.785161787688422], [2.5881904510252065, -1.392580893844212]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 60);
        cqClose(c, st, 60, 59, {});
        atStep(st, 61);
        cqExtrude(c, st, 61, 60, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 61, "extrude");
        atStep(st, 62);
        cqWorkplaneNew(c, st, 62, {"plane": {"origin": [-7.383588799797678, -4.262916981148692, 2.581988897471611], "xDir": [0.4999999999999996, -0.866025403784439, 0], "zDir": [-0.7745966692414836, -0.44721359549995754, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 63);
        cqPolyline(c, st, 63, 62, {"listOfXYTuple": [[3.3306690738754696e-16, -2.968668170843415], [-2.086989525343524, 1.484334085421707], [2.0869895253435216, 1.484334085421707]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 64);
        cqClose(c, st, 64, 63, {});
        atStep(st, 65);
        cqExtrude(c, st, 65, 64, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 65, "extrude");
        atStep(st, 66);
        cqWorkplaneNew(c, st, 66, {"plane": {"origin": [-6.454005151982319, -6.4540051519823125, 1.2909944487358056], "xDir": [0.7071067811865472, -0.7071067811865478, 0], "zDir": [-0.6324555320336762, -0.6324555320336757, 0.447213595499958]}, "obj": undefined});
        atStep(st, 67);
        cqPolyline(c, st, 67, 66, {"listOfXYTuple": [[-2.5881904510252065, -1.3925808938442117], [-4.440892098500626e-16, 2.785161787688422], [2.5881904510252074, -1.392580893844211]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 68);
        cqClose(c, st, 68, 67, {});
        atStep(st, 69);
        cqExtrude(c, st, 69, 68, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 69, "extrude");
        atStep(st, 70);
        cqWorkplaneNew(c, st, 70, {"plane": {"origin": [-4.262916981148699, -7.383588799797674, 2.581988897471611], "xDir": [0.8660254037844384, -0.5000000000000006, 0], "zDir": [-0.44721359549995854, -0.7745966692414832, 0.4472135954999581]}, "obj": undefined});
        atStep(st, 71);
        cqPolyline(c, st, 71, 70, {"listOfXYTuple": [[9.992007221626409e-16, -2.9686681708434137], [-2.0869895253435224, 1.4843340854217064], [2.0869895253435216, 1.4843340854217066]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 72);
        cqClose(c, st, 72, 71, {});
        atStep(st, 73);
        cqExtrude(c, st, 73, 72, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 73, "extrude");
        atStep(st, 74);
        cqWorkplaneNew(c, st, 74, {"plane": {"origin": [-2.3623298417811784, -8.816334993763489, 1.2909944487358056], "xDir": [0.9659258262890682, -0.25881904510252124, 0], "zDir": [-0.2314947914883286, -0.863950323522004, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 75);
        cqPolyline(c, st, 75, 74, {"listOfXYTuple": [[-2.5881904510252083, -1.3925808938442117], [-2.4980018054066026e-15, 2.7851617876884225], [2.588190451025211, -1.3925808938442112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 76);
        cqClose(c, st, 76, 75, {});
        atStep(st, 77);
        cqExtrude(c, st, 77, 76, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 77, "extrude");
        atStep(st, 78);
        cqWorkplaneNew(c, st, 78, {"plane": {"origin": [-3.868977605140803e-15, -8.525833962297389, 2.581988897471611], "xDir": [1, -3.8285686989269494e-16, 0], "zDir": [-3.424375946931434e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 79);
        cqPolyline(c, st, 79, 78, {"listOfXYTuple": [[2.5964020013167108e-15, -2.968668170843414], [-2.0869895253435256, 1.4843340854217073], [2.086989525343523, 1.4843340854217069]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 80);
        cqClose(c, st, 80, 79, {});
        atStep(st, 81);
        cqExtrude(c, st, 81, 80, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 81, "extrude");
        atStep(st, 82);
        cqWorkplaneNew(c, st, 82, {"plane": {"origin": [2.36232984178117, -8.816334993763492, 1.2909944487358056], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 83);
        cqPolyline(c, st, 83, 82, {"listOfXYTuple": [[-2.5881904510252043, -1.3925808938442112], [4.440892098500626e-16, 2.785161787688423], [2.5881904510252047, -1.3925808938442117]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 84);
        cqClose(c, st, 84, 83, {});
        atStep(st, 85);
        cqExtrude(c, st, 85, 84, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 85, "extrude");
        atStep(st, 86);
        cqWorkplaneNew(c, st, 86, {"plane": {"origin": [4.262916981148691, -7.383588799797678, 2.581988897471611], "xDir": [0.866025403784439, 0.4999999999999995, 0], "zDir": [0.4472135954999575, -0.7745966692414836, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 87);
        cqPolyline(c, st, 87, 86, {"listOfXYTuple": [[-3.552713678800501e-15, -2.968668170843414], [-2.0869895253435233, 1.4843340854217066], [2.086989525343525, 1.4843340854217069]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 88);
        cqClose(c, st, 88, 87, {});
        atStep(st, 89);
        cqExtrude(c, st, 89, 88, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 89, "extrude");
        atStep(st, 90);
        cqWorkplaneNew(c, st, 90, {"plane": {"origin": [6.454005151982312, -6.454005151982319, 1.2909944487358056], "xDir": [0.707106781186548, 0.7071067811865472, 2.7755575615628914e-17], "zDir": [0.6324555320336755, -0.6324555320336763, 0.447213595499958]}, "obj": undefined});
        atStep(st, 91);
        cqPolyline(c, st, 91, 90, {"listOfXYTuple": [[-2.58819045102521, -1.392580893844211], [2.55351295663786e-15, 2.7851617876884216], [2.588190451025208, -1.3925808938442112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 92);
        cqClose(c, st, 92, 91, {});
        atStep(st, 93);
        cqExtrude(c, st, 93, 92, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 93, "extrude");
        atStep(st, 94);
        cqWorkplaneNew(c, st, 94, {"plane": {"origin": [7.383588799797675, -4.262916981148699, 2.581988897471611], "xDir": [0.5000000000000006, 0.8660254037844383, 0], "zDir": [0.7745966692414832, -0.4472135954999585, 0.447213595499958]}, "obj": undefined});
        atStep(st, 95);
        cqPolyline(c, st, 95, 94, {"listOfXYTuple": [[4.440892098500626e-16, -2.968668170843414], [-2.0869895253435207, 1.4843340854217069], [2.086989525343521, 1.484334085421707]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 96);
        cqClose(c, st, 96, 95, {});
        atStep(st, 97);
        cqExtrude(c, st, 97, 96, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 97, "extrude");
        atStep(st, 98);
        cqWorkplaneNew(c, st, 98, {"plane": {"origin": [8.816334993763489, -2.362329841781178, 1.2909944487358056], "xDir": [0.2588190451025212, 0.9659258262890682, 0], "zDir": [0.8639503235220041, -0.23149479148832855, 0.447213595499958]}, "obj": undefined});
        atStep(st, 99);
        cqPolyline(c, st, 99, 98, {"listOfXYTuple": [[-2.5881904510252087, -1.3925808938442112], [-3.2751579226442118e-15, 2.785161787688422], [2.5881904510252114, -1.3925808938442108]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 100);
        cqClose(c, st, 100, 99, {});
        atStep(st, 101);
        cqExtrude(c, st, 101, 100, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 101, "extrude");
        atStep(st, 102);
        cqWorkplaneNew(c, st, 102, {"plane": {"origin": [8.525833962297389, -2.220446049250313e-15, 2.581988897471611], "xDir": [2.6043740226111406e-16, 1, 0], "zDir": [0.894427190999916, -2.3294229413572343e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 103);
        cqPolyline(c, st, 103, 102, {"listOfXYTuple": [[2.604374022611141e-15, -2.968668170843414], [-2.0869895253435273, 1.4843340854217075], [2.086989525343525, 1.484334085421707]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 104);
        cqClose(c, st, 104, 103, {});
        atStep(st, 105);
        cqExtrude(c, st, 105, 104, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 105, "extrude");
        atStep(st, 106);
        cqWorkplaneNew(c, st, 106, {"plane": {"origin": [6.265554811047783, 3.6174197567807913, 5.163977794943222], "xDir": [-0.49999999999999994, 0.8660254037844388, 0], "zDir": [0.7745966692414834, 0.4472135954999578, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 107);
        cqPolyline(c, st, 107, 106, {"listOfXYTuple": [[-2.0869895253435224, -1.4024172605264218], [2.0869895253435233, -1.402417260526422], [1.6653345369377348e-16, 2.804834521052843]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 108);
        cqClose(c, st, 108, 107, {});
        atStep(st, 109);
        cqExtrude(c, st, 109, 108, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 109, "extrude");
        atStep(st, 110);
        cqWorkplaneNew(c, st, 110, {"plane": {"origin": [6.407306556277789, 1.7168326174132715, 6.454972243679028], "xDir": [-0.2588190451025208, 0.9659258262890684, 1.3877787807814457e-17], "zDir": [0.8639503235220041, 0.2314947914883282, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 111);
        cqPolyline(c, st, 111, 110, {"listOfXYTuple": [[-2.220446049250313e-16, -2.9489954374789944], [1.5857885996618382, 1.4744977187394963], [-1.5857885996618388, 1.4744977187394968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 112);
        cqClose(c, st, 112, 111, {});
        atStep(st, 113);
        cqExtrude(c, st, 113, 112, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 113, "extrude");
        atStep(st, 114);
        cqWorkplaneNew(c, st, 114, {"plane": {"origin": [3.6174197567807926, 6.265554811047782, 5.163977794943222], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 115);
        cqPolyline(c, st, 115, 114, {"listOfXYTuple": [[-2.086989525343523, -1.4024172605264218], [2.0869895253435224, -1.4024172605264216], [5.551115123125783e-17, 2.8048345210528423]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 116);
        cqClose(c, st, 116, 115, {});
        atStep(st, 117);
        cqExtrude(c, st, 117, 116, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 117, "extrude");
        atStep(st, 118);
        cqWorkplaneNew(c, st, 118, {"plane": {"origin": [4.6904739388645185, 4.6904739388645185, 6.454972243679028], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 119);
        cqPolyline(c, st, 119, 118, {"listOfXYTuple": [[-5.551115123125785e-16, -2.948995437478993], [1.585788599661838, 1.4744977187394963], [-1.585788599661839, 1.4744977187394968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 120);
        cqClose(c, st, 120, 119, {});
        atStep(st, 121);
        cqExtrude(c, st, 121, 120, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 121, "extrude");
        atStep(st, 122);
        cqWorkplaneNew(c, st, 122, {"plane": {"origin": [1.0132356085899522e-15, 7.234839513561583, 5.163977794943222], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 123);
        cqPolyline(c, st, 123, 122, {"listOfXYTuple": [[-2.086989525343524, -1.4024172605264218], [2.086989525343523, -1.402417260526422], [5.70229455958553e-16, 2.8048345210528423]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 124);
        cqClose(c, st, 124, 123, {});
        atStep(st, 125);
        cqExtrude(c, st, 125, 124, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 125, "extrude");
        atStep(st, 126);
        cqWorkplaneNew(c, st, 126, {"plane": {"origin": [1.7168326174132726, 6.407306556277789, 6.454972243679028], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 127);
        cqPolyline(c, st, 127, 126, {"listOfXYTuple": [[-3.3306690738754696e-16, -2.9489954374789935], [1.585788599661839, 1.4744977187394963], [-1.5857885996618384, 1.4744977187394963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 128);
        cqClose(c, st, 128, 127, {});
        atStep(st, 129);
        cqExtrude(c, st, 129, 128, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 129, "extrude");
        atStep(st, 130);
        cqWorkplaneNew(c, st, 130, {"plane": {"origin": [-3.617419756780791, 6.265554811047783, 5.163977794943222], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 131);
        cqPolyline(c, st, 131, 130, {"listOfXYTuple": [[-2.086989525343523, -1.4024172605264216], [2.086989525343524, -1.4024172605264211], [-3.885780586188048e-16, 2.8048345210528427]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 132);
        cqClose(c, st, 132, 131, {});
        atStep(st, 133);
        cqExtrude(c, st, 133, 132, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 133, "extrude");
        atStep(st, 134);
        cqWorkplaneNew(c, st, 134, {"plane": {"origin": [-1.7168326174132709, 6.407306556277789, 6.454972243679028], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 135);
        cqPolyline(c, st, 135, 134, {"listOfXYTuple": [[-2.220446049250313e-16, -2.948995437478994], [1.585788599661838, 1.4744977187394963], [-1.5857885996618386, 1.4744977187394963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 136);
        cqClose(c, st, 136, 135, {});
        atStep(st, 137);
        cqExtrude(c, st, 137, 136, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 137, "extrude");
        atStep(st, 138);
        cqWorkplaneNew(c, st, 138, {"plane": {"origin": [-6.265554811047782, 3.617419756780793, 5.163977794943222], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 139);
        cqPolyline(c, st, 139, 138, {"listOfXYTuple": [[-2.0869895253435224, -1.402417260526422], [2.0869895253435224, -1.4024172605264211], [-1.4432899320127037e-15, 2.8048345210528423]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 140);
        cqClose(c, st, 140, 139, {});
        atStep(st, 141);
        cqExtrude(c, st, 141, 140, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 141, "extrude");
        atStep(st, 142);
        cqWorkplaneNew(c, st, 142, {"plane": {"origin": [-4.690473938864518, 4.690473938864519, 6.454972243679028], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 143);
        cqPolyline(c, st, 143, 142, {"listOfXYTuple": [[7.771561172376098e-16, -2.948995437478993], [1.5857885996618375, 1.4744977187394963], [-1.5857885996618384, 1.4744977187394972]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 144);
        cqClose(c, st, 144, 143, {});
        atStep(st, 145);
        cqExtrude(c, st, 145, 144, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 145, "extrude");
        atStep(st, 146);
        cqWorkplaneNew(c, st, 146, {"plane": {"origin": [-7.234839513561584, 2.0264712171799045e-15, 5.163977794943222], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 147);
        cqPolyline(c, st, 147, 146, {"listOfXYTuple": [[-2.086989525343523, -1.4024172605264211], [2.086989525343522, -1.4024172605264216], [1.1404589119171055e-15, 2.804834521052843]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 148);
        cqClose(c, st, 148, 147, {});
        atStep(st, 149);
        cqExtrude(c, st, 149, 148, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 149, "extrude");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [-6.407306556277789, 1.7168326174132733, 6.454972243679028], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 151);
        cqPolyline(c, st, 151, 150, {"listOfXYTuple": [[-5.551115123125783e-17, -2.9489954374789935], [1.5857885996618395, 1.4744977187394968], [-1.5857885996618393, 1.4744977187394963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 152);
        cqClose(c, st, 152, 151, {});
        atStep(st, 153);
        cqExtrude(c, st, 153, 152, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 153, "extrude");
        atStep(st, 154);
        cqWorkplaneNew(c, st, 154, {"plane": {"origin": [-6.265554811047784, -3.617419756780789, 5.163977794943222], "xDir": [0.4999999999999996, -0.866025403784439, 0], "zDir": [-0.7745966692414836, -0.44721359549995754, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 155);
        cqPolyline(c, st, 155, 154, {"listOfXYTuple": [[-2.0869895253435224, -1.4024172605264216], [2.0869895253435224, -1.4024172605264218], [8.881784197001252e-16, 2.804834521052843]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 156);
        cqClose(c, st, 156, 155, {});
        atStep(st, 157);
        cqExtrude(c, st, 157, 156, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 157, "extrude");
        atStep(st, 158);
        cqWorkplaneNew(c, st, 158, {"plane": {"origin": [-6.407306556277791, -1.7168326174132698, 6.454972243679028], "xDir": [0.2588190451025206, -0.9659258262890684, 0], "zDir": [-0.8639503235220042, -0.231494791488328, 0.447213595499958]}, "obj": undefined});
        atStep(st, 159);
        cqPolyline(c, st, 159, 158, {"listOfXYTuple": [[-1.054711873393899e-15, -2.9489954374789935], [1.5857885996618386, 1.4744977187394968], [-1.5857885996618373, 1.4744977187394968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 160);
        cqClose(c, st, 160, 159, {});
        atStep(st, 161);
        cqExtrude(c, st, 161, 160, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 161, "extrude");
        atStep(st, 162);
        cqWorkplaneNew(c, st, 162, {"plane": {"origin": [-3.617419756780796, -6.265554811047781, 5.163977794943222], "xDir": [0.8660254037844384, -0.5000000000000006, 0], "zDir": [-0.44721359549995854, -0.7745966692414832, 0.4472135954999581]}, "obj": undefined});
        atStep(st, 163);
        cqPolyline(c, st, 163, 162, {"listOfXYTuple": [[-2.086989525343523, -1.4024172605264211], [2.0869895253435216, -1.4024172605264211], [1.1102230246251563e-16, 2.804834521052843]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 164);
        cqClose(c, st, 164, 163, {});
        atStep(st, 165);
        cqExtrude(c, st, 165, 164, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 165, "extrude");
        atStep(st, 166);
        cqWorkplaneNew(c, st, 166, {"plane": {"origin": [-4.69047393886452, -4.690473938864517, 6.454972243679028], "xDir": [0.7071067811865472, -0.7071067811865478, 0], "zDir": [-0.6324555320336762, -0.6324555320336757, 0.447213595499958]}, "obj": undefined});
        atStep(st, 167);
        cqPolyline(c, st, 167, 166, {"listOfXYTuple": [[-1.3322676295501878e-15, -2.948995437478993], [1.5857885996618375, 1.4744977187394968], [-1.5857885996618382, 1.4744977187394968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 168);
        cqClose(c, st, 168, 167, {});
        atStep(st, 169);
        cqExtrude(c, st, 169, 168, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 169, "extrude");
        atStep(st, 170);
        cqWorkplaneNew(c, st, 170, {"plane": {"origin": [-3.6318257722366065e-15, -7.234839513561583, 5.163977794943222], "xDir": [1, -6.049014748177263e-16, 0], "zDir": [-5.410403269529253e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 171);
        cqPolyline(c, st, 171, 170, {"listOfXYTuple": [[-2.086989525343525, -1.4024172605264218], [2.0869895253435233, -1.402417260526422], [1.8361873905171066e-15, 2.8048345210528423]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 172);
        cqClose(c, st, 172, 171, {});
        atStep(st, 173);
        cqExtrude(c, st, 173, 172, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 173, "extrude");
        atStep(st, 174);
        cqWorkplaneNew(c, st, 174, {"plane": {"origin": [-1.716832617413275, -6.4073065562777884, 6.454972243679028], "xDir": [0.9659258262890682, -0.25881904510252124, 0], "zDir": [-0.2314947914883286, -0.863950323522004, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 175);
        cqPolyline(c, st, 175, 174, {"listOfXYTuple": [[-1.887379141862766e-15, -2.9489954374789944], [1.5857885996618408, 1.4744977187394968], [-1.5857885996618384, 1.4744977187394968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 176);
        cqClose(c, st, 176, 175, {});
        atStep(st, 177);
        cqExtrude(c, st, 177, 176, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 177, "extrude");
        atStep(st, 178);
        cqWorkplaneNew(c, st, 178, {"plane": {"origin": [3.6174197567807886, -6.265554811047784, 5.163977794943222], "xDir": [0.866025403784439, 0.4999999999999995, 0], "zDir": [0.4472135954999575, -0.7745966692414836, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 179);
        cqPolyline(c, st, 179, 178, {"listOfXYTuple": [[-2.0869895253435224, -1.4024172605264216], [2.0869895253435256, -1.4024172605264211], [-1.609823385706477e-15, 2.8048345210528427]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 180);
        cqClose(c, st, 180, 179, {});
        atStep(st, 181);
        cqExtrude(c, st, 181, 180, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 181, "extrude");
        atStep(st, 182);
        cqWorkplaneNew(c, st, 182, {"plane": {"origin": [1.7168326174132689, -6.407306556277791, 6.454972243679028], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 183);
        cqPolyline(c, st, 183, 182, {"listOfXYTuple": [[-1.6653345369377348e-16, -2.948995437478994], [1.5857885996618368, 1.4744977187394968], [-1.585788599661837, 1.4744977187394968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 184);
        cqClose(c, st, 184, 183, {});
        atStep(st, 185);
        cqExtrude(c, st, 185, 184, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 185, "extrude");
        atStep(st, 186);
        cqWorkplaneNew(c, st, 186, {"plane": {"origin": [6.265554811047781, -3.617419756780796, 5.163977794943222], "xDir": [0.5000000000000006, 0.8660254037844383, 0], "zDir": [0.7745966692414832, -0.4472135954999585, 0.447213595499958]}, "obj": undefined});
        atStep(st, 187);
        cqPolyline(c, st, 187, 186, {"listOfXYTuple": [[-2.0869895253435207, -1.402417260526422], [2.086989525343521, -1.4024172605264216], [2.220446049250313e-16, 2.8048345210528436]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 188);
        cqClose(c, st, 188, 187, {});
        atStep(st, 189);
        cqExtrude(c, st, 189, 188, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 189, "extrude");
        atStep(st, 190);
        cqWorkplaneNew(c, st, 190, {"plane": {"origin": [4.690473938864516, -4.69047393886452, 6.454972243679028], "xDir": [0.707106781186548, 0.7071067811865472, 2.7755575615628914e-17], "zDir": [0.6324555320336755, -0.6324555320336763, 0.447213595499958]}, "obj": undefined});
        atStep(st, 191);
        cqPolyline(c, st, 191, 190, {"listOfXYTuple": [[2.109423746787798e-15, -2.9489954374789935], [1.5857885996618382, 1.4744977187394968], [-1.5857885996618406, 1.4744977187394963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 192);
        cqClose(c, st, 192, 191, {});
        atStep(st, 193);
        cqExtrude(c, st, 193, 192, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 193, "extrude");
        atStep(st, 194);
        cqWorkplaneNew(c, st, 194, {"plane": {"origin": [7.234839513561583, -2.220446049250313e-15, 5.163977794943222], "xDir": [3.0691020099176013e-16, 1, 2.4651903288156608e-32], "zDir": [0.894427190999916, -2.745088289622796e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 195);
        cqPolyline(c, st, 195, 194, {"listOfXYTuple": [[-2.0869895253435273, -1.4024172605264216], [2.086989525343525, -1.4024172605264218], [1.8804439126953428e-15, 2.8048345210528423]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 196);
        cqClose(c, st, 196, 195, {});
        atStep(st, 197);
        cqExtrude(c, st, 197, 196, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 197, "extrude");
        atStep(st, 198);
        cqWorkplaneNew(c, st, 198, {"plane": {"origin": [6.4073065562777884, -1.7168326174132746, 6.454972243679028], "xDir": [0.25881904510252124, 0.9659258262890682, 0], "zDir": [0.863950323522004, -0.2314947914883286, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 199);
        cqPolyline(c, st, 199, 198, {"listOfXYTuple": [[-2.7755575615628914e-15, -2.9489954374789944], [1.5857885996618415, 1.474497718739497], [-1.5857885996618388, 1.4744977187394968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 200);
        cqClose(c, st, 200, 199, {});
        atStep(st, 201);
        cqExtrude(c, st, 201, 200, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 201, "extrude");
        atStep(st, 202);
        cqWorkplaneNew(c, st, 202, {"plane": {"origin": [5.160301676648057, 1.382698666958815, 9.03696114115064], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.2314947914883281, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 203);
        cqPolyline(c, st, 203, 202, {"listOfXYTuple": [[-1.5857885996618382, -1.4122536272086323], [2.775557561562891e-16, 2.824507254417262], [1.5857885996618386, -1.412253627208632]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 204);
        cqClose(c, st, 204, 203, {});
        atStep(st, 205);
        cqExtrude(c, st, 205, 204, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 205, "extrude");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 206, {"plane": {"origin": [4.1056790022427725, 2.3704148771510583, 10.327955589886445], "xDir": [-0.49999999999999994, 0.8660254037844388, 0], "zDir": [0.7745966692414834, 0.4472135954999578, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 207);
        cqPolyline(c, st, 207, 206, {"listOfXYTuple": [[2.220446049250313e-16, -2.9293227041145724], [-1.0845876739801537, 1.4646613520572864], [1.0845876739801543, 1.4646613520572866]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 208);
        cqClose(c, st, 208, 207, {});
        atStep(st, 209);
        cqExtrude(c, st, 209, 208, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 209, "extrude");
        atStep(st, 210);
        cqWorkplaneNew(c, st, 210, {"plane": {"origin": [3.777603009689242, 3.777603009689241, 9.03696114115064], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 211);
        cqPolyline(c, st, 211, 210, {"listOfXYTuple": [[-1.5857885996618382, -1.412253627208632], [2.220446049250313e-16, 2.824507254417261], [1.5857885996618388, -1.4122536272086323]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 212);
        cqClose(c, st, 212, 211, {});
        atStep(st, 213);
        cqExtrude(c, st, 213, 212, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 213, "extrude");
        atStep(st, 214);
        cqWorkplaneNew(c, st, 214, {"plane": {"origin": [2.370414877151059, 4.1056790022427725, 10.327955589886445], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 215);
        cqPolyline(c, st, 215, 214, {"listOfXYTuple": [[-3.3306690738754696e-16, -2.929322704114572], [-1.084587673980154, 1.4646613520572864], [1.0845876739801534, 1.4646613520572864]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 216);
        cqClose(c, st, 216, 215, {});
        atStep(st, 217);
        cqExtrude(c, st, 217, 216, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 217, "extrude");
        atStep(st, 218);
        cqWorkplaneNew(c, st, 218, {"plane": {"origin": [1.382698666958816, 5.160301676648057, 9.03696114115064], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 219);
        cqPolyline(c, st, 219, 218, {"listOfXYTuple": [[-1.5857885996618388, -1.4122536272086323], [-6.106226635438361e-16, 2.824507254417262], [1.5857885996618388, -1.412253627208632]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 220);
        cqClose(c, st, 220, 219, {});
        atStep(st, 221);
        cqExtrude(c, st, 221, 220, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 221, "extrude");
        atStep(st, 222);
        cqWorkplaneNew(c, st, 222, {"plane": {"origin": [6.431612670482333e-16, 4.740829754302117, 10.327955589886445], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 223);
        cqPolyline(c, st, 223, 222, {"listOfXYTuple": [[3.5286916785280227e-16, -2.929322704114572], [-1.0845876739801543, 1.4646613520572864], [1.084587673980154, 1.4646613520572864]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 224);
        cqClose(c, st, 224, 223, {});
        atStep(st, 225);
        cqExtrude(c, st, 225, 224, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 225, "extrude");
        atStep(st, 226);
        cqWorkplaneNew(c, st, 226, {"plane": {"origin": [-1.3826986669588146, 5.160301676648057, 9.03696114115064], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 227);
        cqPolyline(c, st, 227, 226, {"listOfXYTuple": [[-1.5857885996618382, -1.4122536272086323], [0, 2.824507254417262], [1.5857885996618382, -1.412253627208632]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 228);
        cqClose(c, st, 228, 227, {});
        atStep(st, 229);
        cqExtrude(c, st, 229, 228, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 229, "extrude");
        atStep(st, 230);
        cqWorkplaneNew(c, st, 230, {"plane": {"origin": [-2.370414877151058, 4.1056790022427725, 10.327955589886445], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 231);
        cqPolyline(c, st, 231, 230, {"listOfXYTuple": [[0, -2.9293227041145715], [-1.084587673980154, 1.4646613520572862], [1.0845876739801543, 1.4646613520572864]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 232);
        cqClose(c, st, 232, 231, {});
        atStep(st, 233);
        cqExtrude(c, st, 233, 232, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 233, "extrude");
        atStep(st, 234);
        cqWorkplaneNew(c, st, 234, {"plane": {"origin": [-3.777603009689241, 3.7776030096892423, 9.03696114115064], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 235);
        cqPolyline(c, st, 235, 234, {"listOfXYTuple": [[-1.5857885996618384, -1.412253627208632], [8.881784197001252e-16, 2.824507254417261], [1.5857885996618375, -1.4122536272086326]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 236);
        cqClose(c, st, 236, 235, {});
        atStep(st, 237);
        cqExtrude(c, st, 237, 236, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 237, "extrude");
        atStep(st, 238);
        cqWorkplaneNew(c, st, 238, {"plane": {"origin": [-4.1056790022427725, 2.37041487715106, 10.327955589886445], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 239);
        cqPolyline(c, st, 239, 238, {"listOfXYTuple": [[-7.771561172376098e-16, -2.929322704114572], [-1.0845876739801534, 1.4646613520572866], [1.084587673980154, 1.4646613520572866]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 240);
        cqClose(c, st, 240, 239, {});
        atStep(st, 241);
        cqExtrude(c, st, 241, 240, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 241, "extrude");
        atStep(st, 242);
        cqWorkplaneNew(c, st, 242, {"plane": {"origin": [-5.160301676648056, 1.3826986669588166, 9.03696114115064], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 243);
        cqPolyline(c, st, 243, 242, {"listOfXYTuple": [[-1.5857885996618395, -1.4122536272086326], [-5.551115123125783e-17, 2.8245072544172616], [1.5857885996618395, -1.4122536272086323]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 244);
        cqClose(c, st, 244, 243, {});
        atStep(st, 245);
        cqExtrude(c, st, 245, 244, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 245, "extrude");
        atStep(st, 246);
        cqWorkplaneNew(c, st, 246, {"plane": {"origin": [-4.740829754302118, 1.212307665788123e-15, 10.327955589886445], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 247);
        cqPolyline(c, st, 247, 246, {"listOfXYTuple": [[6.317234673972607e-16, -2.9293227041145715], [-1.084587673980154, 1.4646613520572869], [1.0845876739801534, 1.4646613520572864]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 248);
        cqClose(c, st, 248, 247, {});
        atStep(st, 249);
        cqExtrude(c, st, 249, 248, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 249, "extrude");
        atStep(st, 250);
        cqWorkplaneNew(c, st, 250, {"plane": {"origin": [-5.160301676648057, -1.382698666958814, 9.03696114115064], "xDir": [0.2588190451025206, -0.9659258262890684, 0], "zDir": [-0.8639503235220042, -0.231494791488328, 0.447213595499958]}, "obj": undefined});
        atStep(st, 251);
        cqPolyline(c, st, 251, 250, {"listOfXYTuple": [[-1.5857885996618375, -1.4122536272086326], [-7.2164496600635185e-16, 2.8245072544172616], [1.5857885996618382, -1.4122536272086323]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 252);
        cqClose(c, st, 252, 251, {});
        atStep(st, 253);
        cqExtrude(c, st, 253, 252, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 253, "extrude");
        atStep(st, 254);
        cqWorkplaneNew(c, st, 254, {"plane": {"origin": [-4.105679002242773, -2.370414877151057, 10.327955589886445], "xDir": [0.4999999999999996, -0.866025403784439, 0], "zDir": [-0.7745966692414836, -0.44721359549995754, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 255);
        cqPolyline(c, st, 255, 254, {"listOfXYTuple": [[8.881784197001252e-16, -2.9293227041145724], [-1.0845876739801543, 1.4646613520572864], [1.0845876739801537, 1.4646613520572866]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 256);
        cqClose(c, st, 256, 255, {});
        atStep(st, 257);
        cqExtrude(c, st, 257, 256, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 257, "extrude");
        atStep(st, 258);
        cqWorkplaneNew(c, st, 258, {"plane": {"origin": [-3.777603009689243, -3.77760300968924, 9.03696114115064], "xDir": [0.7071067811865472, -0.7071067811865478, 0], "zDir": [-0.6324555320336762, -0.6324555320336757, 0.447213595499958]}, "obj": undefined});
        atStep(st, 259);
        cqPolyline(c, st, 259, 258, {"listOfXYTuple": [[-1.5857885996618384, -1.4122536272086323], [-7.771561172376098e-16, 2.824507254417261], [1.5857885996618375, -1.412253627208632]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 260);
        cqClose(c, st, 260, 259, {});
        atStep(st, 261);
        cqExtrude(c, st, 261, 260, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 261, "extrude");
        atStep(st, 262);
        cqWorkplaneNew(c, st, 262, {"plane": {"origin": [-2.370414877151061, -4.105679002242771, 10.327955589886445], "xDir": [0.8660254037844384, -0.5000000000000006, 0], "zDir": [-0.44721359549995854, -0.7745966692414832, 0.4472135954999581]}, "obj": undefined});
        atStep(st, 263);
        cqPolyline(c, st, 263, 262, {"listOfXYTuple": [[4.440892098500626e-16, -2.929322704114572], [-1.084587673980154, 1.4646613520572862], [1.084587673980153, 1.4646613520572862]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 264);
        cqClose(c, st, 264, 263, {});
        atStep(st, 265);
        cqExtrude(c, st, 265, 264, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 265, "extrude");
        atStep(st, 266);
        cqWorkplaneNew(c, st, 266, {"plane": {"origin": [-1.3826986669588175, -5.160301676648056, 9.03696114115064], "xDir": [0.9659258262890682, -0.25881904510252124, 0], "zDir": [-0.2314947914883286, -0.863950323522004, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 267);
        cqPolyline(c, st, 267, 266, {"listOfXYTuple": [[-1.585788599661839, -1.4122536272086323], [-1.3877787807814457e-15, 2.8245072544172616], [1.5857885996618404, -1.412253627208632]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 268);
        cqClose(c, st, 268, 267, {});
        atStep(st, 269);
        cqExtrude(c, st, 269, 268, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 269, "extrude");
        atStep(st, 270);
        cqWorkplaneNew(c, st, 270, {"plane": {"origin": [-2.0775135377613873e-15, -4.740829754302117, 10.327955589886445], "xDir": [1, -3.8285686989269494e-16, 0], "zDir": [-3.424375946931434e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 271);
        cqPolyline(c, st, 271, 270, {"listOfXYTuple": [[1.4827100151781248e-15, -2.929322704114572], [-1.0845876739801554, 1.4646613520572866], [1.084587673980154, 1.4646613520572862]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 272);
        cqClose(c, st, 272, 271, {});
        atStep(st, 273);
        cqExtrude(c, st, 273, 272, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 273, "extrude");
        atStep(st, 274);
        cqWorkplaneNew(c, st, 274, {"plane": {"origin": [1.382698666958813, -5.160301676648058, 9.03696114115064], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 275);
        cqPolyline(c, st, 275, 274, {"listOfXYTuple": [[-1.585788599661837, -1.4122536272086323], [-1.6653345369377348e-16, 2.824507254417262], [1.5857885996618368, -1.4122536272086326]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 276);
        cqClose(c, st, 276, 275, {});
        atStep(st, 277);
        cqExtrude(c, st, 277, 276, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 277, "extrude");
        atStep(st, 278);
        cqWorkplaneNew(c, st, 278, {"plane": {"origin": [2.3704148771510565, -4.105679002242773, 10.327955589886445], "xDir": [0.866025403784439, 0.4999999999999995, 0], "zDir": [0.4472135954999575, -0.7745966692414836, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 279);
        cqPolyline(c, st, 279, 278, {"listOfXYTuple": [[-1.5543122344752196e-15, -2.929322704114572], [-1.084587673980154, 1.4646613520572862], [1.0845876739801552, 1.4646613520572864]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 280);
        cqClose(c, st, 280, 279, {});
        atStep(st, 281);
        cqExtrude(c, st, 281, 280, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 281, "extrude");
        atStep(st, 282);
        cqWorkplaneNew(c, st, 282, {"plane": {"origin": [3.7776030096892392, -3.777603009689244, 9.03696114115064], "xDir": [0.707106781186548, 0.707106781186547, 2.7755575615628907e-17], "zDir": [0.6324555320336754, -0.6324555320336763, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 283);
        cqPolyline(c, st, 283, 282, {"listOfXYTuple": [[-1.5857885996618402, -1.4122536272086326], [1.4432899320127037e-15, 2.824507254417262], [1.5857885996618393, -1.4122536272086323]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 284);
        cqClose(c, st, 284, 283, {});
        atStep(st, 285);
        cqExtrude(c, st, 285, 284, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 285, "extrude");
        atStep(st, 286);
        cqWorkplaneNew(c, st, 286, {"plane": {"origin": [4.105679002242771, -2.370414877151061, 10.327955589886445], "xDir": [0.5000000000000006, 0.8660254037844383, 0], "zDir": [0.7745966692414832, -0.4472135954999585, 0.447213595499958]}, "obj": undefined});
        atStep(st, 287);
        cqPolyline(c, st, 287, 286, {"listOfXYTuple": [[3.3306690738754696e-16, -2.9293227041145724], [-1.0845876739801528, 1.4646613520572864], [1.084587673980153, 1.4646613520572864]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 288);
        cqClose(c, st, 288, 287, {});
        atStep(st, 289);
        cqExtrude(c, st, 289, 288, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 289, "extrude");
        atStep(st, 290);
        cqWorkplaneNew(c, st, 290, {"plane": {"origin": [5.160301676648056, -1.3826986669588173, 9.03696114115064], "xDir": [0.25881904510252113, 0.9659258262890682, 0], "zDir": [0.8639503235220041, -0.23149479148832852, 0.447213595499958]}, "obj": undefined});
        atStep(st, 291);
        cqPolyline(c, st, 291, 290, {"listOfXYTuple": [[-1.5857885996618395, -1.4122536272086323], [-1.887379141862766e-15, 2.8245072544172616], [1.5857885996618408, -1.412253627208632]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 292);
        cqClose(c, st, 292, 291, {});
        atStep(st, 293);
        cqExtrude(c, st, 293, 292, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 293, "extrude");
        atStep(st, 294);
        cqWorkplaneNew(c, st, 294, {"plane": {"origin": [4.7408297543021165, -1.1102230246251563e-15, 10.327955589886445], "xDir": [2.3418327216194016e-16, 1, 0], "zDir": [0.894427190999916, -2.0945988629897295e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 295);
        cqPolyline(c, st, 295, 294, {"listOfXYTuple": [[1.4348448085758489e-15, -2.9293227041145724], [-1.0845876739801563, 1.4646613520572862], [1.084587673980155, 1.464661352057286]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 296);
        cqClose(c, st, 296, 295, {});
        atStep(st, 297);
        cqExtrude(c, st, 297, 296, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 297, "extrude");
        atStep(st, 298);
        cqWorkplaneNew(c, st, 298, {"plane": {"origin": [2.987645013492877, 1.7249176527831558, 12.909944487358054], "xDir": [-0.5000000000000001, 0.8660254037844387, 0], "zDir": [0.7745966692414834, 0.447213595499958, 0.447213595499958]}, "obj": undefined});
        atStep(st, 299);
        cqPolyline(c, st, 299, 298, {"listOfXYTuple": [[-1.084587673980154, -1.4220899938908418], [1.0845876739801537, -1.422089993890842], [-3.3306690738754696e-16, 2.844179987781685]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 300);
        cqClose(c, st, 300, 299, {});
        atStep(st, 301);
        cqExtrude(c, st, 301, 300, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 301, "extrude");
        atStep(st, 302);
        cqWorkplaneNew(c, st, 302, {"plane": {"origin": [2.7512732391623564, 0.7372014425909125, 14.20093893609386], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.23149479148832816, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 303);
        cqPolyline(c, st, 303, 302, {"listOfXYTuple": [[1.6653345369377348e-16, -2.9096499707501544], [0.5833867482984691, 1.4548249853750763], [-0.5833867482984694, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 304);
        cqClose(c, st, 304, 303, {});
        atStep(st, 305);
        cqExtrude(c, st, 305, 304, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 305, "extrude");
        atStep(st, 306);
        cqWorkplaneNew(c, st, 306, {"plane": {"origin": [1.7249176527831562, 2.987645013492877, 12.909944487358054], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 307);
        cqPolyline(c, st, 307, 306, {"listOfXYTuple": [[-1.084587673980154, -1.4220899938908418], [1.084587673980154, -1.422089993890842], [1.1102230246251563e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 308);
        cqClose(c, st, 308, 307, {});
        atStep(st, 309);
        cqExtrude(c, st, 309, 308, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 309, "extrude");
        atStep(st, 310);
        cqWorkplaneNew(c, st, 310, {"plane": {"origin": [2.0140717965714443, 2.0140717965714434, 14.20093893609386], "xDir": [-0.7071067811865475, 0.7071067811865478, 0], "zDir": [0.6324555320336761, 0.6324555320336758, 0.447213595499958]}, "obj": undefined});
        atStep(st, 311);
        cqPolyline(c, st, 311, 310, {"listOfXYTuple": [[6.661338147750939e-16, -2.909649970750154], [0.5833867482984696, 1.4548249853750763], [-0.5833867482984691, 1.454824985375076]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 312);
        cqClose(c, st, 312, 311, {});
        atStep(st, 313);
        cqExtrude(c, st, 313, 312, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 313, "extrude");
        atStep(st, 314);
        cqWorkplaneNew(c, st, 314, {"plane": {"origin": [5.64110656080168e-16, 3.4498353055663116, 12.909944487358054], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 315);
        cqPolyline(c, st, 315, 314, {"listOfXYTuple": [[-1.0845876739801543, -1.422089993890842], [1.084587673980154, -1.4220899938908418], [3.5286916785280227e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 316);
        cqClose(c, st, 316, 315, {});
        atStep(st, 317);
        cqExtrude(c, st, 317, 316, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 317, "extrude");
        atStep(st, 318);
        cqWorkplaneNew(c, st, 318, {"plane": {"origin": [0.7372014425909129, 2.7512732391623564, 14.20093893609386], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 319);
        cqPolyline(c, st, 319, 318, {"listOfXYTuple": [[-2.220446049250313e-16, -2.909649970750154], [0.5833867482984695, 1.4548249853750763], [-0.5833867482984694, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 320);
        cqClose(c, st, 320, 319, {});
        atStep(st, 321);
        cqExtrude(c, st, 321, 320, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 321, "extrude");
        atStep(st, 322);
        cqWorkplaneNew(c, st, 322, {"plane": {"origin": [-1.7249176527831551, 2.987645013492877, 12.909944487358054], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 323);
        cqPolyline(c, st, 323, 322, {"listOfXYTuple": [[-1.084587673980154, -1.422089993890842], [1.084587673980154, -1.422089993890842], [-4.440892098500626e-16, 2.844179987781685]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 324);
        cqClose(c, st, 324, 323, {});
        atStep(st, 325);
        cqExtrude(c, st, 325, 324, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 325, "extrude");
        atStep(st, 326);
        cqWorkplaneNew(c, st, 326, {"plane": {"origin": [-0.7372014425909121, 2.7512732391623564, 14.20093893609386], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 327);
        cqPolyline(c, st, 327, 326, {"listOfXYTuple": [[-5.551115123125783e-17, -2.909649970750154], [0.5833867482984691, 1.4548249853750763], [-0.5833867482984691, 1.454824985375076]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 328);
        cqClose(c, st, 328, 327, {});
        atStep(st, 329);
        cqExtrude(c, st, 329, 328, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 329, "extrude");
        atStep(st, 330);
        cqWorkplaneNew(c, st, 330, {"plane": {"origin": [-2.987645013492877, 1.724917652783157, 12.909944487358054], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 331);
        cqPolyline(c, st, 331, 330, {"listOfXYTuple": [[-1.0845876739801532, -1.4220899938908418], [1.084587673980154, -1.4220899938908418], [-4.440892098500626e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 332);
        cqClose(c, st, 332, 331, {});
        atStep(st, 333);
        cqExtrude(c, st, 333, 332, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 333, "extrude");
        atStep(st, 334);
        cqWorkplaneNew(c, st, 334, {"plane": {"origin": [-2.0140717965714434, 2.0140717965714443, 14.20093893609386], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 335);
        cqPolyline(c, st, 335, 334, {"listOfXYTuple": [[4.440892098500626e-16, -2.909649970750154], [0.5833867482984691, 1.454824985375076], [-0.5833867482984691, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 336);
        cqClose(c, st, 336, 335, {});
        atStep(st, 337);
        cqExtrude(c, st, 337, 336, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 337, "extrude");
        atStep(st, 338);
        cqWorkplaneNew(c, st, 338, {"plane": {"origin": [-3.449835305566312, 1.0542064438519925e-15, 12.909944487358054], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 339);
        cqPolyline(c, st, 339, 338, {"listOfXYTuple": [[-1.084587673980154, -1.4220899938908418], [1.0845876739801534, -1.422089993890842], [6.317234673972608e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 340);
        cqClose(c, st, 340, 339, {});
        atStep(st, 341);
        cqExtrude(c, st, 341, 340, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 341, "extrude");
        atStep(st, 342);
        cqWorkplaneNew(c, st, 342, {"plane": {"origin": [-2.7512732391623564, 0.7372014425909131, 14.20093893609386], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 343);
        cqPolyline(c, st, 343, 342, {"listOfXYTuple": [[-1.6653345369377348e-16, -2.9096499707501544], [0.5833867482984696, 1.4548249853750763], [-0.5833867482984697, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 344);
        cqClose(c, st, 344, 343, {});
        atStep(st, 345);
        cqExtrude(c, st, 345, 344, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 345, "extrude");
        atStep(st, 346);
        cqWorkplaneNew(c, st, 346, {"plane": {"origin": [-2.9876450134928785, -1.7249176527831545, 12.909944487358054], "xDir": [0.4999999999999996, -0.866025403784439, 0], "zDir": [-0.7745966692414836, -0.44721359549995754, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 347);
        cqPolyline(c, st, 347, 346, {"listOfXYTuple": [[-1.084587673980154, -1.4220899938908422], [1.084587673980154, -1.422089993890842], [3.3306690738754696e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 348);
        cqClose(c, st, 348, 347, {});
        atStep(st, 349);
        cqExtrude(c, st, 349, 348, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 349, "extrude");
        atStep(st, 350);
        cqWorkplaneNew(c, st, 350, {"plane": {"origin": [-2.751273239162357, -0.7372014425909116, 14.20093893609386], "xDir": [0.2588190451025206, -0.9659258262890684, 0], "zDir": [-0.8639503235220042, -0.231494791488328, 0.447213595499958]}, "obj": undefined});
        atStep(st, 351);
        cqPolyline(c, st, 351, 350, {"listOfXYTuple": [[-6.661338147750939e-16, -2.9096499707501544], [0.5833867482984695, 1.4548249853750763], [-0.5833867482984687, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 352);
        cqClose(c, st, 352, 351, {});
        atStep(st, 353);
        cqExtrude(c, st, 353, 352, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 353, "extrude");
        atStep(st, 354);
        cqWorkplaneNew(c, st, 354, {"plane": {"origin": [-1.7249176527831578, -2.9876450134928763, 12.909944487358054], "xDir": [0.8660254037844384, -0.5000000000000006, 0], "zDir": [-0.44721359549995854, -0.7745966692414832, 0.4472135954999581]}, "obj": undefined});
        atStep(st, 355);
        cqPolyline(c, st, 355, 354, {"listOfXYTuple": [[-1.0845876739801534, -1.4220899938908418], [1.0845876739801534, -1.4220899938908418], [1.1102230246251563e-16, 2.844179987781685]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 356);
        cqClose(c, st, 356, 355, {});
        atStep(st, 357);
        cqExtrude(c, st, 357, 356, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 357, "extrude");
        atStep(st, 358);
        cqWorkplaneNew(c, st, 358, {"plane": {"origin": [-2.0140717965714448, -2.014071796571443, 14.20093893609386], "xDir": [0.7071067811865472, -0.7071067811865478, 0], "zDir": [-0.6324555320336762, -0.6324555320336757, 0.447213595499958]}, "obj": undefined});
        atStep(st, 359);
        cqPolyline(c, st, 359, 358, {"listOfXYTuple": [[-4.440892098500626e-16, -2.909649970750154], [0.583386748298469, 1.4548249853750763], [-0.5833867482984691, 1.454824985375076]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 360);
        cqClose(c, st, 360, 359, {});
        atStep(st, 361);
        cqExtrude(c, st, 361, 360, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 361, "extrude");
        atStep(st, 362);
        cqWorkplaneNew(c, st, 362, {"plane": {"origin": [-1.8403617048571916e-15, -3.4498353055663116, 12.909944487358054], "xDir": [1, -6.049014748177263e-16, 0], "zDir": [-5.410403269529253e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 363);
        cqPolyline(c, st, 363, 362, {"listOfXYTuple": [[-1.084587673980155, -1.4220899938908418], [1.0845876739801543, -1.422089993890842], [7.029601113925594e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 364);
        cqClose(c, st, 364, 363, {});
        atStep(st, 365);
        cqExtrude(c, st, 365, 364, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 365, "extrude");
        atStep(st, 366);
        cqWorkplaneNew(c, st, 366, {"plane": {"origin": [-0.737201442590914, -2.7512732391623556, 14.20093893609386], "xDir": [0.9659258262890682, -0.25881904510252124, 0], "zDir": [-0.2314947914883286, -0.863950323522004, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 367);
        cqPolyline(c, st, 367, 366, {"listOfXYTuple": [[-9.43689570931383e-16, -2.9096499707501544], [0.5833867482984703, 1.4548249853750763], [-0.5833867482984693, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 368);
        cqClose(c, st, 368, 367, {});
        atStep(st, 369);
        cqExtrude(c, st, 369, 368, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 369, "extrude");
        atStep(st, 370);
        cqWorkplaneNew(c, st, 370, {"plane": {"origin": [1.7249176527831545, -2.987645013492878, 12.909944487358054], "xDir": [0.866025403784439, 0.4999999999999995, 0], "zDir": [0.4472135954999575, -0.7745966692414836, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 371);
        cqPolyline(c, st, 371, 370, {"listOfXYTuple": [[-1.0845876739801543, -1.4220899938908422], [1.084587673980155, -1.4220899938908418], [-9.992007221626409e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 372);
        cqClose(c, st, 372, 371, {});
        atStep(st, 373);
        cqExtrude(c, st, 373, 372, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 373, "extrude");
        atStep(st, 374);
        cqWorkplaneNew(c, st, 374, {"plane": {"origin": [0.7372014425909112, -2.751273239162357, 14.20093893609386], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 375);
        cqPolyline(c, st, 375, 374, {"listOfXYTuple": [[-1.1102230246251563e-16, -2.9096499707501544], [0.5833867482984687, 1.4548249853750763], [-0.5833867482984688, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 376);
        cqClose(c, st, 376, 375, {});
        atStep(st, 377);
        cqExtrude(c, st, 377, 376, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 377, "extrude");
        atStep(st, 378);
        cqWorkplaneNew(c, st, 378, {"plane": {"origin": [2.9876450134928767, -1.7249176527831576, 12.909944487358054], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [0.7745966692414832, -0.4472135954999584, 0.447213595499958]}, "obj": undefined});
        atStep(st, 379);
        cqPolyline(c, st, 379, 378, {"listOfXYTuple": [[-1.0845876739801528, -1.4220899938908418], [1.0845876739801528, -1.4220899938908418], [-1.1102230246251563e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 380);
        cqClose(c, st, 380, 379, {});
        atStep(st, 381);
        cqExtrude(c, st, 381, 380, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 381, "extrude");
        atStep(st, 382);
        cqWorkplaneNew(c, st, 382, {"plane": {"origin": [2.014071796571443, -2.0140717965714448, 14.20093893609386], "xDir": [0.7071067811865478, 0.7071067811865472, 0], "zDir": [0.6324555320336757, -0.6324555320336762, 0.447213595499958]}, "obj": undefined});
        atStep(st, 383);
        cqPolyline(c, st, 383, 382, {"listOfXYTuple": [[8.881784197001252e-16, -2.909649970750154], [0.583386748298469, 1.4548249853750763], [-0.58338674829847, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 384);
        cqClose(c, st, 384, 383, {});
        atStep(st, 385);
        cqExtrude(c, st, 385, 384, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 385, "extrude");
        atStep(st, 386);
        cqWorkplaneNew(c, st, 386, {"plane": {"origin": [3.4498353055663116, -1.1102230246251563e-15, 12.909944487358054], "xDir": [3.2181913810024824e-16, 1, 0], "zDir": [0.894427190999916, -2.87843787701019e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 387);
        cqPolyline(c, st, 387, 386, {"listOfXYTuple": [[-1.084587673980156, -1.4220899938908418], [1.0845876739801554, -1.4220899938908418], [7.253910562963098e-16, 2.8441799877816853]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 388);
        cqClose(c, st, 388, 387, {});
        atStep(st, 389);
        cqExtrude(c, st, 389, 388, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 389, "extrude");
        atStep(st, 390);
        cqWorkplaneNew(c, st, 390, {"plane": {"origin": [2.7512732391623556, -0.7372014425909139, 14.20093893609386], "xDir": [0.2588190451025213, 0.9659258262890682, -1.3877787807814457e-17], "zDir": [0.863950323522004, -0.23149479148832863, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 391);
        cqPolyline(c, st, 391, 390, {"listOfXYTuple": [[-1.2212453270876722e-15, -2.9096499707501544], [0.5833867482984706, 1.4548249853750763], [-0.5833867482984694, 1.4548249853750763]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 392);
        cqClose(c, st, 392, 391, {});
        atStep(st, 393);
        cqExtrude(c, st, 393, 392, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 393, "extrude");
        atStep(st, 394);
        cqWorkplaneNew(c, st, 394, {"plane": {"origin": [1.5042683595326232, 0.4030674921364561, 16.78292783356547], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.23149479148832816, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 395);
        cqPolyline(c, st, 395, 394, {"listOfXYTuple": [[-0.5833867482984694, -1.4319263605730506], [0, 2.863852721146104], [0.5833867482984691, -1.4319263605730508]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 396);
        cqClose(c, st, 396, 395, {});
        atStep(st, 397);
        cqExtrude(c, st, 397, 396, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 397, "extrude");
        atStep(st, 398);
        cqWorkplaneNew(c, st, 398, {"plane": {"origin": [0.8277692046878679, 0.4779127731534228, 18.073922282301275], "xDir": [-0.49999999999999983, 0.8660254037844388, 0], "zDir": [0.7745966692414835, 0.4472135954999578, 0.447213595499958]}, "obj": undefined});
        atStep(st, 399);
        cqPolyline(c, st, 399, 398, {"listOfXYTuple": [[1.1102230246251563e-16, -2.88997723738573], [-0.08218582261678509, 1.4449886186928658], [0.08218582261678506, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 400);
        cqClose(c, st, 400, 399, {});
        atStep(st, 401);
        cqExtrude(c, st, 401, 400, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 401, "extrude");
        atStep(st, 402);
        cqWorkplaneNew(c, st, 402, {"plane": {"origin": [1.1012008673961673, 1.101200867396167, 16.78292783356547], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 403);
        cqPolyline(c, st, 403, 402, {"listOfXYTuple": [[-0.5833867482984695, -1.43192636057305], [0, 2.863852721146104], [0.5833867482984694, -1.43192636057305]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 404);
        cqClose(c, st, 404, 403, {});
        atStep(st, 405);
        cqExtrude(c, st, 405, 404, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 405, "extrude");
        atStep(st, 406);
        cqWorkplaneNew(c, st, 406, {"plane": {"origin": [0.47791277315342295, 0.8277692046878679, 18.073922282301275], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 407);
        cqPolyline(c, st, 407, 406, {"listOfXYTuple": [[-1.1102230246251563e-16, -2.88997723738573], [-0.08218582261678511, 1.4449886186928658], [0.08218582261678492, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 408);
        cqClose(c, st, 408, 407, {});
        atStep(st, 409);
        cqExtrude(c, st, 409, 408, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 409, "extrude");
        atStep(st, 410);
        cqWorkplaneNew(c, st, 410, {"plane": {"origin": [0.4030674921364563, 1.5042683595326232, 16.78292783356547], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 411);
        cqPolyline(c, st, 411, 410, {"listOfXYTuple": [[-0.5833867482984693, -1.4319263605730503], [-1.6653345369377348e-16, 2.863852721146104], [0.5833867482984696, -1.4319263605730503]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 412);
        cqClose(c, st, 412, 411, {});
        atStep(st, 413);
        cqExtrude(c, st, 413, 412, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 413, "extrude");
        atStep(st, 414);
        cqWorkplaneNew(c, st, 414, {"plane": {"origin": [7.8388082806662e-17, 0.9558255463068458, 18.073922282301275], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 415);
        cqPolyline(c, st, 415, 414, {"listOfXYTuple": [[1.9860648015264553e-17, -2.88997723738573], [-0.08218582261678509, 1.4449886186928658], [0.08218582261678507, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 416);
        cqClose(c, st, 416, 415, {});
        atStep(st, 417);
        cqExtrude(c, st, 417, 416, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 417, "extrude");
        atStep(st, 418);
        cqWorkplaneNew(c, st, 418, {"plane": {"origin": [-0.40306749213645593, 1.5042683595326232, 16.78292783356547], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 419);
        cqPolyline(c, st, 419, 418, {"listOfXYTuple": [[-0.5833867482984691, -1.4319263605730503], [-5.551115123125783e-17, 2.863852721146104], [0.5833867482984693, -1.4319263605730503]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 420);
        cqClose(c, st, 420, 419, {});
        atStep(st, 421);
        cqExtrude(c, st, 421, 420, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 421, "extrude");
        atStep(st, 422);
        cqWorkplaneNew(c, st, 422, {"plane": {"origin": [-0.47791277315342273, 0.8277692046878679, 18.073922282301275], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 423);
        cqPolyline(c, st, 423, 422, {"listOfXYTuple": [[-1.1102230246251563e-16, -2.8899772373857298], [-0.08218582261678509, 1.4449886186928658], [0.08218582261678511, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 424);
        cqClose(c, st, 424, 423, {});
        atStep(st, 425);
        cqExtrude(c, st, 425, 424, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 425, "extrude");
        atStep(st, 426);
        cqWorkplaneNew(c, st, 426, {"plane": {"origin": [-1.1012008673961668, 1.1012008673961675, 16.78292783356547], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 427);
        cqPolyline(c, st, 427, 426, {"listOfXYTuple": [[-0.5833867482984693, -1.43192636057305], [3.3306690738754696e-16, 2.863852721146104], [0.583386748298469, -1.43192636057305]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 428);
        cqClose(c, st, 428, 427, {});
        atStep(st, 429);
        cqExtrude(c, st, 429, 428, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 429, "extrude");
        atStep(st, 430);
        cqWorkplaneNew(c, st, 430, {"plane": {"origin": [-0.8277692046878675, 0.4779127731534231, 18.073922282301275], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 431);
        cqPolyline(c, st, 431, 430, {"listOfXYTuple": [[0, -2.88997723738573], [-0.08218582261678506, 1.4449886186928658], [0.08218582261678509, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 432);
        cqClose(c, st, 432, 431, {});
        atStep(st, 433);
        cqExtrude(c, st, 433, 432, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 433, "extrude");
        atStep(st, 434);
        cqWorkplaneNew(c, st, 434, {"plane": {"origin": [-1.5042683595326232, 0.40306749213645654, 16.78292783356547], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 435);
        cqPolyline(c, st, 435, 434, {"listOfXYTuple": [[-0.5833867482984696, -1.4319263605730503], [0, 2.863852721146104], [0.5833867482984696, -1.4319263605730503]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 436);
        cqClose(c, st, 436, 435, {});
        atStep(st, 437);
        cqExtrude(c, st, 437, 436, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 437, "extrude");
        atStep(st, 438);
        cqWorkplaneNew(c, st, 438, {"plane": {"origin": [-0.9558255463068458, 1.6602802415186694e-16, 18.073922282301275], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 439);
        cqPolyline(c, st, 439, 438, {"listOfXYTuple": [[4.897315456907207e-17, -2.88997723738573], [-0.08218582261678507, 1.4449886186928658], [0.08218582261678502, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 440);
        cqClose(c, st, 440, 439, {});
        atStep(st, 441);
        cqExtrude(c, st, 441, 440, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 441, "extrude");
        atStep(st, 442);
        cqWorkplaneNew(c, st, 442, {"plane": {"origin": [-1.5042683595326236, -0.4030674921364558, 16.78292783356547], "xDir": [0.2588190451025206, -0.9659258262890684, 0], "zDir": [-0.8639503235220042, -0.231494791488328, 0.447213595499958]}, "obj": undefined});
        atStep(st, 443);
        cqPolyline(c, st, 443, 442, {"listOfXYTuple": [[-0.5833867482984691, -1.4319263605730503], [0, 2.863852721146104], [0.583386748298469, -1.4319263605730503]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 444);
        cqClose(c, st, 444, 443, {});
        atStep(st, 445);
        cqExtrude(c, st, 445, 444, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 445, "extrude");
        atStep(st, 446);
        cqWorkplaneNew(c, st, 446, {"plane": {"origin": [-0.827769204687868, -0.47791277315342257, 18.073922282301275], "xDir": [0.4999999999999996, -0.866025403784439, 0], "zDir": [-0.7745966692414836, -0.44721359549995754, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 447);
        cqPolyline(c, st, 447, 446, {"listOfXYTuple": [[2.220446049250313e-16, -2.88997723738573], [-0.08218582261678514, 1.444988618692866], [0.08218582261678495, 1.444988618692866]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 448);
        cqClose(c, st, 448, 447, {});
        atStep(st, 449);
        cqExtrude(c, st, 449, 448, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 449, "extrude");
        atStep(st, 450);
        cqWorkplaneNew(c, st, 450, {"plane": {"origin": [-1.1012008673961675, -1.1012008673961666, 16.78292783356547], "xDir": [0.7071067811865472, -0.7071067811865478, 0], "zDir": [-0.6324555320336762, -0.6324555320336757, 0.447213595499958]}, "obj": undefined});
        atStep(st, 451);
        cqPolyline(c, st, 451, 450, {"listOfXYTuple": [[-0.5833867482984691, -1.43192636057305], [1.1102230246251563e-16, 2.863852721146104], [0.5833867482984691, -1.43192636057305]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 452);
        cqClose(c, st, 452, 451, {});
        atStep(st, 453);
        cqExtrude(c, st, 453, 452, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 453, "extrude");
        atStep(st, 454);
        cqWorkplaneNew(c, st, 454, {"plane": {"origin": [-0.4779127731534234, -0.8277692046878675, 18.073922282301275], "xDir": [0.8660254037844384, -0.5000000000000006, 0], "zDir": [-0.44721359549995854, -0.7745966692414832, 0.4472135954999581]}, "obj": undefined});
        atStep(st, 455);
        cqPolyline(c, st, 455, 454, {"listOfXYTuple": [[2.220446049250313e-16, -2.8899772373857298], [-0.08218582261678509, 1.4449886186928658], [0.08218582261678498, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 456);
        cqClose(c, st, 456, 455, {});
        atStep(st, 457);
        cqExtrude(c, st, 457, 456, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 457, "extrude");
        atStep(st, 458);
        cqWorkplaneNew(c, st, 458, {"plane": {"origin": [-0.4030674921364568, -1.504268359532623, 16.78292783356547], "xDir": [0.9659258262890682, -0.25881904510252124, 0], "zDir": [-0.2314947914883286, -0.863950323522004, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 459);
        cqPolyline(c, st, 459, 458, {"listOfXYTuple": [[-0.5833867482984697, -1.4319263605730506], [-1.6653345369377348e-16, 2.863852721146104], [0.58338674829847, -1.4319263605730506]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 460);
        cqClose(c, st, 460, 459, {});
        atStep(st, 461);
        cqExtrude(c, st, 461, 460, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 461, "extrude");
        atStep(st, 462);
        cqWorkplaneNew(c, st, 462, {"plane": {"origin": [-2.675457533048863e-16, -0.9558255463068458, 18.073922282301275], "xDir": [1, -3.8285686989269494e-16, 0], "zDir": [-3.424375946931434e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 463);
        cqPolyline(c, st, 463, 462, {"listOfXYTuple": [[3.5051431196245294e-16, -2.88997723738573], [-0.0821858226167853, 1.4449886186928658], [0.08218582261678493, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 464);
        cqClose(c, st, 464, 463, {});
        atStep(st, 465);
        cqExtrude(c, st, 465, 464, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 465, "extrude");
        atStep(st, 466);
        cqWorkplaneNew(c, st, 466, {"plane": {"origin": [0.40306749213645543, -1.5042683595326236, 16.78292783356547], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 467);
        cqPolyline(c, st, 467, 466, {"listOfXYTuple": [[-0.5833867482984687, -1.4319263605730506], [0, 2.863852721146104], [0.5833867482984687, -1.4319263605730506]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 468);
        cqClose(c, st, 468, 467, {});
        atStep(st, 469);
        cqExtrude(c, st, 469, 468, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 469, "extrude");
        atStep(st, 470);
        cqWorkplaneNew(c, st, 470, {"plane": {"origin": [0.4779127731534223, -0.8277692046878681, 18.073922282301275], "xDir": [0.866025403784439, 0.4999999999999993, 0], "zDir": [0.4472135954999573, -0.7745966692414837, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 471);
        cqPolyline(c, st, 471, 470, {"listOfXYTuple": [[-1.1102230246251563e-16, -2.88997723738573], [-0.08218582261678509, 1.4449886186928658], [0.0821858226167852, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 472);
        cqClose(c, st, 472, 471, {});
        atStep(st, 473);
        cqExtrude(c, st, 473, 472, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 473, "extrude");
        atStep(st, 474);
        cqWorkplaneNew(c, st, 474, {"plane": {"origin": [1.1012008673961664, -1.101200867396168, 16.78292783356547], "xDir": [0.707106781186548, 0.707106781186547, 2.7755575615628907e-17], "zDir": [0.6324555320336754, -0.6324555320336764, 0.447213595499958]}, "obj": undefined});
        atStep(st, 475);
        cqPolyline(c, st, 475, 474, {"listOfXYTuple": [[-0.5833867482984695, -1.4319263605730503], [1.1102230246251563e-16, 2.863852721146104], [0.5833867482984696, -1.4319263605730503]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 476);
        cqClose(c, st, 476, 475, {});
        atStep(st, 477);
        cqExtrude(c, st, 477, 476, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 477, "extrude");
        atStep(st, 478);
        cqWorkplaneNew(c, st, 478, {"plane": {"origin": [0.8277692046878675, -0.47791277315342334, 18.073922282301275], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [0.7745966692414832, -0.4472135954999584, 0.447213595499958]}, "obj": undefined});
        atStep(st, 479);
        cqPolyline(c, st, 479, 478, {"listOfXYTuple": [[-1.1102230246251563e-16, -2.88997723738573], [-0.08218582261678495, 1.4449886186928658], [0.08218582261678503, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 480);
        cqClose(c, st, 480, 479, {});
        atStep(st, 481);
        cqExtrude(c, st, 481, 480, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 481, "extrude");
        atStep(st, 482);
        cqWorkplaneNew(c, st, 482, {"plane": {"origin": [1.504268359532623, -0.4030674921364566, 16.78292783356547], "xDir": [0.2588190451025211, 0.9659258262890682, -1.3877787807814457e-17], "zDir": [0.8639503235220041, -0.23149479148832847, 0.447213595499958]}, "obj": undefined});
        atStep(st, 483);
        cqPolyline(c, st, 483, 482, {"listOfXYTuple": [[-0.5833867482984699, -1.4319263605730503], [-1.1102230246251563e-16, 2.8638527211461033], [0.58338674829847, -1.4319263605730503]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 484);
        cqClose(c, st, 484, 483, {});
        atStep(st, 485);
        cqExtrude(c, st, 485, 484, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 485, "extrude");
        atStep(st, 486);
        cqWorkplaneNew(c, st, 486, {"plane": {"origin": [0.9558255463068458, -8.789265611615821e-17, 18.073922282301275], "xDir": [9.195470497284896e-17, 1, 0], "zDir": [0.894427190999916, -8.224678846809133e-17, 0.447213595499958]}, "obj": undefined});
        atStep(st, 487);
        cqPolyline(c, st, 487, 486, {"listOfXYTuple": [[2.0726896779796899e-16, -2.88997723738573], [-0.0821858226167853, 1.4449886186928658], [0.08218582261678509, 1.4449886186928658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 488);
        cqClose(c, st, 488, 487, {});
        atStep(st, 489);
        cqExtrude(c, st, 489, 488, {"until": 0.500001, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 489, "extrude");
        atStep(st, 490);
        cqVal(c, st, 1, 13);
        atStep(st, 491);
        cqUnion(c, st, 490, 9, {"toUnion": {"shape": 1}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 490, "union");
        atStep(st, 492);
        cqVal(c, st, 2, 17);
        atStep(st, 493);
        cqUnion(c, st, 491, 490, {"toUnion": {"shape": 2}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 491, "union");
        atStep(st, 494);
        cqVal(c, st, 3, 21);
        atStep(st, 495);
        cqUnion(c, st, 492, 491, {"toUnion": {"shape": 3}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 492, "union");
        atStep(st, 496);
        cqVal(c, st, 4, 25);
        atStep(st, 497);
        cqUnion(c, st, 493, 492, {"toUnion": {"shape": 4}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 493, "union");
        atStep(st, 498);
        cqVal(c, st, 5, 29);
        atStep(st, 499);
        cqUnion(c, st, 494, 493, {"toUnion": {"shape": 5}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 494, "union");
        atStep(st, 500);
        cqVal(c, st, 6, 33);
        atStep(st, 501);
        cqUnion(c, st, 495, 494, {"toUnion": {"shape": 6}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 495, "union");
        atStep(st, 502);
        cqVal(c, st, 7, 37);
        atStep(st, 503);
        cqUnion(c, st, 496, 495, {"toUnion": {"shape": 7}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 496, "union");
        atStep(st, 504);
        cqVal(c, st, 8, 41);
        atStep(st, 505);
        cqUnion(c, st, 497, 496, {"toUnion": {"shape": 8}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 497, "union");
        atStep(st, 506);
        cqVal(c, st, 9, 45);
        atStep(st, 507);
        cqUnion(c, st, 498, 497, {"toUnion": {"shape": 9}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 498, "union");
        atStep(st, 508);
        cqVal(c, st, 10, 49);
        atStep(st, 509);
        cqUnion(c, st, 499, 498, {"toUnion": {"shape": 10}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 499, "union");
        atStep(st, 510);
        cqVal(c, st, 11, 53);
        atStep(st, 511);
        cqUnion(c, st, 500, 499, {"toUnion": {"shape": 11}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 500, "union");
        atStep(st, 512);
        cqVal(c, st, 12, 57);
        atStep(st, 513);
        cqUnion(c, st, 501, 500, {"toUnion": {"shape": 12}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 501, "union");
        atStep(st, 514);
        cqVal(c, st, 13, 61);
        atStep(st, 515);
        cqUnion(c, st, 502, 501, {"toUnion": {"shape": 13}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 502, "union");
        atStep(st, 516);
        cqVal(c, st, 14, 65);
        atStep(st, 517);
        cqUnion(c, st, 503, 502, {"toUnion": {"shape": 14}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 503, "union");
        atStep(st, 518);
        cqVal(c, st, 15, 69);
        atStep(st, 519);
        cqUnion(c, st, 504, 503, {"toUnion": {"shape": 15}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 504, "union");
        atStep(st, 520);
        cqVal(c, st, 16, 73);
        atStep(st, 521);
        cqUnion(c, st, 505, 504, {"toUnion": {"shape": 16}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 505, "union");
        atStep(st, 522);
        cqVal(c, st, 17, 77);
        atStep(st, 523);
        cqUnion(c, st, 506, 505, {"toUnion": {"shape": 17}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 506, "union");
        atStep(st, 524);
        cqVal(c, st, 18, 81);
        atStep(st, 525);
        cqUnion(c, st, 507, 506, {"toUnion": {"shape": 18}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 507, "union");
        atStep(st, 526);
        cqVal(c, st, 19, 85);
        atStep(st, 527);
        cqUnion(c, st, 508, 507, {"toUnion": {"shape": 19}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 508, "union");
        atStep(st, 528);
        cqVal(c, st, 20, 89);
        atStep(st, 529);
        cqUnion(c, st, 509, 508, {"toUnion": {"shape": 20}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 509, "union");
        atStep(st, 530);
        cqVal(c, st, 21, 93);
        atStep(st, 531);
        cqUnion(c, st, 510, 509, {"toUnion": {"shape": 21}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 510, "union");
        atStep(st, 532);
        cqVal(c, st, 22, 97);
        atStep(st, 533);
        cqUnion(c, st, 511, 510, {"toUnion": {"shape": 22}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 511, "union");
        atStep(st, 534);
        cqVal(c, st, 23, 101);
        atStep(st, 535);
        cqUnion(c, st, 512, 511, {"toUnion": {"shape": 23}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 512, "union");
        atStep(st, 536);
        cqVal(c, st, 24, 105);
        atStep(st, 537);
        cqUnion(c, st, 513, 512, {"toUnion": {"shape": 24}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 513, "union");
        atStep(st, 538);
        cqVal(c, st, 25, 109);
        atStep(st, 539);
        cqUnion(c, st, 514, 513, {"toUnion": {"shape": 25}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 514, "union");
        atStep(st, 540);
        cqVal(c, st, 26, 113);
        atStep(st, 541);
        cqUnion(c, st, 515, 514, {"toUnion": {"shape": 26}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 515, "union");
        atStep(st, 542);
        cqVal(c, st, 27, 117);
        atStep(st, 543);
        cqUnion(c, st, 516, 515, {"toUnion": {"shape": 27}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 516, "union");
        atStep(st, 544);
        cqVal(c, st, 28, 121);
        atStep(st, 545);
        cqUnion(c, st, 517, 516, {"toUnion": {"shape": 28}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 517, "union");
        atStep(st, 546);
        cqVal(c, st, 29, 125);
        atStep(st, 547);
        cqUnion(c, st, 518, 517, {"toUnion": {"shape": 29}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 518, "union");
        atStep(st, 548);
        cqVal(c, st, 30, 129);
        atStep(st, 549);
        cqUnion(c, st, 519, 518, {"toUnion": {"shape": 30}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 519, "union");
        atStep(st, 550);
        cqVal(c, st, 31, 133);
        atStep(st, 551);
        cqUnion(c, st, 520, 519, {"toUnion": {"shape": 31}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 520, "union");
        atStep(st, 552);
        cqVal(c, st, 32, 137);
        atStep(st, 553);
        cqUnion(c, st, 521, 520, {"toUnion": {"shape": 32}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 521, "union");
        atStep(st, 554);
        cqVal(c, st, 33, 141);
        atStep(st, 555);
        cqUnion(c, st, 522, 521, {"toUnion": {"shape": 33}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 522, "union");
        atStep(st, 556);
        cqVal(c, st, 34, 145);
        atStep(st, 557);
        cqUnion(c, st, 523, 522, {"toUnion": {"shape": 34}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 523, "union");
        atStep(st, 558);
        cqVal(c, st, 35, 149);
        atStep(st, 559);
        cqUnion(c, st, 524, 523, {"toUnion": {"shape": 35}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 524, "union");
        atStep(st, 560);
        cqVal(c, st, 36, 153);
        atStep(st, 561);
        cqUnion(c, st, 525, 524, {"toUnion": {"shape": 36}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 525, "union");
        atStep(st, 562);
        cqVal(c, st, 37, 157);
        atStep(st, 563);
        cqUnion(c, st, 526, 525, {"toUnion": {"shape": 37}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 526, "union");
        atStep(st, 564);
        cqVal(c, st, 38, 161);
        atStep(st, 565);
        cqUnion(c, st, 527, 526, {"toUnion": {"shape": 38}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 527, "union");
        atStep(st, 566);
        cqVal(c, st, 39, 165);
        atStep(st, 567);
        cqUnion(c, st, 528, 527, {"toUnion": {"shape": 39}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 528, "union");
        atStep(st, 568);
        cqVal(c, st, 40, 169);
        atStep(st, 569);
        cqUnion(c, st, 529, 528, {"toUnion": {"shape": 40}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 529, "union");
        atStep(st, 570);
        cqVal(c, st, 41, 173);
        atStep(st, 571);
        cqUnion(c, st, 530, 529, {"toUnion": {"shape": 41}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 530, "union");
        atStep(st, 572);
        cqVal(c, st, 42, 177);
        atStep(st, 573);
        cqUnion(c, st, 531, 530, {"toUnion": {"shape": 42}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 531, "union");
        atStep(st, 574);
        cqVal(c, st, 43, 181);
        atStep(st, 575);
        cqUnion(c, st, 532, 531, {"toUnion": {"shape": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 532, "union");
        atStep(st, 576);
        cqVal(c, st, 44, 185);
        atStep(st, 577);
        cqUnion(c, st, 533, 532, {"toUnion": {"shape": 44}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 533, "union");
        atStep(st, 578);
        cqVal(c, st, 45, 189);
        atStep(st, 579);
        cqUnion(c, st, 534, 533, {"toUnion": {"shape": 45}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 534, "union");
        atStep(st, 580);
        cqVal(c, st, 46, 193);
        atStep(st, 581);
        cqUnion(c, st, 535, 534, {"toUnion": {"shape": 46}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 535, "union");
        atStep(st, 582);
        cqVal(c, st, 47, 197);
        atStep(st, 583);
        cqUnion(c, st, 536, 535, {"toUnion": {"shape": 47}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 536, "union");
        atStep(st, 584);
        cqVal(c, st, 48, 201);
        atStep(st, 585);
        cqUnion(c, st, 537, 536, {"toUnion": {"shape": 48}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 537, "union");
        atStep(st, 586);
        cqVal(c, st, 49, 205);
        atStep(st, 587);
        cqUnion(c, st, 538, 537, {"toUnion": {"shape": 49}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 538, "union");
        atStep(st, 588);
        cqVal(c, st, 50, 209);
        atStep(st, 589);
        cqUnion(c, st, 539, 538, {"toUnion": {"shape": 50}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 539, "union");
        atStep(st, 590);
        cqVal(c, st, 51, 213);
        atStep(st, 591);
        cqUnion(c, st, 540, 539, {"toUnion": {"shape": 51}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 540, "union");
        atStep(st, 592);
        cqVal(c, st, 52, 217);
        atStep(st, 593);
        cqUnion(c, st, 541, 540, {"toUnion": {"shape": 52}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 541, "union");
        atStep(st, 594);
        cqVal(c, st, 53, 221);
        atStep(st, 595);
        cqUnion(c, st, 542, 541, {"toUnion": {"shape": 53}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 542, "union");
        atStep(st, 596);
        cqVal(c, st, 54, 225);
        atStep(st, 597);
        cqUnion(c, st, 543, 542, {"toUnion": {"shape": 54}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 543, "union");
        atStep(st, 598);
        cqVal(c, st, 55, 229);
        atStep(st, 599);
        cqUnion(c, st, 544, 543, {"toUnion": {"shape": 55}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 544, "union");
        atStep(st, 600);
        cqVal(c, st, 56, 233);
        atStep(st, 601);
        cqUnion(c, st, 545, 544, {"toUnion": {"shape": 56}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 545, "union");
        atStep(st, 602);
        cqVal(c, st, 57, 237);
        atStep(st, 603);
        cqUnion(c, st, 546, 545, {"toUnion": {"shape": 57}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 546, "union");
        atStep(st, 604);
        cqVal(c, st, 58, 241);
        atStep(st, 605);
        cqUnion(c, st, 547, 546, {"toUnion": {"shape": 58}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 547, "union");
        atStep(st, 606);
        cqVal(c, st, 59, 245);
        atStep(st, 607);
        cqUnion(c, st, 548, 547, {"toUnion": {"shape": 59}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 548, "union");
        atStep(st, 608);
        cqVal(c, st, 60, 249);
        atStep(st, 609);
        cqUnion(c, st, 549, 548, {"toUnion": {"shape": 60}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 549, "union");
        atStep(st, 610);
        cqVal(c, st, 61, 253);
        atStep(st, 611);
        cqUnion(c, st, 550, 549, {"toUnion": {"shape": 61}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 550, "union");
        atStep(st, 612);
        cqVal(c, st, 62, 257);
        atStep(st, 613);
        cqUnion(c, st, 551, 550, {"toUnion": {"shape": 62}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 551, "union");
        atStep(st, 614);
        cqVal(c, st, 63, 261);
        atStep(st, 615);
        cqUnion(c, st, 552, 551, {"toUnion": {"shape": 63}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 552, "union");
        atStep(st, 616);
        cqVal(c, st, 64, 265);
        atStep(st, 617);
        cqUnion(c, st, 553, 552, {"toUnion": {"shape": 64}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 553, "union");
        atStep(st, 618);
        cqVal(c, st, 65, 269);
        atStep(st, 619);
        cqUnion(c, st, 554, 553, {"toUnion": {"shape": 65}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 554, "union");
        atStep(st, 620);
        cqVal(c, st, 66, 273);
        atStep(st, 621);
        cqUnion(c, st, 555, 554, {"toUnion": {"shape": 66}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 555, "union");
        atStep(st, 622);
        cqVal(c, st, 67, 277);
        atStep(st, 623);
        cqUnion(c, st, 556, 555, {"toUnion": {"shape": 67}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 556, "union");
        atStep(st, 624);
        cqVal(c, st, 68, 281);
        atStep(st, 625);
        cqUnion(c, st, 557, 556, {"toUnion": {"shape": 68}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 557, "union");
        atStep(st, 626);
        cqVal(c, st, 69, 285);
        atStep(st, 627);
        cqUnion(c, st, 558, 557, {"toUnion": {"shape": 69}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 558, "union");
        atStep(st, 628);
        cqVal(c, st, 70, 289);
        atStep(st, 629);
        cqUnion(c, st, 559, 558, {"toUnion": {"shape": 70}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 559, "union");
        atStep(st, 630);
        cqVal(c, st, 71, 293);
        atStep(st, 631);
        cqUnion(c, st, 560, 559, {"toUnion": {"shape": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 560, "union");
        atStep(st, 632);
        cqVal(c, st, 72, 297);
        atStep(st, 633);
        cqUnion(c, st, 561, 560, {"toUnion": {"shape": 72}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 561, "union");
        atStep(st, 634);
        cqVal(c, st, 73, 301);
        atStep(st, 635);
        cqUnion(c, st, 562, 561, {"toUnion": {"shape": 73}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 562, "union");
        atStep(st, 636);
        cqVal(c, st, 74, 305);
        atStep(st, 637);
        cqUnion(c, st, 563, 562, {"toUnion": {"shape": 74}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 563, "union");
        atStep(st, 638);
        cqVal(c, st, 75, 309);
        atStep(st, 639);
        cqUnion(c, st, 564, 563, {"toUnion": {"shape": 75}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 564, "union");
        atStep(st, 640);
        cqVal(c, st, 76, 313);
        atStep(st, 641);
        cqUnion(c, st, 565, 564, {"toUnion": {"shape": 76}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 565, "union");
        atStep(st, 642);
        cqVal(c, st, 77, 317);
        atStep(st, 643);
        cqUnion(c, st, 566, 565, {"toUnion": {"shape": 77}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 566, "union");
        atStep(st, 644);
        cqVal(c, st, 78, 321);
        atStep(st, 645);
        cqUnion(c, st, 567, 566, {"toUnion": {"shape": 78}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 567, "union");
        atStep(st, 646);
        cqVal(c, st, 79, 325);
        atStep(st, 647);
        cqUnion(c, st, 568, 567, {"toUnion": {"shape": 79}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 568, "union");
        atStep(st, 648);
        cqVal(c, st, 80, 329);
        atStep(st, 649);
        cqUnion(c, st, 569, 568, {"toUnion": {"shape": 80}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 569, "union");
        atStep(st, 650);
        cqVal(c, st, 81, 333);
        atStep(st, 651);
        cqUnion(c, st, 570, 569, {"toUnion": {"shape": 81}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 570, "union");
        atStep(st, 652);
        cqVal(c, st, 82, 337);
        atStep(st, 653);
        cqUnion(c, st, 571, 570, {"toUnion": {"shape": 82}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 571, "union");
        atStep(st, 654);
        cqVal(c, st, 83, 341);
        atStep(st, 655);
        cqUnion(c, st, 572, 571, {"toUnion": {"shape": 83}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 572, "union");
        atStep(st, 656);
        cqVal(c, st, 84, 345);
        atStep(st, 657);
        cqUnion(c, st, 573, 572, {"toUnion": {"shape": 84}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 573, "union");
        atStep(st, 658);
        cqVal(c, st, 85, 349);
        atStep(st, 659);
        cqUnion(c, st, 574, 573, {"toUnion": {"shape": 85}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 574, "union");
        atStep(st, 660);
        cqVal(c, st, 86, 353);
        atStep(st, 661);
        cqUnion(c, st, 575, 574, {"toUnion": {"shape": 86}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 575, "union");
        atStep(st, 662);
        cqVal(c, st, 87, 357);
        atStep(st, 663);
        cqUnion(c, st, 576, 575, {"toUnion": {"shape": 87}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 576, "union");
        atStep(st, 664);
        cqVal(c, st, 88, 361);
        atStep(st, 665);
        cqUnion(c, st, 577, 576, {"toUnion": {"shape": 88}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 577, "union");
        atStep(st, 666);
        cqVal(c, st, 89, 365);
        atStep(st, 667);
        cqUnion(c, st, 578, 577, {"toUnion": {"shape": 89}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 578, "union");
        atStep(st, 668);
        cqVal(c, st, 90, 369);
        atStep(st, 669);
        cqUnion(c, st, 579, 578, {"toUnion": {"shape": 90}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 579, "union");
        atStep(st, 670);
        cqVal(c, st, 91, 373);
        atStep(st, 671);
        cqUnion(c, st, 580, 579, {"toUnion": {"shape": 91}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 580, "union");
        atStep(st, 672);
        cqVal(c, st, 92, 377);
        atStep(st, 673);
        cqUnion(c, st, 581, 580, {"toUnion": {"shape": 92}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 581, "union");
        atStep(st, 674);
        cqVal(c, st, 93, 381);
        atStep(st, 675);
        cqUnion(c, st, 582, 581, {"toUnion": {"shape": 93}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 582, "union");
        atStep(st, 676);
        cqVal(c, st, 94, 385);
        atStep(st, 677);
        cqUnion(c, st, 583, 582, {"toUnion": {"shape": 94}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 583, "union");
        atStep(st, 678);
        cqVal(c, st, 95, 389);
        atStep(st, 679);
        cqUnion(c, st, 584, 583, {"toUnion": {"shape": 95}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 584, "union");
        atStep(st, 680);
        cqVal(c, st, 96, 393);
        atStep(st, 681);
        cqUnion(c, st, 585, 584, {"toUnion": {"shape": 96}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 585, "union");
        atStep(st, 682);
        cqVal(c, st, 97, 397);
        atStep(st, 683);
        cqUnion(c, st, 586, 585, {"toUnion": {"shape": 97}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 586, "union");
        atStep(st, 684);
        cqVal(c, st, 98, 401);
        atStep(st, 685);
        cqUnion(c, st, 587, 586, {"toUnion": {"shape": 98}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 587, "union");
        atStep(st, 686);
        cqVal(c, st, 99, 405);
        atStep(st, 687);
        cqUnion(c, st, 588, 587, {"toUnion": {"shape": 99}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 588, "union");
        atStep(st, 688);
        cqVal(c, st, 100, 409);
        atStep(st, 689);
        cqUnion(c, st, 589, 588, {"toUnion": {"shape": 100}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 589, "union");
        atStep(st, 690);
        cqVal(c, st, 101, 413);
        atStep(st, 691);
        cqUnion(c, st, 590, 589, {"toUnion": {"shape": 101}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 590, "union");
        atStep(st, 692);
        cqVal(c, st, 102, 417);
        atStep(st, 693);
        cqUnion(c, st, 591, 590, {"toUnion": {"shape": 102}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 591, "union");
        atStep(st, 694);
        cqVal(c, st, 103, 421);
        atStep(st, 695);
        cqUnion(c, st, 592, 591, {"toUnion": {"shape": 103}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 592, "union");
        atStep(st, 696);
        cqVal(c, st, 104, 425);
        atStep(st, 697);
        cqUnion(c, st, 593, 592, {"toUnion": {"shape": 104}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 593, "union");
        atStep(st, 698);
        cqVal(c, st, 105, 429);
        atStep(st, 699);
        cqUnion(c, st, 594, 593, {"toUnion": {"shape": 105}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 594, "union");
        atStep(st, 700);
        cqVal(c, st, 106, 433);
        atStep(st, 701);
        cqUnion(c, st, 595, 594, {"toUnion": {"shape": 106}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 595, "union");
        atStep(st, 702);
        cqVal(c, st, 107, 437);
        atStep(st, 703);
        cqUnion(c, st, 596, 595, {"toUnion": {"shape": 107}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 596, "union");
        atStep(st, 704);
        cqVal(c, st, 108, 441);
        atStep(st, 705);
        cqUnion(c, st, 597, 596, {"toUnion": {"shape": 108}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 597, "union");
        atStep(st, 706);
        cqVal(c, st, 109, 445);
        atStep(st, 707);
        cqUnion(c, st, 598, 597, {"toUnion": {"shape": 109}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 598, "union");
        atStep(st, 708);
        cqVal(c, st, 110, 449);
        atStep(st, 709);
        cqUnion(c, st, 599, 598, {"toUnion": {"shape": 110}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 599, "union");
        atStep(st, 710);
        cqVal(c, st, 111, 453);
        atStep(st, 711);
        cqUnion(c, st, 600, 599, {"toUnion": {"shape": 111}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 600, "union");
        atStep(st, 712);
        cqVal(c, st, 112, 457);
        atStep(st, 713);
        cqUnion(c, st, 601, 600, {"toUnion": {"shape": 112}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 601, "union");
        atStep(st, 714);
        cqVal(c, st, 113, 461);
        atStep(st, 715);
        cqUnion(c, st, 602, 601, {"toUnion": {"shape": 113}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 602, "union");
        atStep(st, 716);
        cqVal(c, st, 114, 465);
        atStep(st, 717);
        cqUnion(c, st, 603, 602, {"toUnion": {"shape": 114}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 603, "union");
        atStep(st, 718);
        cqVal(c, st, 115, 469);
        atStep(st, 719);
        cqUnion(c, st, 604, 603, {"toUnion": {"shape": 115}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 604, "union");
        atStep(st, 720);
        cqVal(c, st, 116, 473);
        atStep(st, 721);
        cqUnion(c, st, 605, 604, {"toUnion": {"shape": 116}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 605, "union");
        atStep(st, 722);
        cqVal(c, st, 117, 477);
        atStep(st, 723);
        cqUnion(c, st, 606, 605, {"toUnion": {"shape": 117}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 606, "union");
        atStep(st, 724);
        cqVal(c, st, 118, 481);
        atStep(st, 725);
        cqUnion(c, st, 607, 606, {"toUnion": {"shape": 118}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 607, "union");
        atStep(st, 726);
        cqVal(c, st, 119, 485);
        atStep(st, 727);
        cqUnion(c, st, 608, 607, {"toUnion": {"shape": 119}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 608, "union");
        atStep(st, 728);
        cqVal(c, st, 120, 489);
        atStep(st, 729);
        cqUnion(c, st, 609, 608, {"toUnion": {"shape": 120}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 609, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 609}), {"i": 5452});
    } catch (e) {
        return {"i": 5452, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c5452"] = case_5452();
    return out;
}
