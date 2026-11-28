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

const case_5568 = function() {
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
        cqSphere(c, st, 4, 3, {"radius": 37, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 4, "sphere");
        atStep(st, 5);
        cqCut(c, st, 5, 2, {"toCut": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 5, "cut");
        atStep(st, 6);
        cqWorkplaneNew(c, st, 6, {"plane": {"origin": [-21.916868011587045, 28.7955707030265, 10.786893258332633], "xDir": [-0.5183325659805633, -0.6415912145319819, 0.5654131803198271], "zDir": [-0.583128783201597, 0.7487834503693297, 0.31509390132915405]}, "obj": undefined});
        atStep(st, 7);
        cqPolyline(c, st, 7, 6, {"listOfXYTuple": [[-9.91495403367814, -6.796790203913798], [11.946368279335598, -6.7967902039137975], [-2.0314142456574595, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 8);
        cqClose(c, st, 8, 7, {});
        atStep(st, 9);
        cqExtrude(c, st, 9, 8, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 9, "extrude");
        atStep(st, 10);
        cqCut(c, st, 10, 5, {"toCut": {"wp": 9}, "clean": true, "tol": undefined});
        stepVol(c, st, 10, "cut");
        atStep(st, 11);
        cqWorkplaneNew(c, st, 11, {"plane": {"origin": [-21.573786516665265, 21.573786516665265, 21.573786516665265], "xDir": [0.5, -0.30901699437494745, 0.8090169943749475], "zDir": [-0.5773502691896258, 0.5773502691896258, 0.5773502691896258]}, "obj": undefined});
        atStep(st, 12);
        cqPolyline(c, st, 12, 11, {"listOfXYTuple": [[-12.360679774997898, -7.1364417954618], [12.360679774997898, -7.136441795461799], [-4.440892098500626e-16, 14.2728835909236]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 13);
        cqClose(c, st, 13, 12, {});
        atStep(st, 14);
        cqExtrude(c, st, 14, 13, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 14, "extrude");
        atStep(st, 15);
        cqCut(c, st, 15, 10, {"toCut": {"wp": 14}, "clean": true, "tol": undefined});
        stepVol(c, st, 15, "cut");
        atStep(st, 16);
        cqWorkplaneNew(c, st, 16, {"plane": {"origin": [-10.786893258332633, 21.916868011587045, 28.7955707030265], "xDir": [-0.30901699437494745, -0.8090169943749473, 0.5], "zDir": [-0.3150939013291541, 0.583128783201597, 0.7487834503693297]}, "obj": undefined});
        atStep(st, 17);
        cqPolyline(c, st, 17, 16, {"listOfXYTuple": [[-12.360679774997898, -6.010463180279224], [12.360679774997896, -6.010463180279222], [0, 12.020926360558448]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 18);
        cqClose(c, st, 18, 17, {});
        atStep(st, 19);
        cqExtrude(c, st, 19, 18, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 19, "extrude");
        atStep(st, 20);
        cqCut(c, st, 20, 15, {"toCut": {"wp": 19}, "clean": true, "tol": undefined});
        stepVol(c, st, 20, "cut");
        atStep(st, 21);
        cqWorkplaneNew(c, st, 21, {"plane": {"origin": [-28.7955707030265, 10.786893258332633, 21.916868011587045], "xDir": [-0.07617803421215472, -0.9148577434446536, 0.3965251774640903], "zDir": [-0.7487834503693297, 0.31509390132915416, 0.583128783201597]}, "obj": undefined});
        atStep(st, 22);
        cqPolyline(c, st, 22, 21, {"listOfXYTuple": [[-11.946368279335594, -6.7967902039137975], [9.914954033678136, -6.7967902039137975], [2.031414245657458, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 23);
        cqClose(c, st, 23, 22, {});
        atStep(st, 24);
        cqExtrude(c, st, 24, 23, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 24, "extrude");
        atStep(st, 25);
        cqCut(c, st, 25, 20, {"toCut": {"wp": 24}, "clean": true, "tol": undefined});
        stepVol(c, st, 25, "cut");
        atStep(st, 26);
        cqWorkplaneNew(c, st, 26, {"plane": {"origin": [-11.129974753254414, 35.46223736969316, 6.666666666666667], "xDir": [0.3965251774640903, -0.07617803421215465, 0.9148577434446536], "zDir": [-0.2680348818724428, 0.9435221910385524, 0.1947387406692229]}, "obj": undefined});
        atStep(st, 27);
        cqPolyline(c, st, 27, 26, {"listOfXYTuple": [[-9.914954033678136, -6.796790203913793], [11.946368279335594, -6.796790203913794], [-2.0314142456574578, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 28);
        cqClose(c, st, 28, 27, {});
        atStep(st, 29);
        cqExtrude(c, st, 29, 28, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 29, "extrude");
        atStep(st, 30);
        cqCut(c, st, 30, 25, {"toCut": {"wp": 29}, "clean": true, "tol": undefined});
        stepVol(c, st, 30, "cut");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 31, {"plane": {"origin": [0, 34.9071198499986, 13.333333333333334], "xDir": [1, 0, 0], "zDir": [0, 0.9341723589627158, 0.35682208977308993]}, "obj": undefined});
        atStep(st, 32);
        cqPolyline(c, st, 32, 31, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997896, -7.136441795461799], [0, 14.272883590923598]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 33);
        cqClose(c, st, 33, 32, {});
        atStep(st, 34);
        cqExtrude(c, st, 34, 33, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 34, "extrude");
        atStep(st, 35);
        cqCut(c, st, 35, 30, {"toCut": {"wp": 34}, "clean": true, "tol": undefined});
        stepVol(c, st, 35, "cut");
        atStep(st, 36);
        cqWorkplaneNew(c, st, 36, {"plane": {"origin": [11.129974753254414, 35.46223736969316, 6.666666666666667], "xDir": [0.5, -0.30901699437494745, 0.8090169943749475], "zDir": [0.2680348818724428, 0.9435221910385524, 0.1947387406692229]}, "obj": undefined});
        atStep(st, 37);
        cqPolyline(c, st, 37, 36, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279224], [12.360679774997894, -6.010463180279223], [-1.7763568394002505e-15, 12.020926360558445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 38);
        cqClose(c, st, 38, 37, {});
        atStep(st, 39);
        cqExtrude(c, st, 39, 38, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 39, "extrude");
        atStep(st, 40);
        cqCut(c, st, 40, 35, {"toCut": {"wp": 39}, "clean": true, "tol": undefined});
        stepVol(c, st, 40, "cut");
        atStep(st, 41);
        cqWorkplaneNew(c, st, 41, {"plane": {"origin": [0, 28.583534678253713, 24.675344111360538], "xDir": [0.5654131803198271, -0.5183325659805634, 0.6415912145319819], "zDir": [0, 0.7778675238708198, 0.6284282897093986]}, "obj": undefined});
        atStep(st, 42);
        cqPolyline(c, st, 42, 41, {"listOfXYTuple": [[-11.946368279335596, -6.7967902039137975], [9.91495403367814, -6.7967902039137975], [2.0314142456574547, 13.5935804078276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 43);
        cqClose(c, st, 43, 42, {});
        atStep(st, 44);
        cqExtrude(c, st, 44, 43, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 44, "extrude");
        atStep(st, 45);
        cqCut(c, st, 45, 40, {"toCut": {"wp": 44}, "clean": true, "tol": undefined});
        stepVol(c, st, 45, "cut");
        atStep(st, 46);
        cqWorkplaneNew(c, st, 46, {"plane": {"origin": [-11.129974753254414, 35.46223736969316, -6.666666666666667], "xDir": [0.9619383577839176, 0.2732665289126717, 0], "zDir": [-0.2680348818724428, 0.9435221910385524, -0.1947387406692229]}, "obj": undefined});
        atStep(st, 47);
        cqPolyline(c, st, 47, 46, {"listOfXYTuple": [[-9.914954033678136, -6.7967902039137975], [11.946368279335598, -6.7967902039137975], [-2.031414245657456, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 48);
        cqClose(c, st, 48, 47, {});
        atStep(st, 49);
        cqExtrude(c, st, 49, 48, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 49, "extrude");
        atStep(st, 50);
        cqCut(c, st, 50, 45, {"toCut": {"wp": 49}, "clean": true, "tol": undefined});
        stepVol(c, st, 50, "cut");
        atStep(st, 51);
        cqWorkplaneNew(c, st, 51, {"plane": {"origin": [0, 34.907119849998594, -13.333333333333334], "xDir": [0.5, -0.30901699437494745, -0.8090169943749475], "zDir": [0, 0.9341723589627158, -0.35682208977308993]}, "obj": undefined});
        atStep(st, 52);
        cqPolyline(c, st, 52, 51, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997894, -7.136441795461801], [-8.881784197001252e-16, 14.272883590923596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 53);
        cqClose(c, st, 53, 52, {});
        atStep(st, 54);
        cqExtrude(c, st, 54, 53, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 54, "extrude");
        atStep(st, 55);
        cqCut(c, st, 55, 50, {"toCut": {"wp": 54}, "clean": true, "tol": undefined});
        stepVol(c, st, 55, "cut");
        atStep(st, 56);
        cqWorkplaneNew(c, st, 56, {"plane": {"origin": [0, 28.583534678253713, -24.675344111360538], "xDir": [1, 0, 0], "zDir": [0, 0.7778675238708198, -0.6284282897093986]}, "obj": undefined});
        atStep(st, 57);
        cqPolyline(c, st, 57, 56, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279226], [12.360679774997896, -6.010463180279226], [0, 12.02092636055845]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 58);
        cqClose(c, st, 58, 57, {});
        atStep(st, 59);
        cqExtrude(c, st, 59, 58, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 59, "extrude");
        atStep(st, 60);
        cqCut(c, st, 60, 55, {"toCut": {"wp": 59}, "clean": true, "tol": undefined});
        stepVol(c, st, 60, "cut");
        atStep(st, 61);
        cqWorkplaneNew(c, st, 61, {"plane": {"origin": [11.129974753254414, 35.46223736969316, -6.666666666666667], "xDir": [0.9619383577839176, -0.2732665289126717, 0], "zDir": [0.2680348818724428, 0.9435221910385524, -0.1947387406692229]}, "obj": undefined});
        atStep(st, 62);
        cqPolyline(c, st, 62, 61, {"listOfXYTuple": [[-11.946368279335598, -6.7967902039137975], [9.914954033678136, -6.7967902039137975], [2.031414245657456, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 63);
        cqClose(c, st, 63, 62, {});
        atStep(st, 64);
        cqExtrude(c, st, 64, 63, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 64, "extrude");
        atStep(st, 65);
        cqCut(c, st, 65, 60, {"toCut": {"wp": 64}, "clean": true, "tol": undefined});
        stepVol(c, st, 65, "cut");
        atStep(st, 66);
        cqWorkplaneNew(c, st, 66, {"plane": {"origin": [-21.916868011587045, 28.7955707030265, -10.786893258332633], "xDir": [0.3965251774640903, -0.07617803421215459, -0.9148577434446536], "zDir": [-0.583128783201597, 0.7487834503693297, -0.31509390132915405]}, "obj": undefined});
        atStep(st, 67);
        cqPolyline(c, st, 67, 66, {"listOfXYTuple": [[-9.914954033678136, -6.796790203913796], [11.946368279335594, -6.796790203913796], [-2.0314142456574587, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 68);
        cqClose(c, st, 68, 67, {});
        atStep(st, 69);
        cqExtrude(c, st, 69, 68, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 69, "extrude");
        atStep(st, 70);
        cqCut(c, st, 70, 65, {"toCut": {"wp": 69}, "clean": true, "tol": undefined});
        stepVol(c, st, 70, "cut");
        atStep(st, 71);
        cqWorkplaneNew(c, st, 71, {"plane": {"origin": [-21.573786516665265, 21.573786516665265, -21.573786516665265], "xDir": [-0.3090169943749474, -0.8090169943749473, -0.5], "zDir": [-0.5773502691896258, 0.5773502691896258, -0.5773502691896258]}, "obj": undefined});
        atStep(st, 72);
        cqPolyline(c, st, 72, 71, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997898, -7.136441795461799], [-8.881784197001252e-16, 14.272883590923602]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 73);
        cqClose(c, st, 73, 72, {});
        atStep(st, 74);
        cqExtrude(c, st, 74, 73, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 74, "extrude");
        atStep(st, 75);
        cqCut(c, st, 75, 70, {"toCut": {"wp": 74}, "clean": true, "tol": undefined});
        stepVol(c, st, 75, "cut");
        atStep(st, 76);
        cqWorkplaneNew(c, st, 76, {"plane": {"origin": [-28.7955707030265, 10.786893258332633, -21.916868011587045], "xDir": [0.5, -0.30901699437494745, -0.8090169943749473], "zDir": [-0.7487834503693297, 0.31509390132915416, -0.583128783201597]}, "obj": undefined});
        atStep(st, 77);
        cqPolyline(c, st, 77, 76, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279222], [12.360679774997898, -6.010463180279224], [0, 12.020926360558448]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 78);
        cqClose(c, st, 78, 77, {});
        atStep(st, 79);
        cqExtrude(c, st, 79, 78, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 79, "extrude");
        atStep(st, 80);
        cqCut(c, st, 80, 75, {"toCut": {"wp": 79}, "clean": true, "tol": undefined});
        stepVol(c, st, 80, "cut");
        atStep(st, 81);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [-10.786893258332633, 21.916868011587045, -28.7955707030265], "xDir": [0.5654131803198271, -0.5183325659805633, -0.641591214531982], "zDir": [-0.3150939013291541, 0.583128783201597, -0.7487834503693297]}, "obj": undefined});
        atStep(st, 82);
        cqPolyline(c, st, 82, 81, {"listOfXYTuple": [[-11.946368279335598, -6.796790203913799], [9.91495403367814, -6.7967902039138], [2.0314142456574587, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 83);
        cqClose(c, st, 83, 82, {});
        atStep(st, 84);
        cqExtrude(c, st, 84, 83, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 84, "extrude");
        atStep(st, 85);
        cqCut(c, st, 85, 80, {"toCut": {"wp": 84}, "clean": true, "tol": undefined});
        stepVol(c, st, 85, "cut");
        atStep(st, 86);
        cqWorkplaneNew(c, st, 86, {"plane": {"origin": [-28.583534678253713, 24.675344111360538, 0], "xDir": [-0.5183325659805634, -0.641591214531982, -0.5654131803198272], "zDir": [-0.7778675238708198, 0.6284282897093986, 0]}, "obj": undefined});
        atStep(st, 87);
        cqPolyline(c, st, 87, 86, {"listOfXYTuple": [[-9.91495403367814, -6.7967902039137975], [11.946368279335598, -6.796790203913798], [-2.031414245657454, 13.5935804078276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 88);
        cqClose(c, st, 88, 87, {});
        atStep(st, 89);
        cqExtrude(c, st, 89, 88, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 89, "extrude");
        atStep(st, 90);
        cqCut(c, st, 90, 85, {"toCut": {"wp": 89}, "clean": true, "tol": undefined});
        stepVol(c, st, 90, "cut");
        atStep(st, 91);
        cqWorkplaneNew(c, st, 91, {"plane": {"origin": [-34.907119849998594, 13.333333333333334, 0], "xDir": [-0.30901699437494745, -0.8090169943749475, 0.5], "zDir": [-0.9341723589627158, 0.35682208977308993, 0]}, "obj": undefined});
        atStep(st, 92);
        cqPolyline(c, st, 92, 91, {"listOfXYTuple": [[-12.360679774997894, -7.136441795461801], [12.360679774997896, -7.136441795461799], [8.881784197001252e-16, 14.272883590923596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 93);
        cqClose(c, st, 93, 92, {});
        atStep(st, 94);
        cqExtrude(c, st, 94, 93, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 94, "extrude");
        atStep(st, 95);
        cqCut(c, st, 95, 90, {"toCut": {"wp": 94}, "clean": true, "tol": undefined});
        stepVol(c, st, 95, "cut");
        atStep(st, 96);
        cqWorkplaneNew(c, st, 96, {"plane": {"origin": [-35.46223736969316, 6.666666666666667, 11.129974753254414], "xDir": [-0.3090169943749474, -0.8090169943749475, -0.5], "zDir": [-0.9435221910385524, 0.1947387406692229, 0.2680348818724428]}, "obj": undefined});
        atStep(st, 97);
        cqPolyline(c, st, 97, 96, {"listOfXYTuple": [[-12.360679774997894, -6.010463180279225], [12.360679774997898, -6.010463180279223], [1.7763568394002505e-15, 12.020926360558446]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 98);
        cqClose(c, st, 98, 97, {});
        atStep(st, 99);
        cqExtrude(c, st, 99, 98, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 99, "extrude");
        atStep(st, 100);
        cqCut(c, st, 100, 95, {"toCut": {"wp": 99}, "clean": true, "tol": undefined});
        stepVol(c, st, 100, "cut");
        atStep(st, 101);
        cqWorkplaneNew(c, st, 101, {"plane": {"origin": [-35.46223736969316, 6.666666666666667, -11.129974753254414], "xDir": [-0.07617803421215465, -0.9148577434446536, -0.3965251774640903], "zDir": [-0.9435221910385524, 0.1947387406692229, -0.2680348818724428]}, "obj": undefined});
        atStep(st, 102);
        cqPolyline(c, st, 102, 101, {"listOfXYTuple": [[-11.946368279335594, -6.796790203913794], [9.914954033678136, -6.796790203913794], [2.0314142456574578, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 103);
        cqClose(c, st, 103, 102, {});
        atStep(st, 104);
        cqExtrude(c, st, 104, 103, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 104, "extrude");
        atStep(st, 105);
        cqCut(c, st, 105, 100, {"toCut": {"wp": 104}, "clean": true, "tol": undefined});
        stepVol(c, st, 105, "cut");
        atStep(st, 106);
        cqWorkplaneNew(c, st, 106, {"plane": {"origin": [21.916868011587045, 28.7955707030265, 10.786893258332633], "xDir": [-0.3965251774640903, -0.07617803421215459, 0.9148577434446536], "zDir": [0.583128783201597, 0.7487834503693297, 0.31509390132915405]}, "obj": undefined});
        atStep(st, 107);
        cqPolyline(c, st, 107, 106, {"listOfXYTuple": [[-9.914954033678136, -6.796790203913796], [11.946368279335594, -6.796790203913796], [-2.0314142456574587, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 108);
        cqClose(c, st, 108, 107, {});
        atStep(st, 109);
        cqExtrude(c, st, 109, 108, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 109, "extrude");
        atStep(st, 110);
        cqCut(c, st, 110, 105, {"toCut": {"wp": 109}, "clean": true, "tol": undefined});
        stepVol(c, st, 110, "cut");
        atStep(st, 111);
        cqWorkplaneNew(c, st, 111, {"plane": {"origin": [21.573786516665265, 21.573786516665265, 21.573786516665265], "xDir": [0.3090169943749474, -0.8090169943749473, 0.5], "zDir": [0.5773502691896258, 0.5773502691896258, 0.5773502691896258]}, "obj": undefined});
        atStep(st, 112);
        cqPolyline(c, st, 112, 111, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997898, -7.136441795461799], [-8.881784197001252e-16, 14.272883590923602]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 113);
        cqClose(c, st, 113, 112, {});
        atStep(st, 114);
        cqExtrude(c, st, 114, 113, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 114, "extrude");
        atStep(st, 115);
        cqCut(c, st, 115, 110, {"toCut": {"wp": 114}, "clean": true, "tol": undefined});
        stepVol(c, st, 115, "cut");
        atStep(st, 116);
        cqWorkplaneNew(c, st, 116, {"plane": {"origin": [28.7955707030265, 10.786893258332633, 21.916868011587045], "xDir": [-0.5, -0.30901699437494745, 0.8090169943749473], "zDir": [0.7487834503693297, 0.31509390132915416, 0.583128783201597]}, "obj": undefined});
        atStep(st, 117);
        cqPolyline(c, st, 117, 116, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279222], [12.360679774997898, -6.010463180279224], [0, 12.020926360558448]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 118);
        cqClose(c, st, 118, 117, {});
        atStep(st, 119);
        cqExtrude(c, st, 119, 118, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 119, "extrude");
        atStep(st, 120);
        cqCut(c, st, 120, 115, {"toCut": {"wp": 119}, "clean": true, "tol": undefined});
        stepVol(c, st, 120, "cut");
        atStep(st, 121);
        cqWorkplaneNew(c, st, 121, {"plane": {"origin": [10.786893258332633, 21.916868011587045, 28.7955707030265], "xDir": [-0.5654131803198271, -0.5183325659805633, 0.641591214531982], "zDir": [0.3150939013291541, 0.583128783201597, 0.7487834503693297]}, "obj": undefined});
        atStep(st, 122);
        cqPolyline(c, st, 122, 121, {"listOfXYTuple": [[-11.946368279335598, -6.796790203913799], [9.91495403367814, -6.7967902039138], [2.0314142456574587, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 123);
        cqClose(c, st, 123, 122, {});
        atStep(st, 124);
        cqExtrude(c, st, 124, 123, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 124, "extrude");
        atStep(st, 125);
        cqCut(c, st, 125, 120, {"toCut": {"wp": 124}, "clean": true, "tol": undefined});
        stepVol(c, st, 125, "cut");
        atStep(st, 126);
        cqWorkplaneNew(c, st, 126, {"plane": {"origin": [-6.666666666666667, 11.129974753254414, 35.46223736969316], "xDir": [-0.9148577434446536, -0.3965251774640902, -0.07617803421215467], "zDir": [-0.1947387406692229, 0.2680348818724428, 0.9435221910385526]}, "obj": undefined});
        atStep(st, 127);
        cqPolyline(c, st, 127, 126, {"listOfXYTuple": [[-9.914954033678136, -6.796790203913793], [11.946368279335594, -6.796790203913795], [-2.0314142456574578, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 128);
        cqClose(c, st, 128, 127, {});
        atStep(st, 129);
        cqExtrude(c, st, 129, 128, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 129, "extrude");
        atStep(st, 130);
        cqCut(c, st, 130, 125, {"toCut": {"wp": 129}, "clean": true, "tol": undefined});
        stepVol(c, st, 130, "cut");
        atStep(st, 131);
        cqWorkplaneNew(c, st, 131, {"plane": {"origin": [-13.333333333333334, 0, 34.9071198499986], "xDir": [0, -1, 0], "zDir": [-0.35682208977308993, 0, 0.9341723589627158]}, "obj": undefined});
        atStep(st, 132);
        cqPolyline(c, st, 132, 131, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997896, -7.136441795461799], [0, 14.272883590923598]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 133);
        cqClose(c, st, 133, 132, {});
        atStep(st, 134);
        cqExtrude(c, st, 134, 133, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 134, "extrude");
        atStep(st, 135);
        cqCut(c, st, 135, 130, {"toCut": {"wp": 134}, "clean": true, "tol": undefined});
        stepVol(c, st, 135, "cut");
        atStep(st, 136);
        cqWorkplaneNew(c, st, 136, {"plane": {"origin": [-6.666666666666667, -11.129974753254414, 35.46223736969316], "xDir": [-0.8090169943749475, -0.5, -0.30901699437494745], "zDir": [-0.1947387406692229, -0.2680348818724428, 0.9435221910385524]}, "obj": undefined});
        atStep(st, 137);
        cqPolyline(c, st, 137, 136, {"listOfXYTuple": [[-12.360679774997894, -6.010463180279224], [12.360679774997894, -6.010463180279223], [-2.0539125955565396e-15, 12.020926360558445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 138);
        cqClose(c, st, 138, 137, {});
        atStep(st, 139);
        cqExtrude(c, st, 139, 138, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 139, "extrude");
        atStep(st, 140);
        cqCut(c, st, 140, 135, {"toCut": {"wp": 139}, "clean": true, "tol": undefined});
        stepVol(c, st, 140, "cut");
        atStep(st, 141);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [-24.675344111360538, 0, 28.583534678253713], "xDir": [-0.6415912145319819, -0.5654131803198271, -0.5183325659805634], "zDir": [-0.6284282897093986, 0, 0.7778675238708198]}, "obj": undefined});
        atStep(st, 142);
        cqPolyline(c, st, 142, 141, {"listOfXYTuple": [[-11.946368279335596, -6.7967902039137975], [9.91495403367814, -6.7967902039137975], [2.0314142456574547, 13.5935804078276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 143);
        cqClose(c, st, 143, 142, {});
        atStep(st, 144);
        cqExtrude(c, st, 144, 143, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 144, "extrude");
        atStep(st, 145);
        cqCut(c, st, 145, 140, {"toCut": {"wp": 144}, "clean": true, "tol": undefined});
        stepVol(c, st, 145, "cut");
        atStep(st, 146);
        cqWorkplaneNew(c, st, 146, {"plane": {"origin": [-35.46223736969316, -6.666666666666667, 11.129974753254414], "xDir": [-0.2732665289126717, 0, -0.9619383577839176], "zDir": [-0.9435221910385524, -0.1947387406692229, 0.2680348818724428]}, "obj": undefined});
        atStep(st, 147);
        cqPolyline(c, st, 147, 146, {"listOfXYTuple": [[-9.914954033678136, -6.7967902039137975], [11.946368279335598, -6.7967902039137975], [-2.031414245657456, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 148);
        cqClose(c, st, 148, 147, {});
        atStep(st, 149);
        cqExtrude(c, st, 149, 148, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 149, "extrude");
        atStep(st, 150);
        cqCut(c, st, 150, 145, {"toCut": {"wp": 149}, "clean": true, "tol": undefined});
        stepVol(c, st, 150, "cut");
        atStep(st, 151);
        cqWorkplaneNew(c, st, 151, {"plane": {"origin": [-34.907119849998594, -13.333333333333334, 0], "xDir": [0.30901699437494745, -0.8090169943749475, -0.5], "zDir": [-0.9341723589627158, -0.35682208977308993, 0]}, "obj": undefined});
        atStep(st, 152);
        cqPolyline(c, st, 152, 151, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997894, -7.136441795461801], [-8.881784197001252e-16, 14.272883590923596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 153);
        cqClose(c, st, 153, 152, {});
        atStep(st, 154);
        cqExtrude(c, st, 154, 153, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 154, "extrude");
        atStep(st, 155);
        cqCut(c, st, 155, 150, {"toCut": {"wp": 154}, "clean": true, "tol": undefined});
        stepVol(c, st, 155, "cut");
        atStep(st, 156);
        cqWorkplaneNew(c, st, 156, {"plane": {"origin": [-28.583534678253713, -24.675344111360538, 0], "xDir": [0, 0, -1], "zDir": [-0.7778675238708198, -0.6284282897093986, 0]}, "obj": undefined});
        atStep(st, 157);
        cqPolyline(c, st, 157, 156, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279226], [12.360679774997896, -6.010463180279226], [0, 12.02092636055845]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 158);
        cqClose(c, st, 158, 157, {});
        atStep(st, 159);
        cqExtrude(c, st, 159, 158, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 159, "extrude");
        atStep(st, 160);
        cqCut(c, st, 160, 155, {"toCut": {"wp": 159}, "clean": true, "tol": undefined});
        stepVol(c, st, 160, "cut");
        atStep(st, 161);
        cqWorkplaneNew(c, st, 161, {"plane": {"origin": [-35.46223736969316, -6.666666666666667, -11.129974753254414], "xDir": [0.2732665289126717, 0, -0.9619383577839176], "zDir": [-0.9435221910385524, -0.1947387406692229, -0.2680348818724428]}, "obj": undefined});
        atStep(st, 162);
        cqPolyline(c, st, 162, 161, {"listOfXYTuple": [[-11.946368279335598, -6.7967902039137975], [9.914954033678136, -6.7967902039137975], [2.031414245657456, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 163);
        cqClose(c, st, 163, 162, {});
        atStep(st, 164);
        cqExtrude(c, st, 164, 163, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 164, "extrude");
        atStep(st, 165);
        cqCut(c, st, 165, 160, {"toCut": {"wp": 164}, "clean": true, "tol": undefined});
        stepVol(c, st, 165, "cut");
        atStep(st, 166);
        cqWorkplaneNew(c, st, 166, {"plane": {"origin": [-24.675344111360538, 0, -28.583534678253713], "xDir": [0.6415912145319819, 0.5654131803198271, -0.5183325659805634], "zDir": [-0.6284282897093986, 0, -0.7778675238708198]}, "obj": undefined});
        atStep(st, 167);
        cqPolyline(c, st, 167, 166, {"listOfXYTuple": [[-9.91495403367814, -6.7967902039137975], [11.946368279335596, -6.7967902039137975], [-2.0314142456574547, 13.5935804078276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 168);
        cqClose(c, st, 168, 167, {});
        atStep(st, 169);
        cqExtrude(c, st, 169, 168, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 169, "extrude");
        atStep(st, 170);
        cqCut(c, st, 170, 165, {"toCut": {"wp": 169}, "clean": true, "tol": undefined});
        stepVol(c, st, 170, "cut");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [-13.333333333333334, 0, -34.907119849998594], "xDir": [0.8090169943749475, -0.5, -0.30901699437494745], "zDir": [-0.35682208977308993, 0, -0.9341723589627158]}, "obj": undefined});
        atStep(st, 172);
        cqPolyline(c, st, 172, 171, {"listOfXYTuple": [[-12.360679774997893, -7.136441795461801], [12.360679774997896, -7.136441795461799], [1.1102230246251563e-15, 14.272883590923596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 173);
        cqClose(c, st, 173, 172, {});
        atStep(st, 174);
        cqExtrude(c, st, 174, 173, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 174, "extrude");
        atStep(st, 175);
        cqCut(c, st, 175, 170, {"toCut": {"wp": 174}, "clean": true, "tol": undefined});
        stepVol(c, st, 175, "cut");
        atStep(st, 176);
        cqWorkplaneNew(c, st, 176, {"plane": {"origin": [-6.666666666666667, -11.129974753254414, -35.46223736969316], "xDir": [0.8090169943749473, 0.5, -0.3090169943749474], "zDir": [-0.1947387406692229, -0.2680348818724428, -0.9435221910385526]}, "obj": undefined});
        atStep(st, 177);
        cqPolyline(c, st, 177, 176, {"listOfXYTuple": [[-12.360679774997894, -6.010463180279223], [12.360679774997894, -6.010463180279224], [2.1649348980190553e-15, 12.020926360558445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 178);
        cqClose(c, st, 178, 177, {});
        atStep(st, 179);
        cqExtrude(c, st, 179, 178, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 179, "extrude");
        atStep(st, 180);
        cqCut(c, st, 180, 175, {"toCut": {"wp": 179}, "clean": true, "tol": undefined});
        stepVol(c, st, 180, "cut");
        atStep(st, 181);
        cqWorkplaneNew(c, st, 181, {"plane": {"origin": [-6.666666666666667, 11.129974753254414, -35.46223736969316], "xDir": [0.9148577434446536, 0.3965251774640902, -0.07617803421215467], "zDir": [-0.1947387406692229, 0.2680348818724428, -0.9435221910385526]}, "obj": undefined});
        atStep(st, 182);
        cqPolyline(c, st, 182, 181, {"listOfXYTuple": [[-11.946368279335594, -6.796790203913795], [9.914954033678136, -6.796790203913793], [2.0314142456574578, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 183);
        cqClose(c, st, 183, 182, {});
        atStep(st, 184);
        cqExtrude(c, st, 184, 183, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 184, "extrude");
        atStep(st, 185);
        cqCut(c, st, 185, 180, {"toCut": {"wp": 184}, "clean": true, "tol": undefined});
        stepVol(c, st, 185, "cut");
        atStep(st, 186);
        cqWorkplaneNew(c, st, 186, {"plane": {"origin": [10.786893258332633, 21.916868011587045, -28.7955707030265], "xDir": [0.5654131803198271, 0.5183325659805634, 0.6415912145319819], "zDir": [0.31509390132915405, 0.5831287832015969, -0.7487834503693297]}, "obj": undefined});
        atStep(st, 187);
        cqPolyline(c, st, 187, 186, {"listOfXYTuple": [[-9.91495403367814, -6.7967902039138], [11.946368279335598, -6.796790203913798], [-2.0314142456574595, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 188);
        cqClose(c, st, 188, 187, {});
        atStep(st, 189);
        cqExtrude(c, st, 189, 188, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 189, "extrude");
        atStep(st, 190);
        cqCut(c, st, 190, 185, {"toCut": {"wp": 189}, "clean": true, "tol": undefined});
        stepVol(c, st, 190, "cut");
        atStep(st, 191);
        cqWorkplaneNew(c, st, 191, {"plane": {"origin": [21.573786516665265, 21.573786516665265, -21.573786516665265], "xDir": [0.8090169943749475, -0.5000000000000001, 0.30901699437494745], "zDir": [0.5773502691896258, 0.5773502691896258, -0.5773502691896258]}, "obj": undefined});
        atStep(st, 192);
        cqPolyline(c, st, 192, 191, {"listOfXYTuple": [[-12.360679774997898, -7.136441795461799], [12.360679774997896, -7.136441795461799], [4.440892098500626e-16, 14.272883590923602]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 193);
        cqClose(c, st, 193, 192, {});
        atStep(st, 194);
        cqExtrude(c, st, 194, 193, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 194, "extrude");
        atStep(st, 195);
        cqCut(c, st, 195, 190, {"toCut": {"wp": 194}, "clean": true, "tol": undefined});
        stepVol(c, st, 195, "cut");
        atStep(st, 196);
        cqWorkplaneNew(c, st, 196, {"plane": {"origin": [28.7955707030265, 10.786893258332633, -21.916868011587045], "xDir": [0.5, 0.30901699437494745, 0.8090169943749473], "zDir": [0.7487834503693297, 0.3150939013291541, -0.583128783201597]}, "obj": undefined});
        atStep(st, 197);
        cqPolyline(c, st, 197, 196, {"listOfXYTuple": [[-12.360679774997898, -6.010463180279224], [12.360679774997896, -6.010463180279222], [0, 12.020926360558448]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 198);
        cqClose(c, st, 198, 197, {});
        atStep(st, 199);
        cqExtrude(c, st, 199, 198, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 199, "extrude");
        atStep(st, 200);
        cqCut(c, st, 200, 195, {"toCut": {"wp": 199}, "clean": true, "tol": undefined});
        stepVol(c, st, 200, "cut");
        atStep(st, 201);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [21.916868011587045, 28.7955707030265, -10.786893258332633], "xDir": [0.3965251774640903, 0.07617803421215472, 0.9148577434446536], "zDir": [0.583128783201597, 0.7487834503693297, -0.31509390132915416]}, "obj": undefined});
        atStep(st, 202);
        cqPolyline(c, st, 202, 201, {"listOfXYTuple": [[-11.946368279335594, -6.7967902039137975], [9.914954033678136, -6.7967902039137975], [2.031414245657458, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 203);
        cqClose(c, st, 203, 202, {});
        atStep(st, 204);
        cqExtrude(c, st, 204, 203, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 204, "extrude");
        atStep(st, 205);
        cqCut(c, st, 205, 200, {"toCut": {"wp": 204}, "clean": true, "tol": undefined});
        stepVol(c, st, 205, "cut");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 206, {"plane": {"origin": [21.916868011587045, -28.7955707030265, 10.786893258332633], "xDir": [0.5183325659805633, 0.6415912145319819, 0.5654131803198271], "zDir": [0.583128783201597, -0.7487834503693297, 0.31509390132915405]}, "obj": undefined});
        atStep(st, 207);
        cqPolyline(c, st, 207, 206, {"listOfXYTuple": [[-9.91495403367814, -6.796790203913798], [11.946368279335598, -6.7967902039137975], [-2.0314142456574595, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 208);
        cqClose(c, st, 208, 207, {});
        atStep(st, 209);
        cqExtrude(c, st, 209, 208, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 209, "extrude");
        atStep(st, 210);
        cqCut(c, st, 210, 205, {"toCut": {"wp": 209}, "clean": true, "tol": undefined});
        stepVol(c, st, 210, "cut");
        atStep(st, 211);
        cqWorkplaneNew(c, st, 211, {"plane": {"origin": [21.573786516665265, -21.573786516665265, 21.573786516665265], "xDir": [-0.5, 0.30901699437494745, 0.8090169943749475], "zDir": [0.5773502691896258, -0.5773502691896258, 0.5773502691896258]}, "obj": undefined});
        atStep(st, 212);
        cqPolyline(c, st, 212, 211, {"listOfXYTuple": [[-12.360679774997898, -7.1364417954618], [12.360679774997898, -7.136441795461799], [-4.440892098500626e-16, 14.2728835909236]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 213);
        cqClose(c, st, 213, 212, {});
        atStep(st, 214);
        cqExtrude(c, st, 214, 213, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 214, "extrude");
        atStep(st, 215);
        cqCut(c, st, 215, 210, {"toCut": {"wp": 214}, "clean": true, "tol": undefined});
        stepVol(c, st, 215, "cut");
        atStep(st, 216);
        cqWorkplaneNew(c, st, 216, {"plane": {"origin": [10.786893258332633, -21.916868011587045, 28.7955707030265], "xDir": [0.30901699437494745, 0.8090169943749473, 0.5], "zDir": [0.3150939013291541, -0.583128783201597, 0.7487834503693297]}, "obj": undefined});
        atStep(st, 217);
        cqPolyline(c, st, 217, 216, {"listOfXYTuple": [[-12.360679774997898, -6.010463180279224], [12.360679774997896, -6.010463180279222], [0, 12.020926360558448]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 218);
        cqClose(c, st, 218, 217, {});
        atStep(st, 219);
        cqExtrude(c, st, 219, 218, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 219, "extrude");
        atStep(st, 220);
        cqCut(c, st, 220, 215, {"toCut": {"wp": 219}, "clean": true, "tol": undefined});
        stepVol(c, st, 220, "cut");
        atStep(st, 221);
        cqWorkplaneNew(c, st, 221, {"plane": {"origin": [28.7955707030265, -10.786893258332633, 21.916868011587045], "xDir": [0.07617803421215472, 0.9148577434446536, 0.3965251774640903], "zDir": [0.7487834503693297, -0.31509390132915416, 0.583128783201597]}, "obj": undefined});
        atStep(st, 222);
        cqPolyline(c, st, 222, 221, {"listOfXYTuple": [[-11.946368279335594, -6.7967902039137975], [9.914954033678136, -6.7967902039137975], [2.031414245657458, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 223);
        cqClose(c, st, 223, 222, {});
        atStep(st, 224);
        cqExtrude(c, st, 224, 223, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 224, "extrude");
        atStep(st, 225);
        cqCut(c, st, 225, 220, {"toCut": {"wp": 224}, "clean": true, "tol": undefined});
        stepVol(c, st, 225, "cut");
        atStep(st, 226);
        cqWorkplaneNew(c, st, 226, {"plane": {"origin": [11.129974753254414, -35.46223736969316, 6.666666666666667], "xDir": [-0.3965251774640903, 0.07617803421215465, 0.9148577434446536], "zDir": [0.2680348818724428, -0.9435221910385524, 0.1947387406692229]}, "obj": undefined});
        atStep(st, 227);
        cqPolyline(c, st, 227, 226, {"listOfXYTuple": [[-9.914954033678136, -6.796790203913793], [11.946368279335594, -6.796790203913794], [-2.0314142456574578, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 228);
        cqClose(c, st, 228, 227, {});
        atStep(st, 229);
        cqExtrude(c, st, 229, 228, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 229, "extrude");
        atStep(st, 230);
        cqCut(c, st, 230, 225, {"toCut": {"wp": 229}, "clean": true, "tol": undefined});
        stepVol(c, st, 230, "cut");
        atStep(st, 231);
        cqWorkplaneNew(c, st, 231, {"plane": {"origin": [0, -34.9071198499986, 13.333333333333334], "xDir": [-1, 0, 0], "zDir": [0, -0.9341723589627158, 0.35682208977308993]}, "obj": undefined});
        atStep(st, 232);
        cqPolyline(c, st, 232, 231, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997896, -7.136441795461799], [0, 14.272883590923598]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 233);
        cqClose(c, st, 233, 232, {});
        atStep(st, 234);
        cqExtrude(c, st, 234, 233, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 234, "extrude");
        atStep(st, 235);
        cqCut(c, st, 235, 230, {"toCut": {"wp": 234}, "clean": true, "tol": undefined});
        stepVol(c, st, 235, "cut");
        atStep(st, 236);
        cqWorkplaneNew(c, st, 236, {"plane": {"origin": [-11.129974753254414, -35.46223736969316, 6.666666666666667], "xDir": [-0.5, 0.30901699437494745, 0.8090169943749475], "zDir": [-0.2680348818724428, -0.9435221910385524, 0.1947387406692229]}, "obj": undefined});
        atStep(st, 237);
        cqPolyline(c, st, 237, 236, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279224], [12.360679774997894, -6.010463180279223], [-1.7763568394002505e-15, 12.020926360558445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 238);
        cqClose(c, st, 238, 237, {});
        atStep(st, 239);
        cqExtrude(c, st, 239, 238, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 239, "extrude");
        atStep(st, 240);
        cqCut(c, st, 240, 235, {"toCut": {"wp": 239}, "clean": true, "tol": undefined});
        stepVol(c, st, 240, "cut");
        atStep(st, 241);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [0, -28.583534678253713, 24.675344111360538], "xDir": [-0.5654131803198271, 0.5183325659805634, 0.6415912145319819], "zDir": [0, -0.7778675238708198, 0.6284282897093986]}, "obj": undefined});
        atStep(st, 242);
        cqPolyline(c, st, 242, 241, {"listOfXYTuple": [[-11.946368279335596, -6.7967902039137975], [9.91495403367814, -6.7967902039137975], [2.0314142456574547, 13.5935804078276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 243);
        cqClose(c, st, 243, 242, {});
        atStep(st, 244);
        cqExtrude(c, st, 244, 243, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 244, "extrude");
        atStep(st, 245);
        cqCut(c, st, 245, 240, {"toCut": {"wp": 244}, "clean": true, "tol": undefined});
        stepVol(c, st, 245, "cut");
        atStep(st, 246);
        cqWorkplaneNew(c, st, 246, {"plane": {"origin": [11.129974753254414, -35.46223736969316, -6.666666666666667], "xDir": [-0.9619383577839176, -0.2732665289126717, 0], "zDir": [0.2680348818724428, -0.9435221910385524, -0.1947387406692229]}, "obj": undefined});
        atStep(st, 247);
        cqPolyline(c, st, 247, 246, {"listOfXYTuple": [[-9.914954033678136, -6.7967902039137975], [11.946368279335598, -6.7967902039137975], [-2.031414245657456, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 248);
        cqClose(c, st, 248, 247, {});
        atStep(st, 249);
        cqExtrude(c, st, 249, 248, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 249, "extrude");
        atStep(st, 250);
        cqCut(c, st, 250, 245, {"toCut": {"wp": 249}, "clean": true, "tol": undefined});
        stepVol(c, st, 250, "cut");
        atStep(st, 251);
        cqWorkplaneNew(c, st, 251, {"plane": {"origin": [0, -34.907119849998594, -13.333333333333334], "xDir": [-0.5, 0.30901699437494745, -0.8090169943749475], "zDir": [0, -0.9341723589627158, -0.35682208977308993]}, "obj": undefined});
        atStep(st, 252);
        cqPolyline(c, st, 252, 251, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997894, -7.136441795461801], [-8.881784197001252e-16, 14.272883590923596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 253);
        cqClose(c, st, 253, 252, {});
        atStep(st, 254);
        cqExtrude(c, st, 254, 253, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        atStep(st, 255);
        cqCut(c, st, 255, 250, {"toCut": {"wp": 254}, "clean": true, "tol": undefined});
        stepVol(c, st, 255, "cut");
        atStep(st, 256);
        cqWorkplaneNew(c, st, 256, {"plane": {"origin": [0, -28.583534678253713, -24.675344111360538], "xDir": [-1, 0, 0], "zDir": [0, -0.7778675238708198, -0.6284282897093986]}, "obj": undefined});
        atStep(st, 257);
        cqPolyline(c, st, 257, 256, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279226], [12.360679774997896, -6.010463180279226], [0, 12.02092636055845]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 258);
        cqClose(c, st, 258, 257, {});
        atStep(st, 259);
        cqExtrude(c, st, 259, 258, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 259, "extrude");
        atStep(st, 260);
        cqCut(c, st, 260, 255, {"toCut": {"wp": 259}, "clean": true, "tol": undefined});
        stepVol(c, st, 260, "cut");
        atStep(st, 261);
        cqWorkplaneNew(c, st, 261, {"plane": {"origin": [-11.129974753254414, -35.46223736969316, -6.666666666666667], "xDir": [-0.9619383577839176, 0.2732665289126717, 0], "zDir": [-0.2680348818724428, -0.9435221910385524, -0.1947387406692229]}, "obj": undefined});
        atStep(st, 262);
        cqPolyline(c, st, 262, 261, {"listOfXYTuple": [[-11.946368279335598, -6.7967902039137975], [9.914954033678136, -6.7967902039137975], [2.031414245657456, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 263);
        cqClose(c, st, 263, 262, {});
        atStep(st, 264);
        cqExtrude(c, st, 264, 263, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 264, "extrude");
        atStep(st, 265);
        cqCut(c, st, 265, 260, {"toCut": {"wp": 264}, "clean": true, "tol": undefined});
        stepVol(c, st, 265, "cut");
        atStep(st, 266);
        cqWorkplaneNew(c, st, 266, {"plane": {"origin": [21.916868011587045, -28.7955707030265, -10.786893258332633], "xDir": [-0.3965251774640903, 0.07617803421215459, -0.9148577434446536], "zDir": [0.583128783201597, -0.7487834503693297, -0.31509390132915405]}, "obj": undefined});
        atStep(st, 267);
        cqPolyline(c, st, 267, 266, {"listOfXYTuple": [[-9.914954033678136, -6.796790203913796], [11.946368279335594, -6.796790203913796], [-2.0314142456574587, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 268);
        cqClose(c, st, 268, 267, {});
        atStep(st, 269);
        cqExtrude(c, st, 269, 268, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 269, "extrude");
        atStep(st, 270);
        cqCut(c, st, 270, 265, {"toCut": {"wp": 269}, "clean": true, "tol": undefined});
        stepVol(c, st, 270, "cut");
        atStep(st, 271);
        cqWorkplaneNew(c, st, 271, {"plane": {"origin": [21.573786516665265, -21.573786516665265, -21.573786516665265], "xDir": [0.3090169943749474, 0.8090169943749473, -0.5], "zDir": [0.5773502691896258, -0.5773502691896258, -0.5773502691896258]}, "obj": undefined});
        atStep(st, 272);
        cqPolyline(c, st, 272, 271, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997898, -7.136441795461799], [-8.881784197001252e-16, 14.272883590923602]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 273);
        cqClose(c, st, 273, 272, {});
        atStep(st, 274);
        cqExtrude(c, st, 274, 273, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 274, "extrude");
        atStep(st, 275);
        cqCut(c, st, 275, 270, {"toCut": {"wp": 274}, "clean": true, "tol": undefined});
        stepVol(c, st, 275, "cut");
        atStep(st, 276);
        cqWorkplaneNew(c, st, 276, {"plane": {"origin": [28.7955707030265, -10.786893258332633, -21.916868011587045], "xDir": [-0.5, 0.30901699437494745, -0.8090169943749473], "zDir": [0.7487834503693297, -0.31509390132915416, -0.583128783201597]}, "obj": undefined});
        atStep(st, 277);
        cqPolyline(c, st, 277, 276, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279222], [12.360679774997898, -6.010463180279224], [0, 12.020926360558448]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 278);
        cqClose(c, st, 278, 277, {});
        atStep(st, 279);
        cqExtrude(c, st, 279, 278, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 279, "extrude");
        atStep(st, 280);
        cqCut(c, st, 280, 275, {"toCut": {"wp": 279}, "clean": true, "tol": undefined});
        stepVol(c, st, 280, "cut");
        atStep(st, 281);
        cqWorkplaneNew(c, st, 281, {"plane": {"origin": [10.786893258332633, -21.916868011587045, -28.7955707030265], "xDir": [-0.5654131803198271, 0.5183325659805633, -0.641591214531982], "zDir": [0.3150939013291541, -0.583128783201597, -0.7487834503693297]}, "obj": undefined});
        atStep(st, 282);
        cqPolyline(c, st, 282, 281, {"listOfXYTuple": [[-11.946368279335598, -6.796790203913799], [9.91495403367814, -6.7967902039138], [2.0314142456574587, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 283);
        cqClose(c, st, 283, 282, {});
        atStep(st, 284);
        cqExtrude(c, st, 284, 283, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 284, "extrude");
        atStep(st, 285);
        cqCut(c, st, 285, 280, {"toCut": {"wp": 284}, "clean": true, "tol": undefined});
        stepVol(c, st, 285, "cut");
        atStep(st, 286);
        cqWorkplaneNew(c, st, 286, {"plane": {"origin": [28.583534678253713, -24.675344111360538, 0], "xDir": [0.5183325659805634, 0.641591214531982, -0.5654131803198272], "zDir": [0.7778675238708198, -0.6284282897093986, 0]}, "obj": undefined});
        atStep(st, 287);
        cqPolyline(c, st, 287, 286, {"listOfXYTuple": [[-9.91495403367814, -6.7967902039137975], [11.946368279335598, -6.796790203913798], [-2.031414245657454, 13.5935804078276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 288);
        cqClose(c, st, 288, 287, {});
        atStep(st, 289);
        cqExtrude(c, st, 289, 288, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 289, "extrude");
        atStep(st, 290);
        cqCut(c, st, 290, 285, {"toCut": {"wp": 289}, "clean": true, "tol": undefined});
        stepVol(c, st, 290, "cut");
        atStep(st, 291);
        cqWorkplaneNew(c, st, 291, {"plane": {"origin": [34.907119849998594, -13.333333333333334, 0], "xDir": [0.30901699437494745, 0.8090169943749475, 0.5], "zDir": [0.9341723589627158, -0.35682208977308993, 0]}, "obj": undefined});
        atStep(st, 292);
        cqPolyline(c, st, 292, 291, {"listOfXYTuple": [[-12.360679774997894, -7.136441795461801], [12.360679774997896, -7.136441795461799], [8.881784197001252e-16, 14.272883590923596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 293);
        cqClose(c, st, 293, 292, {});
        atStep(st, 294);
        cqExtrude(c, st, 294, 293, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 294, "extrude");
        atStep(st, 295);
        cqCut(c, st, 295, 290, {"toCut": {"wp": 294}, "clean": true, "tol": undefined});
        stepVol(c, st, 295, "cut");
        atStep(st, 296);
        cqWorkplaneNew(c, st, 296, {"plane": {"origin": [35.46223736969316, -6.666666666666667, 11.129974753254414], "xDir": [0.3090169943749474, 0.8090169943749475, -0.5], "zDir": [0.9435221910385524, -0.1947387406692229, 0.2680348818724428]}, "obj": undefined});
        atStep(st, 297);
        cqPolyline(c, st, 297, 296, {"listOfXYTuple": [[-12.360679774997894, -6.010463180279225], [12.360679774997898, -6.010463180279223], [1.7763568394002505e-15, 12.020926360558446]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 298);
        cqClose(c, st, 298, 297, {});
        atStep(st, 299);
        cqExtrude(c, st, 299, 298, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 299, "extrude");
        atStep(st, 300);
        cqCut(c, st, 300, 295, {"toCut": {"wp": 299}, "clean": true, "tol": undefined});
        stepVol(c, st, 300, "cut");
        atStep(st, 301);
        cqWorkplaneNew(c, st, 301, {"plane": {"origin": [35.46223736969316, -6.666666666666667, -11.129974753254414], "xDir": [0.07617803421215465, 0.9148577434446536, -0.3965251774640903], "zDir": [0.9435221910385524, -0.1947387406692229, -0.2680348818724428]}, "obj": undefined});
        atStep(st, 302);
        cqPolyline(c, st, 302, 301, {"listOfXYTuple": [[-11.946368279335594, -6.796790203913794], [9.914954033678136, -6.796790203913794], [2.0314142456574578, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 303);
        cqClose(c, st, 303, 302, {});
        atStep(st, 304);
        cqExtrude(c, st, 304, 303, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 304, "extrude");
        atStep(st, 305);
        cqCut(c, st, 305, 300, {"toCut": {"wp": 304}, "clean": true, "tol": undefined});
        stepVol(c, st, 305, "cut");
        atStep(st, 306);
        cqWorkplaneNew(c, st, 306, {"plane": {"origin": [6.666666666666667, -11.129974753254414, 35.46223736969316], "xDir": [0.9148577434446536, 0.3965251774640902, -0.07617803421215467], "zDir": [0.1947387406692229, -0.2680348818724428, 0.9435221910385526]}, "obj": undefined});
        atStep(st, 307);
        cqPolyline(c, st, 307, 306, {"listOfXYTuple": [[-9.914954033678136, -6.796790203913793], [11.946368279335594, -6.796790203913795], [-2.0314142456574578, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 308);
        cqClose(c, st, 308, 307, {});
        atStep(st, 309);
        cqExtrude(c, st, 309, 308, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 309, "extrude");
        atStep(st, 310);
        cqCut(c, st, 310, 305, {"toCut": {"wp": 309}, "clean": true, "tol": undefined});
        stepVol(c, st, 310, "cut");
        atStep(st, 311);
        cqWorkplaneNew(c, st, 311, {"plane": {"origin": [13.333333333333334, 0, 34.9071198499986], "xDir": [0, 1, 0], "zDir": [0.35682208977308993, 0, 0.9341723589627158]}, "obj": undefined});
        atStep(st, 312);
        cqPolyline(c, st, 312, 311, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997896, -7.136441795461799], [0, 14.272883590923598]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 313);
        cqClose(c, st, 313, 312, {});
        atStep(st, 314);
        cqExtrude(c, st, 314, 313, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 314, "extrude");
        atStep(st, 315);
        cqCut(c, st, 315, 310, {"toCut": {"wp": 314}, "clean": true, "tol": undefined});
        stepVol(c, st, 315, "cut");
        atStep(st, 316);
        cqWorkplaneNew(c, st, 316, {"plane": {"origin": [6.666666666666667, 11.129974753254414, 35.46223736969316], "xDir": [0.8090169943749475, 0.5, -0.30901699437494745], "zDir": [0.1947387406692229, 0.2680348818724428, 0.9435221910385524]}, "obj": undefined});
        atStep(st, 317);
        cqPolyline(c, st, 317, 316, {"listOfXYTuple": [[-12.360679774997894, -6.010463180279224], [12.360679774997894, -6.010463180279223], [-2.0539125955565396e-15, 12.020926360558445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 318);
        cqClose(c, st, 318, 317, {});
        atStep(st, 319);
        cqExtrude(c, st, 319, 318, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 319, "extrude");
        atStep(st, 320);
        cqCut(c, st, 320, 315, {"toCut": {"wp": 319}, "clean": true, "tol": undefined});
        stepVol(c, st, 320, "cut");
        atStep(st, 321);
        cqWorkplaneNew(c, st, 321, {"plane": {"origin": [24.675344111360538, 0, 28.583534678253713], "xDir": [0.6415912145319819, 0.5654131803198271, -0.5183325659805634], "zDir": [0.6284282897093986, 0, 0.7778675238708198]}, "obj": undefined});
        atStep(st, 322);
        cqPolyline(c, st, 322, 321, {"listOfXYTuple": [[-11.946368279335596, -6.7967902039137975], [9.91495403367814, -6.7967902039137975], [2.0314142456574547, 13.5935804078276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 323);
        cqClose(c, st, 323, 322, {});
        atStep(st, 324);
        cqExtrude(c, st, 324, 323, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 324, "extrude");
        atStep(st, 325);
        cqCut(c, st, 325, 320, {"toCut": {"wp": 324}, "clean": true, "tol": undefined});
        stepVol(c, st, 325, "cut");
        atStep(st, 326);
        cqWorkplaneNew(c, st, 326, {"plane": {"origin": [-21.916868011587045, -28.7955707030265, 10.786893258332633], "xDir": [0.3965251774640903, 0.07617803421215459, 0.9148577434446536], "zDir": [-0.583128783201597, -0.7487834503693297, 0.31509390132915405]}, "obj": undefined});
        atStep(st, 327);
        cqPolyline(c, st, 327, 326, {"listOfXYTuple": [[-9.914954033678136, -6.796790203913796], [11.946368279335594, -6.796790203913796], [-2.0314142456574587, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 328);
        cqClose(c, st, 328, 327, {});
        atStep(st, 329);
        cqExtrude(c, st, 329, 328, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 329, "extrude");
        atStep(st, 330);
        cqCut(c, st, 330, 325, {"toCut": {"wp": 329}, "clean": true, "tol": undefined});
        stepVol(c, st, 330, "cut");
        atStep(st, 331);
        cqWorkplaneNew(c, st, 331, {"plane": {"origin": [-21.573786516665265, -21.573786516665265, 21.573786516665265], "xDir": [-0.3090169943749474, 0.8090169943749473, 0.5], "zDir": [-0.5773502691896258, -0.5773502691896258, 0.5773502691896258]}, "obj": undefined});
        atStep(st, 332);
        cqPolyline(c, st, 332, 331, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997898, -7.136441795461799], [-8.881784197001252e-16, 14.272883590923602]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 333);
        cqClose(c, st, 333, 332, {});
        atStep(st, 334);
        cqExtrude(c, st, 334, 333, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 334, "extrude");
        atStep(st, 335);
        cqCut(c, st, 335, 330, {"toCut": {"wp": 334}, "clean": true, "tol": undefined});
        stepVol(c, st, 335, "cut");
        atStep(st, 336);
        cqWorkplaneNew(c, st, 336, {"plane": {"origin": [-28.7955707030265, -10.786893258332633, 21.916868011587045], "xDir": [0.5, 0.30901699437494745, 0.8090169943749473], "zDir": [-0.7487834503693297, -0.31509390132915416, 0.583128783201597]}, "obj": undefined});
        atStep(st, 337);
        cqPolyline(c, st, 337, 336, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279222], [12.360679774997898, -6.010463180279224], [0, 12.020926360558448]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 338);
        cqClose(c, st, 338, 337, {});
        atStep(st, 339);
        cqExtrude(c, st, 339, 338, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 339, "extrude");
        atStep(st, 340);
        cqCut(c, st, 340, 335, {"toCut": {"wp": 339}, "clean": true, "tol": undefined});
        stepVol(c, st, 340, "cut");
        atStep(st, 341);
        cqWorkplaneNew(c, st, 341, {"plane": {"origin": [-10.786893258332633, -21.916868011587045, 28.7955707030265], "xDir": [0.5654131803198271, 0.5183325659805633, 0.641591214531982], "zDir": [-0.3150939013291541, -0.583128783201597, 0.7487834503693297]}, "obj": undefined});
        atStep(st, 342);
        cqPolyline(c, st, 342, 341, {"listOfXYTuple": [[-11.946368279335598, -6.796790203913799], [9.91495403367814, -6.7967902039138], [2.0314142456574587, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 343);
        cqClose(c, st, 343, 342, {});
        atStep(st, 344);
        cqExtrude(c, st, 344, 343, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 344, "extrude");
        atStep(st, 345);
        cqCut(c, st, 345, 340, {"toCut": {"wp": 344}, "clean": true, "tol": undefined});
        stepVol(c, st, 345, "cut");
        atStep(st, 346);
        cqWorkplaneNew(c, st, 346, {"plane": {"origin": [-10.786893258332633, -21.916868011587045, -28.7955707030265], "xDir": [-0.5654131803198271, -0.5183325659805634, 0.6415912145319819], "zDir": [-0.31509390132915405, -0.5831287832015969, -0.7487834503693297]}, "obj": undefined});
        atStep(st, 347);
        cqPolyline(c, st, 347, 346, {"listOfXYTuple": [[-9.91495403367814, -6.7967902039138], [11.946368279335598, -6.796790203913798], [-2.0314142456574595, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 348);
        cqClose(c, st, 348, 347, {});
        atStep(st, 349);
        cqExtrude(c, st, 349, 348, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 349, "extrude");
        atStep(st, 350);
        cqCut(c, st, 350, 345, {"toCut": {"wp": 349}, "clean": true, "tol": undefined});
        stepVol(c, st, 350, "cut");
        atStep(st, 351);
        cqWorkplaneNew(c, st, 351, {"plane": {"origin": [-21.573786516665265, -21.573786516665265, -21.573786516665265], "xDir": [-0.8090169943749475, 0.5000000000000001, 0.30901699437494745], "zDir": [-0.5773502691896258, -0.5773502691896258, -0.5773502691896258]}, "obj": undefined});
        atStep(st, 352);
        cqPolyline(c, st, 352, 351, {"listOfXYTuple": [[-12.360679774997898, -7.136441795461799], [12.360679774997896, -7.136441795461799], [4.440892098500626e-16, 14.272883590923602]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 353);
        cqClose(c, st, 353, 352, {});
        atStep(st, 354);
        cqExtrude(c, st, 354, 353, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 354, "extrude");
        atStep(st, 355);
        cqCut(c, st, 355, 350, {"toCut": {"wp": 354}, "clean": true, "tol": undefined});
        stepVol(c, st, 355, "cut");
        atStep(st, 356);
        cqWorkplaneNew(c, st, 356, {"plane": {"origin": [-28.7955707030265, -10.786893258332633, -21.916868011587045], "xDir": [-0.5, -0.30901699437494745, 0.8090169943749473], "zDir": [-0.7487834503693297, -0.3150939013291541, -0.583128783201597]}, "obj": undefined});
        atStep(st, 357);
        cqPolyline(c, st, 357, 356, {"listOfXYTuple": [[-12.360679774997898, -6.010463180279224], [12.360679774997896, -6.010463180279222], [0, 12.020926360558448]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 358);
        cqClose(c, st, 358, 357, {});
        atStep(st, 359);
        cqExtrude(c, st, 359, 358, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 359, "extrude");
        atStep(st, 360);
        cqCut(c, st, 360, 355, {"toCut": {"wp": 359}, "clean": true, "tol": undefined});
        stepVol(c, st, 360, "cut");
        atStep(st, 361);
        cqWorkplaneNew(c, st, 361, {"plane": {"origin": [-21.916868011587045, -28.7955707030265, -10.786893258332633], "xDir": [-0.3965251774640903, -0.07617803421215472, 0.9148577434446536], "zDir": [-0.583128783201597, -0.7487834503693297, -0.31509390132915416]}, "obj": undefined});
        atStep(st, 362);
        cqPolyline(c, st, 362, 361, {"listOfXYTuple": [[-11.946368279335594, -6.7967902039137975], [9.914954033678136, -6.7967902039137975], [2.031414245657458, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 363);
        cqClose(c, st, 363, 362, {});
        atStep(st, 364);
        cqExtrude(c, st, 364, 363, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 364, "extrude");
        atStep(st, 365);
        cqCut(c, st, 365, 360, {"toCut": {"wp": 364}, "clean": true, "tol": undefined});
        stepVol(c, st, 365, "cut");
        atStep(st, 366);
        cqWorkplaneNew(c, st, 366, {"plane": {"origin": [24.675344111360538, 0, -28.583534678253713], "xDir": [-0.6415912145319819, -0.5654131803198271, -0.5183325659805634], "zDir": [0.6284282897093986, 0, -0.7778675238708198]}, "obj": undefined});
        atStep(st, 367);
        cqPolyline(c, st, 367, 366, {"listOfXYTuple": [[-9.91495403367814, -6.7967902039137975], [11.946368279335596, -6.7967902039137975], [-2.0314142456574547, 13.5935804078276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 368);
        cqClose(c, st, 368, 367, {});
        atStep(st, 369);
        cqExtrude(c, st, 369, 368, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 369, "extrude");
        atStep(st, 370);
        cqCut(c, st, 370, 365, {"toCut": {"wp": 369}, "clean": true, "tol": undefined});
        stepVol(c, st, 370, "cut");
        atStep(st, 371);
        cqWorkplaneNew(c, st, 371, {"plane": {"origin": [13.333333333333334, 0, -34.907119849998594], "xDir": [-0.8090169943749475, 0.5, -0.30901699437494745], "zDir": [0.35682208977308993, 0, -0.9341723589627158]}, "obj": undefined});
        atStep(st, 372);
        cqPolyline(c, st, 372, 371, {"listOfXYTuple": [[-12.360679774997893, -7.136441795461801], [12.360679774997896, -7.136441795461799], [1.1102230246251563e-15, 14.272883590923596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 373);
        cqClose(c, st, 373, 372, {});
        atStep(st, 374);
        cqExtrude(c, st, 374, 373, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 374, "extrude");
        atStep(st, 375);
        cqCut(c, st, 375, 370, {"toCut": {"wp": 374}, "clean": true, "tol": undefined});
        stepVol(c, st, 375, "cut");
        atStep(st, 376);
        cqWorkplaneNew(c, st, 376, {"plane": {"origin": [6.666666666666667, 11.129974753254414, -35.46223736969316], "xDir": [-0.8090169943749473, -0.5, -0.3090169943749474], "zDir": [0.1947387406692229, 0.2680348818724428, -0.9435221910385526]}, "obj": undefined});
        atStep(st, 377);
        cqPolyline(c, st, 377, 376, {"listOfXYTuple": [[-12.360679774997894, -6.010463180279223], [12.360679774997894, -6.010463180279224], [2.1649348980190553e-15, 12.020926360558445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 378);
        cqClose(c, st, 378, 377, {});
        atStep(st, 379);
        cqExtrude(c, st, 379, 378, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 379, "extrude");
        atStep(st, 380);
        cqCut(c, st, 380, 375, {"toCut": {"wp": 379}, "clean": true, "tol": undefined});
        stepVol(c, st, 380, "cut");
        atStep(st, 381);
        cqWorkplaneNew(c, st, 381, {"plane": {"origin": [6.666666666666667, -11.129974753254414, -35.46223736969316], "xDir": [-0.9148577434446536, -0.3965251774640902, -0.07617803421215467], "zDir": [0.1947387406692229, -0.2680348818724428, -0.9435221910385526]}, "obj": undefined});
        atStep(st, 382);
        cqPolyline(c, st, 382, 381, {"listOfXYTuple": [[-11.946368279335594, -6.796790203913795], [9.914954033678136, -6.796790203913793], [2.0314142456574578, 13.593580407827597]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 383);
        cqClose(c, st, 383, 382, {});
        atStep(st, 384);
        cqExtrude(c, st, 384, 383, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 384, "extrude");
        atStep(st, 385);
        cqCut(c, st, 385, 380, {"toCut": {"wp": 384}, "clean": true, "tol": undefined});
        stepVol(c, st, 385, "cut");
        atStep(st, 386);
        cqWorkplaneNew(c, st, 386, {"plane": {"origin": [35.46223736969316, 6.666666666666667, 11.129974753254414], "xDir": [0.2732665289126717, 0, -0.9619383577839176], "zDir": [0.9435221910385524, 0.1947387406692229, 0.2680348818724428]}, "obj": undefined});
        atStep(st, 387);
        cqPolyline(c, st, 387, 386, {"listOfXYTuple": [[-9.914954033678136, -6.7967902039137975], [11.946368279335598, -6.7967902039137975], [-2.031414245657456, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 388);
        cqClose(c, st, 388, 387, {});
        atStep(st, 389);
        cqExtrude(c, st, 389, 388, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 389, "extrude");
        atStep(st, 390);
        cqCut(c, st, 390, 385, {"toCut": {"wp": 389}, "clean": true, "tol": undefined});
        stepVol(c, st, 390, "cut");
        atStep(st, 391);
        cqWorkplaneNew(c, st, 391, {"plane": {"origin": [34.907119849998594, 13.333333333333334, 0], "xDir": [-0.30901699437494745, 0.8090169943749475, -0.5], "zDir": [0.9341723589627158, 0.35682208977308993, 0]}, "obj": undefined});
        atStep(st, 392);
        cqPolyline(c, st, 392, 391, {"listOfXYTuple": [[-12.360679774997896, -7.136441795461799], [12.360679774997894, -7.136441795461801], [-8.881784197001252e-16, 14.272883590923596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 393);
        cqClose(c, st, 393, 392, {});
        atStep(st, 394);
        cqExtrude(c, st, 394, 393, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 394, "extrude");
        atStep(st, 395);
        cqCut(c, st, 395, 390, {"toCut": {"wp": 394}, "clean": true, "tol": undefined});
        stepVol(c, st, 395, "cut");
        atStep(st, 396);
        cqWorkplaneNew(c, st, 396, {"plane": {"origin": [28.583534678253713, 24.675344111360538, 0], "xDir": [0, 0, -1], "zDir": [0.7778675238708198, 0.6284282897093986, 0]}, "obj": undefined});
        atStep(st, 397);
        cqPolyline(c, st, 397, 396, {"listOfXYTuple": [[-12.360679774997896, -6.010463180279226], [12.360679774997896, -6.010463180279226], [0, 12.02092636055845]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 398);
        cqClose(c, st, 398, 397, {});
        atStep(st, 399);
        cqExtrude(c, st, 399, 398, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 399, "extrude");
        atStep(st, 400);
        cqCut(c, st, 400, 395, {"toCut": {"wp": 399}, "clean": true, "tol": undefined});
        stepVol(c, st, 400, "cut");
        atStep(st, 401);
        cqWorkplaneNew(c, st, 401, {"plane": {"origin": [35.46223736969316, 6.666666666666667, -11.129974753254414], "xDir": [-0.2732665289126717, 0, -0.9619383577839176], "zDir": [0.9435221910385524, 0.1947387406692229, -0.2680348818724428]}, "obj": undefined});
        atStep(st, 402);
        cqPolyline(c, st, 402, 401, {"listOfXYTuple": [[-11.946368279335598, -6.7967902039137975], [9.914954033678136, -6.7967902039137975], [2.031414245657456, 13.593580407827591]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 403);
        cqClose(c, st, 403, 402, {});
        atStep(st, 404);
        cqExtrude(c, st, 404, 403, {"until": 1.501, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 404, "extrude");
        atStep(st, 405);
        cqCut(c, st, 405, 400, {"toCut": {"wp": 404}, "clean": true, "tol": undefined});
        stepVol(c, st, 405, "cut");
        atStep(st, 406);
        cqWorkplaneNew(c, st, 406, {"plane": {"origin": [10.514622, 37.013016, 0], "xDir": [0.9619383591156128, -0.2732665242249084, 0], "zDir": [0.2732665242249084, 0.9619383591156128, 0]}, "obj": undefined});
        atStep(st, 407);
        cqRect(c, st, 407, 406, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 408);
        cqExtrude(c, st, 408, 407, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 408, "extrude");
        atStep(st, 409);
        cqWorkplaneNew(c, st, 409, {"plane": {"origin": [-22.36068, 26.18034, 16.18034], "xDir": [0.8090169929686842, 0.5000000082324079, 0.3090169847362764], "zDir": [-0.5877852542280283, 0.6881909585341867, 0.4253254042540716]}, "obj": undefined});
        atStep(st, 410);
        cqRect(c, st, 410, 409, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 411);
        cqExtrude(c, st, 411, 410, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 411, "extrude");
        atStep(st, 412);
        cqUnion(c, st, 412, 408, {"toUnion": {"wp": 411}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 412, "union");
        atStep(st, 413);
        cqWorkplaneNew(c, st, 413, {"plane": {"origin": [10, -16.694962, 33.193356], "xDir": [0.914857747234018, 0.396525167186828, -0.07617804219954263], "zDir": [0.2598919137061835, -0.4338885623432013, 0.8626684813170629]}, "obj": undefined});
        atStep(st, 414);
        cqRect(c, st, 414, 413, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 415);
        cqExtrude(c, st, 415, 414, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 415, "extrude");
        atStep(st, 416);
        cqUnion(c, st, 416, 412, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        atStep(st, 417);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [-20, 0, -32.36068], "xDir": [0, 1, 0], "zDir": [-0.5257311094740752, 0, -0.8506508099867759]}, "obj": undefined});
        atStep(st, 418);
        cqRect(c, st, 418, 417, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 419);
        cqExtrude(c, st, 419, 418, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 419, "extrude");
        atStep(st, 420);
        cqUnion(c, st, 420, 416, {"toUnion": {"wp": 419}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 420, "union");
        atStep(st, 421);
        cqWorkplaneNew(c, st, 421, {"plane": {"origin": [37.013016, 0, 10.514622], "xDir": [0.2732665242249084, 0, -0.9619383591156128], "zDir": [0.9619383591156128, 0, 0.2732665242249084]}, "obj": undefined});
        atStep(st, 422);
        cqRect(c, st, 422, 421, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 423);
        cqExtrude(c, st, 423, 422, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 423, "extrude");
        atStep(st, 424);
        cqUnion(c, st, 424, 420, {"toUnion": {"wp": 423}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 424, "union");
        atStep(st, 425);
        cqWorkplaneNew(c, st, 425, {"plane": {"origin": [6.18034, 36.18034, 10], "xDir": [0.5000000064941709, -0.3090169949120921, 0.809016990156158], "zDir": [0.162459850538744, 0.9510565161206248, 0.2628655551939602]}, "obj": undefined});
        atStep(st, 426);
        cqRect(c, st, 426, 425, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 427);
        cqExtrude(c, st, 427, 426, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 427, "extrude");
        atStep(st, 428);
        cqUnion(c, st, 428, 424, {"toUnion": {"wp": 427}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 428, "union");
        atStep(st, 429);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [26.18034, -16.18034, -22.36068], "xDir": [0.5000000082324079, -0.3090169847362764, 0.8090169929686842], "zDir": [0.6881909585341867, -0.4253254042540716, -0.5877852542280283]}, "obj": undefined});
        atStep(st, 430);
        cqRect(c, st, 430, 429, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 431);
        cqExtrude(c, st, 431, 430, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 431, "extrude");
        atStep(st, 432);
        cqUnion(c, st, 432, 428, {"toUnion": {"wp": 431}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        atStep(st, 433);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [-10.514622, 37.013016, 0], "xDir": [0.9619383591156128, 0.2732665242249084, 0], "zDir": [-0.2732665242249084, 0.9619383591156128, 0]}, "obj": undefined});
        atStep(st, 434);
        cqRect(c, st, 434, 433, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 435);
        cqExtrude(c, st, 435, 434, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 435, "extrude");
        atStep(st, 436);
        cqUnion(c, st, 436, 432, {"toUnion": {"wp": 435}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 436, "union");
        atStep(st, 437);
        cqWorkplaneNew(c, st, 437, {"plane": {"origin": [-26.18034, 16.18034, 22.36068], "xDir": [0.5000000082324079, -0.3090169847362764, 0.8090169929686842], "zDir": [-0.6881909585341867, 0.4253254042540716, 0.5877852542280283]}, "obj": undefined});
        atStep(st, 438);
        cqRect(c, st, 438, 437, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 439);
        cqExtrude(c, st, 439, 438, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 439, "extrude");
        atStep(st, 440);
        cqUnion(c, st, 440, 436, {"toUnion": {"wp": 439}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 440, "union");
        atStep(st, 441);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [-10, 16.694962, -33.193356], "xDir": [0.914857747234018, 0.396525167186828, -0.07617804219954263], "zDir": [-0.2598919137061835, 0.4338885623432013, -0.8626684813170629]}, "obj": undefined});
        atStep(st, 442);
        cqRect(c, st, 442, 441, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 443);
        cqExtrude(c, st, 443, 442, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 443, "extrude");
        atStep(st, 444);
        cqUnion(c, st, 444, 440, {"toUnion": {"wp": 443}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 444, "union");
        atStep(st, 445);
        cqWorkplaneNew(c, st, 445, {"plane": {"origin": [0, -10.514622, 37.013016], "xDir": [0, 0.9619383591156128, 0.2732665242249084], "zDir": [0, -0.2732665242249084, 0.9619383591156128]}, "obj": undefined});
        atStep(st, 446);
        cqRect(c, st, 446, 445, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 447);
        cqExtrude(c, st, 447, 446, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 447, "extrude");
        atStep(st, 448);
        cqUnion(c, st, 448, 444, {"toUnion": {"wp": 447}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 448, "union");
        atStep(st, 449);
        cqWorkplaneNew(c, st, 449, {"plane": {"origin": [26.694962, -27.013016, 6.18034], "xDir": [0.5183325657073299, 0.6415912174037162, 0.5654131773116665], "zDir": [0.6937804775983331, -0.7020464438889784, 0.1606220393540954]}, "obj": undefined});
        atStep(st, 450);
        cqRect(c, st, 450, 449, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 451);
        cqExtrude(c, st, 451, 450, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 451, "extrude");
        atStep(st, 452);
        cqUnion(c, st, 452, 448, {"toUnion": {"wp": 451}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 452, "union");
        atStep(st, 453);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [27.013016, -6.18034, 26.694962], "xDir": [0.6415912174037162, 0.5654131773116665, -0.5183325657073299], "zDir": [0.7020464438889784, -0.1606220393540954, 0.6937804775983331]}, "obj": undefined});
        atStep(st, 454);
        cqRect(c, st, 454, 453, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 455);
        cqExtrude(c, st, 455, 454, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 455, "extrude");
        atStep(st, 456);
        cqUnion(c, st, 456, 452, {"toUnion": {"wp": 455}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 456, "union");
        atStep(st, 457);
        cqWorkplaneNew(c, st, 457, {"plane": {"origin": [-27.013016, 6.18034, 26.694962], "xDir": [0.6415912174037162, 0.5654131773116665, 0.5183325657073299], "zDir": [-0.7020464438889784, 0.1606220393540954, 0.6937804775983331]}, "obj": undefined});
        atStep(st, 458);
        cqRect(c, st, 458, 457, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 459);
        cqExtrude(c, st, 459, 458, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 459, "extrude");
        atStep(st, 460);
        cqUnion(c, st, 460, 456, {"toUnion": {"wp": 459}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 460, "union");
        atStep(st, 461);
        cqWorkplaneNew(c, st, 461, {"plane": {"origin": [26.694962, 27.013016, 6.18034], "xDir": [0.5183325657073299, -0.6415912174037162, 0.5654131773116665], "zDir": [0.6937804775983331, 0.7020464438889784, 0.1606220393540954]}, "obj": undefined});
        atStep(st, 462);
        cqRect(c, st, 462, 461, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 463);
        cqExtrude(c, st, 463, 462, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 463, "extrude");
        atStep(st, 464);
        cqUnion(c, st, 464, 460, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 464, "union");
        atStep(st, 465);
        cqWorkplaneNew(c, st, 465, {"plane": {"origin": [20, 0, -32.36068], "xDir": [0, 1, 0], "zDir": [0.5257311094740752, 0, -0.8506508099867759]}, "obj": undefined});
        atStep(st, 466);
        cqRect(c, st, 466, 465, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 467);
        cqExtrude(c, st, 467, 466, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 467, "extrude");
        atStep(st, 468);
        cqUnion(c, st, 468, 464, {"toUnion": {"wp": 467}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 468, "union");
        atStep(st, 469);
        cqWorkplaneNew(c, st, 469, {"plane": {"origin": [26.694962, 27.013016, -6.18034], "xDir": [0.5183325657073299, -0.6415912174037162, -0.5654131773116665], "zDir": [0.6937804775983331, 0.7020464438889784, -0.1606220393540954]}, "obj": undefined});
        atStep(st, 470);
        cqRect(c, st, 470, 469, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 471);
        cqExtrude(c, st, 471, 470, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 471, "extrude");
        atStep(st, 472);
        cqUnion(c, st, 472, 468, {"toUnion": {"wp": 471}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 472, "union");
        atStep(st, 473);
        cqWorkplaneNew(c, st, 473, {"plane": {"origin": [-37.013016, 0, -10.514622], "xDir": [0.2732665242249084, 0, -0.9619383591156128], "zDir": [-0.9619383591156128, 0, -0.2732665242249084]}, "obj": undefined});
        atStep(st, 474);
        cqRect(c, st, 474, 473, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 475);
        cqExtrude(c, st, 475, 474, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 475, "extrude");
        atStep(st, 476);
        cqUnion(c, st, 476, 472, {"toUnion": {"wp": 475}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 476, "union");
        atStep(st, 477);
        cqWorkplaneNew(c, st, 477, {"plane": {"origin": [-6.18034, -26.694962, -27.013016], "xDir": [0.5654131773116665, 0.5183325657073299, -0.6415912174037162], "zDir": [-0.1606220393540954, -0.6937804775983331, -0.7020464438889784]}, "obj": undefined});
        atStep(st, 478);
        cqRect(c, st, 478, 477, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 479);
        cqExtrude(c, st, 479, 478, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 479, "extrude");
        atStep(st, 480);
        cqUnion(c, st, 480, 476, {"toUnion": {"wp": 479}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 480, "union");
        atStep(st, 481);
        cqWorkplaneNew(c, st, 481, {"plane": {"origin": [-37.013016, 0, 10.514622], "xDir": [0.2732665242249084, 0, 0.9619383591156128], "zDir": [-0.9619383591156128, 0, 0.2732665242249084]}, "obj": undefined});
        atStep(st, 482);
        cqRect(c, st, 482, 481, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 483);
        cqExtrude(c, st, 483, 482, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 483, "extrude");
        atStep(st, 484);
        cqUnion(c, st, 484, 480, {"toUnion": {"wp": 483}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 484, "union");
        atStep(st, 485);
        cqWorkplaneNew(c, st, 485, {"plane": {"origin": [22.36068, 26.18034, 16.18034], "xDir": [0.8090169929686842, -0.5000000082324079, -0.3090169847362764], "zDir": [0.5877852542280283, 0.6881909585341867, 0.4253254042540716]}, "obj": undefined});
        atStep(st, 486);
        cqRect(c, st, 486, 485, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 487);
        cqExtrude(c, st, 487, 486, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 487, "extrude");
        atStep(st, 488);
        cqUnion(c, st, 488, 484, {"toUnion": {"wp": 487}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 488, "union");
        atStep(st, 489);
        cqWorkplaneNew(c, st, 489, {"plane": {"origin": [-33.193356, -10, -16.694962], "xDir": [0.07617804219954263, -0.914857747234018, 0.396525167186828], "zDir": [-0.8626684813170629, -0.2598919137061835, -0.4338885623432013]}, "obj": undefined});
        atStep(st, 490);
        cqRect(c, st, 490, 489, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 491);
        cqExtrude(c, st, 491, 490, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 491, "extrude");
        atStep(st, 492);
        cqUnion(c, st, 492, 488, {"toUnion": {"wp": 491}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 492, "union");
        atStep(st, 493);
        cqWorkplaneNew(c, st, 493, {"plane": {"origin": [-16.18034, -22.36068, -26.18034], "xDir": [0.3090169847362764, -0.8090169929686841, 0.5000000082324079], "zDir": [-0.42532540425407167, -0.5877852542280284, -0.6881909585341868]}, "obj": undefined});
        atStep(st, 494);
        cqRect(c, st, 494, 493, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 495);
        cqExtrude(c, st, 495, 494, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 495, "extrude");
        atStep(st, 496);
        cqUnion(c, st, 496, 492, {"toUnion": {"wp": 495}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 496, "union");
        atStep(st, 497);
        cqWorkplaneNew(c, st, 497, {"plane": {"origin": [6.18034, -26.694962, 27.013016], "xDir": [0.5654131773116665, -0.5183325657073299, -0.6415912174037162], "zDir": [0.1606220393540954, -0.6937804775983331, 0.7020464438889784]}, "obj": undefined});
        atStep(st, 498);
        cqRect(c, st, 498, 497, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 499);
        cqExtrude(c, st, 499, 498, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 499, "extrude");
        atStep(st, 500);
        cqUnion(c, st, 500, 496, {"toUnion": {"wp": 499}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 500, "union");
        atStep(st, 501);
        cqWorkplaneNew(c, st, 501, {"plane": {"origin": [0, 32.36068, 20], "xDir": [1, 0, 0], "zDir": [0, 0.8506508099867759, 0.5257311094740752]}, "obj": undefined});
        atStep(st, 502);
        cqRect(c, st, 502, 501, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 503);
        cqExtrude(c, st, 503, 502, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 503, "extrude");
        atStep(st, 504);
        cqUnion(c, st, 504, 500, {"toUnion": {"wp": 503}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 504, "union");
        atStep(st, 505);
        cqWorkplaneNew(c, st, 505, {"plane": {"origin": [-16.18034, -22.36068, 26.18034], "xDir": [0.3090169847362764, -0.8090169929686841, -0.5000000082324079], "zDir": [-0.42532540425407167, -0.5877852542280284, 0.6881909585341868]}, "obj": undefined});
        atStep(st, 506);
        cqRect(c, st, 506, 505, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 507);
        cqExtrude(c, st, 507, 506, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 507, "extrude");
        atStep(st, 508);
        cqUnion(c, st, 508, 504, {"toUnion": {"wp": 507}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 508, "union");
        atStep(st, 509);
        cqWorkplaneNew(c, st, 509, {"plane": {"origin": [-22.36068, -26.18034, 16.18034], "xDir": [0.8090169929686842, -0.5000000082324079, 0.3090169847362764], "zDir": [-0.5877852542280283, -0.6881909585341867, 0.4253254042540716]}, "obj": undefined});
        atStep(st, 510);
        cqRect(c, st, 510, 509, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 511);
        cqExtrude(c, st, 511, 510, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 511, "extrude");
        atStep(st, 512);
        cqUnion(c, st, 512, 508, {"toUnion": {"wp": 511}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 512, "union");
        atStep(st, 513);
        cqWorkplaneNew(c, st, 513, {"plane": {"origin": [22.36068, 26.18034, -16.18034], "xDir": [0.8090169929686842, -0.5000000082324079, 0.3090169847362764], "zDir": [0.5877852542280283, 0.6881909585341867, -0.4253254042540716]}, "obj": undefined});
        atStep(st, 514);
        cqRect(c, st, 514, 513, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 515);
        cqExtrude(c, st, 515, 514, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 515, "extrude");
        atStep(st, 516);
        cqUnion(c, st, 516, 512, {"toUnion": {"wp": 515}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 516, "union");
        atStep(st, 517);
        cqWorkplaneNew(c, st, 517, {"plane": {"origin": [27.013016, -6.18034, -26.694962], "xDir": [0.6415912174037162, 0.5654131773116665, 0.5183325657073299], "zDir": [0.7020464438889784, -0.1606220393540954, -0.6937804775983331]}, "obj": undefined});
        atStep(st, 518);
        cqRect(c, st, 518, 517, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 519);
        cqExtrude(c, st, 519, 518, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 519, "extrude");
        atStep(st, 520);
        cqUnion(c, st, 520, 516, {"toUnion": {"wp": 519}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 520, "union");
        atStep(st, 521);
        cqWorkplaneNew(c, st, 521, {"plane": {"origin": [-10.514622, -37.013016, 0], "xDir": [0.9619383591156128, -0.2732665242249084, 0], "zDir": [-0.2732665242249084, -0.9619383591156128, 0]}, "obj": undefined});
        atStep(st, 522);
        cqRect(c, st, 522, 521, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 523);
        cqExtrude(c, st, 523, 522, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 523, "extrude");
        atStep(st, 524);
        cqUnion(c, st, 524, 520, {"toUnion": {"wp": 523}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 524, "union");
        atStep(st, 525);
        cqWorkplaneNew(c, st, 525, {"plane": {"origin": [-16.18034, 22.36068, -26.18034], "xDir": [0.3090169847362764, 0.8090169929686841, 0.5000000082324079], "zDir": [-0.42532540425407167, 0.5877852542280284, -0.6881909585341868]}, "obj": undefined});
        atStep(st, 526);
        cqRect(c, st, 526, 525, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 527);
        cqExtrude(c, st, 527, 526, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 527, "extrude");
        atStep(st, 528);
        cqUnion(c, st, 528, 524, {"toUnion": {"wp": 527}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 528, "union");
        atStep(st, 529);
        cqWorkplaneNew(c, st, 529, {"plane": {"origin": [0, 10.514622, 37.013016], "xDir": [0, 0.9619383591156128, -0.2732665242249084], "zDir": [0, 0.2732665242249084, 0.9619383591156128]}, "obj": undefined});
        atStep(st, 530);
        cqRect(c, st, 530, 529, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 531);
        cqExtrude(c, st, 531, 530, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 531, "extrude");
        atStep(st, 532);
        cqUnion(c, st, 532, 528, {"toUnion": {"wp": 531}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 532, "union");
        atStep(st, 533);
        cqWorkplaneNew(c, st, 533, {"plane": {"origin": [16.18034, 22.36068, -26.18034], "xDir": [0.3090169847362764, -0.8090169929686841, -0.5000000082324079], "zDir": [0.42532540425407167, 0.5877852542280284, -0.6881909585341868]}, "obj": undefined});
        atStep(st, 534);
        cqRect(c, st, 534, 533, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 535);
        cqExtrude(c, st, 535, 534, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 535, "extrude");
        atStep(st, 536);
        cqUnion(c, st, 536, 532, {"toUnion": {"wp": 535}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 536, "union");
        atStep(st, 537);
        cqWorkplaneNew(c, st, 537, {"plane": {"origin": [6.18034, 36.18034, -10], "xDir": [0.5000000064941709, -0.3090169949120921, -0.809016990156158], "zDir": [0.162459850538744, 0.9510565161206248, -0.2628655551939602]}, "obj": undefined});
        atStep(st, 538);
        cqRect(c, st, 538, 537, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 539);
        cqExtrude(c, st, 539, 538, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 539, "extrude");
        atStep(st, 540);
        cqUnion(c, st, 540, 536, {"toUnion": {"wp": 539}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 540, "union");
        atStep(st, 541);
        cqWorkplaneNew(c, st, 541, {"plane": {"origin": [0, 32.36068, -20], "xDir": [1, 0, 0], "zDir": [0, 0.8506508099867759, -0.5257311094740752]}, "obj": undefined});
        atStep(st, 542);
        cqRect(c, st, 542, 541, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 543);
        cqExtrude(c, st, 543, 542, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 543, "extrude");
        atStep(st, 544);
        cqUnion(c, st, 544, 540, {"toUnion": {"wp": 543}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 544, "union");
        atStep(st, 545);
        cqWorkplaneNew(c, st, 545, {"plane": {"origin": [-10, 6.18034, 36.18034], "xDir": [0.8090169901561581, -0.500000006494171, 0.30901699491209217], "zDir": [-0.2628655551939602, 0.162459850538744, 0.9510565161206248]}, "obj": undefined});
        atStep(st, 546);
        cqRect(c, st, 546, 545, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 547);
        cqExtrude(c, st, 547, 546, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 547, "extrude");
        atStep(st, 548);
        cqUnion(c, st, 548, 544, {"toUnion": {"wp": 547}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 548, "union");
        atStep(st, 549);
        cqWorkplaneNew(c, st, 549, {"plane": {"origin": [-16.694962, -33.193356, -10], "xDir": [0.396525167186828, 0.07617804219954263, -0.914857747234018], "zDir": [-0.4338885623432013, -0.8626684813170629, -0.2598919137061835]}, "obj": undefined});
        atStep(st, 550);
        cqRect(c, st, 550, 549, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 551);
        cqExtrude(c, st, 551, 550, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 551, "extrude");
        atStep(st, 552);
        cqUnion(c, st, 552, 548, {"toUnion": {"wp": 551}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 552, "union");
        atStep(st, 553);
        cqWorkplaneNew(c, st, 553, {"plane": {"origin": [-6.18034, 36.18034, -10], "xDir": [0.5000000064941709, 0.3090169949120921, 0.809016990156158], "zDir": [-0.162459850538744, 0.9510565161206248, -0.2628655551939602]}, "obj": undefined});
        atStep(st, 554);
        cqRect(c, st, 554, 553, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 555);
        cqExtrude(c, st, 555, 554, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 555, "extrude");
        atStep(st, 556);
        cqUnion(c, st, 556, 552, {"toUnion": {"wp": 555}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 556, "union");
        atStep(st, 557);
        cqWorkplaneNew(c, st, 557, {"plane": {"origin": [6.18034, 26.694962, -27.013016], "xDir": [0.5654131773116665, 0.5183325657073299, 0.6415912174037162], "zDir": [0.1606220393540954, 0.6937804775983331, -0.7020464438889784]}, "obj": undefined});
        atStep(st, 558);
        cqRect(c, st, 558, 557, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 559);
        cqExtrude(c, st, 559, 558, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 559, "extrude");
        atStep(st, 560);
        cqUnion(c, st, 560, 556, {"toUnion": {"wp": 559}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 560, "union");
        atStep(st, 561);
        cqWorkplaneNew(c, st, 561, {"plane": {"origin": [-33.193356, -10, 16.694962], "xDir": [0.07617804219954263, -0.914857747234018, -0.396525167186828], "zDir": [-0.8626684813170629, -0.2598919137061835, 0.4338885623432013]}, "obj": undefined});
        atStep(st, 562);
        cqRect(c, st, 562, 561, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 563);
        cqExtrude(c, st, 563, 562, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 563, "extrude");
        atStep(st, 564);
        cqUnion(c, st, 564, 560, {"toUnion": {"wp": 563}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 564, "union");
        atStep(st, 565);
        cqWorkplaneNew(c, st, 565, {"plane": {"origin": [22.36068, -26.18034, 16.18034], "xDir": [0.8090169929686842, 0.5000000082324079, -0.3090169847362764], "zDir": [0.5877852542280283, -0.6881909585341867, 0.4253254042540716]}, "obj": undefined});
        atStep(st, 566);
        cqRect(c, st, 566, 565, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 567);
        cqExtrude(c, st, 567, 566, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 567, "extrude");
        atStep(st, 568);
        cqUnion(c, st, 568, 564, {"toUnion": {"wp": 567}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 568, "union");
        atStep(st, 569);
        cqWorkplaneNew(c, st, 569, {"plane": {"origin": [-10, 6.18034, -36.18034], "xDir": [0.8090169901561581, -0.500000006494171, -0.30901699491209217], "zDir": [-0.2628655551939602, 0.162459850538744, -0.9510565161206248]}, "obj": undefined});
        atStep(st, 570);
        cqRect(c, st, 570, 569, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 571);
        cqExtrude(c, st, 571, 570, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 571, "extrude");
        atStep(st, 572);
        cqUnion(c, st, 572, 568, {"toUnion": {"wp": 571}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 572, "union");
        atStep(st, 573);
        cqWorkplaneNew(c, st, 573, {"plane": {"origin": [-16.694962, -33.193356, 10], "xDir": [0.396525167186828, 0.07617804219954263, 0.914857747234018], "zDir": [-0.4338885623432013, -0.8626684813170629, 0.2598919137061835]}, "obj": undefined});
        atStep(st, 574);
        cqRect(c, st, 574, 573, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 575);
        cqExtrude(c, st, 575, 574, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 575, "extrude");
        atStep(st, 576);
        cqUnion(c, st, 576, 572, {"toUnion": {"wp": 575}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 576, "union");
        atStep(st, 577);
        cqWorkplaneNew(c, st, 577, {"plane": {"origin": [-6.18034, -36.18034, -10], "xDir": [0.5000000064941709, -0.3090169949120921, 0.809016990156158], "zDir": [-0.162459850538744, -0.9510565161206248, -0.2628655551939602]}, "obj": undefined});
        atStep(st, 578);
        cqRect(c, st, 578, 577, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 579);
        cqExtrude(c, st, 579, 578, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 579, "extrude");
        atStep(st, 580);
        cqUnion(c, st, 580, 576, {"toUnion": {"wp": 579}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 580, "union");
        atStep(st, 581);
        cqWorkplaneNew(c, st, 581, {"plane": {"origin": [-26.18034, -16.18034, -22.36068], "xDir": [0.5000000082324079, 0.3090169847362764, -0.8090169929686842], "zDir": [-0.6881909585341867, -0.4253254042540716, -0.5877852542280283]}, "obj": undefined});
        atStep(st, 582);
        cqRect(c, st, 582, 581, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 583);
        cqExtrude(c, st, 583, 582, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 583, "extrude");
        atStep(st, 584);
        cqUnion(c, st, 584, 580, {"toUnion": {"wp": 583}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 584, "union");
        atStep(st, 585);
        cqWorkplaneNew(c, st, 585, {"plane": {"origin": [10, 6.18034, -36.18034], "xDir": [0.8090169901561581, 0.500000006494171, 0.30901699491209217], "zDir": [0.2628655551939602, 0.162459850538744, -0.9510565161206248]}, "obj": undefined});
        atStep(st, 586);
        cqRect(c, st, 586, 585, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 587);
        cqExtrude(c, st, 587, 586, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 587, "extrude");
        atStep(st, 588);
        cqUnion(c, st, 588, 584, {"toUnion": {"wp": 587}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 588, "union");
        atStep(st, 589);
        cqWorkplaneNew(c, st, 589, {"plane": {"origin": [26.694962, -27.013016, -6.18034], "xDir": [0.5183325657073299, 0.6415912174037162, -0.5654131773116665], "zDir": [0.6937804775983331, -0.7020464438889784, -0.1606220393540954]}, "obj": undefined});
        atStep(st, 590);
        cqRect(c, st, 590, 589, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 591);
        cqExtrude(c, st, 591, 590, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 591, "extrude");
        atStep(st, 592);
        cqUnion(c, st, 592, 588, {"toUnion": {"wp": 591}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 592, "union");
        atStep(st, 593);
        cqWorkplaneNew(c, st, 593, {"plane": {"origin": [10, 6.18034, 36.18034], "xDir": [0.8090169901561581, 0.500000006494171, -0.30901699491209217], "zDir": [0.2628655551939602, 0.162459850538744, 0.9510565161206248]}, "obj": undefined});
        atStep(st, 594);
        cqRect(c, st, 594, 593, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 595);
        cqExtrude(c, st, 595, 594, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 595, "extrude");
        atStep(st, 596);
        cqUnion(c, st, 596, 592, {"toUnion": {"wp": 595}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 596, "union");
        atStep(st, 597);
        cqWorkplaneNew(c, st, 597, {"plane": {"origin": [-32.36068, 20, 0], "xDir": [0, 0, 1], "zDir": [-0.8506508099867759, 0.5257311094740752, 0]}, "obj": undefined});
        atStep(st, 598);
        cqRect(c, st, 598, 597, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 599);
        cqExtrude(c, st, 599, 598, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 599, "extrude");
        atStep(st, 600);
        cqUnion(c, st, 600, 596, {"toUnion": {"wp": 599}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 600, "union");
        atStep(st, 601);
        cqWorkplaneNew(c, st, 601, {"plane": {"origin": [16.18034, -22.36068, 26.18034], "xDir": [0.3090169847362764, 0.8090169929686841, 0.5000000082324079], "zDir": [0.42532540425407167, -0.5877852542280284, 0.6881909585341868]}, "obj": undefined});
        atStep(st, 602);
        cqRect(c, st, 602, 601, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 603);
        cqExtrude(c, st, 603, 602, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 603, "extrude");
        atStep(st, 604);
        cqUnion(c, st, 604, 600, {"toUnion": {"wp": 603}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 604, "union");
        atStep(st, 605);
        cqWorkplaneNew(c, st, 605, {"plane": {"origin": [-36.18034, -10, -6.18034], "xDir": [0.3090169949120921, -0.809016990156158, -0.5000000064941709], "zDir": [-0.9510565161206248, -0.2628655551939602, -0.162459850538744]}, "obj": undefined});
        atStep(st, 606);
        cqRect(c, st, 606, 605, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 607);
        cqExtrude(c, st, 607, 606, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 607, "extrude");
        atStep(st, 608);
        cqUnion(c, st, 608, 604, {"toUnion": {"wp": 607}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 608, "union");
        atStep(st, 609);
        cqWorkplaneNew(c, st, 609, {"plane": {"origin": [33.193356, -10, -16.694962], "xDir": [0.07617804219954263, 0.914857747234018, -0.396525167186828], "zDir": [0.8626684813170629, -0.2598919137061835, -0.4338885623432013]}, "obj": undefined});
        atStep(st, 610);
        cqRect(c, st, 610, 609, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 611);
        cqExtrude(c, st, 611, 610, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 611, "extrude");
        atStep(st, 612);
        cqUnion(c, st, 612, 608, {"toUnion": {"wp": 611}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 612, "union");
        atStep(st, 613);
        cqWorkplaneNew(c, st, 613, {"plane": {"origin": [26.18034, 16.18034, -22.36068], "xDir": [0.5000000082324079, 0.3090169847362764, 0.8090169929686842], "zDir": [0.6881909585341867, 0.4253254042540716, -0.5877852542280283]}, "obj": undefined});
        atStep(st, 614);
        cqRect(c, st, 614, 613, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 615);
        cqExtrude(c, st, 615, 614, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 615, "extrude");
        atStep(st, 616);
        cqUnion(c, st, 616, 612, {"toUnion": {"wp": 615}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 616, "union");
        atStep(st, 617);
        cqWorkplaneNew(c, st, 617, {"plane": {"origin": [22.36068, -26.18034, -16.18034], "xDir": [0.8090169929686842, 0.5000000082324079, 0.3090169847362764], "zDir": [0.5877852542280283, -0.6881909585341867, -0.4253254042540716]}, "obj": undefined});
        atStep(st, 618);
        cqRect(c, st, 618, 617, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 619);
        cqExtrude(c, st, 619, 618, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 619, "extrude");
        atStep(st, 620);
        cqUnion(c, st, 620, 616, {"toUnion": {"wp": 619}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 620, "union");
        atStep(st, 621);
        cqWorkplaneNew(c, st, 621, {"plane": {"origin": [-16.694962, 33.193356, -10], "xDir": [0.396525167186828, -0.07617804219954263, -0.914857747234018], "zDir": [-0.4338885623432013, 0.8626684813170629, -0.2598919137061835]}, "obj": undefined});
        atStep(st, 622);
        cqRect(c, st, 622, 621, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 623);
        cqExtrude(c, st, 623, 622, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 623, "extrude");
        atStep(st, 624);
        cqUnion(c, st, 624, 620, {"toUnion": {"wp": 623}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 624, "union");
        atStep(st, 625);
        cqWorkplaneNew(c, st, 625, {"plane": {"origin": [27.013016, 6.18034, 26.694962], "xDir": [0.6415912174037162, -0.5654131773116665, -0.5183325657073299], "zDir": [0.7020464438889784, 0.1606220393540954, 0.6937804775983331]}, "obj": undefined});
        atStep(st, 626);
        cqRect(c, st, 626, 625, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 627);
        cqExtrude(c, st, 627, 626, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 627, "extrude");
        atStep(st, 628);
        cqUnion(c, st, 628, 624, {"toUnion": {"wp": 627}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 628, "union");
        atStep(st, 629);
        cqWorkplaneNew(c, st, 629, {"plane": {"origin": [-26.694962, -27.013016, -6.18034], "xDir": [0.5183325657073299, -0.6415912174037162, 0.5654131773116665], "zDir": [-0.6937804775983331, -0.7020464438889784, -0.1606220393540954]}, "obj": undefined});
        atStep(st, 630);
        cqRect(c, st, 630, 629, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 631);
        cqExtrude(c, st, 631, 630, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 631, "extrude");
        atStep(st, 632);
        cqUnion(c, st, 632, 628, {"toUnion": {"wp": 631}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 632, "union");
        atStep(st, 633);
        cqWorkplaneNew(c, st, 633, {"plane": {"origin": [-6.18034, 36.18034, 10], "xDir": [0.5000000064941709, 0.3090169949120921, -0.809016990156158], "zDir": [-0.162459850538744, 0.9510565161206248, 0.2628655551939602]}, "obj": undefined});
        atStep(st, 634);
        cqRect(c, st, 634, 633, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 635);
        cqExtrude(c, st, 635, 634, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 635, "extrude");
        atStep(st, 636);
        cqUnion(c, st, 636, 632, {"toUnion": {"wp": 635}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 636, "union");
        atStep(st, 637);
        cqWorkplaneNew(c, st, 637, {"plane": {"origin": [-16.694962, 33.193356, 10], "xDir": [0.396525167186828, -0.07617804219954263, 0.914857747234018], "zDir": [-0.4338885623432013, 0.8626684813170629, 0.2598919137061835]}, "obj": undefined});
        atStep(st, 638);
        cqRect(c, st, 638, 637, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 639);
        cqExtrude(c, st, 639, 638, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 639, "extrude");
        atStep(st, 640);
        cqUnion(c, st, 640, 636, {"toUnion": {"wp": 639}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 640, "union");
        atStep(st, 641);
        cqWorkplaneNew(c, st, 641, {"plane": {"origin": [-6.18034, -36.18034, 10], "xDir": [0.5000000064941709, -0.3090169949120921, -0.809016990156158], "zDir": [-0.162459850538744, -0.9510565161206248, 0.2628655551939602]}, "obj": undefined});
        atStep(st, 642);
        cqRect(c, st, 642, 641, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 643);
        cqExtrude(c, st, 643, 642, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 643, "extrude");
        atStep(st, 644);
        cqUnion(c, st, 644, 640, {"toUnion": {"wp": 643}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 644, "union");
        atStep(st, 645);
        cqWorkplaneNew(c, st, 645, {"plane": {"origin": [10, 16.694962, 33.193356], "xDir": [0.914857747234018, -0.396525167186828, -0.07617804219954263], "zDir": [0.2598919137061835, 0.4338885623432013, 0.8626684813170629]}, "obj": undefined});
        atStep(st, 646);
        cqRect(c, st, 646, 645, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 647);
        cqExtrude(c, st, 647, 646, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 647, "extrude");
        atStep(st, 648);
        cqUnion(c, st, 648, 644, {"toUnion": {"wp": 647}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 648, "union");
        atStep(st, 649);
        cqWorkplaneNew(c, st, 649, {"plane": {"origin": [-16.18034, 22.36068, 26.18034], "xDir": [0.3090169847362764, 0.8090169929686841, -0.5000000082324079], "zDir": [-0.42532540425407167, 0.5877852542280284, 0.6881909585341868]}, "obj": undefined});
        atStep(st, 650);
        cqRect(c, st, 650, 649, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 651);
        cqExtrude(c, st, 651, 650, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 651, "extrude");
        atStep(st, 652);
        cqUnion(c, st, 652, 648, {"toUnion": {"wp": 651}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 652, "union");
        atStep(st, 653);
        cqWorkplaneNew(c, st, 653, {"plane": {"origin": [-10, 16.694962, 33.193356], "xDir": [0.914857747234018, 0.396525167186828, 0.07617804219954263], "zDir": [-0.2598919137061835, 0.4338885623432013, 0.8626684813170629]}, "obj": undefined});
        atStep(st, 654);
        cqRect(c, st, 654, 653, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 655);
        cqExtrude(c, st, 655, 654, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 655, "extrude");
        atStep(st, 656);
        cqUnion(c, st, 656, 652, {"toUnion": {"wp": 655}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 656, "union");
        atStep(st, 657);
        cqWorkplaneNew(c, st, 657, {"plane": {"origin": [26.18034, 16.18034, 22.36068], "xDir": [0.5000000082324079, 0.3090169847362764, -0.8090169929686842], "zDir": [0.6881909585341867, 0.4253254042540716, 0.5877852542280283]}, "obj": undefined});
        atStep(st, 658);
        cqRect(c, st, 658, 657, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 659);
        cqExtrude(c, st, 659, 658, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 659, "extrude");
        atStep(st, 660);
        cqUnion(c, st, 660, 656, {"toUnion": {"wp": 659}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 660, "union");
        atStep(st, 661);
        cqWorkplaneNew(c, st, 661, {"plane": {"origin": [6.18034, -26.694962, -27.013016], "xDir": [0.5654131773116665, -0.5183325657073299, 0.6415912174037162], "zDir": [0.1606220393540954, -0.6937804775983331, -0.7020464438889784]}, "obj": undefined});
        atStep(st, 662);
        cqRect(c, st, 662, 661, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 663);
        cqExtrude(c, st, 663, 662, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 663, "extrude");
        atStep(st, 664);
        cqUnion(c, st, 664, 660, {"toUnion": {"wp": 663}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 664, "union");
        atStep(st, 665);
        cqWorkplaneNew(c, st, 665, {"plane": {"origin": [-36.18034, -10, 6.18034], "xDir": [0.3090169949120921, -0.809016990156158, 0.5000000064941709], "zDir": [-0.9510565161206248, -0.2628655551939602, 0.162459850538744]}, "obj": undefined});
        atStep(st, 666);
        cqRect(c, st, 666, 665, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 667);
        cqExtrude(c, st, 667, 666, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 667, "extrude");
        atStep(st, 668);
        cqUnion(c, st, 668, 664, {"toUnion": {"wp": 667}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 668, "union");
        atStep(st, 669);
        cqWorkplaneNew(c, st, 669, {"plane": {"origin": [10, 16.694962, -33.193356], "xDir": [0.914857747234018, -0.396525167186828, 0.07617804219954263], "zDir": [0.2598919137061835, 0.4338885623432013, -0.8626684813170629]}, "obj": undefined});
        atStep(st, 670);
        cqRect(c, st, 670, 669, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 671);
        cqExtrude(c, st, 671, 670, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 671, "extrude");
        atStep(st, 672);
        cqUnion(c, st, 672, 668, {"toUnion": {"wp": 671}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 672, "union");
        atStep(st, 673);
        cqWorkplaneNew(c, st, 673, {"plane": {"origin": [-26.18034, -16.18034, 22.36068], "xDir": [0.5000000082324079, 0.3090169847362764, 0.8090169929686842], "zDir": [-0.6881909585341867, -0.4253254042540716, 0.5877852542280283]}, "obj": undefined});
        atStep(st, 674);
        cqRect(c, st, 674, 673, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 675);
        cqExtrude(c, st, 675, 674, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 675, "extrude");
        atStep(st, 676);
        cqUnion(c, st, 676, 672, {"toUnion": {"wp": 675}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 676, "union");
        atStep(st, 677);
        cqWorkplaneNew(c, st, 677, {"plane": {"origin": [-26.694962, 27.013016, -6.18034], "xDir": [0.5183325657073299, 0.6415912174037162, 0.5654131773116665], "zDir": [-0.6937804775983331, 0.7020464438889784, -0.1606220393540954]}, "obj": undefined});
        atStep(st, 678);
        cqRect(c, st, 678, 677, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 679);
        cqExtrude(c, st, 679, 678, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 679, "extrude");
        atStep(st, 680);
        cqUnion(c, st, 680, 676, {"toUnion": {"wp": 679}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 680, "union");
        atStep(st, 681);
        cqWorkplaneNew(c, st, 681, {"plane": {"origin": [20, 0, 32.36068], "xDir": [0, 1, 0], "zDir": [0.5257311094740752, 0, 0.8506508099867759]}, "obj": undefined});
        atStep(st, 682);
        cqRect(c, st, 682, 681, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 683);
        cqExtrude(c, st, 683, 682, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 683, "extrude");
        atStep(st, 684);
        cqUnion(c, st, 684, 680, {"toUnion": {"wp": 683}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 684, "union");
        atStep(st, 685);
        cqWorkplaneNew(c, st, 685, {"plane": {"origin": [16.18034, 22.36068, 26.18034], "xDir": [0.3090169847362764, -0.8090169929686841, 0.5000000082324079], "zDir": [0.42532540425407167, 0.5877852542280284, 0.6881909585341868]}, "obj": undefined});
        atStep(st, 686);
        cqRect(c, st, 686, 685, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 687);
        cqExtrude(c, st, 687, 686, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 687, "extrude");
        atStep(st, 688);
        cqUnion(c, st, 688, 684, {"toUnion": {"wp": 687}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 688, "union");
        atStep(st, 689);
        cqWorkplaneNew(c, st, 689, {"plane": {"origin": [37.013016, 0, -10.514622], "xDir": [0.2732665242249084, 0, 0.9619383591156128], "zDir": [0.9619383591156128, 0, -0.2732665242249084]}, "obj": undefined});
        atStep(st, 690);
        cqRect(c, st, 690, 689, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 691);
        cqExtrude(c, st, 691, 690, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 691, "extrude");
        atStep(st, 692);
        cqUnion(c, st, 692, 688, {"toUnion": {"wp": 691}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 692, "union");
        atStep(st, 693);
        cqWorkplaneNew(c, st, 693, {"plane": {"origin": [36.18034, 10, -6.18034], "xDir": [0.3090169949120921, -0.809016990156158, 0.5000000064941709], "zDir": [0.9510565161206248, 0.2628655551939602, -0.162459850538744]}, "obj": undefined});
        atStep(st, 694);
        cqRect(c, st, 694, 693, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 695);
        cqExtrude(c, st, 695, 694, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 695, "extrude");
        atStep(st, 696);
        cqUnion(c, st, 696, 692, {"toUnion": {"wp": 695}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 696, "union");
        atStep(st, 697);
        cqWorkplaneNew(c, st, 697, {"plane": {"origin": [-10, -6.18034, 36.18034], "xDir": [0.8090169901561581, 0.500000006494171, 0.30901699491209217], "zDir": [-0.2628655551939602, -0.162459850538744, 0.9510565161206248]}, "obj": undefined});
        atStep(st, 698);
        cqRect(c, st, 698, 697, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 699);
        cqExtrude(c, st, 699, 698, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 699, "extrude");
        atStep(st, 700);
        cqUnion(c, st, 700, 696, {"toUnion": {"wp": 699}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 700, "union");
        atStep(st, 701);
        cqWorkplaneNew(c, st, 701, {"plane": {"origin": [27.013016, 6.18034, -26.694962], "xDir": [0.6415912174037162, -0.5654131773116665, 0.5183325657073299], "zDir": [0.7020464438889784, 0.1606220393540954, -0.6937804775983331]}, "obj": undefined});
        atStep(st, 702);
        cqRect(c, st, 702, 701, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 703);
        cqExtrude(c, st, 703, 702, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 703, "extrude");
        atStep(st, 704);
        cqUnion(c, st, 704, 700, {"toUnion": {"wp": 703}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 704, "union");
        atStep(st, 705);
        cqWorkplaneNew(c, st, 705, {"plane": {"origin": [16.694962, -33.193356, -10], "xDir": [0.396525167186828, -0.07617804219954263, 0.914857747234018], "zDir": [0.4338885623432013, -0.8626684813170629, -0.2598919137061835]}, "obj": undefined});
        atStep(st, 706);
        cqRect(c, st, 706, 705, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 707);
        cqExtrude(c, st, 707, 706, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 707, "extrude");
        atStep(st, 708);
        cqUnion(c, st, 708, 704, {"toUnion": {"wp": 707}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 708, "union");
        atStep(st, 709);
        cqWorkplaneNew(c, st, 709, {"plane": {"origin": [-33.193356, 10, -16.694962], "xDir": [0.07617804219954263, 0.914857747234018, 0.396525167186828], "zDir": [-0.8626684813170629, 0.2598919137061835, -0.4338885623432013]}, "obj": undefined});
        atStep(st, 710);
        cqRect(c, st, 710, 709, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 711);
        cqExtrude(c, st, 711, 710, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 711, "extrude");
        atStep(st, 712);
        cqUnion(c, st, 712, 708, {"toUnion": {"wp": 711}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 712, "union");
        atStep(st, 713);
        cqWorkplaneNew(c, st, 713, {"plane": {"origin": [-33.193356, 10, 16.694962], "xDir": [0.07617804219954263, 0.914857747234018, -0.396525167186828], "zDir": [-0.8626684813170629, 0.2598919137061835, 0.4338885623432013]}, "obj": undefined});
        atStep(st, 714);
        cqRect(c, st, 714, 713, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 715);
        cqExtrude(c, st, 715, 714, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 715, "extrude");
        atStep(st, 716);
        cqUnion(c, st, 716, 712, {"toUnion": {"wp": 715}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 716, "union");
        atStep(st, 717);
        cqWorkplaneNew(c, st, 717, {"plane": {"origin": [6.18034, 26.694962, 27.013016], "xDir": [0.5654131773116665, 0.5183325657073299, -0.6415912174037162], "zDir": [0.1606220393540954, 0.6937804775983331, 0.7020464438889784]}, "obj": undefined});
        atStep(st, 718);
        cqRect(c, st, 718, 717, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 719);
        cqExtrude(c, st, 719, 718, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 719, "extrude");
        atStep(st, 720);
        cqUnion(c, st, 720, 716, {"toUnion": {"wp": 719}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 720, "union");
        atStep(st, 721);
        cqWorkplaneNew(c, st, 721, {"plane": {"origin": [16.694962, 33.193356, 10], "xDir": [0.396525167186828, 0.07617804219954263, -0.914857747234018], "zDir": [0.4338885623432013, 0.8626684813170629, 0.2598919137061835]}, "obj": undefined});
        atStep(st, 722);
        cqRect(c, st, 722, 721, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 723);
        cqExtrude(c, st, 723, 722, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 723, "extrude");
        atStep(st, 724);
        cqUnion(c, st, 724, 720, {"toUnion": {"wp": 723}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 724, "union");
        atStep(st, 725);
        cqWorkplaneNew(c, st, 725, {"plane": {"origin": [0, 10.514622, -37.013016], "xDir": [0, 0.9619383591156128, 0.2732665242249084], "zDir": [0, 0.2732665242249084, -0.9619383591156128]}, "obj": undefined});
        atStep(st, 726);
        cqRect(c, st, 726, 725, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 727);
        cqExtrude(c, st, 727, 726, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 727, "extrude");
        atStep(st, 728);
        cqUnion(c, st, 728, 724, {"toUnion": {"wp": 727}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 728, "union");
        atStep(st, 729);
        cqWorkplaneNew(c, st, 729, {"plane": {"origin": [6.18034, -36.18034, -10], "xDir": [0.5000000064941709, 0.3090169949120921, -0.809016990156158], "zDir": [0.162459850538744, -0.9510565161206248, -0.2628655551939602]}, "obj": undefined});
        atStep(st, 730);
        cqRect(c, st, 730, 729, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 731);
        cqExtrude(c, st, 731, 730, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 731, "extrude");
        atStep(st, 732);
        cqUnion(c, st, 732, 728, {"toUnion": {"wp": 731}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 732, "union");
        atStep(st, 733);
        cqWorkplaneNew(c, st, 733, {"plane": {"origin": [36.18034, -10, 6.18034], "xDir": [0.3090169949120921, 0.809016990156158, -0.5000000064941709], "zDir": [0.9510565161206248, -0.2628655551939602, 0.162459850538744]}, "obj": undefined});
        atStep(st, 734);
        cqRect(c, st, 734, 733, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 735);
        cqExtrude(c, st, 735, 734, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 735, "extrude");
        atStep(st, 736);
        cqUnion(c, st, 736, 732, {"toUnion": {"wp": 735}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 736, "union");
        atStep(st, 737);
        cqWorkplaneNew(c, st, 737, {"plane": {"origin": [-27.013016, -6.18034, 26.694962], "xDir": [0.6415912174037162, -0.5654131773116665, 0.5183325657073299], "zDir": [-0.7020464438889784, -0.1606220393540954, 0.6937804775983331]}, "obj": undefined});
        atStep(st, 738);
        cqRect(c, st, 738, 737, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 739);
        cqExtrude(c, st, 739, 738, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 739, "extrude");
        atStep(st, 740);
        cqUnion(c, st, 740, 736, {"toUnion": {"wp": 739}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 740, "union");
        atStep(st, 741);
        cqWorkplaneNew(c, st, 741, {"plane": {"origin": [6.18034, -36.18034, 10], "xDir": [0.5000000064941709, 0.3090169949120921, 0.809016990156158], "zDir": [0.162459850538744, -0.9510565161206248, 0.2628655551939602]}, "obj": undefined});
        atStep(st, 742);
        cqRect(c, st, 742, 741, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 743);
        cqExtrude(c, st, 743, 742, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 743, "extrude");
        atStep(st, 744);
        cqUnion(c, st, 744, 740, {"toUnion": {"wp": 743}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 744, "union");
        atStep(st, 745);
        cqWorkplaneNew(c, st, 745, {"plane": {"origin": [10, -16.694962, -33.193356], "xDir": [0.914857747234018, 0.396525167186828, 0.07617804219954263], "zDir": [0.2598919137061835, -0.4338885623432013, -0.8626684813170629]}, "obj": undefined});
        atStep(st, 746);
        cqRect(c, st, 746, 745, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 747);
        cqExtrude(c, st, 747, 746, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 747, "extrude");
        atStep(st, 748);
        cqUnion(c, st, 748, 744, {"toUnion": {"wp": 747}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 748, "union");
        atStep(st, 749);
        cqWorkplaneNew(c, st, 749, {"plane": {"origin": [0, -10.514622, -37.013016], "xDir": [0, 0.9619383591156128, -0.2732665242249084], "zDir": [0, -0.2732665242249084, -0.9619383591156128]}, "obj": undefined});
        atStep(st, 750);
        cqRect(c, st, 750, 749, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 751);
        cqExtrude(c, st, 751, 750, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 751, "extrude");
        atStep(st, 752);
        cqUnion(c, st, 752, 748, {"toUnion": {"wp": 751}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 752, "union");
        atStep(st, 753);
        cqWorkplaneNew(c, st, 753, {"plane": {"origin": [16.694962, 33.193356, -10], "xDir": [0.396525167186828, 0.07617804219954263, 0.914857747234018], "zDir": [0.4338885623432013, 0.8626684813170629, -0.2598919137061835]}, "obj": undefined});
        atStep(st, 754);
        cqRect(c, st, 754, 753, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 755);
        cqExtrude(c, st, 755, 754, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 755, "extrude");
        atStep(st, 756);
        cqUnion(c, st, 756, 752, {"toUnion": {"wp": 755}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 756, "union");
        atStep(st, 757);
        cqWorkplaneNew(c, st, 757, {"plane": {"origin": [-22.36068, 26.18034, -16.18034], "xDir": [0.8090169929686842, 0.5000000082324079, -0.3090169847362764], "zDir": [-0.5877852542280283, 0.6881909585341867, -0.4253254042540716]}, "obj": undefined});
        atStep(st, 758);
        cqRect(c, st, 758, 757, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 759);
        cqExtrude(c, st, 759, 758, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 759, "extrude");
        atStep(st, 760);
        cqUnion(c, st, 760, 756, {"toUnion": {"wp": 759}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 760, "union");
        atStep(st, 761);
        cqWorkplaneNew(c, st, 761, {"plane": {"origin": [-27.013016, -6.18034, -26.694962], "xDir": [0.6415912174037162, -0.5654131773116665, -0.5183325657073299], "zDir": [-0.7020464438889784, -0.1606220393540954, -0.6937804775983331]}, "obj": undefined});
        atStep(st, 762);
        cqRect(c, st, 762, 761, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 763);
        cqExtrude(c, st, 763, 762, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 763, "extrude");
        atStep(st, 764);
        cqUnion(c, st, 764, 760, {"toUnion": {"wp": 763}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 764, "union");
        atStep(st, 765);
        cqWorkplaneNew(c, st, 765, {"plane": {"origin": [36.18034, -10, -6.18034], "xDir": [0.3090169949120921, 0.809016990156158, 0.5000000064941709], "zDir": [0.9510565161206248, -0.2628655551939602, -0.162459850538744]}, "obj": undefined});
        atStep(st, 766);
        cqRect(c, st, 766, 765, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 767);
        cqExtrude(c, st, 767, 766, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 767, "extrude");
        atStep(st, 768);
        cqUnion(c, st, 768, 764, {"toUnion": {"wp": 767}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 768, "union");
        atStep(st, 769);
        cqWorkplaneNew(c, st, 769, {"plane": {"origin": [-26.694962, 27.013016, 6.18034], "xDir": [0.5183325657073299, 0.6415912174037162, -0.5654131773116665], "zDir": [-0.6937804775983331, 0.7020464438889784, 0.1606220393540954]}, "obj": undefined});
        atStep(st, 770);
        cqRect(c, st, 770, 769, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 771);
        cqExtrude(c, st, 771, 770, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 771, "extrude");
        atStep(st, 772);
        cqUnion(c, st, 772, 768, {"toUnion": {"wp": 771}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 772, "union");
        atStep(st, 773);
        cqWorkplaneNew(c, st, 773, {"plane": {"origin": [16.694962, -33.193356, 10], "xDir": [0.396525167186828, -0.07617804219954263, -0.914857747234018], "zDir": [0.4338885623432013, -0.8626684813170629, 0.2598919137061835]}, "obj": undefined});
        atStep(st, 774);
        cqRect(c, st, 774, 773, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 775);
        cqExtrude(c, st, 775, 774, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 775, "extrude");
        atStep(st, 776);
        cqUnion(c, st, 776, 772, {"toUnion": {"wp": 775}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 776, "union");
        atStep(st, 777);
        cqWorkplaneNew(c, st, 777, {"plane": {"origin": [32.36068, 20, 0], "xDir": [0, 0, 1], "zDir": [0.8506508099867759, 0.5257311094740752, 0]}, "obj": undefined});
        atStep(st, 778);
        cqRect(c, st, 778, 777, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 779);
        cqExtrude(c, st, 779, 778, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 779, "extrude");
        atStep(st, 780);
        cqUnion(c, st, 780, 776, {"toUnion": {"wp": 779}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 780, "union");
        atStep(st, 781);
        cqWorkplaneNew(c, st, 781, {"plane": {"origin": [-26.694962, -27.013016, 6.18034], "xDir": [0.5183325657073299, -0.6415912174037162, -0.5654131773116665], "zDir": [-0.6937804775983331, -0.7020464438889784, 0.1606220393540954]}, "obj": undefined});
        atStep(st, 782);
        cqRect(c, st, 782, 781, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 783);
        cqExtrude(c, st, 783, 782, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 783, "extrude");
        atStep(st, 784);
        cqUnion(c, st, 784, 780, {"toUnion": {"wp": 783}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 784, "union");
        atStep(st, 785);
        cqWorkplaneNew(c, st, 785, {"plane": {"origin": [36.18034, 10, 6.18034], "xDir": [0.3090169949120921, -0.809016990156158, -0.5000000064941709], "zDir": [0.9510565161206248, 0.2628655551939602, 0.162459850538744]}, "obj": undefined});
        atStep(st, 786);
        cqRect(c, st, 786, 785, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 787);
        cqExtrude(c, st, 787, 786, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 787, "extrude");
        atStep(st, 788);
        cqUnion(c, st, 788, 784, {"toUnion": {"wp": 787}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 788, "union");
        atStep(st, 789);
        cqWorkplaneNew(c, st, 789, {"plane": {"origin": [0, -32.36068, 20], "xDir": [1, 0, 0], "zDir": [0, -0.8506508099867759, 0.5257311094740752]}, "obj": undefined});
        atStep(st, 790);
        cqRect(c, st, 790, 789, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 791);
        cqExtrude(c, st, 791, 790, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 791, "extrude");
        atStep(st, 792);
        cqUnion(c, st, 792, 788, {"toUnion": {"wp": 791}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 792, "union");
        atStep(st, 793);
        cqWorkplaneNew(c, st, 793, {"plane": {"origin": [-10, -16.694962, 33.193356], "xDir": [0.914857747234018, -0.396525167186828, 0.07617804219954263], "zDir": [-0.2598919137061835, -0.4338885623432013, 0.8626684813170629]}, "obj": undefined});
        atStep(st, 794);
        cqRect(c, st, 794, 793, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 795);
        cqExtrude(c, st, 795, 794, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 795, "extrude");
        atStep(st, 796);
        cqUnion(c, st, 796, 792, {"toUnion": {"wp": 795}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 796, "union");
        atStep(st, 797);
        cqWorkplaneNew(c, st, 797, {"plane": {"origin": [33.193356, -10, 16.694962], "xDir": [0.07617804219954263, 0.914857747234018, 0.396525167186828], "zDir": [0.8626684813170629, -0.2598919137061835, 0.4338885623432013]}, "obj": undefined});
        atStep(st, 798);
        cqRect(c, st, 798, 797, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 799);
        cqExtrude(c, st, 799, 798, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 799, "extrude");
        atStep(st, 800);
        cqUnion(c, st, 800, 796, {"toUnion": {"wp": 799}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 800, "union");
        atStep(st, 801);
        cqWorkplaneNew(c, st, 801, {"plane": {"origin": [26.18034, -16.18034, 22.36068], "xDir": [0.5000000082324079, -0.3090169847362764, -0.8090169929686842], "zDir": [0.6881909585341867, -0.4253254042540716, 0.5877852542280283]}, "obj": undefined});
        atStep(st, 802);
        cqRect(c, st, 802, 801, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 803);
        cqExtrude(c, st, 803, 802, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 803, "extrude");
        atStep(st, 804);
        cqUnion(c, st, 804, 800, {"toUnion": {"wp": 803}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 804, "union");
        atStep(st, 805);
        cqWorkplaneNew(c, st, 805, {"plane": {"origin": [-6.18034, 26.694962, -27.013016], "xDir": [0.5654131773116665, -0.5183325657073299, -0.6415912174037162], "zDir": [-0.1606220393540954, 0.6937804775983331, -0.7020464438889784]}, "obj": undefined});
        atStep(st, 806);
        cqRect(c, st, 806, 805, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 807);
        cqExtrude(c, st, 807, 806, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 807, "extrude");
        atStep(st, 808);
        cqUnion(c, st, 808, 804, {"toUnion": {"wp": 807}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 808, "union");
        atStep(st, 809);
        cqWorkplaneNew(c, st, 809, {"plane": {"origin": [-36.18034, 10, 6.18034], "xDir": [0.3090169949120921, 0.809016990156158, 0.5000000064941709], "zDir": [-0.9510565161206248, 0.2628655551939602, 0.162459850538744]}, "obj": undefined});
        atStep(st, 810);
        cqRect(c, st, 810, 809, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 811);
        cqExtrude(c, st, 811, 810, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 811, "extrude");
        atStep(st, 812);
        cqUnion(c, st, 812, 808, {"toUnion": {"wp": 811}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 812, "union");
        atStep(st, 813);
        cqWorkplaneNew(c, st, 813, {"plane": {"origin": [16.18034, -22.36068, -26.18034], "xDir": [0.3090169847362764, 0.8090169929686841, -0.5000000082324079], "zDir": [0.42532540425407167, -0.5877852542280284, -0.6881909585341868]}, "obj": undefined});
        atStep(st, 814);
        cqRect(c, st, 814, 813, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 815);
        cqExtrude(c, st, 815, 814, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 815, "extrude");
        atStep(st, 816);
        cqUnion(c, st, 816, 812, {"toUnion": {"wp": 815}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 816, "union");
        atStep(st, 817);
        cqWorkplaneNew(c, st, 817, {"plane": {"origin": [-36.18034, 10, -6.18034], "xDir": [0.3090169949120921, 0.809016990156158, -0.5000000064941709], "zDir": [-0.9510565161206248, 0.2628655551939602, -0.162459850538744]}, "obj": undefined});
        atStep(st, 818);
        cqRect(c, st, 818, 817, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 819);
        cqExtrude(c, st, 819, 818, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 819, "extrude");
        atStep(st, 820);
        cqUnion(c, st, 820, 816, {"toUnion": {"wp": 819}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 820, "union");
        atStep(st, 821);
        cqWorkplaneNew(c, st, 821, {"plane": {"origin": [-32.36068, -20, 0], "xDir": [0, 0, 1], "zDir": [-0.8506508099867759, -0.5257311094740752, 0]}, "obj": undefined});
        atStep(st, 822);
        cqRect(c, st, 822, 821, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 823);
        cqExtrude(c, st, 823, 822, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 823, "extrude");
        atStep(st, 824);
        cqUnion(c, st, 824, 820, {"toUnion": {"wp": 823}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 824, "union");
        atStep(st, 825);
        cqWorkplaneNew(c, st, 825, {"plane": {"origin": [-10, -6.18034, -36.18034], "xDir": [0.8090169901561581, 0.500000006494171, -0.30901699491209217], "zDir": [-0.2628655551939602, -0.162459850538744, -0.9510565161206248]}, "obj": undefined});
        atStep(st, 826);
        cqRect(c, st, 826, 825, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 827);
        cqExtrude(c, st, 827, 826, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 827, "extrude");
        atStep(st, 828);
        cqUnion(c, st, 828, 824, {"toUnion": {"wp": 827}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 828, "union");
        atStep(st, 829);
        cqWorkplaneNew(c, st, 829, {"plane": {"origin": [10.514622, -37.013016, 0], "xDir": [0.9619383591156128, 0.2732665242249084, 0], "zDir": [0.2732665242249084, -0.9619383591156128, 0]}, "obj": undefined});
        atStep(st, 830);
        cqRect(c, st, 830, 829, {"xLen": 21.86132193799268, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 831);
        cqExtrude(c, st, 831, 830, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 831, "extrude");
        atStep(st, 832);
        cqUnion(c, st, 832, 828, {"toUnion": {"wp": 831}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 832, "union");
        atStep(st, 833);
        cqWorkplaneNew(c, st, 833, {"plane": {"origin": [-27.013016, 6.18034, -26.694962], "xDir": [0.6415912174037162, 0.5654131773116665, -0.5183325657073299], "zDir": [-0.7020464438889784, 0.1606220393540954, -0.6937804775983331]}, "obj": undefined});
        atStep(st, 834);
        cqRect(c, st, 834, 833, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 835);
        cqExtrude(c, st, 835, 834, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 835, "extrude");
        atStep(st, 836);
        cqUnion(c, st, 836, 832, {"toUnion": {"wp": 835}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 836, "union");
        atStep(st, 837);
        cqWorkplaneNew(c, st, 837, {"plane": {"origin": [-10, -16.694962, -33.193356], "xDir": [0.914857747234018, -0.396525167186828, -0.07617804219954263], "zDir": [-0.2598919137061835, -0.4338885623432013, -0.8626684813170629]}, "obj": undefined});
        atStep(st, 838);
        cqRect(c, st, 838, 837, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 839);
        cqExtrude(c, st, 839, 838, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 839, "extrude");
        atStep(st, 840);
        cqUnion(c, st, 840, 836, {"toUnion": {"wp": 839}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 840, "union");
        atStep(st, 841);
        cqWorkplaneNew(c, st, 841, {"plane": {"origin": [-22.36068, -26.18034, -16.18034], "xDir": [0.8090169929686842, -0.5000000082324079, -0.3090169847362764], "zDir": [-0.5877852542280283, -0.6881909585341867, -0.4253254042540716]}, "obj": undefined});
        atStep(st, 842);
        cqRect(c, st, 842, 841, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 843);
        cqExtrude(c, st, 843, 842, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 843, "extrude");
        atStep(st, 844);
        cqUnion(c, st, 844, 840, {"toUnion": {"wp": 843}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 844, "union");
        atStep(st, 845);
        cqWorkplaneNew(c, st, 845, {"plane": {"origin": [32.36068, -20, 0], "xDir": [0, 0, 1], "zDir": [0.8506508099867759, -0.5257311094740752, 0]}, "obj": undefined});
        atStep(st, 846);
        cqRect(c, st, 846, 845, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 847);
        cqExtrude(c, st, 847, 846, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 847, "extrude");
        atStep(st, 848);
        cqUnion(c, st, 848, 844, {"toUnion": {"wp": 847}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 848, "union");
        atStep(st, 849);
        cqWorkplaneNew(c, st, 849, {"plane": {"origin": [0, -32.36068, -20], "xDir": [1, 0, 0], "zDir": [0, -0.8506508099867759, -0.5257311094740752]}, "obj": undefined});
        atStep(st, 850);
        cqRect(c, st, 850, 849, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 851);
        cqExtrude(c, st, 851, 850, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 851, "extrude");
        atStep(st, 852);
        cqUnion(c, st, 852, 848, {"toUnion": {"wp": 851}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 852, "union");
        atStep(st, 853);
        cqWorkplaneNew(c, st, 853, {"plane": {"origin": [10, -6.18034, -36.18034], "xDir": [0.8090169901561581, -0.500000006494171, 0.30901699491209217], "zDir": [0.2628655551939602, -0.162459850538744, -0.9510565161206248]}, "obj": undefined});
        atStep(st, 854);
        cqRect(c, st, 854, 853, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 855);
        cqExtrude(c, st, 855, 854, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 855, "extrude");
        atStep(st, 856);
        cqUnion(c, st, 856, 852, {"toUnion": {"wp": 855}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 856, "union");
        atStep(st, 857);
        cqWorkplaneNew(c, st, 857, {"plane": {"origin": [-6.18034, -26.694962, 27.013016], "xDir": [0.5654131773116665, 0.5183325657073299, 0.6415912174037162], "zDir": [-0.1606220393540954, -0.6937804775983331, 0.7020464438889784]}, "obj": undefined});
        atStep(st, 858);
        cqRect(c, st, 858, 857, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 859);
        cqExtrude(c, st, 859, 858, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 859, "extrude");
        atStep(st, 860);
        cqUnion(c, st, 860, 856, {"toUnion": {"wp": 859}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 860, "union");
        atStep(st, 861);
        cqWorkplaneNew(c, st, 861, {"plane": {"origin": [10, -6.18034, 36.18034], "xDir": [0.8090169901561581, -0.500000006494171, -0.30901699491209217], "zDir": [0.2628655551939602, -0.162459850538744, 0.9510565161206248]}, "obj": undefined});
        atStep(st, 862);
        cqRect(c, st, 862, 861, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 863);
        cqExtrude(c, st, 863, 862, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 863, "extrude");
        atStep(st, 864);
        cqUnion(c, st, 864, 860, {"toUnion": {"wp": 863}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 864, "union");
        atStep(st, 865);
        cqWorkplaneNew(c, st, 865, {"plane": {"origin": [33.193356, 10, 16.694962], "xDir": [0.07617804219954263, -0.914857747234018, 0.396525167186828], "zDir": [0.8626684813170629, 0.2598919137061835, 0.4338885623432013]}, "obj": undefined});
        atStep(st, 866);
        cqRect(c, st, 866, 865, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 867);
        cqExtrude(c, st, 867, 866, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 867, "extrude");
        atStep(st, 868);
        cqUnion(c, st, 868, 864, {"toUnion": {"wp": 867}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 868, "union");
        atStep(st, 869);
        cqWorkplaneNew(c, st, 869, {"plane": {"origin": [-20, 0, 32.36068], "xDir": [0, 1, 0], "zDir": [-0.5257311094740752, 0, 0.8506508099867759]}, "obj": undefined});
        atStep(st, 870);
        cqRect(c, st, 870, 869, {"xLen": 24.72136, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 871);
        cqExtrude(c, st, 871, 870, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 871, "extrude");
        atStep(st, 872);
        cqUnion(c, st, 872, 868, {"toUnion": {"wp": 871}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 872, "union");
        atStep(st, 873);
        cqWorkplaneNew(c, st, 873, {"plane": {"origin": [-6.18034, 26.694962, 27.013016], "xDir": [0.5654131773116665, -0.5183325657073299, 0.6415912174037162], "zDir": [-0.1606220393540954, 0.6937804775983331, 0.7020464438889784]}, "obj": undefined});
        atStep(st, 874);
        cqRect(c, st, 874, 873, {"xLen": 21.86132259378467, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 875);
        cqExtrude(c, st, 875, 874, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 875, "extrude");
        atStep(st, 876);
        cqUnion(c, st, 876, 872, {"toUnion": {"wp": 875}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 876, "union");
        atStep(st, 877);
        cqWorkplaneNew(c, st, 877, {"plane": {"origin": [-26.18034, 16.18034, -22.36068], "xDir": [0.5000000082324079, -0.3090169847362764, -0.8090169929686842], "zDir": [-0.6881909585341867, 0.4253254042540716, -0.5877852542280283]}, "obj": undefined});
        atStep(st, 878);
        cqRect(c, st, 878, 877, {"xLen": 24.721359592967374, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 879);
        cqExtrude(c, st, 879, 878, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 879, "extrude");
        atStep(st, 880);
        cqUnion(c, st, 880, 876, {"toUnion": {"wp": 879}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 880, "union");
        atStep(st, 881);
        cqWorkplaneNew(c, st, 881, {"plane": {"origin": [33.193356, 10, -16.694962], "xDir": [0.07617804219954263, -0.914857747234018, -0.396525167186828], "zDir": [0.8626684813170629, 0.2598919137061835, -0.4338885623432013]}, "obj": undefined});
        atStep(st, 882);
        cqRect(c, st, 882, 881, {"xLen": 21.86132198898319, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 883);
        cqExtrude(c, st, 883, 882, {"until": 0.8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 883, "extrude");
        atStep(st, 884);
        cqUnion(c, st, 884, 880, {"toUnion": {"wp": 883}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 884, "union");
        atStep(st, 885);
        cqUnion(c, st, 885, 405, {"toUnion": {"wp": 884}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 885, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 885}), {"i": 5568});
    } catch (e) {
        return {"i": 5568, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c5568"] = case_5568();
    return out;
}
