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

const case_5599 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqCircle(c, st, 2, 1, {"radius": 30, "forConstruction": false});
        atStep(st, 3);
        cqCircle(c, st, 3, 2, {"radius": 24, "forConstruction": false});
        atStep(st, 4);
        cqExtrude(c, st, 4, 3, {"until": 8, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 4, "extrude");
        atStep(st, 5);
        cqWorkplaneNew(c, st, 5, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 6);
        cqWorkplane(c, st, 6, 5, {"offset": 8, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 7);
        cqCircle(c, st, 7, 6, {"radius": 38, "forConstruction": false});
        atStep(st, 8);
        cqCircle(c, st, 8, 7, {"radius": 30, "forConstruction": false});
        atStep(st, 9);
        cqExtrude(c, st, 9, 8, {"until": 2.5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 9, "extrude");
        atStep(st, 10);
        cqUnion(c, st, 10, 4, {"toUnion": {"wp": 9}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 10, "union");
        atStep(st, 11);
        cqWorkplaneNew(c, st, 11, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 12);
        cqWorkplane(c, st, 12, 11, {"offset": 8.3, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 13);
        cqTransformed(c, st, 13, 12, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 14);
        cqCenter(c, st, 14, 13, {"x": 27.9, "y": 0});
        atStep(st, 15);
        cqRect(c, st, 15, 14, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 16);
        cqExtrude(c, st, 16, 15, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 16, "extrude");
        atStep(st, 17);
        cqWorkplaneNew(c, st, 17, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 18);
        cqWorkplane(c, st, 18, 17, {"offset": 8.243396226415095, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 19);
        cqTransformed(c, st, 19, 18, {"rotate": [0, 0, 10.18867924528302], "offset": [0, 0, 0]});
        atStep(st, 20);
        cqCenter(c, st, 20, 19, {"x": 27.9, "y": 0});
        atStep(st, 21);
        cqRect(c, st, 21, 20, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 22);
        cqExtrude(c, st, 22, 21, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 22, "extrude");
        atStep(st, 23);
        cqUnion(c, st, 23, 16, {"toUnion": {"wp": 22}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 23, "union");
        atStep(st, 24);
        cqWorkplaneNew(c, st, 24, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 25);
        cqWorkplane(c, st, 25, 24, {"offset": 8.186792452830188, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 26);
        cqTransformed(c, st, 26, 25, {"rotate": [0, 0, 20.37735849056604], "offset": [0, 0, 0]});
        atStep(st, 27);
        cqCenter(c, st, 27, 26, {"x": 27.9, "y": 0});
        atStep(st, 28);
        cqRect(c, st, 28, 27, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 29);
        cqExtrude(c, st, 29, 28, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 29, "extrude");
        atStep(st, 30);
        cqUnion(c, st, 30, 23, {"toUnion": {"wp": 29}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 30, "union");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 31, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 32);
        cqWorkplane(c, st, 32, 31, {"offset": 8.130188679245283, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 33);
        cqTransformed(c, st, 33, 32, {"rotate": [0, 0, 30.566037735849054], "offset": [0, 0, 0]});
        atStep(st, 34);
        cqCenter(c, st, 34, 33, {"x": 27.9, "y": 0});
        atStep(st, 35);
        cqRect(c, st, 35, 34, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 36);
        cqExtrude(c, st, 36, 35, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 36, "extrude");
        atStep(st, 37);
        cqUnion(c, st, 37, 30, {"toUnion": {"wp": 36}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 37, "union");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 38, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 39);
        cqWorkplane(c, st, 39, 38, {"offset": 8.073584905660377, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 40);
        cqTransformed(c, st, 40, 39, {"rotate": [0, 0, 40.75471698113208], "offset": [0, 0, 0]});
        atStep(st, 41);
        cqCenter(c, st, 41, 40, {"x": 27.9, "y": 0});
        atStep(st, 42);
        cqRect(c, st, 42, 41, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 43);
        cqExtrude(c, st, 43, 42, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 43, "extrude");
        atStep(st, 44);
        cqUnion(c, st, 44, 37, {"toUnion": {"wp": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 44, "union");
        atStep(st, 45);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 46);
        cqWorkplane(c, st, 46, 45, {"offset": 8.016981132075472, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 47);
        cqTransformed(c, st, 47, 46, {"rotate": [0, 0, 50.943396226415096], "offset": [0, 0, 0]});
        atStep(st, 48);
        cqCenter(c, st, 48, 47, {"x": 27.9, "y": 0});
        atStep(st, 49);
        cqRect(c, st, 49, 48, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 50);
        cqExtrude(c, st, 50, 49, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 50, "extrude");
        atStep(st, 51);
        cqUnion(c, st, 51, 44, {"toUnion": {"wp": 50}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 51, "union");
        atStep(st, 52);
        cqWorkplaneNew(c, st, 52, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 53);
        cqWorkplane(c, st, 53, 52, {"offset": 7.960377358490565, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 54);
        cqTransformed(c, st, 54, 53, {"rotate": [0, 0, 61.13207547169811], "offset": [0, 0, 0]});
        atStep(st, 55);
        cqCenter(c, st, 55, 54, {"x": 27.9, "y": 0});
        atStep(st, 56);
        cqRect(c, st, 56, 55, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 57);
        cqExtrude(c, st, 57, 56, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 57, "extrude");
        atStep(st, 58);
        cqUnion(c, st, 58, 51, {"toUnion": {"wp": 57}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 58, "union");
        atStep(st, 59);
        cqWorkplaneNew(c, st, 59, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 60);
        cqWorkplane(c, st, 60, 59, {"offset": 7.90377358490566, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 61);
        cqTransformed(c, st, 61, 60, {"rotate": [0, 0, 71.32075471698113], "offset": [0, 0, 0]});
        atStep(st, 62);
        cqCenter(c, st, 62, 61, {"x": 27.9, "y": 0});
        atStep(st, 63);
        cqRect(c, st, 63, 62, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 64);
        cqExtrude(c, st, 64, 63, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 64, "extrude");
        atStep(st, 65);
        cqUnion(c, st, 65, 58, {"toUnion": {"wp": 64}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 65, "union");
        atStep(st, 66);
        cqWorkplaneNew(c, st, 66, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 67);
        cqWorkplane(c, st, 67, 66, {"offset": 7.847169811320755, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 68);
        cqTransformed(c, st, 68, 67, {"rotate": [0, 0, 81.50943396226415], "offset": [0, 0, 0]});
        atStep(st, 69);
        cqCenter(c, st, 69, 68, {"x": 27.9, "y": 0});
        atStep(st, 70);
        cqRect(c, st, 70, 69, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 71);
        cqExtrude(c, st, 71, 70, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 71, "extrude");
        atStep(st, 72);
        cqUnion(c, st, 72, 65, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 74);
        cqWorkplane(c, st, 74, 73, {"offset": 7.790566037735849, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 75);
        cqTransformed(c, st, 75, 74, {"rotate": [0, 0, 91.69811320754718], "offset": [0, 0, 0]});
        atStep(st, 76);
        cqCenter(c, st, 76, 75, {"x": 27.9, "y": 0});
        atStep(st, 77);
        cqRect(c, st, 77, 76, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 78);
        cqExtrude(c, st, 78, 77, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 78, "extrude");
        atStep(st, 79);
        cqUnion(c, st, 79, 72, {"toUnion": {"wp": 78}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 79, "union");
        atStep(st, 80);
        cqWorkplaneNew(c, st, 80, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 81);
        cqWorkplane(c, st, 81, 80, {"offset": 7.733962264150943, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 82);
        cqTransformed(c, st, 82, 81, {"rotate": [0, 0, 101.8867924528302], "offset": [0, 0, 0]});
        atStep(st, 83);
        cqCenter(c, st, 83, 82, {"x": 27.9, "y": 0});
        atStep(st, 84);
        cqRect(c, st, 84, 83, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 85);
        cqExtrude(c, st, 85, 84, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 85, "extrude");
        atStep(st, 86);
        cqUnion(c, st, 86, 79, {"toUnion": {"wp": 85}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 86, "union");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 88);
        cqWorkplane(c, st, 88, 87, {"offset": 7.677358490566037, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 89);
        cqTransformed(c, st, 89, 88, {"rotate": [0, 0, 112.0754716981132], "offset": [0, 0, 0]});
        atStep(st, 90);
        cqCenter(c, st, 90, 89, {"x": 27.9, "y": 0});
        atStep(st, 91);
        cqRect(c, st, 91, 90, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 92);
        cqExtrude(c, st, 92, 91, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 92, "extrude");
        atStep(st, 93);
        cqUnion(c, st, 93, 86, {"toUnion": {"wp": 92}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 93, "union");
        atStep(st, 94);
        cqWorkplaneNew(c, st, 94, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 95);
        cqWorkplane(c, st, 95, 94, {"offset": 7.620754716981132, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 96);
        cqTransformed(c, st, 96, 95, {"rotate": [0, 0, 122.26415094339625], "offset": [0, 0, 0]});
        atStep(st, 97);
        cqCenter(c, st, 97, 96, {"x": 27.9, "y": 0});
        atStep(st, 98);
        cqRect(c, st, 98, 97, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 99);
        cqExtrude(c, st, 99, 98, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 99, "extrude");
        atStep(st, 100);
        cqUnion(c, st, 100, 93, {"toUnion": {"wp": 99}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 100, "union");
        atStep(st, 101);
        cqWorkplaneNew(c, st, 101, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 102);
        cqWorkplane(c, st, 102, 101, {"offset": 7.564150943396226, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 103);
        cqTransformed(c, st, 103, 102, {"rotate": [0, 0, 132.45283018867926], "offset": [0, 0, 0]});
        atStep(st, 104);
        cqCenter(c, st, 104, 103, {"x": 27.9, "y": 0});
        atStep(st, 105);
        cqRect(c, st, 105, 104, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 106);
        cqExtrude(c, st, 106, 105, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 106, "extrude");
        atStep(st, 107);
        cqUnion(c, st, 107, 100, {"toUnion": {"wp": 106}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 107, "union");
        atStep(st, 108);
        cqWorkplaneNew(c, st, 108, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 109);
        cqWorkplane(c, st, 109, 108, {"offset": 7.50754716981132, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 110);
        cqTransformed(c, st, 110, 109, {"rotate": [0, 0, 142.64150943396226], "offset": [0, 0, 0]});
        atStep(st, 111);
        cqCenter(c, st, 111, 110, {"x": 27.9, "y": 0});
        atStep(st, 112);
        cqRect(c, st, 112, 111, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 113);
        cqExtrude(c, st, 113, 112, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 113, "extrude");
        atStep(st, 114);
        cqUnion(c, st, 114, 107, {"toUnion": {"wp": 113}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 114, "union");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 115, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 116);
        cqWorkplane(c, st, 116, 115, {"offset": 7.450943396226415, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 117);
        cqTransformed(c, st, 117, 116, {"rotate": [0, 0, 152.83018867924528], "offset": [0, 0, 0]});
        atStep(st, 118);
        cqCenter(c, st, 118, 117, {"x": 27.9, "y": 0});
        atStep(st, 119);
        cqRect(c, st, 119, 118, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 120);
        cqExtrude(c, st, 120, 119, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 120, "extrude");
        atStep(st, 121);
        cqUnion(c, st, 121, 114, {"toUnion": {"wp": 120}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 121, "union");
        atStep(st, 122);
        cqWorkplaneNew(c, st, 122, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 123);
        cqWorkplane(c, st, 123, 122, {"offset": 7.394339622641509, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 124);
        cqTransformed(c, st, 124, 123, {"rotate": [0, 0, 163.0188679245283], "offset": [0, 0, 0]});
        atStep(st, 125);
        cqCenter(c, st, 125, 124, {"x": 27.9, "y": 0});
        atStep(st, 126);
        cqRect(c, st, 126, 125, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 127);
        cqExtrude(c, st, 127, 126, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 127, "extrude");
        atStep(st, 128);
        cqUnion(c, st, 128, 121, {"toUnion": {"wp": 127}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 128, "union");
        atStep(st, 129);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 130);
        cqWorkplane(c, st, 130, 129, {"offset": 7.337735849056604, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 131);
        cqTransformed(c, st, 131, 130, {"rotate": [0, 0, 173.2075471698113], "offset": [0, 0, 0]});
        atStep(st, 132);
        cqCenter(c, st, 132, 131, {"x": 27.9, "y": 0});
        atStep(st, 133);
        cqRect(c, st, 133, 132, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 134);
        cqExtrude(c, st, 134, 133, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 134, "extrude");
        atStep(st, 135);
        cqUnion(c, st, 135, 128, {"toUnion": {"wp": 134}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 135, "union");
        atStep(st, 136);
        cqWorkplaneNew(c, st, 136, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 137);
        cqWorkplane(c, st, 137, 136, {"offset": 7.281132075471698, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 138);
        cqTransformed(c, st, 138, 137, {"rotate": [0, 0, 183.39622641509436], "offset": [0, 0, 0]});
        atStep(st, 139);
        cqCenter(c, st, 139, 138, {"x": 27.9, "y": 0});
        atStep(st, 140);
        cqRect(c, st, 140, 139, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 141);
        cqExtrude(c, st, 141, 140, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 141, "extrude");
        atStep(st, 142);
        cqUnion(c, st, 142, 135, {"toUnion": {"wp": 141}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 142, "union");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 143, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 144);
        cqWorkplane(c, st, 144, 143, {"offset": 7.224528301886792, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 145);
        cqTransformed(c, st, 145, 144, {"rotate": [0, 0, 193.58490566037736], "offset": [0, 0, 0]});
        atStep(st, 146);
        cqCenter(c, st, 146, 145, {"x": 27.9, "y": 0});
        atStep(st, 147);
        cqRect(c, st, 147, 146, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 148);
        cqExtrude(c, st, 148, 147, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 148, "extrude");
        atStep(st, 149);
        cqUnion(c, st, 149, 142, {"toUnion": {"wp": 148}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 149, "union");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 151);
        cqWorkplane(c, st, 151, 150, {"offset": 7.167924528301886, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 152);
        cqTransformed(c, st, 152, 151, {"rotate": [0, 0, 203.7735849056604], "offset": [0, 0, 0]});
        atStep(st, 153);
        cqCenter(c, st, 153, 152, {"x": 27.9, "y": 0});
        atStep(st, 154);
        cqRect(c, st, 154, 153, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 155);
        cqExtrude(c, st, 155, 154, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 155, "extrude");
        atStep(st, 156);
        cqUnion(c, st, 156, 149, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        atStep(st, 157);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 158);
        cqWorkplane(c, st, 158, 157, {"offset": 7.1113207547169806, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 159);
        cqTransformed(c, st, 159, 158, {"rotate": [0, 0, 213.9622641509434], "offset": [0, 0, 0]});
        atStep(st, 160);
        cqCenter(c, st, 160, 159, {"x": 27.9, "y": 0});
        atStep(st, 161);
        cqRect(c, st, 161, 160, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 162);
        cqExtrude(c, st, 162, 161, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 162, "extrude");
        atStep(st, 163);
        cqUnion(c, st, 163, 156, {"toUnion": {"wp": 162}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 163, "union");
        atStep(st, 164);
        cqWorkplaneNew(c, st, 164, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 165);
        cqWorkplane(c, st, 165, 164, {"offset": 7.054716981132075, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 166);
        cqTransformed(c, st, 166, 165, {"rotate": [0, 0, 224.1509433962264], "offset": [0, 0, 0]});
        atStep(st, 167);
        cqCenter(c, st, 167, 166, {"x": 27.9, "y": 0});
        atStep(st, 168);
        cqRect(c, st, 168, 167, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 169);
        cqExtrude(c, st, 169, 168, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 169, "extrude");
        atStep(st, 170);
        cqUnion(c, st, 170, 163, {"toUnion": {"wp": 169}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 170, "union");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 172);
        cqWorkplane(c, st, 172, 171, {"offset": 6.99811320754717, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 173);
        cqTransformed(c, st, 173, 172, {"rotate": [0, 0, 234.33962264150944], "offset": [0, 0, 0]});
        atStep(st, 174);
        cqCenter(c, st, 174, 173, {"x": 27.9, "y": 0});
        atStep(st, 175);
        cqRect(c, st, 175, 174, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 176);
        cqExtrude(c, st, 176, 175, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 176, "extrude");
        atStep(st, 177);
        cqUnion(c, st, 177, 170, {"toUnion": {"wp": 176}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 177, "union");
        atStep(st, 178);
        cqWorkplaneNew(c, st, 178, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 179);
        cqWorkplane(c, st, 179, 178, {"offset": 6.941509433962264, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 180);
        cqTransformed(c, st, 180, 179, {"rotate": [0, 0, 244.5283018867925], "offset": [0, 0, 0]});
        atStep(st, 181);
        cqCenter(c, st, 181, 180, {"x": 27.9, "y": 0});
        atStep(st, 182);
        cqRect(c, st, 182, 181, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 183);
        cqExtrude(c, st, 183, 182, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 183, "extrude");
        atStep(st, 184);
        cqUnion(c, st, 184, 177, {"toUnion": {"wp": 183}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 184, "union");
        atStep(st, 185);
        cqWorkplaneNew(c, st, 185, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 186);
        cqWorkplane(c, st, 186, 185, {"offset": 6.884905660377359, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 187);
        cqTransformed(c, st, 187, 186, {"rotate": [0, 0, 254.7169811320755], "offset": [0, 0, 0]});
        atStep(st, 188);
        cqCenter(c, st, 188, 187, {"x": 27.9, "y": 0});
        atStep(st, 189);
        cqRect(c, st, 189, 188, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 190);
        cqExtrude(c, st, 190, 189, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 190, "extrude");
        atStep(st, 191);
        cqUnion(c, st, 191, 184, {"toUnion": {"wp": 190}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 191, "union");
        atStep(st, 192);
        cqWorkplaneNew(c, st, 192, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 193);
        cqWorkplane(c, st, 193, 192, {"offset": 6.8283018867924525, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 194);
        cqTransformed(c, st, 194, 193, {"rotate": [0, 0, 264.9056603773585], "offset": [0, 0, 0]});
        atStep(st, 195);
        cqCenter(c, st, 195, 194, {"x": 27.9, "y": 0});
        atStep(st, 196);
        cqRect(c, st, 196, 195, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 197);
        cqExtrude(c, st, 197, 196, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 197, "extrude");
        atStep(st, 198);
        cqUnion(c, st, 198, 191, {"toUnion": {"wp": 197}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 198, "union");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 199, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 200);
        cqWorkplane(c, st, 200, 199, {"offset": 6.771698113207547, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 201);
        cqTransformed(c, st, 201, 200, {"rotate": [0, 0, 275.09433962264154], "offset": [0, 0, 0]});
        atStep(st, 202);
        cqCenter(c, st, 202, 201, {"x": 27.9, "y": 0});
        atStep(st, 203);
        cqRect(c, st, 203, 202, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 204);
        cqExtrude(c, st, 204, 203, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 204, "extrude");
        atStep(st, 205);
        cqUnion(c, st, 205, 198, {"toUnion": {"wp": 204}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 205, "union");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 206, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 207);
        cqWorkplane(c, st, 207, 206, {"offset": 6.715094339622641, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 208);
        cqTransformed(c, st, 208, 207, {"rotate": [0, 0, 285.2830188679245], "offset": [0, 0, 0]});
        atStep(st, 209);
        cqCenter(c, st, 209, 208, {"x": 27.9, "y": 0});
        atStep(st, 210);
        cqRect(c, st, 210, 209, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 211);
        cqExtrude(c, st, 211, 210, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 211, "extrude");
        atStep(st, 212);
        cqUnion(c, st, 212, 205, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 214);
        cqWorkplane(c, st, 214, 213, {"offset": 6.658490566037735, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 215);
        cqTransformed(c, st, 215, 214, {"rotate": [0, 0, 295.47169811320754], "offset": [0, 0, 0]});
        atStep(st, 216);
        cqCenter(c, st, 216, 215, {"x": 27.9, "y": 0});
        atStep(st, 217);
        cqRect(c, st, 217, 216, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 218);
        cqExtrude(c, st, 218, 217, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 218, "extrude");
        atStep(st, 219);
        cqUnion(c, st, 219, 212, {"toUnion": {"wp": 218}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 219, "union");
        atStep(st, 220);
        cqWorkplaneNew(c, st, 220, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 221);
        cqWorkplane(c, st, 221, 220, {"offset": 6.60188679245283, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 222);
        cqTransformed(c, st, 222, 221, {"rotate": [0, 0, 305.66037735849056], "offset": [0, 0, 0]});
        atStep(st, 223);
        cqCenter(c, st, 223, 222, {"x": 27.9, "y": 0});
        atStep(st, 224);
        cqRect(c, st, 224, 223, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 225);
        cqExtrude(c, st, 225, 224, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 225, "extrude");
        atStep(st, 226);
        cqUnion(c, st, 226, 219, {"toUnion": {"wp": 225}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 226, "union");
        atStep(st, 227);
        cqWorkplaneNew(c, st, 227, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 228);
        cqWorkplane(c, st, 228, 227, {"offset": 6.5452830188679245, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 229);
        cqTransformed(c, st, 229, 228, {"rotate": [0, 0, 315.8490566037736], "offset": [0, 0, 0]});
        atStep(st, 230);
        cqCenter(c, st, 230, 229, {"x": 27.9, "y": 0});
        atStep(st, 231);
        cqRect(c, st, 231, 230, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 232);
        cqExtrude(c, st, 232, 231, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 232, "extrude");
        atStep(st, 233);
        cqUnion(c, st, 233, 226, {"toUnion": {"wp": 232}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 233, "union");
        atStep(st, 234);
        cqWorkplaneNew(c, st, 234, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 235);
        cqWorkplane(c, st, 235, 234, {"offset": 6.488679245283019, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 236);
        cqTransformed(c, st, 236, 235, {"rotate": [0, 0, 326.0377358490566], "offset": [0, 0, 0]});
        atStep(st, 237);
        cqCenter(c, st, 237, 236, {"x": 27.9, "y": 0});
        atStep(st, 238);
        cqRect(c, st, 238, 237, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 239);
        cqExtrude(c, st, 239, 238, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 239, "extrude");
        atStep(st, 240);
        cqUnion(c, st, 240, 233, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        atStep(st, 241);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 242);
        cqWorkplane(c, st, 242, 241, {"offset": 6.432075471698114, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 243);
        cqTransformed(c, st, 243, 242, {"rotate": [0, 0, 336.22641509433964], "offset": [0, 0, 0]});
        atStep(st, 244);
        cqCenter(c, st, 244, 243, {"x": 27.9, "y": 0});
        atStep(st, 245);
        cqRect(c, st, 245, 244, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 246);
        cqExtrude(c, st, 246, 245, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 246, "extrude");
        atStep(st, 247);
        cqUnion(c, st, 247, 240, {"toUnion": {"wp": 246}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 247, "union");
        atStep(st, 248);
        cqWorkplaneNew(c, st, 248, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 249);
        cqWorkplane(c, st, 249, 248, {"offset": 6.375471698113207, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 250);
        cqTransformed(c, st, 250, 249, {"rotate": [0, 0, 346.41509433962267], "offset": [0, 0, 0]});
        atStep(st, 251);
        cqCenter(c, st, 251, 250, {"x": 27.9, "y": 0});
        atStep(st, 252);
        cqRect(c, st, 252, 251, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 253);
        cqExtrude(c, st, 253, 252, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 253, "extrude");
        atStep(st, 254);
        cqUnion(c, st, 254, 247, {"toUnion": {"wp": 253}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 254, "union");
        atStep(st, 255);
        cqWorkplaneNew(c, st, 255, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 256);
        cqWorkplane(c, st, 256, 255, {"offset": 6.318867924528302, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 257);
        cqTransformed(c, st, 257, 256, {"rotate": [0, 0, 356.6037735849057], "offset": [0, 0, 0]});
        atStep(st, 258);
        cqCenter(c, st, 258, 257, {"x": 27.9, "y": 0});
        atStep(st, 259);
        cqRect(c, st, 259, 258, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 260);
        cqExtrude(c, st, 260, 259, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 260, "extrude");
        atStep(st, 261);
        cqUnion(c, st, 261, 254, {"toUnion": {"wp": 260}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 261, "union");
        atStep(st, 262);
        cqWorkplaneNew(c, st, 262, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 263);
        cqWorkplane(c, st, 263, 262, {"offset": 6.262264150943396, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 264);
        cqTransformed(c, st, 264, 263, {"rotate": [0, 0, 366.7924528301887], "offset": [0, 0, 0]});
        atStep(st, 265);
        cqCenter(c, st, 265, 264, {"x": 27.9, "y": 0});
        atStep(st, 266);
        cqRect(c, st, 266, 265, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 267);
        cqExtrude(c, st, 267, 266, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 267, "extrude");
        atStep(st, 268);
        cqUnion(c, st, 268, 261, {"toUnion": {"wp": 267}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 268, "union");
        atStep(st, 269);
        cqWorkplaneNew(c, st, 269, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 270);
        cqWorkplane(c, st, 270, 269, {"offset": 6.20566037735849, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 271);
        cqTransformed(c, st, 271, 270, {"rotate": [0, 0, 376.9811320754717], "offset": [0, 0, 0]});
        atStep(st, 272);
        cqCenter(c, st, 272, 271, {"x": 27.9, "y": 0});
        atStep(st, 273);
        cqRect(c, st, 273, 272, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 274);
        cqExtrude(c, st, 274, 273, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 274, "extrude");
        atStep(st, 275);
        cqUnion(c, st, 275, 268, {"toUnion": {"wp": 274}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 275, "union");
        atStep(st, 276);
        cqWorkplaneNew(c, st, 276, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 277);
        cqWorkplane(c, st, 277, 276, {"offset": 6.149056603773585, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 278);
        cqTransformed(c, st, 278, 277, {"rotate": [0, 0, 387.1698113207547], "offset": [0, 0, 0]});
        atStep(st, 279);
        cqCenter(c, st, 279, 278, {"x": 27.9, "y": 0});
        atStep(st, 280);
        cqRect(c, st, 280, 279, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 281);
        cqExtrude(c, st, 281, 280, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 281, "extrude");
        atStep(st, 282);
        cqUnion(c, st, 282, 275, {"toUnion": {"wp": 281}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 282, "union");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 284);
        cqWorkplane(c, st, 284, 283, {"offset": 6.092452830188679, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 285);
        cqTransformed(c, st, 285, 284, {"rotate": [0, 0, 397.35849056603774], "offset": [0, 0, 0]});
        atStep(st, 286);
        cqCenter(c, st, 286, 285, {"x": 27.9, "y": 0});
        atStep(st, 287);
        cqRect(c, st, 287, 286, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 288);
        cqExtrude(c, st, 288, 287, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 288, "extrude");
        atStep(st, 289);
        cqUnion(c, st, 289, 282, {"toUnion": {"wp": 288}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 289, "union");
        atStep(st, 290);
        cqWorkplaneNew(c, st, 290, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 291);
        cqWorkplane(c, st, 291, 290, {"offset": 6.035849056603773, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 292);
        cqTransformed(c, st, 292, 291, {"rotate": [0, 0, 407.5471698113208], "offset": [0, 0, 0]});
        atStep(st, 293);
        cqCenter(c, st, 293, 292, {"x": 27.9, "y": 0});
        atStep(st, 294);
        cqRect(c, st, 294, 293, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 295);
        cqExtrude(c, st, 295, 294, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 295, "extrude");
        atStep(st, 296);
        cqUnion(c, st, 296, 289, {"toUnion": {"wp": 295}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 296, "union");
        atStep(st, 297);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 298);
        cqWorkplane(c, st, 298, 297, {"offset": 5.979245283018868, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 299);
        cqTransformed(c, st, 299, 298, {"rotate": [0, 0, 417.7358490566038], "offset": [0, 0, 0]});
        atStep(st, 300);
        cqCenter(c, st, 300, 299, {"x": 27.9, "y": 0});
        atStep(st, 301);
        cqRect(c, st, 301, 300, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 302);
        cqExtrude(c, st, 302, 301, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 302, "extrude");
        atStep(st, 303);
        cqUnion(c, st, 303, 296, {"toUnion": {"wp": 302}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 303, "union");
        atStep(st, 304);
        cqWorkplaneNew(c, st, 304, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 305);
        cqWorkplane(c, st, 305, 304, {"offset": 5.922641509433962, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 306);
        cqTransformed(c, st, 306, 305, {"rotate": [0, 0, 427.9245283018868], "offset": [0, 0, 0]});
        atStep(st, 307);
        cqCenter(c, st, 307, 306, {"x": 27.9, "y": 0});
        atStep(st, 308);
        cqRect(c, st, 308, 307, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 309);
        cqExtrude(c, st, 309, 308, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 309, "extrude");
        atStep(st, 310);
        cqUnion(c, st, 310, 303, {"toUnion": {"wp": 309}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 310, "union");
        atStep(st, 311);
        cqWorkplaneNew(c, st, 311, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 312);
        cqWorkplane(c, st, 312, 311, {"offset": 5.866037735849056, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 313);
        cqTransformed(c, st, 313, 312, {"rotate": [0, 0, 438.11320754716985], "offset": [0, 0, 0]});
        atStep(st, 314);
        cqCenter(c, st, 314, 313, {"x": 27.9, "y": 0});
        atStep(st, 315);
        cqRect(c, st, 315, 314, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 316);
        cqExtrude(c, st, 316, 315, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 316, "extrude");
        atStep(st, 317);
        cqUnion(c, st, 317, 310, {"toUnion": {"wp": 316}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 317, "union");
        atStep(st, 318);
        cqWorkplaneNew(c, st, 318, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 319);
        cqWorkplane(c, st, 319, 318, {"offset": 5.80943396226415, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 320);
        cqTransformed(c, st, 320, 319, {"rotate": [0, 0, 448.3018867924528], "offset": [0, 0, 0]});
        atStep(st, 321);
        cqCenter(c, st, 321, 320, {"x": 27.9, "y": 0});
        atStep(st, 322);
        cqRect(c, st, 322, 321, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 323);
        cqExtrude(c, st, 323, 322, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 323, "extrude");
        atStep(st, 324);
        cqUnion(c, st, 324, 317, {"toUnion": {"wp": 323}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 324, "union");
        atStep(st, 325);
        cqWorkplaneNew(c, st, 325, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 326);
        cqWorkplane(c, st, 326, 325, {"offset": 5.752830188679245, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 327);
        cqTransformed(c, st, 327, 326, {"rotate": [0, 0, 458.49056603773585], "offset": [0, 0, 0]});
        atStep(st, 328);
        cqCenter(c, st, 328, 327, {"x": 27.9, "y": 0});
        atStep(st, 329);
        cqRect(c, st, 329, 328, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 330);
        cqExtrude(c, st, 330, 329, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 330, "extrude");
        atStep(st, 331);
        cqUnion(c, st, 331, 324, {"toUnion": {"wp": 330}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 331, "union");
        atStep(st, 332);
        cqWorkplaneNew(c, st, 332, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 333);
        cqWorkplane(c, st, 333, 332, {"offset": 5.69622641509434, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 334);
        cqTransformed(c, st, 334, 333, {"rotate": [0, 0, 468.6792452830189], "offset": [0, 0, 0]});
        atStep(st, 335);
        cqCenter(c, st, 335, 334, {"x": 27.9, "y": 0});
        atStep(st, 336);
        cqRect(c, st, 336, 335, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 337);
        cqExtrude(c, st, 337, 336, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 337, "extrude");
        atStep(st, 338);
        cqUnion(c, st, 338, 331, {"toUnion": {"wp": 337}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 338, "union");
        atStep(st, 339);
        cqWorkplaneNew(c, st, 339, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 340);
        cqWorkplane(c, st, 340, 339, {"offset": 5.639622641509434, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 341);
        cqTransformed(c, st, 341, 340, {"rotate": [0, 0, 478.8679245283019], "offset": [0, 0, 0]});
        atStep(st, 342);
        cqCenter(c, st, 342, 341, {"x": 27.9, "y": 0});
        atStep(st, 343);
        cqRect(c, st, 343, 342, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 344);
        cqExtrude(c, st, 344, 343, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 344, "extrude");
        atStep(st, 345);
        cqUnion(c, st, 345, 338, {"toUnion": {"wp": 344}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 345, "union");
        atStep(st, 346);
        cqWorkplaneNew(c, st, 346, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 347);
        cqWorkplane(c, st, 347, 346, {"offset": 5.583018867924528, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 348);
        cqTransformed(c, st, 348, 347, {"rotate": [0, 0, 489.0566037735849], "offset": [0, 0, 0]});
        atStep(st, 349);
        cqCenter(c, st, 349, 348, {"x": 27.9, "y": 0});
        atStep(st, 350);
        cqRect(c, st, 350, 349, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 351);
        cqExtrude(c, st, 351, 350, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 351, "extrude");
        atStep(st, 352);
        cqUnion(c, st, 352, 345, {"toUnion": {"wp": 351}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 352, "union");
        atStep(st, 353);
        cqWorkplaneNew(c, st, 353, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 354);
        cqWorkplane(c, st, 354, 353, {"offset": 5.526415094339622, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 355);
        cqTransformed(c, st, 355, 354, {"rotate": [0, 0, 499.2452830188679], "offset": [0, 0, 0]});
        atStep(st, 356);
        cqCenter(c, st, 356, 355, {"x": 27.9, "y": 0});
        atStep(st, 357);
        cqRect(c, st, 357, 356, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 358);
        cqExtrude(c, st, 358, 357, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 358, "extrude");
        atStep(st, 359);
        cqUnion(c, st, 359, 352, {"toUnion": {"wp": 358}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 359, "union");
        atStep(st, 360);
        cqWorkplaneNew(c, st, 360, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 361);
        cqWorkplane(c, st, 361, 360, {"offset": 5.469811320754717, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 362);
        cqTransformed(c, st, 362, 361, {"rotate": [0, 0, 509.433962264151], "offset": [0, 0, 0]});
        atStep(st, 363);
        cqCenter(c, st, 363, 362, {"x": 27.9, "y": 0});
        atStep(st, 364);
        cqRect(c, st, 364, 363, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 365);
        cqExtrude(c, st, 365, 364, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 365, "extrude");
        atStep(st, 366);
        cqUnion(c, st, 366, 359, {"toUnion": {"wp": 365}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 366, "union");
        atStep(st, 367);
        cqWorkplaneNew(c, st, 367, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 368);
        cqWorkplane(c, st, 368, 367, {"offset": 5.413207547169811, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 369);
        cqTransformed(c, st, 369, 368, {"rotate": [0, 0, 519.622641509434], "offset": [0, 0, 0]});
        atStep(st, 370);
        cqCenter(c, st, 370, 369, {"x": 27.9, "y": 0});
        atStep(st, 371);
        cqRect(c, st, 371, 370, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 372);
        cqExtrude(c, st, 372, 371, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 372, "extrude");
        atStep(st, 373);
        cqUnion(c, st, 373, 366, {"toUnion": {"wp": 372}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 373, "union");
        atStep(st, 374);
        cqWorkplaneNew(c, st, 374, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 375);
        cqWorkplane(c, st, 375, 374, {"offset": 5.356603773584905, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 376);
        cqTransformed(c, st, 376, 375, {"rotate": [0, 0, 529.811320754717], "offset": [0, 0, 0]});
        atStep(st, 377);
        cqCenter(c, st, 377, 376, {"x": 27.9, "y": 0});
        atStep(st, 378);
        cqRect(c, st, 378, 377, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 379);
        cqExtrude(c, st, 379, 378, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 379, "extrude");
        atStep(st, 380);
        cqUnion(c, st, 380, 373, {"toUnion": {"wp": 379}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 380, "union");
        atStep(st, 381);
        cqWorkplaneNew(c, st, 381, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 382);
        cqWorkplane(c, st, 382, 381, {"offset": 5.3, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 383);
        cqTransformed(c, st, 383, 382, {"rotate": [0, 0, 540], "offset": [0, 0, 0]});
        atStep(st, 384);
        cqCenter(c, st, 384, 383, {"x": 27.9, "y": 0});
        atStep(st, 385);
        cqRect(c, st, 385, 384, {"xLen": 7.938152606957456, "yLen": 8, "centered": true, "forConstruction": false});
        atStep(st, 386);
        cqExtrude(c, st, 386, 385, {"until": -3.4, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 386, "extrude");
        atStep(st, 387);
        cqUnion(c, st, 387, 380, {"toUnion": {"wp": 386}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 387, "union");
        atStep(st, 388);
        cqCut(c, st, 388, 10, {"toCut": {"wp": 387}, "clean": true, "tol": undefined});
        stepVol(c, st, 388, "cut");
        atStep(st, 389);
        cqWorkplaneNew(c, st, 389, {"plane": {"origin": [26.731086732175225, 13.620168982686142, 7.85], "xDir": [-0.2702551725892812, -0.13770188829174793, 0.9528905139886872], "zDir": [-0.8490316648011277, -0.4326032406427977, -0.30331447105335285]}, "obj": undefined});
        atStep(st, 390);
        cqPolyline(c, st, 390, 389, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 391);
        cqClose(c, st, 391, 390, {});
        atStep(st, 392);
        cqExtrude(c, st, 392, 391, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 392, "extrude");
        atStep(st, 393);
        cqWorkplaneNew(c, st, 393, {"plane": {"origin": [4.693190385671969, 29.63163790619473, 7.55], "xDir": [-0.047448837018191865, -0.299580166593178, 0.9528905139886872], "zDir": [-0.149064917797731, -0.9411588505303344, -0.3033144710533529]}, "obj": undefined});
        atStep(st, 394);
        cqPolyline(c, st, 394, 393, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 395);
        cqClose(c, st, 395, 394, {});
        atStep(st, 396);
        cqExtrude(c, st, 396, 395, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 396, "extrude");
        atStep(st, 397);
        cqUnion(c, st, 397, 392, {"toUnion": {"wp": 396}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 397, "union");
        atStep(st, 398);
        cqWorkplaneNew(c, st, 398, {"plane": {"origin": [-21.21391054237761, 21.213910542377615, 7.25], "xDir": [0.21447571931383655, -0.21447571931383663, 0.9528905139886872], "zDir": [0.6737953441697355, -0.6737953441697356, -0.3033144710533529]}, "obj": undefined});
        atStep(st, 399);
        cqPolyline(c, st, 399, 398, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 400);
        cqClose(c, st, 400, 399, {});
        atStep(st, 401);
        cqExtrude(c, st, 401, 400, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 401, "extrude");
        atStep(st, 402);
        cqUnion(c, st, 402, 397, {"toUnion": {"wp": 401}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 402, "union");
        atStep(st, 403);
        cqWorkplaneNew(c, st, 403, {"plane": {"origin": [-29.63163790619473, -4.693190385671962, 6.95], "xDir": [0.29958016659317804, 0.04744883701819182, 0.9528905139886872], "zDir": [0.9411588505303344, 0.14906491779773082, -0.3033144710533529]}, "obj": undefined});
        atStep(st, 404);
        cqPolyline(c, st, 404, 403, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 405);
        cqClose(c, st, 405, 404, {});
        atStep(st, 406);
        cqExtrude(c, st, 406, 405, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 406, "extrude");
        atStep(st, 407);
        cqUnion(c, st, 407, 402, {"toUnion": {"wp": 406}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 407, "union");
        atStep(st, 408);
        cqWorkplaneNew(c, st, 408, {"plane": {"origin": [-13.620168982686147, -26.731086732175225, 6.65], "xDir": [0.137701888291748, 0.27025517258928117, 0.9528905139886872], "zDir": [0.4326032406427978, 0.8490316648011276, -0.30331447105335285]}, "obj": undefined});
        atStep(st, 409);
        cqPolyline(c, st, 409, 408, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 410);
        cqClose(c, st, 410, 409, {});
        atStep(st, 411);
        cqExtrude(c, st, 411, 410, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 411, "extrude");
        atStep(st, 412);
        cqUnion(c, st, 412, 407, {"toUnion": {"wp": 411}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 412, "union");
        atStep(st, 413);
        cqWorkplaneNew(c, st, 413, {"plane": {"origin": [13.62016898268614, -26.73108673217523, 6.35], "xDir": [-0.13770188829174787, 0.2702551725892812, 0.9528905139886872], "zDir": [-0.4326032406427976, 0.8490316648011278, -0.30331447105335285]}, "obj": undefined});
        atStep(st, 414);
        cqPolyline(c, st, 414, 413, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 415);
        cqClose(c, st, 415, 414, {});
        atStep(st, 416);
        cqExtrude(c, st, 416, 415, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 416, "extrude");
        atStep(st, 417);
        cqUnion(c, st, 417, 412, {"toUnion": {"wp": 416}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 417, "union");
        atStep(st, 418);
        cqWorkplaneNew(c, st, 418, {"plane": {"origin": [29.63163790619473, -4.693190385671974, 6.05], "xDir": [-0.299580166593178, 0.047448837018191935, 0.9528905139886872], "zDir": [-0.9411588505303344, 0.1490649177977312, -0.3033144710533529]}, "obj": undefined});
        atStep(st, 419);
        cqPolyline(c, st, 419, 418, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 420);
        cqClose(c, st, 420, 419, {});
        atStep(st, 421);
        cqExtrude(c, st, 421, 420, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 421, "extrude");
        atStep(st, 422);
        cqUnion(c, st, 422, 417, {"toUnion": {"wp": 421}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 422, "union");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [21.21391054237762, 21.213910542377608, 5.75], "xDir": [-0.21447571931383663, -0.21447571931383655, 0.9528905139886872], "zDir": [-0.6737953441697357, -0.6737953441697354, -0.3033144710533529]}, "obj": undefined});
        atStep(st, 424);
        cqPolyline(c, st, 424, 423, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 425);
        cqClose(c, st, 425, 424, {});
        atStep(st, 426);
        cqExtrude(c, st, 426, 425, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 427);
        cqUnion(c, st, 427, 422, {"toUnion": {"wp": 426}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 427, "union");
        atStep(st, 428);
        cqWorkplaneNew(c, st, 428, {"plane": {"origin": [-4.693190385671983, 29.63163790619473, 5.449999999999999], "xDir": [0.047448837018192025, -0.29958016659317804, 0.9528905139886872], "zDir": [0.14906491779773148, -0.9411588505303344, -0.3033144710533529]}, "obj": undefined});
        atStep(st, 429);
        cqPolyline(c, st, 429, 428, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 430);
        cqClose(c, st, 430, 429, {});
        atStep(st, 431);
        cqExtrude(c, st, 431, 430, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 431, "extrude");
        atStep(st, 432);
        cqUnion(c, st, 432, 427, {"toUnion": {"wp": 431}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        atStep(st, 433);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [-26.731086732175232, 13.62016898268613, 5.15], "xDir": [0.27025517258928133, -0.13770188829174784, 0.9528905139886872], "zDir": [0.8490316648011279, -0.43260324064279737, -0.3033144710533529]}, "obj": undefined});
        atStep(st, 434);
        cqPolyline(c, st, 434, 433, {"listOfXYTuple": [[0, -1.25], [0, 1.25], [3, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 435);
        cqClose(c, st, 435, 434, {});
        atStep(st, 436);
        cqExtrude(c, st, 436, 435, {"until": 3.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 436, "extrude");
        atStep(st, 437);
        cqUnion(c, st, 437, 432, {"toUnion": {"wp": 436}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 437, "union");
        atStep(st, 438);
        cqUnion(c, st, 438, 388, {"toUnion": {"wp": 437}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 438, "union");
        atStep(st, 439);
        cqWorkplaneNew(c, st, 439, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 440);
        cqWorkplane(c, st, 440, 439, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 441);
        cqTransformed(c, st, 441, 440, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 442);
        cqCenter(c, st, 442, 441, {"x": 29.52, "y": 0});
        atStep(st, 443);
        cqPolyline(c, st, 443, 442, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 444);
        cqClose(c, st, 444, 443, {});
        atStep(st, 445);
        cqExtrude(c, st, 445, 444, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 445, "extrude");
        atStep(st, 446);
        cqWorkplaneNew(c, st, 446, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 447);
        cqWorkplane(c, st, 447, 446, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 448);
        cqTransformed(c, st, 448, 447, {"rotate": [0, 0, 15], "offset": [0, 0, 0]});
        atStep(st, 449);
        cqCenter(c, st, 449, 448, {"x": 29.52, "y": 0});
        atStep(st, 450);
        cqPolyline(c, st, 450, 449, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 451);
        cqClose(c, st, 451, 450, {});
        atStep(st, 452);
        cqExtrude(c, st, 452, 451, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 452, "extrude");
        atStep(st, 453);
        cqUnion(c, st, 453, 445, {"toUnion": {"wp": 452}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 453, "union");
        atStep(st, 454);
        cqWorkplaneNew(c, st, 454, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 455);
        cqWorkplane(c, st, 455, 454, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 456);
        cqTransformed(c, st, 456, 455, {"rotate": [0, 0, 30], "offset": [0, 0, 0]});
        atStep(st, 457);
        cqCenter(c, st, 457, 456, {"x": 29.52, "y": 0});
        atStep(st, 458);
        cqPolyline(c, st, 458, 457, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 459);
        cqClose(c, st, 459, 458, {});
        atStep(st, 460);
        cqExtrude(c, st, 460, 459, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 460, "extrude");
        atStep(st, 461);
        cqUnion(c, st, 461, 453, {"toUnion": {"wp": 460}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 461, "union");
        atStep(st, 462);
        cqWorkplaneNew(c, st, 462, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 463);
        cqWorkplane(c, st, 463, 462, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 464);
        cqTransformed(c, st, 464, 463, {"rotate": [0, 0, 45], "offset": [0, 0, 0]});
        atStep(st, 465);
        cqCenter(c, st, 465, 464, {"x": 29.52, "y": 0});
        atStep(st, 466);
        cqPolyline(c, st, 466, 465, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 467);
        cqClose(c, st, 467, 466, {});
        atStep(st, 468);
        cqExtrude(c, st, 468, 467, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 468, "extrude");
        atStep(st, 469);
        cqUnion(c, st, 469, 461, {"toUnion": {"wp": 468}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 469, "union");
        atStep(st, 470);
        cqWorkplaneNew(c, st, 470, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 471);
        cqWorkplane(c, st, 471, 470, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 472);
        cqTransformed(c, st, 472, 471, {"rotate": [0, 0, 60], "offset": [0, 0, 0]});
        atStep(st, 473);
        cqCenter(c, st, 473, 472, {"x": 29.52, "y": 0});
        atStep(st, 474);
        cqPolyline(c, st, 474, 473, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 475);
        cqClose(c, st, 475, 474, {});
        atStep(st, 476);
        cqExtrude(c, st, 476, 475, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 476, "extrude");
        atStep(st, 477);
        cqUnion(c, st, 477, 469, {"toUnion": {"wp": 476}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 477, "union");
        atStep(st, 478);
        cqWorkplaneNew(c, st, 478, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 479);
        cqWorkplane(c, st, 479, 478, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 480);
        cqTransformed(c, st, 480, 479, {"rotate": [0, 0, 75], "offset": [0, 0, 0]});
        atStep(st, 481);
        cqCenter(c, st, 481, 480, {"x": 29.52, "y": 0});
        atStep(st, 482);
        cqPolyline(c, st, 482, 481, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 483);
        cqClose(c, st, 483, 482, {});
        atStep(st, 484);
        cqExtrude(c, st, 484, 483, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 484, "extrude");
        atStep(st, 485);
        cqUnion(c, st, 485, 477, {"toUnion": {"wp": 484}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 485, "union");
        atStep(st, 486);
        cqWorkplaneNew(c, st, 486, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 487);
        cqWorkplane(c, st, 487, 486, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 488);
        cqTransformed(c, st, 488, 487, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 489);
        cqCenter(c, st, 489, 488, {"x": 29.52, "y": 0});
        atStep(st, 490);
        cqPolyline(c, st, 490, 489, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 491);
        cqClose(c, st, 491, 490, {});
        atStep(st, 492);
        cqExtrude(c, st, 492, 491, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 492, "extrude");
        atStep(st, 493);
        cqUnion(c, st, 493, 485, {"toUnion": {"wp": 492}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 493, "union");
        atStep(st, 494);
        cqWorkplaneNew(c, st, 494, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 495);
        cqWorkplane(c, st, 495, 494, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 496);
        cqTransformed(c, st, 496, 495, {"rotate": [0, 0, 105], "offset": [0, 0, 0]});
        atStep(st, 497);
        cqCenter(c, st, 497, 496, {"x": 29.52, "y": 0});
        atStep(st, 498);
        cqPolyline(c, st, 498, 497, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 499);
        cqClose(c, st, 499, 498, {});
        atStep(st, 500);
        cqExtrude(c, st, 500, 499, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 500, "extrude");
        atStep(st, 501);
        cqUnion(c, st, 501, 493, {"toUnion": {"wp": 500}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 501, "union");
        atStep(st, 502);
        cqWorkplaneNew(c, st, 502, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 503);
        cqWorkplane(c, st, 503, 502, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 504);
        cqTransformed(c, st, 504, 503, {"rotate": [0, 0, 120], "offset": [0, 0, 0]});
        atStep(st, 505);
        cqCenter(c, st, 505, 504, {"x": 29.52, "y": 0});
        atStep(st, 506);
        cqPolyline(c, st, 506, 505, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 507);
        cqClose(c, st, 507, 506, {});
        atStep(st, 508);
        cqExtrude(c, st, 508, 507, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 508, "extrude");
        atStep(st, 509);
        cqUnion(c, st, 509, 501, {"toUnion": {"wp": 508}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 509, "union");
        atStep(st, 510);
        cqWorkplaneNew(c, st, 510, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 511);
        cqWorkplane(c, st, 511, 510, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 512);
        cqTransformed(c, st, 512, 511, {"rotate": [0, 0, 135], "offset": [0, 0, 0]});
        atStep(st, 513);
        cqCenter(c, st, 513, 512, {"x": 29.52, "y": 0});
        atStep(st, 514);
        cqPolyline(c, st, 514, 513, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 515);
        cqClose(c, st, 515, 514, {});
        atStep(st, 516);
        cqExtrude(c, st, 516, 515, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 516, "extrude");
        atStep(st, 517);
        cqUnion(c, st, 517, 509, {"toUnion": {"wp": 516}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 517, "union");
        atStep(st, 518);
        cqWorkplaneNew(c, st, 518, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 519);
        cqWorkplane(c, st, 519, 518, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 520);
        cqTransformed(c, st, 520, 519, {"rotate": [0, 0, 150], "offset": [0, 0, 0]});
        atStep(st, 521);
        cqCenter(c, st, 521, 520, {"x": 29.52, "y": 0});
        atStep(st, 522);
        cqPolyline(c, st, 522, 521, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 523);
        cqClose(c, st, 523, 522, {});
        atStep(st, 524);
        cqExtrude(c, st, 524, 523, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 524, "extrude");
        atStep(st, 525);
        cqUnion(c, st, 525, 517, {"toUnion": {"wp": 524}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 525, "union");
        atStep(st, 526);
        cqWorkplaneNew(c, st, 526, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 527);
        cqWorkplane(c, st, 527, 526, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 528);
        cqTransformed(c, st, 528, 527, {"rotate": [0, 0, 165], "offset": [0, 0, 0]});
        atStep(st, 529);
        cqCenter(c, st, 529, 528, {"x": 29.52, "y": 0});
        atStep(st, 530);
        cqPolyline(c, st, 530, 529, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 531);
        cqClose(c, st, 531, 530, {});
        atStep(st, 532);
        cqExtrude(c, st, 532, 531, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 532, "extrude");
        atStep(st, 533);
        cqUnion(c, st, 533, 525, {"toUnion": {"wp": 532}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 533, "union");
        atStep(st, 534);
        cqWorkplaneNew(c, st, 534, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 535);
        cqWorkplane(c, st, 535, 534, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 536);
        cqTransformed(c, st, 536, 535, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 537);
        cqCenter(c, st, 537, 536, {"x": 29.52, "y": 0});
        atStep(st, 538);
        cqPolyline(c, st, 538, 537, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 539);
        cqClose(c, st, 539, 538, {});
        atStep(st, 540);
        cqExtrude(c, st, 540, 539, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 540, "extrude");
        atStep(st, 541);
        cqUnion(c, st, 541, 533, {"toUnion": {"wp": 540}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 541, "union");
        atStep(st, 542);
        cqWorkplaneNew(c, st, 542, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 543);
        cqWorkplane(c, st, 543, 542, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 544);
        cqTransformed(c, st, 544, 543, {"rotate": [0, 0, 195], "offset": [0, 0, 0]});
        atStep(st, 545);
        cqCenter(c, st, 545, 544, {"x": 29.52, "y": 0});
        atStep(st, 546);
        cqPolyline(c, st, 546, 545, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 547);
        cqClose(c, st, 547, 546, {});
        atStep(st, 548);
        cqExtrude(c, st, 548, 547, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 548, "extrude");
        atStep(st, 549);
        cqUnion(c, st, 549, 541, {"toUnion": {"wp": 548}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 549, "union");
        atStep(st, 550);
        cqWorkplaneNew(c, st, 550, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 551);
        cqWorkplane(c, st, 551, 550, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 552);
        cqTransformed(c, st, 552, 551, {"rotate": [0, 0, 210], "offset": [0, 0, 0]});
        atStep(st, 553);
        cqCenter(c, st, 553, 552, {"x": 29.52, "y": 0});
        atStep(st, 554);
        cqPolyline(c, st, 554, 553, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 555);
        cqClose(c, st, 555, 554, {});
        atStep(st, 556);
        cqExtrude(c, st, 556, 555, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 556, "extrude");
        atStep(st, 557);
        cqUnion(c, st, 557, 549, {"toUnion": {"wp": 556}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 557, "union");
        atStep(st, 558);
        cqWorkplaneNew(c, st, 558, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 559);
        cqWorkplane(c, st, 559, 558, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 560);
        cqTransformed(c, st, 560, 559, {"rotate": [0, 0, 225], "offset": [0, 0, 0]});
        atStep(st, 561);
        cqCenter(c, st, 561, 560, {"x": 29.52, "y": 0});
        atStep(st, 562);
        cqPolyline(c, st, 562, 561, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 563);
        cqClose(c, st, 563, 562, {});
        atStep(st, 564);
        cqExtrude(c, st, 564, 563, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 564, "extrude");
        atStep(st, 565);
        cqUnion(c, st, 565, 557, {"toUnion": {"wp": 564}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 565, "union");
        atStep(st, 566);
        cqWorkplaneNew(c, st, 566, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 567);
        cqWorkplane(c, st, 567, 566, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 568);
        cqTransformed(c, st, 568, 567, {"rotate": [0, 0, 240], "offset": [0, 0, 0]});
        atStep(st, 569);
        cqCenter(c, st, 569, 568, {"x": 29.52, "y": 0});
        atStep(st, 570);
        cqPolyline(c, st, 570, 569, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 571);
        cqClose(c, st, 571, 570, {});
        atStep(st, 572);
        cqExtrude(c, st, 572, 571, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 572, "extrude");
        atStep(st, 573);
        cqUnion(c, st, 573, 565, {"toUnion": {"wp": 572}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 573, "union");
        atStep(st, 574);
        cqWorkplaneNew(c, st, 574, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 575);
        cqWorkplane(c, st, 575, 574, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 576);
        cqTransformed(c, st, 576, 575, {"rotate": [0, 0, 255], "offset": [0, 0, 0]});
        atStep(st, 577);
        cqCenter(c, st, 577, 576, {"x": 29.52, "y": 0});
        atStep(st, 578);
        cqPolyline(c, st, 578, 577, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 579);
        cqClose(c, st, 579, 578, {});
        atStep(st, 580);
        cqExtrude(c, st, 580, 579, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 580, "extrude");
        atStep(st, 581);
        cqUnion(c, st, 581, 573, {"toUnion": {"wp": 580}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 581, "union");
        atStep(st, 582);
        cqWorkplaneNew(c, st, 582, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 583);
        cqWorkplane(c, st, 583, 582, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 584);
        cqTransformed(c, st, 584, 583, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 585);
        cqCenter(c, st, 585, 584, {"x": 29.52, "y": 0});
        atStep(st, 586);
        cqPolyline(c, st, 586, 585, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 587);
        cqClose(c, st, 587, 586, {});
        atStep(st, 588);
        cqExtrude(c, st, 588, 587, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 588, "extrude");
        atStep(st, 589);
        cqUnion(c, st, 589, 581, {"toUnion": {"wp": 588}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 589, "union");
        atStep(st, 590);
        cqWorkplaneNew(c, st, 590, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 591);
        cqWorkplane(c, st, 591, 590, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 592);
        cqTransformed(c, st, 592, 591, {"rotate": [0, 0, 285], "offset": [0, 0, 0]});
        atStep(st, 593);
        cqCenter(c, st, 593, 592, {"x": 29.52, "y": 0});
        atStep(st, 594);
        cqPolyline(c, st, 594, 593, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 595);
        cqClose(c, st, 595, 594, {});
        atStep(st, 596);
        cqExtrude(c, st, 596, 595, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 596, "extrude");
        atStep(st, 597);
        cqUnion(c, st, 597, 589, {"toUnion": {"wp": 596}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 597, "union");
        atStep(st, 598);
        cqWorkplaneNew(c, st, 598, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 599);
        cqWorkplane(c, st, 599, 598, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 600);
        cqTransformed(c, st, 600, 599, {"rotate": [0, 0, 300], "offset": [0, 0, 0]});
        atStep(st, 601);
        cqCenter(c, st, 601, 600, {"x": 29.52, "y": 0});
        atStep(st, 602);
        cqPolyline(c, st, 602, 601, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 603);
        cqClose(c, st, 603, 602, {});
        atStep(st, 604);
        cqExtrude(c, st, 604, 603, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 604, "extrude");
        atStep(st, 605);
        cqUnion(c, st, 605, 597, {"toUnion": {"wp": 604}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 605, "union");
        atStep(st, 606);
        cqWorkplaneNew(c, st, 606, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 607);
        cqWorkplane(c, st, 607, 606, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 608);
        cqTransformed(c, st, 608, 607, {"rotate": [0, 0, 315], "offset": [0, 0, 0]});
        atStep(st, 609);
        cqCenter(c, st, 609, 608, {"x": 29.52, "y": 0});
        atStep(st, 610);
        cqPolyline(c, st, 610, 609, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 611);
        cqClose(c, st, 611, 610, {});
        atStep(st, 612);
        cqExtrude(c, st, 612, 611, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 612, "extrude");
        atStep(st, 613);
        cqUnion(c, st, 613, 605, {"toUnion": {"wp": 612}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 613, "union");
        atStep(st, 614);
        cqWorkplaneNew(c, st, 614, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 615);
        cqWorkplane(c, st, 615, 614, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 616);
        cqTransformed(c, st, 616, 615, {"rotate": [0, 0, 330], "offset": [0, 0, 0]});
        atStep(st, 617);
        cqCenter(c, st, 617, 616, {"x": 29.52, "y": 0});
        atStep(st, 618);
        cqPolyline(c, st, 618, 617, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 619);
        cqClose(c, st, 619, 618, {});
        atStep(st, 620);
        cqExtrude(c, st, 620, 619, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 620, "extrude");
        atStep(st, 621);
        cqUnion(c, st, 621, 613, {"toUnion": {"wp": 620}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 621, "union");
        atStep(st, 622);
        cqWorkplaneNew(c, st, 622, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 623);
        cqWorkplane(c, st, 623, 622, {"offset": 7.95, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 624);
        cqTransformed(c, st, 624, 623, {"rotate": [0, 0, 345], "offset": [0, 0, 0]});
        atStep(st, 625);
        cqCenter(c, st, 625, 624, {"x": 29.52, "y": 0});
        atStep(st, 626);
        cqPolyline(c, st, 626, 625, {"listOfXYTuple": [[-0.6, -1.1], [-0.6, 1.1], [1.6, 0.385]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 627);
        cqClose(c, st, 627, 626, {});
        atStep(st, 628);
        cqExtrude(c, st, 628, 627, {"until": -2.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 628, "extrude");
        atStep(st, 629);
        cqUnion(c, st, 629, 621, {"toUnion": {"wp": 628}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 629, "union");
        atStep(st, 630);
        cqUnion(c, st, 630, 438, {"toUnion": {"wp": 629}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 630, "union");
        atStep(st, 631);
        cqWorkplaneNew(c, st, 631, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 632);
        cqWorkplane(c, st, 632, 631, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 633);
        cqCenter(c, st, 633, 632, {"x": 34, "y": 0});
        atStep(st, 634);
        cqCircle(c, st, 634, 633, {"radius": 3.5, "forConstruction": false});
        atStep(st, 635);
        cqExtrude(c, st, 635, 634, {"until": -1.8, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 635, "extrude");
        atStep(st, 636);
        cqWorkplaneNew(c, st, 636, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 637);
        cqWorkplane(c, st, 637, 636, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 638);
        cqCenter(c, st, 638, 637, {"x": 34, "y": 0});
        atStep(st, 639);
        cqCircle(c, st, 639, 638, {"radius": 1.75, "forConstruction": false});
        atStep(st, 640);
        cqExtrude(c, st, 640, 639, {"until": -12.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 640, "extrude");
        atStep(st, 641);
        cqUnion(c, st, 641, 635, {"toUnion": {"wp": 640}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 641, "union");
        atStep(st, 642);
        cqWorkplaneNew(c, st, 642, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 643);
        cqWorkplane(c, st, 643, 642, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 644);
        cqCenter(c, st, 644, 643, {"x": 17.000000000000004, "y": 29.444863728670914});
        atStep(st, 645);
        cqCircle(c, st, 645, 644, {"radius": 3.5, "forConstruction": false});
        atStep(st, 646);
        cqExtrude(c, st, 646, 645, {"until": -1.8, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 646, "extrude");
        atStep(st, 647);
        cqWorkplaneNew(c, st, 647, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 648);
        cqWorkplane(c, st, 648, 647, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 649);
        cqCenter(c, st, 649, 648, {"x": 17.000000000000004, "y": 29.444863728670914});
        atStep(st, 650);
        cqCircle(c, st, 650, 649, {"radius": 1.75, "forConstruction": false});
        atStep(st, 651);
        cqExtrude(c, st, 651, 650, {"until": -12.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 651, "extrude");
        atStep(st, 652);
        cqUnion(c, st, 652, 646, {"toUnion": {"wp": 651}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 652, "union");
        atStep(st, 653);
        cqUnion(c, st, 653, 641, {"toUnion": {"wp": 652}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 653, "union");
        atStep(st, 654);
        cqWorkplaneNew(c, st, 654, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 655);
        cqWorkplane(c, st, 655, 654, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 656);
        cqCenter(c, st, 656, 655, {"x": -16.999999999999993, "y": 29.444863728670917});
        atStep(st, 657);
        cqCircle(c, st, 657, 656, {"radius": 3.5, "forConstruction": false});
        atStep(st, 658);
        cqExtrude(c, st, 658, 657, {"until": -1.8, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 658, "extrude");
        atStep(st, 659);
        cqWorkplaneNew(c, st, 659, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 660);
        cqWorkplane(c, st, 660, 659, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 661);
        cqCenter(c, st, 661, 660, {"x": -16.999999999999993, "y": 29.444863728670917});
        atStep(st, 662);
        cqCircle(c, st, 662, 661, {"radius": 1.75, "forConstruction": false});
        atStep(st, 663);
        cqExtrude(c, st, 663, 662, {"until": -12.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 663, "extrude");
        atStep(st, 664);
        cqUnion(c, st, 664, 658, {"toUnion": {"wp": 663}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 664, "union");
        atStep(st, 665);
        cqUnion(c, st, 665, 653, {"toUnion": {"wp": 664}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 665, "union");
        atStep(st, 666);
        cqWorkplaneNew(c, st, 666, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 667);
        cqWorkplane(c, st, 667, 666, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 668);
        cqCenter(c, st, 668, 667, {"x": -34, "y": 4.1637991171010006e-15});
        atStep(st, 669);
        cqCircle(c, st, 669, 668, {"radius": 3.5, "forConstruction": false});
        atStep(st, 670);
        cqExtrude(c, st, 670, 669, {"until": -1.8, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 670, "extrude");
        atStep(st, 671);
        cqWorkplaneNew(c, st, 671, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 672);
        cqWorkplane(c, st, 672, 671, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 673);
        cqCenter(c, st, 673, 672, {"x": -34, "y": 4.1637991171010006e-15});
        atStep(st, 674);
        cqCircle(c, st, 674, 673, {"radius": 1.75, "forConstruction": false});
        atStep(st, 675);
        cqExtrude(c, st, 675, 674, {"until": -12.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 675, "extrude");
        atStep(st, 676);
        cqUnion(c, st, 676, 670, {"toUnion": {"wp": 675}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 676, "union");
        atStep(st, 677);
        cqUnion(c, st, 677, 665, {"toUnion": {"wp": 676}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 677, "union");
        atStep(st, 678);
        cqWorkplaneNew(c, st, 678, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 679);
        cqWorkplane(c, st, 679, 678, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 680);
        cqCenter(c, st, 680, 679, {"x": -17.000000000000014, "y": -29.444863728670903});
        atStep(st, 681);
        cqCircle(c, st, 681, 680, {"radius": 3.5, "forConstruction": false});
        atStep(st, 682);
        cqExtrude(c, st, 682, 681, {"until": -1.8, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 682, "extrude");
        atStep(st, 683);
        cqWorkplaneNew(c, st, 683, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 684);
        cqWorkplane(c, st, 684, 683, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 685);
        cqCenter(c, st, 685, 684, {"x": -17.000000000000014, "y": -29.444863728670903});
        atStep(st, 686);
        cqCircle(c, st, 686, 685, {"radius": 1.75, "forConstruction": false});
        atStep(st, 687);
        cqExtrude(c, st, 687, 686, {"until": -12.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 687, "extrude");
        atStep(st, 688);
        cqUnion(c, st, 688, 682, {"toUnion": {"wp": 687}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 688, "union");
        atStep(st, 689);
        cqUnion(c, st, 689, 677, {"toUnion": {"wp": 688}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 689, "union");
        atStep(st, 690);
        cqWorkplaneNew(c, st, 690, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 691);
        cqWorkplane(c, st, 691, 690, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 692);
        cqCenter(c, st, 692, 691, {"x": 17.000000000000004, "y": -29.444863728670914});
        atStep(st, 693);
        cqCircle(c, st, 693, 692, {"radius": 3.5, "forConstruction": false});
        atStep(st, 694);
        cqExtrude(c, st, 694, 693, {"until": -1.8, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 694, "extrude");
        atStep(st, 695);
        cqWorkplaneNew(c, st, 695, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 696);
        cqWorkplane(c, st, 696, 695, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 697);
        cqCenter(c, st, 697, 696, {"x": 17.000000000000004, "y": -29.444863728670914});
        atStep(st, 698);
        cqCircle(c, st, 698, 697, {"radius": 1.75, "forConstruction": false});
        atStep(st, 699);
        cqExtrude(c, st, 699, 698, {"until": -12.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 699, "extrude");
        atStep(st, 700);
        cqUnion(c, st, 700, 694, {"toUnion": {"wp": 699}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 700, "union");
        atStep(st, 701);
        cqUnion(c, st, 701, 689, {"toUnion": {"wp": 700}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 701, "union");
        atStep(st, 702);
        cqCut(c, st, 702, 630, {"toCut": {"wp": 701}, "clean": true, "tol": undefined});
        stepVol(c, st, 702, "cut");
        // step 703: guard_begin {"before":{"wp":702},"block":1,"g":1,"op":"guard_begin","skippable":true}
        // step 704: guard_caught {"block":1,"g":1,"op":"guard_caught"}
        // step 705: guard_begin {"block":2,"g":2,"op":"guard_begin","skippable":false}
        atStep(st, 706);
        cqSelect("Edges")(c, st, 703, 702, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 707);
        cqFillet(c, st, 704, 703, {"radius": 0.6});
        stepVol(c, st, 704, "fillet");
        // step 708: guard_end {"block":2,"g":2,"op":"guard_end"}
        // step 709: guard_begin {"block":4,"g":3,"op":"guard_begin","skippable":false}
        // step 710: guard_begin {"block":3,"g":4,"op":"guard_begin","skippable":false}
        atStep(st, 711);
        cqSelect("Edges")(c, st, 705, 704, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 712);
        cqChamfer(c, st, 706, 705, {"length": 0.5, "length2": undefined});
        stepVol(c, st, 706, "chamfer");
        // step 713: guard_end {"block":3,"g":4,"op":"guard_end"}
        // step 714: guard_end {"block":4,"g":3,"op":"guard_end"}
        // step 715: guard_begin {"before":{"wp":706},"block":5,"g":5,"op":"guard_begin","skippable":true}
        // step 716: guard_caught {"block":5,"g":5,"op":"guard_caught"}
        // step 717: guard_begin {"block":6,"g":6,"op":"guard_begin","skippable":false}
        atStep(st, 718);
        cqVal(c, st, 1, 706);
        atStep(st, 719);
        cqWorkplaneNew(c, st, 707, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 720);
        cqAdd(c, st, 707, 707, {"obj": {"shape": 1}});
        // step 721: guard_end {"block":6,"g":6,"op":"guard_end"}
        return mergeMaps(cqMeasure(c, st, {"wp": 707}), {"i": 5599});
    } catch (e) {
        return {"i": 5599, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c5599"] = case_5599();
    return out;
}
