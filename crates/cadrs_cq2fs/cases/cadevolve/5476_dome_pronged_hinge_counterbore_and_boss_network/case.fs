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

const case_5476 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqSphere(c, st, 2, 1, {"radius": 40, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "sphere");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqRect(c, st, 4, 3, {"xLen": 80, "yLen": 80, "centered": true, "forConstruction": false});
        atStep(st, 5);
        cqExtrude(c, st, 5, 4, {"until": -40, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 5, "extrude");
        atStep(st, 6);
        cqCut(c, st, 6, 2, {"toCut": {"wp": 5}, "clean": true, "tol": undefined});
        stepVol(c, st, 6, "cut");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 8);
        cqSphere(c, st, 8, 7, {"radius": 38, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 8, "sphere");
        atStep(st, 9);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 10);
        cqRect(c, st, 10, 9, {"xLen": 76, "yLen": 76, "centered": true, "forConstruction": false});
        atStep(st, 11);
        cqExtrude(c, st, 11, 10, {"until": -38, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 11, "extrude");
        atStep(st, 12);
        cqCut(c, st, 12, 8, {"toCut": {"wp": 11}, "clean": true, "tol": undefined});
        stepVol(c, st, 12, "cut");
        atStep(st, 13);
        cqCut(c, st, 13, 6, {"toCut": {"wp": 12}, "clean": true, "tol": undefined});
        stepVol(c, st, 13, "cut");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 14, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 15);
        cqWorkplane(c, st, 15, 14, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 16);
        cqRect(c, st, 16, 15, {"xLen": 80, "yLen": 80, "centered": true, "forConstruction": false});
        atStep(st, 17);
        cqExtrude(c, st, 17, 16, {"until": -40, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 17, "extrude");
        atStep(st, 18);
        cqCut(c, st, 18, 13, {"toCut": {"wp": 17}, "clean": true, "tol": undefined});
        stepVol(c, st, 18, "cut");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 20);
        cqMoveTo(c, st, 20, 19, {"x": 39.68626966596886, "y": 5});
        atStep(st, 21);
        cqLineTo(c, st, 21, 20, {"x": 36.68626966596886, "y": 5, "forConstruction": false});
        atStep(st, 22);
        cqLineTo(c, st, 22, 21, {"x": 36.68626966596886, "y": 8, "forConstruction": false});
        atStep(st, 23);
        cqLineTo(c, st, 23, 22, {"x": 39.68626966596886, "y": 8, "forConstruction": false});
        atStep(st, 24);
        cqClose(c, st, 24, 23, {});
        atStep(st, 25);
        cqExtrude(c, st, 25, 24, {"until": 3, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 25, "extrude");
        atStep(st, 26);
        cqRotate(c, st, 26, 25, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 0});
        stepVol(c, st, 26, "rotate");
        atStep(st, 27);
        cqUnion(c, st, 27, 18, {"toUnion": {"wp": 26}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 27, "union");
        atStep(st, 28);
        cqWorkplaneNew(c, st, 28, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 29);
        cqMoveTo(c, st, 29, 28, {"x": 38.18626966596886, "y": 6.5});
        atStep(st, 30);
        cqCircle(c, st, 30, 29, {"radius": 1.5, "forConstruction": false});
        atStep(st, 31);
        cqExtrude(c, st, 31, 30, {"until": 12, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 31, "extrude");
        atStep(st, 32);
        cqRotate(c, st, 32, 31, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 0});
        stepVol(c, st, 32, "rotate");
        atStep(st, 33);
        cqCut(c, st, 33, 27, {"toCut": {"wp": 32}, "clean": true, "tol": undefined});
        stepVol(c, st, 33, "cut");
        atStep(st, 34);
        cqWorkplaneNew(c, st, 34, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 35);
        cqMoveTo(c, st, 35, 34, {"x": 39.68626966596886, "y": 5});
        atStep(st, 36);
        cqLineTo(c, st, 36, 35, {"x": 36.68626966596886, "y": 5, "forConstruction": false});
        atStep(st, 37);
        cqLineTo(c, st, 37, 36, {"x": 36.68626966596886, "y": 8, "forConstruction": false});
        atStep(st, 38);
        cqLineTo(c, st, 38, 37, {"x": 39.68626966596886, "y": 8, "forConstruction": false});
        atStep(st, 39);
        cqClose(c, st, 39, 38, {});
        atStep(st, 40);
        cqExtrude(c, st, 40, 39, {"until": 3, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 40, "extrude");
        atStep(st, 41);
        cqRotate(c, st, 41, 40, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 90});
        stepVol(c, st, 41, "rotate");
        atStep(st, 42);
        cqUnion(c, st, 42, 33, {"toUnion": {"wp": 41}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 42, "union");
        atStep(st, 43);
        cqWorkplaneNew(c, st, 43, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 44);
        cqMoveTo(c, st, 44, 43, {"x": 38.18626966596886, "y": 6.5});
        atStep(st, 45);
        cqCircle(c, st, 45, 44, {"radius": 1.5, "forConstruction": false});
        atStep(st, 46);
        cqExtrude(c, st, 46, 45, {"until": 12, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 46, "extrude");
        atStep(st, 47);
        cqRotate(c, st, 47, 46, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 90});
        stepVol(c, st, 47, "rotate");
        atStep(st, 48);
        cqCut(c, st, 48, 42, {"toCut": {"wp": 47}, "clean": true, "tol": undefined});
        stepVol(c, st, 48, "cut");
        atStep(st, 49);
        cqWorkplaneNew(c, st, 49, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 50);
        cqMoveTo(c, st, 50, 49, {"x": 39.68626966596886, "y": 5});
        atStep(st, 51);
        cqLineTo(c, st, 51, 50, {"x": 36.68626966596886, "y": 5, "forConstruction": false});
        atStep(st, 52);
        cqLineTo(c, st, 52, 51, {"x": 36.68626966596886, "y": 8, "forConstruction": false});
        atStep(st, 53);
        cqLineTo(c, st, 53, 52, {"x": 39.68626966596886, "y": 8, "forConstruction": false});
        atStep(st, 54);
        cqClose(c, st, 54, 53, {});
        atStep(st, 55);
        cqExtrude(c, st, 55, 54, {"until": 3, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 55, "extrude");
        atStep(st, 56);
        cqRotate(c, st, 56, 55, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 180});
        stepVol(c, st, 56, "rotate");
        atStep(st, 57);
        cqUnion(c, st, 57, 48, {"toUnion": {"wp": 56}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 57, "union");
        atStep(st, 58);
        cqWorkplaneNew(c, st, 58, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 59);
        cqMoveTo(c, st, 59, 58, {"x": 38.18626966596886, "y": 6.5});
        atStep(st, 60);
        cqCircle(c, st, 60, 59, {"radius": 1.5, "forConstruction": false});
        atStep(st, 61);
        cqExtrude(c, st, 61, 60, {"until": 12, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 61, "extrude");
        atStep(st, 62);
        cqRotate(c, st, 62, 61, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 180});
        stepVol(c, st, 62, "rotate");
        atStep(st, 63);
        cqCut(c, st, 63, 57, {"toCut": {"wp": 62}, "clean": true, "tol": undefined});
        stepVol(c, st, 63, "cut");
        atStep(st, 64);
        cqWorkplaneNew(c, st, 64, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 65);
        cqMoveTo(c, st, 65, 64, {"x": 39.68626966596886, "y": 5});
        atStep(st, 66);
        cqLineTo(c, st, 66, 65, {"x": 36.68626966596886, "y": 5, "forConstruction": false});
        atStep(st, 67);
        cqLineTo(c, st, 67, 66, {"x": 36.68626966596886, "y": 8, "forConstruction": false});
        atStep(st, 68);
        cqLineTo(c, st, 68, 67, {"x": 39.68626966596886, "y": 8, "forConstruction": false});
        atStep(st, 69);
        cqClose(c, st, 69, 68, {});
        atStep(st, 70);
        cqExtrude(c, st, 70, 69, {"until": 3, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 70, "extrude");
        atStep(st, 71);
        cqRotate(c, st, 71, 70, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 270});
        stepVol(c, st, 71, "rotate");
        atStep(st, 72);
        cqUnion(c, st, 72, 63, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 74);
        cqMoveTo(c, st, 74, 73, {"x": 38.18626966596886, "y": 6.5});
        atStep(st, 75);
        cqCircle(c, st, 75, 74, {"radius": 1.5, "forConstruction": false});
        atStep(st, 76);
        cqExtrude(c, st, 76, 75, {"until": 12, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 76, "extrude");
        atStep(st, 77);
        cqRotate(c, st, 77, 76, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 270});
        stepVol(c, st, 77, "rotate");
        atStep(st, 78);
        cqCut(c, st, 78, 72, {"toCut": {"wp": 77}, "clean": true, "tol": undefined});
        stepVol(c, st, 78, "cut");
        atStep(st, 79);
        cqWorkplaneNew(c, st, 79, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 80);
        cqWorkplane(c, st, 80, 79, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 81);
        cqCenter(c, st, 81, 80, {"x": 39.68626966596886, "y": 0});
        atStep(st, 82);
        cqCircle(c, st, 82, 81, {"radius": 2.5, "forConstruction": false});
        atStep(st, 83);
        cqExtrude(c, st, 83, 82, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 83, "extrude");
        atStep(st, 84);
        cqCut(c, st, 84, 78, {"toCut": {"wp": 83}, "clean": true, "tol": undefined});
        stepVol(c, st, 84, "cut");
        atStep(st, 85);
        cqWorkplaneNew(c, st, 85, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 86);
        cqWorkplane(c, st, 86, 85, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 87);
        cqCenter(c, st, 87, 86, {"x": 28.06243040080456, "y": 28.06243040080456});
        atStep(st, 88);
        cqCircle(c, st, 88, 87, {"radius": 2.5, "forConstruction": false});
        atStep(st, 89);
        cqExtrude(c, st, 89, 88, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 89, "extrude");
        atStep(st, 90);
        cqCut(c, st, 90, 84, {"toCut": {"wp": 89}, "clean": true, "tol": undefined});
        stepVol(c, st, 90, "cut");
        atStep(st, 91);
        cqWorkplaneNew(c, st, 91, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 92);
        cqWorkplane(c, st, 92, 91, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 93);
        cqCenter(c, st, 93, 92, {"x": 2.430083155826373e-15, "y": 39.68626966596886});
        atStep(st, 94);
        cqCircle(c, st, 94, 93, {"radius": 2.5, "forConstruction": false});
        atStep(st, 95);
        cqExtrude(c, st, 95, 94, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 95, "extrude");
        atStep(st, 96);
        cqCut(c, st, 96, 90, {"toCut": {"wp": 95}, "clean": true, "tol": undefined});
        stepVol(c, st, 96, "cut");
        atStep(st, 97);
        cqWorkplaneNew(c, st, 97, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 98);
        cqWorkplane(c, st, 98, 97, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 99);
        cqCenter(c, st, 99, 98, {"x": -28.06243040080456, "y": 28.06243040080456});
        atStep(st, 100);
        cqCircle(c, st, 100, 99, {"radius": 2.5, "forConstruction": false});
        atStep(st, 101);
        cqExtrude(c, st, 101, 100, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 101, "extrude");
        atStep(st, 102);
        cqCut(c, st, 102, 96, {"toCut": {"wp": 101}, "clean": true, "tol": undefined});
        stepVol(c, st, 102, "cut");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 104);
        cqWorkplane(c, st, 104, 103, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 105);
        cqCenter(c, st, 105, 104, {"x": -39.68626966596886, "y": 4.860166311652746e-15});
        atStep(st, 106);
        cqCircle(c, st, 106, 105, {"radius": 2.5, "forConstruction": false});
        atStep(st, 107);
        cqExtrude(c, st, 107, 106, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 107, "extrude");
        atStep(st, 108);
        cqCut(c, st, 108, 102, {"toCut": {"wp": 107}, "clean": true, "tol": undefined});
        stepVol(c, st, 108, "cut");
        atStep(st, 109);
        cqWorkplaneNew(c, st, 109, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 110);
        cqWorkplane(c, st, 110, 109, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 111);
        cqCenter(c, st, 111, 110, {"x": -28.06243040080457, "y": -28.06243040080456});
        atStep(st, 112);
        cqCircle(c, st, 112, 111, {"radius": 2.5, "forConstruction": false});
        atStep(st, 113);
        cqExtrude(c, st, 113, 112, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 113, "extrude");
        atStep(st, 114);
        cqCut(c, st, 114, 108, {"toCut": {"wp": 113}, "clean": true, "tol": undefined});
        stepVol(c, st, 114, "cut");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 115, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 116);
        cqWorkplane(c, st, 116, 115, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 117);
        cqCenter(c, st, 117, 116, {"x": -7.290249467479118e-15, "y": -39.68626966596886});
        atStep(st, 118);
        cqCircle(c, st, 118, 117, {"radius": 2.5, "forConstruction": false});
        atStep(st, 119);
        cqExtrude(c, st, 119, 118, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 119, "extrude");
        atStep(st, 120);
        cqCut(c, st, 120, 114, {"toCut": {"wp": 119}, "clean": true, "tol": undefined});
        stepVol(c, st, 120, "cut");
        atStep(st, 121);
        cqWorkplaneNew(c, st, 121, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 122);
        cqWorkplane(c, st, 122, 121, {"offset": 5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 123);
        cqCenter(c, st, 123, 122, {"x": 28.06243040080455, "y": -28.06243040080457});
        atStep(st, 124);
        cqCircle(c, st, 124, 123, {"radius": 2.5, "forConstruction": false});
        atStep(st, 125);
        cqExtrude(c, st, 125, 124, {"until": 1, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 125, "extrude");
        atStep(st, 126);
        cqCut(c, st, 126, 120, {"toCut": {"wp": 125}, "clean": true, "tol": undefined});
        stepVol(c, st, 126, "cut");
        atStep(st, 127);
        cqWorkplaneNew(c, st, 127, {"plane": {"origin": [-10, -34.64101615137754, 12.000000000000009], "xDir": [-1.3877787807814457e-17, -0.3273268353539888, -0.944911182523068], "zDir": [-0.2631578947368421, -0.911605688194146, 0.3157894736842108]}, "obj": undefined});
        atStep(st, 128);
        cqCircle(c, st, 128, 127, {"radius": 4, "forConstruction": false});
        atStep(st, 129);
        cqExtrude(c, st, 129, 128, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 129, "extrude");
        // step 130: guard_begin {"before":{"wp":129},"block":1,"g":1,"op":"guard_begin","skippable":true}
        atStep(st, 131);
        cqSelect("Edges")(c, st, 130, 129, {"selector": undefined, "tag": undefined});
        atStep(st, 132);
        cqFillet(c, st, 131, 130, {"radius": 0.4});
        stepVol(c, st, 131, "fillet");
        // step 133: guard_end {"after":{"wp":131},"block":1,"g":1,"op":"guard_end"}
        atStep(st, 134);
        cqUnion(c, st, 132, 126, {"toUnion": {"wp": 131}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 132, "union");
        atStep(st, 135);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [0, -34.64101615137754, 15.620499351813317], "xDir": [0, -0.4110657724161399, -0.911605688194146], "zDir": [0, -0.911605688194146, 0.4110657724161399]}, "obj": undefined});
        atStep(st, 136);
        cqCircle(c, st, 134, 133, {"radius": 4, "forConstruction": false});
        atStep(st, 137);
        cqExtrude(c, st, 135, 134, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 135, "extrude");
        // step 138: guard_begin {"before":{"wp":135},"block":1,"g":2,"op":"guard_begin","skippable":true}
        atStep(st, 139);
        cqSelect("Edges")(c, st, 136, 135, {"selector": undefined, "tag": undefined});
        atStep(st, 140);
        cqFillet(c, st, 137, 136, {"radius": 0.4});
        stepVol(c, st, 137, "fillet");
        // step 141: guard_end {"after":{"wp":137},"block":1,"g":2,"op":"guard_end"}
        atStep(st, 142);
        cqUnion(c, st, 138, 132, {"toUnion": {"wp": 137}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 138, "union");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 139, {"plane": {"origin": [10, -34.64101615137754, 12.000000000000009], "xDir": [1.3877787807814457e-17, -0.3273268353539888, -0.944911182523068], "zDir": [0.2631578947368421, -0.911605688194146, 0.3157894736842108]}, "obj": undefined});
        atStep(st, 144);
        cqCircle(c, st, 140, 139, {"radius": 4, "forConstruction": false});
        atStep(st, 145);
        cqExtrude(c, st, 141, 140, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 141, "extrude");
        // step 146: guard_begin {"before":{"wp":141},"block":1,"g":3,"op":"guard_begin","skippable":true}
        atStep(st, 147);
        cqSelect("Edges")(c, st, 142, 141, {"selector": undefined, "tag": undefined});
        atStep(st, 148);
        cqFillet(c, st, 143, 142, {"radius": 0.4});
        stepVol(c, st, 143, "fillet");
        // step 149: guard_end {"after":{"wp":143},"block":1,"g":3,"op":"guard_end"}
        atStep(st, 150);
        cqUnion(c, st, 144, 138, {"toUnion": {"wp": 143}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 144, "union");
        atStep(st, 151);
        cqWorkplaneNew(c, st, 145, {"plane": {"origin": [-25, -25.980762113533157, 12.000000000000004], "xDir": [-0.7205766921228921, 0.6933752452815365, 0], "zDir": [-0.6578947368421053, -0.6837042661456094, 0.3157894736842107]}, "obj": undefined});
        atStep(st, 152);
        cqCircle(c, st, 146, 145, {"radius": 4, "forConstruction": false});
        atStep(st, 153);
        cqExtrude(c, st, 147, 146, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 147, "extrude");
        // step 154: guard_begin {"before":{"wp":147},"block":1,"g":4,"op":"guard_begin","skippable":true}
        atStep(st, 155);
        cqSelect("Edges")(c, st, 148, 147, {"selector": undefined, "tag": undefined});
        atStep(st, 156);
        cqFillet(c, st, 149, 148, {"radius": 0.4});
        stepVol(c, st, 149, "fillet");
        // step 157: guard_end {"after":{"wp":149},"block":1,"g":4,"op":"guard_end"}
        atStep(st, 158);
        cqUnion(c, st, 150, 144, {"toUnion": {"wp": 149}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 150, "union");
        atStep(st, 159);
        cqWorkplaneNew(c, st, 151, {"plane": {"origin": [-15, -25.980762113533157, 23.323807579381203], "xDir": [-2.7755575615628914e-17, -0.6680325084317194, -0.7441320902087377], "zDir": [-0.3947368421052632, -0.6837042661456094, 0.6137844099837159]}, "obj": undefined});
        atStep(st, 160);
        cqCircle(c, st, 152, 151, {"radius": 4, "forConstruction": false});
        atStep(st, 161);
        cqExtrude(c, st, 153, 152, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 153, "extrude");
        // step 162: guard_begin {"before":{"wp":153},"block":1,"g":5,"op":"guard_begin","skippable":true}
        atStep(st, 163);
        cqSelect("Edges")(c, st, 154, 153, {"selector": undefined, "tag": undefined});
        atStep(st, 164);
        cqFillet(c, st, 155, 154, {"radius": 0.4});
        stepVol(c, st, 155, "fillet");
        // step 165: guard_end {"after":{"wp":155},"block":1,"g":5,"op":"guard_end"}
        atStep(st, 166);
        cqUnion(c, st, 156, 150, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        atStep(st, 167);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [-5, -25.980762113533157, 27.27636339397171], "xDir": [0, 0.7240945355448554, 0.6897007348075543], "zDir": [-0.13157894736842105, -0.6837042661456094, 0.7177990366834661]}, "obj": undefined});
        atStep(st, 168);
        cqCircle(c, st, 158, 157, {"radius": 4, "forConstruction": false});
        atStep(st, 169);
        cqExtrude(c, st, 159, 158, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 159, "extrude");
        // step 170: guard_begin {"before":{"wp":159},"block":1,"g":6,"op":"guard_begin","skippable":true}
        atStep(st, 171);
        cqSelect("Edges")(c, st, 160, 159, {"selector": undefined, "tag": undefined});
        atStep(st, 172);
        cqFillet(c, st, 161, 160, {"radius": 0.4});
        stepVol(c, st, 161, "fillet");
        // step 173: guard_end {"after":{"wp":161},"block":1,"g":6,"op":"guard_end"}
        atStep(st, 174);
        cqUnion(c, st, 162, 156, {"toUnion": {"wp": 161}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 162, "union");
        atStep(st, 175);
        cqWorkplaneNew(c, st, 163, {"plane": {"origin": [5, -25.980762113533157, 27.27636339397171], "xDir": [0, 0.7240945355448554, 0.6897007348075543], "zDir": [0.13157894736842105, -0.6837042661456094, 0.7177990366834661]}, "obj": undefined});
        atStep(st, 176);
        cqCircle(c, st, 164, 163, {"radius": 4, "forConstruction": false});
        atStep(st, 177);
        cqExtrude(c, st, 165, 164, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 165, "extrude");
        // step 178: guard_begin {"before":{"wp":165},"block":1,"g":7,"op":"guard_begin","skippable":true}
        atStep(st, 179);
        cqSelect("Edges")(c, st, 166, 165, {"selector": undefined, "tag": undefined});
        atStep(st, 180);
        cqFillet(c, st, 167, 166, {"radius": 0.4});
        stepVol(c, st, 167, "fillet");
        // step 181: guard_end {"after":{"wp":167},"block":1,"g":7,"op":"guard_end"}
        atStep(st, 182);
        cqUnion(c, st, 168, 162, {"toUnion": {"wp": 167}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 168, "union");
        atStep(st, 183);
        cqWorkplaneNew(c, st, 169, {"plane": {"origin": [15, -25.980762113533157, 23.323807579381203], "xDir": [2.7755575615628914e-17, -0.6680325084317194, -0.7441320902087377], "zDir": [0.3947368421052632, -0.6837042661456094, 0.6137844099837159]}, "obj": undefined});
        atStep(st, 184);
        cqCircle(c, st, 170, 169, {"radius": 4, "forConstruction": false});
        atStep(st, 185);
        cqExtrude(c, st, 171, 170, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 171, "extrude");
        // step 186: guard_begin {"before":{"wp":171},"block":1,"g":8,"op":"guard_begin","skippable":true}
        atStep(st, 187);
        cqSelect("Edges")(c, st, 172, 171, {"selector": undefined, "tag": undefined});
        atStep(st, 188);
        cqFillet(c, st, 173, 172, {"radius": 0.4});
        stepVol(c, st, 173, "fillet");
        // step 189: guard_end {"after":{"wp":173},"block":1,"g":8,"op":"guard_end"}
        atStep(st, 190);
        cqUnion(c, st, 174, 168, {"toUnion": {"wp": 173}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 174, "union");
        atStep(st, 191);
        cqWorkplaneNew(c, st, 175, {"plane": {"origin": [25, -25.980762113533157, 12.000000000000004], "xDir": [-0.7205766921228921, -0.6933752452815365, 0], "zDir": [0.6578947368421053, -0.6837042661456094, 0.3157894736842107]}, "obj": undefined});
        atStep(st, 192);
        cqCircle(c, st, 176, 175, {"radius": 4, "forConstruction": false});
        atStep(st, 193);
        cqExtrude(c, st, 177, 176, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 177, "extrude");
        // step 194: guard_begin {"before":{"wp":177},"block":1,"g":9,"op":"guard_begin","skippable":true}
        atStep(st, 195);
        cqSelect("Edges")(c, st, 178, 177, {"selector": undefined, "tag": undefined});
        atStep(st, 196);
        cqFillet(c, st, 179, 178, {"radius": 0.4});
        stepVol(c, st, 179, "fillet");
        // step 197: guard_end {"after":{"wp":179},"block":1,"g":9,"op":"guard_end"}
        atStep(st, 198);
        cqUnion(c, st, 180, 174, {"toUnion": {"wp": 179}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 180, "union");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 181, {"plane": {"origin": [-30, -17.32050807568877, 15.62049935181331], "xDir": [0.4999999999999999, -0.8660254037844386, -2.7755575615628907e-17], "zDir": [-0.7894736842105263, -0.45580284409707295, 0.4110657724161398]}, "obj": undefined});
        atStep(st, 200);
        cqCircle(c, st, 182, 181, {"radius": 4, "forConstruction": false});
        atStep(st, 201);
        cqExtrude(c, st, 183, 182, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 183, "extrude");
        // step 202: guard_begin {"before":{"wp":183},"block":1,"g":10,"op":"guard_begin","skippable":true}
        atStep(st, 203);
        cqSelect("Edges")(c, st, 184, 183, {"selector": undefined, "tag": undefined});
        atStep(st, 204);
        cqFillet(c, st, 185, 184, {"radius": 0.4});
        stepVol(c, st, 185, "fillet");
        // step 205: guard_end {"after":{"wp":185},"block":1,"g":10,"op":"guard_end"}
        atStep(st, 206);
        cqUnion(c, st, 186, 180, {"toUnion": {"wp": 185}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 186, "union");
        atStep(st, 207);
        cqWorkplaneNew(c, st, 187, {"plane": {"origin": [-20, -17.32050807568877, 27.27636339397171], "xDir": [0.8064425896179158, -2.7755575615628907e-17, 0.5913123959890827], "zDir": [-0.5263157894736843, -0.455802844097073, 0.7177990366834661]}, "obj": undefined});
        atStep(st, 208);
        cqCircle(c, st, 188, 187, {"radius": 4, "forConstruction": false});
        atStep(st, 209);
        cqExtrude(c, st, 189, 188, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 189, "extrude");
        // step 210: guard_begin {"before":{"wp":189},"block":1,"g":11,"op":"guard_begin","skippable":true}
        atStep(st, 211);
        cqSelect("Edges")(c, st, 190, 189, {"selector": undefined, "tag": undefined});
        atStep(st, 212);
        cqFillet(c, st, 191, 190, {"radius": 0.4});
        stepVol(c, st, 191, "fillet");
        // step 213: guard_end {"after":{"wp":191},"block":1,"g":11,"op":"guard_end"}
        atStep(st, 214);
        cqUnion(c, st, 192, 186, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        atStep(st, 215);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [-10, -17.32050807568877, 32.31098884280702], "xDir": [0, 0.8813544770895048, 0.472455591261534], "zDir": [-0.26315789473684215, -0.455802844097073, 0.8502891800738691]}, "obj": undefined});
        atStep(st, 216);
        cqCircle(c, st, 194, 193, {"radius": 4, "forConstruction": false});
        atStep(st, 217);
        cqExtrude(c, st, 195, 194, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 195, "extrude");
        // step 218: guard_begin {"before":{"wp":195},"block":1,"g":12,"op":"guard_begin","skippable":true}
        atStep(st, 219);
        cqSelect("Edges")(c, st, 196, 195, {"selector": undefined, "tag": undefined});
        atStep(st, 220);
        cqFillet(c, st, 197, 196, {"radius": 0.4});
        stepVol(c, st, 197, "fillet");
        // step 221: guard_end {"after":{"wp":197},"block":1,"g":12,"op":"guard_end"}
        atStep(st, 222);
        cqUnion(c, st, 198, 192, {"toUnion": {"wp": 197}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 198, "union");
        atStep(st, 223);
        cqWorkplaneNew(c, st, 199, {"plane": {"origin": [0, -17.32050807568877, 33.823069050575526], "xDir": [0, 0.8900807644888297, 0.45580284409707295], "zDir": [0, -0.45580284409707295, 0.8900807644888297]}, "obj": undefined});
        atStep(st, 224);
        cqCircle(c, st, 200, 199, {"radius": 4, "forConstruction": false});
        atStep(st, 225);
        cqExtrude(c, st, 201, 200, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 201, "extrude");
        // step 226: guard_begin {"before":{"wp":201},"block":1,"g":13,"op":"guard_begin","skippable":true}
        atStep(st, 227);
        cqSelect("Edges")(c, st, 202, 201, {"selector": undefined, "tag": undefined});
        atStep(st, 228);
        cqFillet(c, st, 203, 202, {"radius": 0.4});
        stepVol(c, st, 203, "fillet");
        // step 229: guard_end {"after":{"wp":203},"block":1,"g":13,"op":"guard_end"}
        atStep(st, 230);
        cqUnion(c, st, 204, 198, {"toUnion": {"wp": 203}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 204, "union");
        atStep(st, 231);
        cqWorkplaneNew(c, st, 205, {"plane": {"origin": [10, -17.32050807568877, 32.31098884280702], "xDir": [0, 0.8813544770895048, 0.472455591261534], "zDir": [0.26315789473684215, -0.455802844097073, 0.8502891800738691]}, "obj": undefined});
        atStep(st, 232);
        cqCircle(c, st, 206, 205, {"radius": 4, "forConstruction": false});
        atStep(st, 233);
        cqExtrude(c, st, 207, 206, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 207, "extrude");
        // step 234: guard_begin {"before":{"wp":207},"block":1,"g":14,"op":"guard_begin","skippable":true}
        atStep(st, 235);
        cqSelect("Edges")(c, st, 208, 207, {"selector": undefined, "tag": undefined});
        atStep(st, 236);
        cqFillet(c, st, 209, 208, {"radius": 0.4});
        stepVol(c, st, 209, "fillet");
        // step 237: guard_end {"after":{"wp":209},"block":1,"g":14,"op":"guard_end"}
        atStep(st, 238);
        cqUnion(c, st, 210, 204, {"toUnion": {"wp": 209}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 210, "union");
        atStep(st, 239);
        cqWorkplaneNew(c, st, 211, {"plane": {"origin": [20, -17.32050807568877, 27.27636339397171], "xDir": [0.8064425896179158, 2.7755575615628907e-17, -0.5913123959890827], "zDir": [0.5263157894736843, -0.455802844097073, 0.7177990366834661]}, "obj": undefined});
        atStep(st, 240);
        cqCircle(c, st, 212, 211, {"radius": 4, "forConstruction": false});
        atStep(st, 241);
        cqExtrude(c, st, 213, 212, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 213, "extrude");
        // step 242: guard_begin {"before":{"wp":213},"block":1,"g":15,"op":"guard_begin","skippable":true}
        atStep(st, 243);
        cqSelect("Edges")(c, st, 214, 213, {"selector": undefined, "tag": undefined});
        atStep(st, 244);
        cqFillet(c, st, 215, 214, {"radius": 0.4});
        stepVol(c, st, 215, "fillet");
        // step 245: guard_end {"after":{"wp":215},"block":1,"g":15,"op":"guard_end"}
        atStep(st, 246);
        cqUnion(c, st, 216, 210, {"toUnion": {"wp": 215}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 216, "union");
        atStep(st, 247);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [30, -17.32050807568877, 15.62049935181331], "xDir": [0.4999999999999999, 0.8660254037844386, 2.7755575615628907e-17], "zDir": [0.7894736842105263, -0.45580284409707295, 0.4110657724161398]}, "obj": undefined});
        atStep(st, 248);
        cqCircle(c, st, 218, 217, {"radius": 4, "forConstruction": false});
        atStep(st, 249);
        cqExtrude(c, st, 219, 218, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 219, "extrude");
        // step 250: guard_begin {"before":{"wp":219},"block":1,"g":16,"op":"guard_begin","skippable":true}
        atStep(st, 251);
        cqSelect("Edges")(c, st, 220, 219, {"selector": undefined, "tag": undefined});
        atStep(st, 252);
        cqFillet(c, st, 221, 220, {"radius": 0.4});
        stepVol(c, st, 221, "fillet");
        // step 253: guard_end {"after":{"wp":221},"block":1,"g":16,"op":"guard_end"}
        atStep(st, 254);
        cqUnion(c, st, 222, 216, {"toUnion": {"wp": 221}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 222, "union");
        atStep(st, 255);
        cqWorkplaneNew(c, st, 223, {"plane": {"origin": [-35, -8.660254037844386, 12], "xDir": [-0.32432432432432434, 0, -0.945945945945946], "zDir": [-0.9210526315789472, -0.22790142204853647, 0.3157894736842105]}, "obj": undefined});
        atStep(st, 256);
        cqCircle(c, st, 224, 223, {"radius": 4, "forConstruction": false});
        atStep(st, 257);
        cqExtrude(c, st, 225, 224, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 225, "extrude");
        // step 258: guard_begin {"before":{"wp":225},"block":1,"g":17,"op":"guard_begin","skippable":true}
        atStep(st, 259);
        cqSelect("Edges")(c, st, 226, 225, {"selector": undefined, "tag": undefined});
        atStep(st, 260);
        cqFillet(c, st, 227, 226, {"radius": 0.4});
        stepVol(c, st, 227, "fillet");
        // step 261: guard_end {"after":{"wp":227},"block":1,"g":17,"op":"guard_end"}
        atStep(st, 262);
        cqUnion(c, st, 228, 222, {"toUnion": {"wp": 227}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 228, "union");
        atStep(st, 263);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [-25, -8.660254037844386, 27.27636339397171], "xDir": [0.737199010647884, 0, 0.6756756756756758], "zDir": [-0.6578947368421053, -0.22790142204853647, 0.717799036683466]}, "obj": undefined});
        atStep(st, 264);
        cqCircle(c, st, 230, 229, {"radius": 4, "forConstruction": false});
        atStep(st, 265);
        cqExtrude(c, st, 231, 230, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 231, "extrude");
        // step 266: guard_begin {"before":{"wp":231},"block":1,"g":18,"op":"guard_begin","skippable":true}
        atStep(st, 267);
        cqSelect("Edges")(c, st, 232, 231, {"selector": undefined, "tag": undefined});
        atStep(st, 268);
        cqFillet(c, st, 233, 232, {"radius": 0.4});
        stepVol(c, st, 233, "fillet");
        // step 269: guard_end {"after":{"wp":233},"block":1,"g":18,"op":"guard_end"}
        atStep(st, 270);
        cqUnion(c, st, 234, 228, {"toUnion": {"wp": 233}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 234, "union");
        atStep(st, 271);
        cqWorkplaneNew(c, st, 235, {"plane": {"origin": [-15, -8.660254037844386, 33.823069050575526], "xDir": [0.9141370013669062, -1.3877787807814457e-17, 0.4054054054054054], "zDir": [-0.3947368421052632, -0.22790142204853647, 0.8900807644888297]}, "obj": undefined});
        atStep(st, 272);
        cqCircle(c, st, 236, 235, {"radius": 4, "forConstruction": false});
        atStep(st, 273);
        cqExtrude(c, st, 237, 236, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 237, "extrude");
        // step 274: guard_begin {"before":{"wp":237},"block":1,"g":19,"op":"guard_begin","skippable":true}
        atStep(st, 275);
        cqSelect("Edges")(c, st, 238, 237, {"selector": undefined, "tag": undefined});
        atStep(st, 276);
        cqFillet(c, st, 239, 238, {"radius": 0.4});
        stepVol(c, st, 239, "fillet");
        // step 277: guard_end {"after":{"wp":239},"block":1,"g":19,"op":"guard_end"}
        atStep(st, 278);
        cqUnion(c, st, 240, 234, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        atStep(st, 279);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [-5, -8.660254037844386, 36.66060555964672], "xDir": [3.469446951953614e-18, 0.9732141991249592, 0.22990024493585143], "zDir": [-0.13157894736842105, -0.22790142204853647, 0.96475277788544]}, "obj": undefined});
        atStep(st, 280);
        cqCircle(c, st, 242, 241, {"radius": 4, "forConstruction": false});
        atStep(st, 281);
        cqExtrude(c, st, 243, 242, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 243, "extrude");
        // step 282: guard_begin {"before":{"wp":243},"block":1,"g":20,"op":"guard_begin","skippable":true}
        atStep(st, 283);
        cqSelect("Edges")(c, st, 244, 243, {"selector": undefined, "tag": undefined});
        atStep(st, 284);
        cqFillet(c, st, 245, 244, {"radius": 0.4});
        stepVol(c, st, 245, "fillet");
        // step 285: guard_end {"after":{"wp":245},"block":1,"g":20,"op":"guard_end"}
        atStep(st, 286);
        cqUnion(c, st, 246, 240, {"toUnion": {"wp": 245}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 246, "union");
        atStep(st, 287);
        cqWorkplaneNew(c, st, 247, {"plane": {"origin": [5, -8.660254037844386, 36.66060555964672], "xDir": [-3.469446951953614e-18, 0.9732141991249592, 0.22990024493585143], "zDir": [0.13157894736842105, -0.22790142204853647, 0.96475277788544]}, "obj": undefined});
        atStep(st, 288);
        cqCircle(c, st, 248, 247, {"radius": 4, "forConstruction": false});
        atStep(st, 289);
        cqExtrude(c, st, 249, 248, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 249, "extrude");
        // step 290: guard_begin {"before":{"wp":249},"block":1,"g":21,"op":"guard_begin","skippable":true}
        atStep(st, 291);
        cqSelect("Edges")(c, st, 250, 249, {"selector": undefined, "tag": undefined});
        atStep(st, 292);
        cqFillet(c, st, 251, 250, {"radius": 0.4});
        stepVol(c, st, 251, "fillet");
        // step 293: guard_end {"after":{"wp":251},"block":1,"g":21,"op":"guard_end"}
        atStep(st, 294);
        cqUnion(c, st, 252, 246, {"toUnion": {"wp": 251}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 252, "union");
        atStep(st, 295);
        cqWorkplaneNew(c, st, 253, {"plane": {"origin": [15, -8.660254037844386, 33.823069050575526], "xDir": [0.9141370013669062, 1.3877787807814457e-17, -0.4054054054054054], "zDir": [0.3947368421052632, -0.22790142204853647, 0.8900807644888297]}, "obj": undefined});
        atStep(st, 296);
        cqCircle(c, st, 254, 253, {"radius": 4, "forConstruction": false});
        atStep(st, 297);
        cqExtrude(c, st, 255, 254, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 255, "extrude");
        // step 298: guard_begin {"before":{"wp":255},"block":1,"g":22,"op":"guard_begin","skippable":true}
        atStep(st, 299);
        cqSelect("Edges")(c, st, 256, 255, {"selector": undefined, "tag": undefined});
        atStep(st, 300);
        cqFillet(c, st, 257, 256, {"radius": 0.4});
        stepVol(c, st, 257, "fillet");
        // step 301: guard_end {"after":{"wp":257},"block":1,"g":22,"op":"guard_end"}
        atStep(st, 302);
        cqUnion(c, st, 258, 252, {"toUnion": {"wp": 257}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 258, "union");
        atStep(st, 303);
        cqWorkplaneNew(c, st, 259, {"plane": {"origin": [25, -8.660254037844386, 27.27636339397171], "xDir": [0.737199010647884, 0, -0.6756756756756758], "zDir": [0.6578947368421053, -0.22790142204853647, 0.717799036683466]}, "obj": undefined});
        atStep(st, 304);
        cqCircle(c, st, 260, 259, {"radius": 4, "forConstruction": false});
        atStep(st, 305);
        cqExtrude(c, st, 261, 260, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 261, "extrude");
        // step 306: guard_begin {"before":{"wp":261},"block":1,"g":23,"op":"guard_begin","skippable":true}
        atStep(st, 307);
        cqSelect("Edges")(c, st, 262, 261, {"selector": undefined, "tag": undefined});
        atStep(st, 308);
        cqFillet(c, st, 263, 262, {"radius": 0.4});
        stepVol(c, st, 263, "fillet");
        // step 309: guard_end {"after":{"wp":263},"block":1,"g":23,"op":"guard_end"}
        atStep(st, 310);
        cqUnion(c, st, 264, 258, {"toUnion": {"wp": 263}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 264, "union");
        atStep(st, 311);
        cqWorkplaneNew(c, st, 265, {"plane": {"origin": [35, -8.660254037844386, 12], "xDir": [-0.32432432432432434, 0, 0.945945945945946], "zDir": [0.9210526315789472, -0.22790142204853647, 0.3157894736842105]}, "obj": undefined});
        atStep(st, 312);
        cqCircle(c, st, 266, 265, {"radius": 4, "forConstruction": false});
        atStep(st, 313);
        cqExtrude(c, st, 267, 266, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 267, "extrude");
        // step 314: guard_begin {"before":{"wp":267},"block":1,"g":24,"op":"guard_begin","skippable":true}
        atStep(st, 315);
        cqSelect("Edges")(c, st, 268, 267, {"selector": undefined, "tag": undefined});
        atStep(st, 316);
        cqFillet(c, st, 269, 268, {"radius": 0.4});
        stepVol(c, st, 269, "fillet");
        // step 317: guard_end {"after":{"wp":269},"block":1,"g":24,"op":"guard_end"}
        atStep(st, 318);
        cqUnion(c, st, 270, 264, {"toUnion": {"wp": 269}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 270, "union");
        atStep(st, 319);
        cqWorkplaneNew(c, st, 271, {"plane": {"origin": [-30, 0, 23.323807579381203], "xDir": [-0.6137844099837159, 0, -0.7894736842105263], "zDir": [-0.7894736842105263, 0, 0.6137844099837159]}, "obj": undefined});
        atStep(st, 320);
        cqCircle(c, st, 272, 271, {"radius": 4, "forConstruction": false});
        atStep(st, 321);
        cqExtrude(c, st, 273, 272, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 273, "extrude");
        // step 322: guard_begin {"before":{"wp":273},"block":1,"g":25,"op":"guard_begin","skippable":true}
        atStep(st, 323);
        cqSelect("Edges")(c, st, 274, 273, {"selector": undefined, "tag": undefined});
        atStep(st, 324);
        cqFillet(c, st, 275, 274, {"radius": 0.4});
        stepVol(c, st, 275, "fillet");
        // step 325: guard_end {"after":{"wp":275},"block":1,"g":25,"op":"guard_end"}
        atStep(st, 326);
        cqUnion(c, st, 276, 270, {"toUnion": {"wp": 275}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 276, "union");
        atStep(st, 327);
        cqWorkplaneNew(c, st, 277, {"plane": {"origin": [-20, 0, 32.31098884280702], "xDir": [0.8502891800738691, 0, 0.5263157894736843], "zDir": [-0.5263157894736843, 0, 0.8502891800738691]}, "obj": undefined});
        atStep(st, 328);
        cqCircle(c, st, 278, 277, {"radius": 4, "forConstruction": false});
        atStep(st, 329);
        cqExtrude(c, st, 279, 278, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 279, "extrude");
        // step 330: guard_begin {"before":{"wp":279},"block":1,"g":26,"op":"guard_begin","skippable":true}
        atStep(st, 331);
        cqSelect("Edges")(c, st, 280, 279, {"selector": undefined, "tag": undefined});
        atStep(st, 332);
        cqFillet(c, st, 281, 280, {"radius": 0.4});
        stepVol(c, st, 281, "fillet");
        // step 333: guard_end {"after":{"wp":281},"block":1,"g":26,"op":"guard_end"}
        atStep(st, 334);
        cqUnion(c, st, 282, 276, {"toUnion": {"wp": 281}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 282, "union");
        atStep(st, 335);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [-10, 0, 36.66060555964672], "xDir": [0.96475277788544, 0, 0.2631578947368421], "zDir": [-0.2631578947368421, 0, 0.96475277788544]}, "obj": undefined});
        atStep(st, 336);
        cqCircle(c, st, 284, 283, {"radius": 4, "forConstruction": false});
        atStep(st, 337);
        cqExtrude(c, st, 285, 284, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 285, "extrude");
        // step 338: guard_begin {"before":{"wp":285},"block":1,"g":27,"op":"guard_begin","skippable":true}
        atStep(st, 339);
        cqSelect("Edges")(c, st, 286, 285, {"selector": undefined, "tag": undefined});
        atStep(st, 340);
        cqFillet(c, st, 287, 286, {"radius": 0.4});
        stepVol(c, st, 287, "fillet");
        // step 341: guard_end {"after":{"wp":287},"block":1,"g":27,"op":"guard_end"}
        atStep(st, 342);
        cqUnion(c, st, 288, 282, {"toUnion": {"wp": 287}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 288, "union");
        atStep(st, 343);
        cqWorkplaneNew(c, st, 289, {"plane": {"origin": [0, 0, 38], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 344);
        cqCircle(c, st, 290, 289, {"radius": 4, "forConstruction": false});
        atStep(st, 345);
        cqExtrude(c, st, 291, 290, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 291, "extrude");
        // step 346: guard_begin {"before":{"wp":291},"block":1,"g":28,"op":"guard_begin","skippable":true}
        atStep(st, 347);
        cqSelect("Edges")(c, st, 292, 291, {"selector": undefined, "tag": undefined});
        atStep(st, 348);
        cqFillet(c, st, 293, 292, {"radius": 0.4});
        stepVol(c, st, 293, "fillet");
        // step 349: guard_end {"after":{"wp":293},"block":1,"g":28,"op":"guard_end"}
        atStep(st, 350);
        cqUnion(c, st, 294, 288, {"toUnion": {"wp": 293}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 294, "union");
        atStep(st, 351);
        cqWorkplaneNew(c, st, 295, {"plane": {"origin": [10, 0, 36.66060555964672], "xDir": [0.96475277788544, 0, -0.2631578947368421], "zDir": [0.2631578947368421, 0, 0.96475277788544]}, "obj": undefined});
        atStep(st, 352);
        cqCircle(c, st, 296, 295, {"radius": 4, "forConstruction": false});
        atStep(st, 353);
        cqExtrude(c, st, 297, 296, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 297, "extrude");
        // step 354: guard_begin {"before":{"wp":297},"block":1,"g":29,"op":"guard_begin","skippable":true}
        atStep(st, 355);
        cqSelect("Edges")(c, st, 298, 297, {"selector": undefined, "tag": undefined});
        atStep(st, 356);
        cqFillet(c, st, 299, 298, {"radius": 0.4});
        stepVol(c, st, 299, "fillet");
        // step 357: guard_end {"after":{"wp":299},"block":1,"g":29,"op":"guard_end"}
        atStep(st, 358);
        cqUnion(c, st, 300, 294, {"toUnion": {"wp": 299}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 300, "union");
        atStep(st, 359);
        cqWorkplaneNew(c, st, 301, {"plane": {"origin": [20, 0, 32.31098884280702], "xDir": [0.8502891800738691, 0, -0.5263157894736843], "zDir": [0.5263157894736843, 0, 0.8502891800738691]}, "obj": undefined});
        atStep(st, 360);
        cqCircle(c, st, 302, 301, {"radius": 4, "forConstruction": false});
        atStep(st, 361);
        cqExtrude(c, st, 303, 302, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 303, "extrude");
        // step 362: guard_begin {"before":{"wp":303},"block":1,"g":30,"op":"guard_begin","skippable":true}
        atStep(st, 363);
        cqSelect("Edges")(c, st, 304, 303, {"selector": undefined, "tag": undefined});
        atStep(st, 364);
        cqFillet(c, st, 305, 304, {"radius": 0.4});
        stepVol(c, st, 305, "fillet");
        // step 365: guard_end {"after":{"wp":305},"block":1,"g":30,"op":"guard_end"}
        atStep(st, 366);
        cqUnion(c, st, 306, 300, {"toUnion": {"wp": 305}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 306, "union");
        atStep(st, 367);
        cqWorkplaneNew(c, st, 307, {"plane": {"origin": [30, 0, 23.323807579381203], "xDir": [-0.6137844099837159, 0, 0.7894736842105263], "zDir": [0.7894736842105263, 0, 0.6137844099837159]}, "obj": undefined});
        atStep(st, 368);
        cqCircle(c, st, 308, 307, {"radius": 4, "forConstruction": false});
        atStep(st, 369);
        cqExtrude(c, st, 309, 308, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 309, "extrude");
        // step 370: guard_begin {"before":{"wp":309},"block":1,"g":31,"op":"guard_begin","skippable":true}
        atStep(st, 371);
        cqSelect("Edges")(c, st, 310, 309, {"selector": undefined, "tag": undefined});
        atStep(st, 372);
        cqFillet(c, st, 311, 310, {"radius": 0.4});
        stepVol(c, st, 311, "fillet");
        // step 373: guard_end {"after":{"wp":311},"block":1,"g":31,"op":"guard_end"}
        atStep(st, 374);
        cqUnion(c, st, 312, 306, {"toUnion": {"wp": 311}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 312, "union");
        atStep(st, 375);
        cqWorkplaneNew(c, st, 313, {"plane": {"origin": [-35, 8.660254037844386, 12], "xDir": [-0.32432432432432434, 0, -0.945945945945946], "zDir": [-0.9210526315789472, 0.22790142204853647, 0.3157894736842105]}, "obj": undefined});
        atStep(st, 376);
        cqCircle(c, st, 314, 313, {"radius": 4, "forConstruction": false});
        atStep(st, 377);
        cqExtrude(c, st, 315, 314, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 315, "extrude");
        // step 378: guard_begin {"before":{"wp":315},"block":1,"g":32,"op":"guard_begin","skippable":true}
        atStep(st, 379);
        cqSelect("Edges")(c, st, 316, 315, {"selector": undefined, "tag": undefined});
        atStep(st, 380);
        cqFillet(c, st, 317, 316, {"radius": 0.4});
        stepVol(c, st, 317, "fillet");
        // step 381: guard_end {"after":{"wp":317},"block":1,"g":32,"op":"guard_end"}
        atStep(st, 382);
        cqUnion(c, st, 318, 312, {"toUnion": {"wp": 317}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 318, "union");
        atStep(st, 383);
        cqWorkplaneNew(c, st, 319, {"plane": {"origin": [-25, 8.660254037844386, 27.27636339397171], "xDir": [0.737199010647884, 0, 0.6756756756756758], "zDir": [-0.6578947368421053, 0.22790142204853647, 0.717799036683466]}, "obj": undefined});
        atStep(st, 384);
        cqCircle(c, st, 320, 319, {"radius": 4, "forConstruction": false});
        atStep(st, 385);
        cqExtrude(c, st, 321, 320, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 321, "extrude");
        // step 386: guard_begin {"before":{"wp":321},"block":1,"g":33,"op":"guard_begin","skippable":true}
        atStep(st, 387);
        cqSelect("Edges")(c, st, 322, 321, {"selector": undefined, "tag": undefined});
        atStep(st, 388);
        cqFillet(c, st, 323, 322, {"radius": 0.4});
        stepVol(c, st, 323, "fillet");
        // step 389: guard_end {"after":{"wp":323},"block":1,"g":33,"op":"guard_end"}
        atStep(st, 390);
        cqUnion(c, st, 324, 318, {"toUnion": {"wp": 323}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 324, "union");
        atStep(st, 391);
        cqWorkplaneNew(c, st, 325, {"plane": {"origin": [-15, 8.660254037844386, 33.823069050575526], "xDir": [0.9141370013669062, 1.3877787807814457e-17, 0.4054054054054054], "zDir": [-0.3947368421052632, 0.22790142204853647, 0.8900807644888297]}, "obj": undefined});
        atStep(st, 392);
        cqCircle(c, st, 326, 325, {"radius": 4, "forConstruction": false});
        atStep(st, 393);
        cqExtrude(c, st, 327, 326, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 327, "extrude");
        // step 394: guard_begin {"before":{"wp":327},"block":1,"g":34,"op":"guard_begin","skippable":true}
        atStep(st, 395);
        cqSelect("Edges")(c, st, 328, 327, {"selector": undefined, "tag": undefined});
        atStep(st, 396);
        cqFillet(c, st, 329, 328, {"radius": 0.4});
        stepVol(c, st, 329, "fillet");
        // step 397: guard_end {"after":{"wp":329},"block":1,"g":34,"op":"guard_end"}
        atStep(st, 398);
        cqUnion(c, st, 330, 324, {"toUnion": {"wp": 329}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 330, "union");
        atStep(st, 399);
        cqWorkplaneNew(c, st, 331, {"plane": {"origin": [-5, 8.660254037844386, 36.66060555964672], "xDir": [-3.469446951953614e-18, 0.9732141991249592, -0.22990024493585143], "zDir": [-0.13157894736842105, 0.22790142204853647, 0.96475277788544]}, "obj": undefined});
        atStep(st, 400);
        cqCircle(c, st, 332, 331, {"radius": 4, "forConstruction": false});
        atStep(st, 401);
        cqExtrude(c, st, 333, 332, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 333, "extrude");
        // step 402: guard_begin {"before":{"wp":333},"block":1,"g":35,"op":"guard_begin","skippable":true}
        atStep(st, 403);
        cqSelect("Edges")(c, st, 334, 333, {"selector": undefined, "tag": undefined});
        atStep(st, 404);
        cqFillet(c, st, 335, 334, {"radius": 0.4});
        stepVol(c, st, 335, "fillet");
        // step 405: guard_end {"after":{"wp":335},"block":1,"g":35,"op":"guard_end"}
        atStep(st, 406);
        cqUnion(c, st, 336, 330, {"toUnion": {"wp": 335}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 336, "union");
        atStep(st, 407);
        cqWorkplaneNew(c, st, 337, {"plane": {"origin": [5, 8.660254037844386, 36.66060555964672], "xDir": [3.469446951953614e-18, 0.9732141991249592, -0.22990024493585143], "zDir": [0.13157894736842105, 0.22790142204853647, 0.96475277788544]}, "obj": undefined});
        atStep(st, 408);
        cqCircle(c, st, 338, 337, {"radius": 4, "forConstruction": false});
        atStep(st, 409);
        cqExtrude(c, st, 339, 338, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 339, "extrude");
        // step 410: guard_begin {"before":{"wp":339},"block":1,"g":36,"op":"guard_begin","skippable":true}
        atStep(st, 411);
        cqSelect("Edges")(c, st, 340, 339, {"selector": undefined, "tag": undefined});
        atStep(st, 412);
        cqFillet(c, st, 341, 340, {"radius": 0.4});
        stepVol(c, st, 341, "fillet");
        // step 413: guard_end {"after":{"wp":341},"block":1,"g":36,"op":"guard_end"}
        atStep(st, 414);
        cqUnion(c, st, 342, 336, {"toUnion": {"wp": 341}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 342, "union");
        atStep(st, 415);
        cqWorkplaneNew(c, st, 343, {"plane": {"origin": [15, 8.660254037844386, 33.823069050575526], "xDir": [0.9141370013669062, -1.3877787807814457e-17, -0.4054054054054054], "zDir": [0.3947368421052632, 0.22790142204853647, 0.8900807644888297]}, "obj": undefined});
        atStep(st, 416);
        cqCircle(c, st, 344, 343, {"radius": 4, "forConstruction": false});
        atStep(st, 417);
        cqExtrude(c, st, 345, 344, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 345, "extrude");
        // step 418: guard_begin {"before":{"wp":345},"block":1,"g":37,"op":"guard_begin","skippable":true}
        atStep(st, 419);
        cqSelect("Edges")(c, st, 346, 345, {"selector": undefined, "tag": undefined});
        atStep(st, 420);
        cqFillet(c, st, 347, 346, {"radius": 0.4});
        stepVol(c, st, 347, "fillet");
        // step 421: guard_end {"after":{"wp":347},"block":1,"g":37,"op":"guard_end"}
        atStep(st, 422);
        cqUnion(c, st, 348, 342, {"toUnion": {"wp": 347}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 348, "union");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 349, {"plane": {"origin": [25, 8.660254037844386, 27.27636339397171], "xDir": [0.737199010647884, 0, -0.6756756756756758], "zDir": [0.6578947368421053, 0.22790142204853647, 0.717799036683466]}, "obj": undefined});
        atStep(st, 424);
        cqCircle(c, st, 350, 349, {"radius": 4, "forConstruction": false});
        atStep(st, 425);
        cqExtrude(c, st, 351, 350, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 351, "extrude");
        // step 426: guard_begin {"before":{"wp":351},"block":1,"g":38,"op":"guard_begin","skippable":true}
        atStep(st, 427);
        cqSelect("Edges")(c, st, 352, 351, {"selector": undefined, "tag": undefined});
        atStep(st, 428);
        cqFillet(c, st, 353, 352, {"radius": 0.4});
        stepVol(c, st, 353, "fillet");
        // step 429: guard_end {"after":{"wp":353},"block":1,"g":38,"op":"guard_end"}
        atStep(st, 430);
        cqUnion(c, st, 354, 348, {"toUnion": {"wp": 353}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 354, "union");
        atStep(st, 431);
        cqWorkplaneNew(c, st, 355, {"plane": {"origin": [35, 8.660254037844386, 12], "xDir": [-0.32432432432432434, 0, 0.945945945945946], "zDir": [0.9210526315789472, 0.22790142204853647, 0.3157894736842105]}, "obj": undefined});
        atStep(st, 432);
        cqCircle(c, st, 356, 355, {"radius": 4, "forConstruction": false});
        atStep(st, 433);
        cqExtrude(c, st, 357, 356, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 357, "extrude");
        // step 434: guard_begin {"before":{"wp":357},"block":1,"g":39,"op":"guard_begin","skippable":true}
        atStep(st, 435);
        cqSelect("Edges")(c, st, 358, 357, {"selector": undefined, "tag": undefined});
        atStep(st, 436);
        cqFillet(c, st, 359, 358, {"radius": 0.4});
        stepVol(c, st, 359, "fillet");
        // step 437: guard_end {"after":{"wp":359},"block":1,"g":39,"op":"guard_end"}
        atStep(st, 438);
        cqUnion(c, st, 360, 354, {"toUnion": {"wp": 359}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 360, "union");
        atStep(st, 439);
        cqWorkplaneNew(c, st, 361, {"plane": {"origin": [-30, 17.32050807568877, 15.62049935181331], "xDir": [-0.4999999999999999, -0.8660254037844386, 2.7755575615628907e-17], "zDir": [-0.7894736842105263, 0.45580284409707295, 0.4110657724161398]}, "obj": undefined});
        atStep(st, 440);
        cqCircle(c, st, 362, 361, {"radius": 4, "forConstruction": false});
        atStep(st, 441);
        cqExtrude(c, st, 363, 362, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 363, "extrude");
        // step 442: guard_begin {"before":{"wp":363},"block":1,"g":40,"op":"guard_begin","skippable":true}
        atStep(st, 443);
        cqSelect("Edges")(c, st, 364, 363, {"selector": undefined, "tag": undefined});
        atStep(st, 444);
        cqFillet(c, st, 365, 364, {"radius": 0.4});
        stepVol(c, st, 365, "fillet");
        // step 445: guard_end {"after":{"wp":365},"block":1,"g":40,"op":"guard_end"}
        atStep(st, 446);
        cqUnion(c, st, 366, 360, {"toUnion": {"wp": 365}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 366, "union");
        atStep(st, 447);
        cqWorkplaneNew(c, st, 367, {"plane": {"origin": [-20, 17.32050807568877, 27.27636339397171], "xDir": [0.8064425896179158, 2.7755575615628907e-17, 0.5913123959890827], "zDir": [-0.5263157894736843, 0.455802844097073, 0.7177990366834661]}, "obj": undefined});
        atStep(st, 448);
        cqCircle(c, st, 368, 367, {"radius": 4, "forConstruction": false});
        atStep(st, 449);
        cqExtrude(c, st, 369, 368, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 369, "extrude");
        // step 450: guard_begin {"before":{"wp":369},"block":1,"g":41,"op":"guard_begin","skippable":true}
        atStep(st, 451);
        cqSelect("Edges")(c, st, 370, 369, {"selector": undefined, "tag": undefined});
        atStep(st, 452);
        cqFillet(c, st, 371, 370, {"radius": 0.4});
        stepVol(c, st, 371, "fillet");
        // step 453: guard_end {"after":{"wp":371},"block":1,"g":41,"op":"guard_end"}
        atStep(st, 454);
        cqUnion(c, st, 372, 366, {"toUnion": {"wp": 371}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 372, "union");
        atStep(st, 455);
        cqWorkplaneNew(c, st, 373, {"plane": {"origin": [-10, 17.32050807568877, 32.31098884280702], "xDir": [0, 0.8813544770895048, -0.472455591261534], "zDir": [-0.26315789473684215, 0.455802844097073, 0.8502891800738691]}, "obj": undefined});
        atStep(st, 456);
        cqCircle(c, st, 374, 373, {"radius": 4, "forConstruction": false});
        atStep(st, 457);
        cqExtrude(c, st, 375, 374, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 375, "extrude");
        // step 458: guard_begin {"before":{"wp":375},"block":1,"g":42,"op":"guard_begin","skippable":true}
        atStep(st, 459);
        cqSelect("Edges")(c, st, 376, 375, {"selector": undefined, "tag": undefined});
        atStep(st, 460);
        cqFillet(c, st, 377, 376, {"radius": 0.4});
        stepVol(c, st, 377, "fillet");
        // step 461: guard_end {"after":{"wp":377},"block":1,"g":42,"op":"guard_end"}
        atStep(st, 462);
        cqUnion(c, st, 378, 372, {"toUnion": {"wp": 377}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 378, "union");
        atStep(st, 463);
        cqWorkplaneNew(c, st, 379, {"plane": {"origin": [0, 17.32050807568877, 33.823069050575526], "xDir": [0, 0.8900807644888297, -0.45580284409707295], "zDir": [0, 0.45580284409707295, 0.8900807644888297]}, "obj": undefined});
        atStep(st, 464);
        cqCircle(c, st, 380, 379, {"radius": 4, "forConstruction": false});
        atStep(st, 465);
        cqExtrude(c, st, 381, 380, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 381, "extrude");
        // step 466: guard_begin {"before":{"wp":381},"block":1,"g":43,"op":"guard_begin","skippable":true}
        atStep(st, 467);
        cqSelect("Edges")(c, st, 382, 381, {"selector": undefined, "tag": undefined});
        atStep(st, 468);
        cqFillet(c, st, 383, 382, {"radius": 0.4});
        stepVol(c, st, 383, "fillet");
        // step 469: guard_end {"after":{"wp":383},"block":1,"g":43,"op":"guard_end"}
        atStep(st, 470);
        cqUnion(c, st, 384, 378, {"toUnion": {"wp": 383}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 384, "union");
        atStep(st, 471);
        cqWorkplaneNew(c, st, 385, {"plane": {"origin": [10, 17.32050807568877, 32.31098884280702], "xDir": [0, 0.8813544770895048, -0.472455591261534], "zDir": [0.26315789473684215, 0.455802844097073, 0.8502891800738691]}, "obj": undefined});
        atStep(st, 472);
        cqCircle(c, st, 386, 385, {"radius": 4, "forConstruction": false});
        atStep(st, 473);
        cqExtrude(c, st, 387, 386, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 387, "extrude");
        // step 474: guard_begin {"before":{"wp":387},"block":1,"g":44,"op":"guard_begin","skippable":true}
        atStep(st, 475);
        cqSelect("Edges")(c, st, 388, 387, {"selector": undefined, "tag": undefined});
        atStep(st, 476);
        cqFillet(c, st, 389, 388, {"radius": 0.4});
        stepVol(c, st, 389, "fillet");
        // step 477: guard_end {"after":{"wp":389},"block":1,"g":44,"op":"guard_end"}
        atStep(st, 478);
        cqUnion(c, st, 390, 384, {"toUnion": {"wp": 389}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 390, "union");
        atStep(st, 479);
        cqWorkplaneNew(c, st, 391, {"plane": {"origin": [20, 17.32050807568877, 27.27636339397171], "xDir": [0.8064425896179158, -2.7755575615628907e-17, -0.5913123959890827], "zDir": [0.5263157894736843, 0.455802844097073, 0.7177990366834661]}, "obj": undefined});
        atStep(st, 480);
        cqCircle(c, st, 392, 391, {"radius": 4, "forConstruction": false});
        atStep(st, 481);
        cqExtrude(c, st, 393, 392, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 393, "extrude");
        // step 482: guard_begin {"before":{"wp":393},"block":1,"g":45,"op":"guard_begin","skippable":true}
        atStep(st, 483);
        cqSelect("Edges")(c, st, 394, 393, {"selector": undefined, "tag": undefined});
        atStep(st, 484);
        cqFillet(c, st, 395, 394, {"radius": 0.4});
        stepVol(c, st, 395, "fillet");
        // step 485: guard_end {"after":{"wp":395},"block":1,"g":45,"op":"guard_end"}
        atStep(st, 486);
        cqUnion(c, st, 396, 390, {"toUnion": {"wp": 395}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 396, "union");
        atStep(st, 487);
        cqWorkplaneNew(c, st, 397, {"plane": {"origin": [30, 17.32050807568877, 15.62049935181331], "xDir": [-0.4999999999999999, 0.8660254037844386, -2.7755575615628907e-17], "zDir": [0.7894736842105263, 0.45580284409707295, 0.4110657724161398]}, "obj": undefined});
        atStep(st, 488);
        cqCircle(c, st, 398, 397, {"radius": 4, "forConstruction": false});
        atStep(st, 489);
        cqExtrude(c, st, 399, 398, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 399, "extrude");
        // step 490: guard_begin {"before":{"wp":399},"block":1,"g":46,"op":"guard_begin","skippable":true}
        atStep(st, 491);
        cqSelect("Edges")(c, st, 400, 399, {"selector": undefined, "tag": undefined});
        atStep(st, 492);
        cqFillet(c, st, 401, 400, {"radius": 0.4});
        stepVol(c, st, 401, "fillet");
        // step 493: guard_end {"after":{"wp":401},"block":1,"g":46,"op":"guard_end"}
        atStep(st, 494);
        cqUnion(c, st, 402, 396, {"toUnion": {"wp": 401}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 402, "union");
        atStep(st, 495);
        cqWorkplaneNew(c, st, 403, {"plane": {"origin": [-25, 25.980762113533157, 12.000000000000004], "xDir": [0.7205766921228921, 0.6933752452815365, 0], "zDir": [-0.6578947368421053, 0.6837042661456094, 0.3157894736842107]}, "obj": undefined});
        atStep(st, 496);
        cqCircle(c, st, 404, 403, {"radius": 4, "forConstruction": false});
        atStep(st, 497);
        cqExtrude(c, st, 405, 404, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 405, "extrude");
        // step 498: guard_begin {"before":{"wp":405},"block":1,"g":47,"op":"guard_begin","skippable":true}
        atStep(st, 499);
        cqSelect("Edges")(c, st, 406, 405, {"selector": undefined, "tag": undefined});
        atStep(st, 500);
        cqFillet(c, st, 407, 406, {"radius": 0.4});
        stepVol(c, st, 407, "fillet");
        // step 501: guard_end {"after":{"wp":407},"block":1,"g":47,"op":"guard_end"}
        atStep(st, 502);
        cqUnion(c, st, 408, 402, {"toUnion": {"wp": 407}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 408, "union");
        atStep(st, 503);
        cqWorkplaneNew(c, st, 409, {"plane": {"origin": [-15, 25.980762113533157, 23.323807579381203], "xDir": [2.7755575615628914e-17, -0.6680325084317194, 0.7441320902087377], "zDir": [-0.3947368421052632, 0.6837042661456094, 0.6137844099837159]}, "obj": undefined});
        atStep(st, 504);
        cqCircle(c, st, 410, 409, {"radius": 4, "forConstruction": false});
        atStep(st, 505);
        cqExtrude(c, st, 411, 410, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 411, "extrude");
        // step 506: guard_begin {"before":{"wp":411},"block":1,"g":48,"op":"guard_begin","skippable":true}
        atStep(st, 507);
        cqSelect("Edges")(c, st, 412, 411, {"selector": undefined, "tag": undefined});
        atStep(st, 508);
        cqFillet(c, st, 413, 412, {"radius": 0.4});
        stepVol(c, st, 413, "fillet");
        // step 509: guard_end {"after":{"wp":413},"block":1,"g":48,"op":"guard_end"}
        atStep(st, 510);
        cqUnion(c, st, 414, 408, {"toUnion": {"wp": 413}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 414, "union");
        atStep(st, 511);
        cqWorkplaneNew(c, st, 415, {"plane": {"origin": [-5, 25.980762113533157, 27.27636339397171], "xDir": [0, 0.7240945355448554, -0.6897007348075543], "zDir": [-0.13157894736842105, 0.6837042661456094, 0.7177990366834661]}, "obj": undefined});
        atStep(st, 512);
        cqCircle(c, st, 416, 415, {"radius": 4, "forConstruction": false});
        atStep(st, 513);
        cqExtrude(c, st, 417, 416, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 417, "extrude");
        // step 514: guard_begin {"before":{"wp":417},"block":1,"g":49,"op":"guard_begin","skippable":true}
        atStep(st, 515);
        cqSelect("Edges")(c, st, 418, 417, {"selector": undefined, "tag": undefined});
        atStep(st, 516);
        cqFillet(c, st, 419, 418, {"radius": 0.4});
        stepVol(c, st, 419, "fillet");
        // step 517: guard_end {"after":{"wp":419},"block":1,"g":49,"op":"guard_end"}
        atStep(st, 518);
        cqUnion(c, st, 420, 414, {"toUnion": {"wp": 419}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 420, "union");
        atStep(st, 519);
        cqWorkplaneNew(c, st, 421, {"plane": {"origin": [5, 25.980762113533157, 27.27636339397171], "xDir": [0, 0.7240945355448554, -0.6897007348075543], "zDir": [0.13157894736842105, 0.6837042661456094, 0.7177990366834661]}, "obj": undefined});
        atStep(st, 520);
        cqCircle(c, st, 422, 421, {"radius": 4, "forConstruction": false});
        atStep(st, 521);
        cqExtrude(c, st, 423, 422, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 423, "extrude");
        // step 522: guard_begin {"before":{"wp":423},"block":1,"g":50,"op":"guard_begin","skippable":true}
        atStep(st, 523);
        cqSelect("Edges")(c, st, 424, 423, {"selector": undefined, "tag": undefined});
        atStep(st, 524);
        cqFillet(c, st, 425, 424, {"radius": 0.4});
        stepVol(c, st, 425, "fillet");
        // step 525: guard_end {"after":{"wp":425},"block":1,"g":50,"op":"guard_end"}
        atStep(st, 526);
        cqUnion(c, st, 426, 420, {"toUnion": {"wp": 425}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 426, "union");
        atStep(st, 527);
        cqWorkplaneNew(c, st, 427, {"plane": {"origin": [15, 25.980762113533157, 23.323807579381203], "xDir": [-2.7755575615628914e-17, -0.6680325084317194, 0.7441320902087377], "zDir": [0.3947368421052632, 0.6837042661456094, 0.6137844099837159]}, "obj": undefined});
        atStep(st, 528);
        cqCircle(c, st, 428, 427, {"radius": 4, "forConstruction": false});
        atStep(st, 529);
        cqExtrude(c, st, 429, 428, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 429, "extrude");
        // step 530: guard_begin {"before":{"wp":429},"block":1,"g":51,"op":"guard_begin","skippable":true}
        atStep(st, 531);
        cqSelect("Edges")(c, st, 430, 429, {"selector": undefined, "tag": undefined});
        atStep(st, 532);
        cqFillet(c, st, 431, 430, {"radius": 0.4});
        stepVol(c, st, 431, "fillet");
        // step 533: guard_end {"after":{"wp":431},"block":1,"g":51,"op":"guard_end"}
        atStep(st, 534);
        cqUnion(c, st, 432, 426, {"toUnion": {"wp": 431}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        atStep(st, 535);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [25, 25.980762113533157, 12.000000000000004], "xDir": [0.7205766921228921, -0.6933752452815365, 0], "zDir": [0.6578947368421053, 0.6837042661456094, 0.3157894736842107]}, "obj": undefined});
        atStep(st, 536);
        cqCircle(c, st, 434, 433, {"radius": 4, "forConstruction": false});
        atStep(st, 537);
        cqExtrude(c, st, 435, 434, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 435, "extrude");
        // step 538: guard_begin {"before":{"wp":435},"block":1,"g":52,"op":"guard_begin","skippable":true}
        atStep(st, 539);
        cqSelect("Edges")(c, st, 436, 435, {"selector": undefined, "tag": undefined});
        atStep(st, 540);
        cqFillet(c, st, 437, 436, {"radius": 0.4});
        stepVol(c, st, 437, "fillet");
        // step 541: guard_end {"after":{"wp":437},"block":1,"g":52,"op":"guard_end"}
        atStep(st, 542);
        cqUnion(c, st, 438, 432, {"toUnion": {"wp": 437}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 438, "union");
        atStep(st, 543);
        cqWorkplaneNew(c, st, 439, {"plane": {"origin": [-10, 34.64101615137754, 12.000000000000009], "xDir": [1.3877787807814457e-17, -0.3273268353539888, 0.944911182523068], "zDir": [-0.2631578947368421, 0.911605688194146, 0.3157894736842108]}, "obj": undefined});
        atStep(st, 544);
        cqCircle(c, st, 440, 439, {"radius": 4, "forConstruction": false});
        atStep(st, 545);
        cqExtrude(c, st, 441, 440, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 441, "extrude");
        // step 546: guard_begin {"before":{"wp":441},"block":1,"g":53,"op":"guard_begin","skippable":true}
        atStep(st, 547);
        cqSelect("Edges")(c, st, 442, 441, {"selector": undefined, "tag": undefined});
        atStep(st, 548);
        cqFillet(c, st, 443, 442, {"radius": 0.4});
        stepVol(c, st, 443, "fillet");
        // step 549: guard_end {"after":{"wp":443},"block":1,"g":53,"op":"guard_end"}
        atStep(st, 550);
        cqUnion(c, st, 444, 438, {"toUnion": {"wp": 443}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 444, "union");
        atStep(st, 551);
        cqWorkplaneNew(c, st, 445, {"plane": {"origin": [0, 34.64101615137754, 15.620499351813317], "xDir": [0, -0.4110657724161399, 0.911605688194146], "zDir": [0, 0.911605688194146, 0.4110657724161399]}, "obj": undefined});
        atStep(st, 552);
        cqCircle(c, st, 446, 445, {"radius": 4, "forConstruction": false});
        atStep(st, 553);
        cqExtrude(c, st, 447, 446, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 447, "extrude");
        // step 554: guard_begin {"before":{"wp":447},"block":1,"g":54,"op":"guard_begin","skippable":true}
        atStep(st, 555);
        cqSelect("Edges")(c, st, 448, 447, {"selector": undefined, "tag": undefined});
        atStep(st, 556);
        cqFillet(c, st, 449, 448, {"radius": 0.4});
        stepVol(c, st, 449, "fillet");
        // step 557: guard_end {"after":{"wp":449},"block":1,"g":54,"op":"guard_end"}
        atStep(st, 558);
        cqUnion(c, st, 450, 444, {"toUnion": {"wp": 449}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 450, "union");
        atStep(st, 559);
        cqWorkplaneNew(c, st, 451, {"plane": {"origin": [10, 34.64101615137754, 12.000000000000009], "xDir": [-1.3877787807814457e-17, -0.3273268353539888, 0.944911182523068], "zDir": [0.2631578947368421, 0.911605688194146, 0.3157894736842108]}, "obj": undefined});
        atStep(st, 560);
        cqCircle(c, st, 452, 451, {"radius": 4, "forConstruction": false});
        atStep(st, 561);
        cqExtrude(c, st, 453, 452, {"until": -2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 453, "extrude");
        // step 562: guard_begin {"before":{"wp":453},"block":1,"g":55,"op":"guard_begin","skippable":true}
        atStep(st, 563);
        cqSelect("Edges")(c, st, 454, 453, {"selector": undefined, "tag": undefined});
        atStep(st, 564);
        cqFillet(c, st, 455, 454, {"radius": 0.4});
        stepVol(c, st, 455, "fillet");
        // step 565: guard_end {"after":{"wp":455},"block":1,"g":55,"op":"guard_end"}
        atStep(st, 566);
        cqUnion(c, st, 456, 450, {"toUnion": {"wp": 455}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 456, "union");
        // step 567: guard_begin {"before":{"wp":456},"block":2,"g":56,"op":"guard_begin","skippable":true}
        atStep(st, 568);
        cqSelect("Edges")(c, st, 457, 456, {"selector": undefined, "tag": undefined});
        atStep(st, 569);
        cqFillet(c, st, 458, 457, {"radius": 0.4});
        stepVol(c, st, 458, "fillet");
        // step 570: guard_end {"after":{"wp":458},"block":2,"g":56,"op":"guard_end"}
        atStep(st, 571);
        cqCombine(c, st, 459, 458, {"clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 459, "combine");
        return mergeMaps(cqMeasure(c, st, {"wp": 459}), {"i": 5476});
    } catch (e) {
        return {"i": 5476, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c5476"] = case_5476();
    return out;
}
