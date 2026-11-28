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

const case_5611 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqBox(c, st, 2, 1, {"length": 60, "width": 40, "height": 5, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 2, "box");
        atStep(st, 3);
        cqSelect("Faces")(c, st, 3, 2, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 4);
        cqWorkplane(c, st, 4, 3, {"offset": 0, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 5);
        cqCboreHole(c, st, 5, 4, {"diameter": 4, "cboreDiameter": 8, "cboreDepth": 2.5, "depth": 6, "clean": true});
        stepVol(c, st, 5, "cboreHole");
        atStep(st, 6);
        cqSelect("Faces")(c, st, 6, 5, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 7);
        cqWorkplane(c, st, 7, 6, {"offset": 0, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 8);
        cqPushPoints(c, st, 8, 7, {"pntList": [[-22, -12], [-22, 12], [0, -12], [0, 12], [22, -12], [22, 12]]});
        atStep(st, 9);
        cqHole(c, st, 9, 8, {"diameter": 4, "depth": 4, "clean": true});
        stepVol(c, st, 9, "hole");
        atStep(st, 10);
        cqWorkplaneNew(c, st, 10, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 11);
        cqTransformed(c, st, 11, 10, {"rotate": [0, 0, 31.86660938152477], "offset": [0.482596828841095, 14.3, 2.05]});
        atStep(st, 12);
        cqBox(c, st, 12, 11, {"length": 8.336485282275984, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 12, "box");
        atStep(st, 13);
        cqWorkplaneNew(c, st, 13, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 14);
        cqTransformed(c, st, 14, 13, {"rotate": [0, 0, 32.548540920829005], "offset": [1.4352201128956277, 14.9, 2.05]});
        atStep(st, 15);
        cqBox(c, st, 15, 14, {"length": 8.315212748493325, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 15, "box");
        // step 16: guard_begin {"block":1,"g":1,"op":"guard_begin","skippable":false}
        atStep(st, 17);
        cqUnion(c, st, 16, 12, {"toUnion": {"wp": 15}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 16, "union");
        // step 18: guard_end {"block":1,"g":1,"op":"guard_end"}
        atStep(st, 19);
        cqWorkplaneNew(c, st, 17, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 20);
        cqTransformed(c, st, 18, 17, {"rotate": [0, 0, 33.973451961252316], "offset": [2.3504597011220074, 15.5, 2.05]});
        atStep(st, 21);
        cqBox(c, st, 19, 18, {"length": 8.273712687467556, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 19, "box");
        // step 22: guard_begin {"block":1,"g":2,"op":"guard_begin","skippable":false}
        atStep(st, 23);
        cqUnion(c, st, 20, 16, {"toUnion": {"wp": 19}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 20, "union");
        // step 24: guard_end {"block":1,"g":2,"op":"guard_end"}
        atStep(st, 25);
        cqWorkplaneNew(c, st, 21, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 26);
        cqTransformed(c, st, 22, 21, {"rotate": [0, 0, 36.27306366767706], "offset": [3.204476017925721, 16.1, 2.05]});
        atStep(st, 27);
        cqBox(c, st, 23, 22, {"length": 8.214140114732613, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 23, "box");
        // step 28: guard_begin {"block":1,"g":3,"op":"guard_begin","skippable":false}
        atStep(st, 29);
        cqUnion(c, st, 24, 20, {"toUnion": {"wp": 23}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 24, "union");
        // step 30: guard_end {"block":1,"g":3,"op":"guard_end"}
        atStep(st, 31);
        cqWorkplaneNew(c, st, 25, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 32);
        cqTransformed(c, st, 26, 25, {"rotate": [0, 0, 39.669375439852416], "offset": [3.975024192340421, 16.7, 2.05]});
        atStep(st, 33);
        cqBox(c, st, 27, 26, {"length": 8.139913694424136, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 27, "box");
        // step 34: guard_begin {"block":1,"g":4,"op":"guard_begin","skippable":false}
        atStep(st, 35);
        cqUnion(c, st, 28, 24, {"toUnion": {"wp": 27}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 28, "union");
        // step 36: guard_end {"block":1,"g":4,"op":"guard_end"}
        atStep(st, 37);
        cqWorkplaneNew(c, st, 29, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 38);
        cqTransformed(c, st, 30, 29, {"rotate": [0, 0, 44.50168166788707], "offset": [4.642033478203597, 17.3, 2.05]});
        atStep(st, 39);
        cqBox(c, st, 31, 30, {"length": 8.056005350328874, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 31, "box");
        // step 40: guard_begin {"block":1,"g":5,"op":"guard_begin","skippable":false}
        atStep(st, 41);
        cqUnion(c, st, 32, 28, {"toUnion": {"wp": 31}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 32, "union");
        // step 42: guard_end {"block":1,"g":5,"op":"guard_end"}
        atStep(st, 43);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 44);
        cqTransformed(c, st, 34, 33, {"rotate": [0, 0, 51.243400878305565], "offset": [5.188130044119481, 17.9, 2.05]});
        atStep(st, 45);
        cqBox(c, st, 35, 34, {"length": 7.969415837110891, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 35, "box");
        // step 46: guard_begin {"block":1,"g":6,"op":"guard_begin","skippable":false}
        atStep(st, 47);
        cqUnion(c, st, 36, 32, {"toUnion": {"wp": 35}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 36, "union");
        // step 48: guard_end {"block":1,"g":6,"op":"guard_end"}
        atStep(st, 49);
        cqWorkplaneNew(c, st, 37, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 50);
        cqTransformed(c, st, 38, 37, {"rotate": [0, 0, 60.442832303470354], "offset": [5.599089515910505, 18.5, 2.05]});
        atStep(st, 51);
        cqBox(c, st, 39, 38, {"length": 7.889763044991584, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 39, "box");
        // step 52: guard_begin {"block":1,"g":7,"op":"guard_begin","skippable":false}
        atStep(st, 53);
        cqUnion(c, st, 40, 36, {"toUnion": {"wp": 39}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 40, "union");
        // step 54: guard_end {"block":1,"g":7,"op":"guard_end"}
        atStep(st, 55);
        cqWorkplaneNew(c, st, 41, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 56);
        cqTransformed(c, st, 42, 41, {"rotate": [0, 0, 72.43034389831762], "offset": [5.864207484020885, 19.1, 2.05]});
        atStep(st, 57);
        cqBox(c, st, 43, 42, {"length": 7.829359184613956, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 43, "box");
        // step 58: guard_begin {"block":1,"g":8,"op":"guard_begin","skippable":false}
        atStep(st, 59);
        cqUnion(c, st, 44, 40, {"toUnion": {"wp": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 44, "union");
        // step 60: guard_end {"block":1,"g":8,"op":"guard_end"}
        atStep(st, 61);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 62);
        cqTransformed(c, st, 46, 45, {"rotate": [0, 0, 86.68435646886022], "offset": [5.976578325132383, 19.7, 2.05]});
        atStep(st, 63);
        cqBox(c, st, 47, 46, {"length": 7.801006046479603, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 47, "box");
        // step 64: guard_begin {"block":1,"g":9,"op":"guard_begin","skippable":false}
        atStep(st, 65);
        cqUnion(c, st, 48, 44, {"toUnion": {"wp": 47}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 48, "union");
        // step 66: guard_end {"block":1,"g":9,"op":"guard_end"}
        atStep(st, 67);
        cqWorkplaneNew(c, st, 49, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 68);
        cqTransformed(c, st, 50, 49, {"rotate": [0, 0, 101.43536313617142], "offset": [5.933275075425078, 20.3, 2.05]});
        atStep(st, 69);
        cqBox(c, st, 51, 50, {"length": 7.812151830153874, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 51, "box");
        // step 70: guard_begin {"block":1,"g":10,"op":"guard_begin","skippable":false}
        atStep(st, 71);
        cqUnion(c, st, 52, 48, {"toUnion": {"wp": 51}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 52, "union");
        // step 72: guard_end {"block":1,"g":10,"op":"guard_end"}
        atStep(st, 73);
        cqWorkplaneNew(c, st, 53, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 74);
        cqTransformed(c, st, 54, 53, {"rotate": [0, 0, 114.57084000587643], "offset": [5.735425670259467, 20.9, 2.05]});
        atStep(st, 75);
        cqBox(c, st, 55, 54, {"length": 7.859740960895833, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 55, "box");
        // step 76: guard_begin {"block":1,"g":11,"op":"guard_begin","skippable":false}
        atStep(st, 77);
        cqUnion(c, st, 56, 52, {"toUnion": {"wp": 55}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 56, "union");
        // step 78: guard_end {"block":1,"g":11,"op":"guard_end"}
        atStep(st, 79);
        cqWorkplaneNew(c, st, 57, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 80);
        cqTransformed(c, st, 58, 57, {"rotate": [0, 0, 125.00176374047624], "offset": [5.388183564437384, 21.5, 2.05]});
        atStep(st, 81);
        cqBox(c, st, 59, 58, {"length": 7.932480541887688, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 59, "box");
        // step 82: guard_begin {"block":1,"g":12,"op":"guard_begin","skippable":false}
        atStep(st, 83);
        cqUnion(c, st, 60, 56, {"toUnion": {"wp": 59}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 60, "union");
        // step 84: guard_end {"block":1,"g":12,"op":"guard_end"}
        atStep(st, 85);
        cqWorkplaneNew(c, st, 61, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 86);
        cqTransformed(c, st, 62, 61, {"rotate": [0, 0, 132.77027056292738], "offset": [4.900593498306419, 22.1, 2.05]});
        atStep(st, 87);
        cqBox(c, st, 63, 62, {"length": 8.017347033551479, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 63, "box");
        // step 88: guard_begin {"block":1,"g":13,"op":"guard_begin","skippable":false}
        atStep(st, 89);
        cqUnion(c, st, 64, 60, {"toUnion": {"wp": 63}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 64, "union");
        // step 90: guard_end {"block":1,"g":13,"op":"guard_end"}
        atStep(st, 91);
        cqWorkplaneNew(c, st, 65, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 92);
        cqTransformed(c, st, 66, 65, {"rotate": [0, 0, 138.38529838310015], "offset": [4.285355906146508, 22.7, 2.05]});
        atStep(st, 93);
        cqBox(c, st, 67, 66, {"length": 8.103453809705586, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 67, "box");
        // step 94: guard_begin {"block":1,"g":14,"op":"guard_begin","skippable":false}
        atStep(st, 95);
        cqUnion(c, st, 68, 64, {"toUnion": {"wp": 67}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 68, "union");
        // step 96: guard_end {"block":1,"g":14,"op":"guard_end"}
        atStep(st, 97);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 98);
        cqTransformed(c, st, 70, 69, {"rotate": [0, 0, 142.36999776432305], "offset": [3.558496103378393, 23.3, 2.05]});
        atStep(st, 99);
        cqBox(c, st, 71, 70, {"length": 8.182704478106963, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 71, "box");
        // step 100: guard_begin {"block":1,"g":15,"op":"guard_begin","skippable":false}
        atStep(st, 101);
        cqUnion(c, st, 72, 68, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        // step 102: guard_end {"block":1,"g":15,"op":"guard_end"}
        atStep(st, 103);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 104);
        cqTransformed(c, st, 74, 73, {"rotate": [0, 0, 145.12331031276193], "offset": [2.7389468693940624, 23.9, 2.05]});
        atStep(st, 105);
        cqBox(c, st, 75, 74, {"length": 8.24929563963056, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 75, "box");
        // step 106: guard_begin {"block":1,"g":16,"op":"guard_begin","skippable":false}
        atStep(st, 107);
        cqUnion(c, st, 76, 72, {"toUnion": {"wp": 75}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 76, "union");
        // step 108: guard_end {"block":1,"g":16,"op":"guard_end"}
        atStep(st, 109);
        cqWorkplaneNew(c, st, 77, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 110);
        cqTransformed(c, st, 78, 77, {"rotate": [0, 0, 146.9159138046005], "offset": [1.8480552986246677, 24.5, 2.05]});
        atStep(st, 111);
        cqBox(c, st, 79, 78, {"length": 8.299164284297532, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 79, "box");
        // step 112: guard_begin {"block":1,"g":17,"op":"guard_begin","skippable":false}
        atStep(st, 113);
        cqUnion(c, st, 80, 76, {"toUnion": {"wp": 79}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 80, "union");
        // step 114: guard_end {"block":1,"g":17,"op":"guard_end"}
        atStep(st, 115);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 116);
        cqTransformed(c, st, 82, 81, {"rotate": [0, 0, 147.91665528796608], "offset": [0.9090267650739982, 25.1, 2.05]});
        atStep(st, 117);
        cqBox(c, st, 83, 82, {"length": 8.329619492371528, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 83, "box");
        // step 118: guard_begin {"block":1,"g":18,"op":"guard_begin","skippable":false}
        atStep(st, 119);
        cqUnion(c, st, 84, 80, {"toUnion": {"wp": 83}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 84, "union");
        // step 120: guard_end {"block":1,"g":18,"op":"guard_end"}
        atStep(st, 121);
        cqWorkplaneNew(c, st, 85, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 122);
        cqTransformed(c, st, 86, 85, {"rotate": [0, 0, 148.21621391635253], "offset": [-0.05367951642616087, 25.7, 2.05]});
        atStep(st, 123);
        cqBox(c, st, 87, 86, {"length": 8.339135389059392, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 87, "box");
        // step 124: guard_begin {"block":1,"g":19,"op":"guard_begin","skippable":false}
        atStep(st, 125);
        cqUnion(c, st, 88, 84, {"toUnion": {"wp": 87}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 88, "union");
        // step 126: guard_end {"block":1,"g":19,"op":"guard_end"}
        atStep(st, 127);
        cqWorkplaneNew(c, st, 89, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 128);
        cqTransformed(c, st, 90, 89, {"rotate": [0, 0, 147.8411593710671], "offset": [-1.0149875882048651, 26.3, 2.05]});
        atStep(st, 129);
        cqBox(c, st, 91, 90, {"length": 8.327251130862388, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 91, "box");
        // step 130: guard_begin {"block":1,"g":20,"op":"guard_begin","skippable":false}
        atStep(st, 131);
        cqUnion(c, st, 92, 88, {"toUnion": {"wp": 91}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 92, "union");
        // step 132: guard_end {"block":1,"g":20,"op":"guard_end"}
        atStep(st, 133);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 134);
        cqTransformed(c, st, 94, 93, {"rotate": [0, 0, 146.75818250451104], "offset": [-1.9498579122639867, 26.9, 2.05]});
        atStep(st, 135);
        cqBox(c, st, 95, 94, {"length": 8.294543387843103, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 95, "box");
        // step 136: guard_begin {"block":1,"g":21,"op":"guard_begin","skippable":false}
        atStep(st, 137);
        cqUnion(c, st, 96, 92, {"toUnion": {"wp": 95}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 96, "union");
        // step 138: guard_end {"block":1,"g":21,"op":"guard_end"}
        atStep(st, 139);
        cqWorkplaneNew(c, st, 97, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 140);
        cqTransformed(c, st, 98, 97, {"rotate": [0, 0, 144.86879816078573], "offset": [-2.833939584153839, 27.5, 2.05]});
        atStep(st, 141);
        cqBox(c, st, 99, 98, {"length": 8.242660976956483, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 99, "box");
        // step 142: guard_begin {"block":1,"g":22,"op":"guard_begin","skippable":false}
        atStep(st, 143);
        cqUnion(c, st, 100, 96, {"toUnion": {"wp": 99}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 100, "union");
        // step 144: guard_end {"block":1,"g":22,"op":"guard_end"}
        atStep(st, 145);
        cqWorkplaneNew(c, st, 101, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 146);
        cqTransformed(c, st, 102, 101, {"rotate": [0, 0, 141.9941683766417], "offset": [-3.6442046097624265, 28.1, 2.05]});
        atStep(st, 147);
        cqBox(c, st, 103, 102, {"length": 8.174434609061592, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 103, "box");
        // step 148: guard_begin {"block":1,"g":23,"op":"guard_begin","skippable":false}
        atStep(st, 149);
        cqUnion(c, st, 104, 100, {"toUnion": {"wp": 103}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 104, "union");
        // step 150: guard_end {"block":1,"g":23,"op":"guard_end"}
        atStep(st, 151);
        cqWorkplaneNew(c, st, 105, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 152);
        cqTransformed(c, st, 106, 105, {"rotate": [0, 0, 137.85055823409164], "offset": [-4.359547723782098, 28.7, 2.05]});
        atStep(st, 153);
        cqBox(c, st, 107, 106, {"length": 8.094098987475412, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 107, "box");
        // step 154: guard_begin {"block":1,"g":24,"op":"guard_begin","skippable":false}
        atStep(st, 155);
        cqUnion(c, st, 108, 104, {"toUnion": {"wp": 107}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 108, "union");
        // step 156: guard_end {"block":1,"g":24,"op":"guard_end"}
        atStep(st, 157);
        cqWorkplaneNew(c, st, 109, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 158);
        cqTransformed(c, st, 110, 109, {"rotate": [0, 0, 132.02383025868693], "offset": [-4.961336126165676, 29.3, 2.05]});
        atStep(st, 159);
        cqBox(c, st, 111, 110, {"length": 8.00768217883686, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 111, "box");
        // step 160: guard_begin {"block":1,"g":25,"op":"guard_begin","skippable":false}
        atStep(st, 161);
        cqUnion(c, st, 112, 108, {"toUnion": {"wp": 111}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 112, "union");
        // step 162: guard_end {"block":1,"g":25,"op":"guard_end"}
        atStep(st, 163);
        cqWorkplaneNew(c, st, 113, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 164);
        cqTransformed(c, st, 114, 113, {"rotate": [0, 0, 123.98147506500884], "offset": [-5.433894817388358, 29.9, 2.05]});
        atStep(st, 165);
        cqBox(c, st, 115, 114, {"length": 7.923573008067072, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 115, "box");
        // step 166: guard_begin {"block":1,"g":26,"op":"guard_begin","skippable":false}
        atStep(st, 167);
        cqUnion(c, st, 116, 112, {"toUnion": {"wp": 115}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 116, "union");
        // step 168: guard_end {"block":1,"g":26,"op":"guard_end"}
        atStep(st, 169);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 170);
        cqTransformed(c, st, 118, 117, {"rotate": [0, 0, 113.23684939741842], "offset": [-5.764914890812836, 30.5, 2.05]});
        atStep(st, 171);
        cqBox(c, st, 119, 118, {"length": 7.85296756099233, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 119, "box");
        // step 172: guard_begin {"block":1,"g":27,"op":"guard_begin","skippable":false}
        atStep(st, 173);
        cqUnion(c, st, 120, 116, {"toUnion": {"wp": 119}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 120, "union");
        // step 174: guard_end {"block":1,"g":27,"op":"guard_end"}
        atStep(st, 175);
        cqWorkplaneNew(c, st, 121, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 176);
        cqTransformed(c, st, 122, 121, {"rotate": [0, 0, 99.84295365271876], "offset": [-5.9457741472191294, 31.1, 2.05]});
        atStep(st, 177);
        cqBox(c, st, 123, 122, {"length": 7.808963937916802, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 123, "box");
        // step 178: guard_begin {"block":1,"g":28,"op":"guard_begin","skippable":false}
        atStep(st, 179);
        cqUnion(c, st, 124, 120, {"toUnion": {"wp": 123}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 124, "union");
        // step 180: guard_end {"block":1,"g":28,"op":"guard_end"}
        atStep(st, 181);
        cqWorkplaneNew(c, st, 125, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 182);
        cqTransformed(c, st, 126, 125, {"rotate": [0, 0, 85.03477952539433], "offset": [-5.9717616803368685, 31.7, 2.05]});
        atStep(st, 183);
        cqBox(c, st, 127, 126, {"length": 7.802260029185931, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 127, "box");
        // step 184: guard_begin {"block":1,"g":29,"op":"guard_begin","skippable":false}
        atStep(st, 185);
        cqUnion(c, st, 128, 124, {"toUnion": {"wp": 127}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 128, "union");
        // step 186: guard_end {"block":1,"g":29,"op":"guard_end"}
        atStep(st, 187);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 188);
        cqTransformed(c, st, 130, 129, {"rotate": [0, 0, 70.96590919306254], "offset": [-5.842200583519699, 32.3, 2.05]});
        atStep(st, 189);
        cqBox(c, st, 131, 130, {"length": 7.834702555001158, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 131, "box");
        // step 190: guard_begin {"block":1,"g":30,"op":"guard_begin","skippable":false}
        atStep(st, 191);
        cqUnion(c, st, 132, 128, {"toUnion": {"wp": 131}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 132, "union");
        // step 192: guard_end {"block":1,"g":30,"op":"guard_end"}
        atStep(st, 193);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 194);
        cqTransformed(c, st, 134, 133, {"rotate": [0, 0, 59.28446290181838], "offset": [-5.560465581376802, 32.900000000000006, 2.05]});
        atStep(st, 195);
        cqBox(c, st, 135, 134, {"length": 7.897906680110492, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 135, "box");
        // step 196: guard_begin {"block":1,"g":31,"op":"guard_begin","skippable":false}
        atStep(st, 197);
        cqUnion(c, st, 136, 132, {"toUnion": {"wp": 135}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 136, "union");
        // step 198: guard_end {"block":1,"g":31,"op":"guard_end"}
        atStep(st, 199);
        cqWorkplaneNew(c, st, 137, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 200);
        cqTransformed(c, st, 138, 137, {"rotate": [0, 0, 50.382785915221106], "offset": [-5.133895127104044, 33.5, 2.05]});
        atStep(st, 201);
        cqBox(c, st, 139, 138, {"length": 7.978895364047632, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 139, "box");
        // step 202: guard_begin {"block":1,"g":32,"op":"guard_begin","skippable":false}
        atStep(st, 203);
        cqUnion(c, st, 140, 136, {"toUnion": {"wp": 139}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 140, "union");
        // step 204: guard_end {"block":1,"g":32,"op":"guard_end"}
        atStep(st, 205);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 206);
        cqTransformed(c, st, 142, 141, {"rotate": [0, 0, 43.880236043324835], "offset": [-4.57360025514731, 34.099999999999994, 2.05]});
        atStep(st, 207);
        cqBox(c, st, 143, 142, {"length": 8.06560946278688, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 143, "box");
        // step 208: guard_begin {"block":1,"g":33,"op":"guard_begin","skippable":false}
        atStep(st, 209);
        cqUnion(c, st, 144, 140, {"toUnion": {"wp": 143}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 144, "union");
        // step 210: guard_end {"block":1,"g":33,"op":"guard_end"}
        atStep(st, 211);
        cqWorkplaneNew(c, st, 145, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 212);
        cqTransformed(c, st, 146, 145, {"rotate": [0, 0, 39.22845017302384], "offset": [-3.894175168081593, 34.7, 2.05]});
        atStep(st, 213);
        cqBox(c, st, 147, 146, {"length": 8.148745566000013, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 147, "box");
        // step 214: guard_begin {"block":1,"g":34,"op":"guard_begin","skippable":false}
        atStep(st, 215);
        cqUnion(c, st, 148, 144, {"toUnion": {"wp": 147}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 148, "union");
        // step 216: guard_end {"block":1,"g":34,"op":"guard_end"}
        atStep(st, 217);
        cqWorkplaneNew(c, st, 149, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 218);
        cqTransformed(c, st, 150, 149, {"rotate": [0, 0, 35.96836219991155], "offset": [-3.113317096153788, 35.3, 2.05]});
        atStep(st, 219);
        cqBox(c, st, 151, 150, {"length": 8.221557525553429, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 151, "box");
        // step 220: guard_begin {"block":1,"g":35,"op":"guard_begin","skippable":false}
        atStep(st, 221);
        cqUnion(c, st, 152, 148, {"toUnion": {"wp": 151}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 152, "union");
        // step 222: guard_end {"block":1,"g":35,"op":"guard_end"}
        atStep(st, 223);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 224);
        cqTransformed(c, st, 154, 153, {"rotate": [0, 0, 33.775033380903096], "offset": [-2.251365331144722, 35.9, 2.05]});
        atStep(st, 225);
        cqBox(c, st, 155, 154, {"length": 8.279265850314399, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 155, "box");
        // step 226: guard_begin {"block":1,"g":36,"op":"guard_begin","skippable":false}
        atStep(st, 227);
        cqUnion(c, st, 156, 152, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        // step 228: guard_end {"block":1,"g":36,"op":"guard_end"}
        atStep(st, 229);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 230);
        cqTransformed(c, st, 158, 157, {"rotate": [0, 0, 32.437754871678585], "offset": [-1.3307714415017156, 36.5, 2.05]});
        atStep(st, 231);
        cqBox(c, st, 159, 158, {"length": 8.318603588775158, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 159, "box");
        // step 232: guard_begin {"block":1,"g":37,"op":"guard_begin","skippable":false}
        atStep(st, 233);
        cqUnion(c, st, 160, 156, {"toUnion": {"wp": 159}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 160, "union");
        // step 234: guard_end {"block":1,"g":37,"op":"guard_end"}
        atStep(st, 235);
        cqWorkplaneNew(c, st, 161, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 236);
        cqTransformed(c, st, 162, 161, {"rotate": [0, 0, 31.833444439262525], "offset": [-0.375514468239066, 37.1, 2.05]});
        atStep(st, 237);
        cqBox(c, st, 163, 162, {"length": 8.337544697957062, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 163, "box");
        // step 238: guard_begin {"block":1,"g":38,"op":"guard_begin","skippable":false}
        atStep(st, 239);
        cqUnion(c, st, 164, 160, {"toUnion": {"wp": 163}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 164, "union");
        // step 240: guard_end {"block":1,"g":38,"op":"guard_end"}
        atStep(st, 241);
        cqWorkplaneNew(c, st, 165, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 242);
        cqTransformed(c, st, 166, 165, {"rotate": [0, 0, 31.90813248365265], "offset": [0.5895236657902044, 37.7, 2.05]});
        atStep(st, 243);
        cqBox(c, st, 167, 166, {"length": 8.33516218916197, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 167, "box");
        // step 244: guard_begin {"block":1,"g":39,"op":"guard_begin","skippable":false}
        atStep(st, 245);
        cqUnion(c, st, 168, 164, {"toUnion": {"wp": 167}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 168, "union");
        // step 246: guard_end {"block":1,"g":39,"op":"guard_end"}
        atStep(st, 247);
        cqWorkplaneNew(c, st, 169, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 248);
        cqTransformed(c, st, 170, 169, {"rotate": [0, 0, 32.66842912880229], "offset": [1.539206264311443, 38.3, 2.05]});
        atStep(st, 249);
        cqBox(c, st, 171, 170, {"length": 8.311571069180175, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 171, "box");
        // step 250: guard_begin {"block":1,"g":40,"op":"guard_begin","skippable":false}
        atStep(st, 251);
        cqUnion(c, st, 172, 168, {"toUnion": {"wp": 171}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 172, "union");
        // step 252: guard_end {"block":1,"g":40,"op":"guard_end"}
        atStep(st, 253);
        cqWorkplaneNew(c, st, 173, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 254);
        cqTransformed(c, st, 174, 173, {"rotate": [0, 0, 34.18257882960505], "offset": [2.448796602220681, 38.900000000000006, 2.05]});
        atStep(st, 255);
        cqBox(c, st, 175, 174, {"length": 8.267935141203434, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 175, "box");
        // step 256: guard_begin {"block":1,"g":41,"op":"guard_begin","skippable":false}
        atStep(st, 257);
        cqUnion(c, st, 176, 172, {"toUnion": {"wp": 175}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 176, "union");
        // step 258: guard_end {"block":1,"g":41,"op":"guard_end"}
        atStep(st, 259);
        cqWorkplaneNew(c, st, 177, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 260);
        cqTransformed(c, st, 178, 177, {"rotate": [0, 0, 36.59118499284407], "offset": [3.2946022519835543, 39.5, 2.05]});
        atStep(st, 261);
        cqBox(c, st, 179, 178, {"length": 8.206540240608025, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 179, "box");
        // step 262: guard_begin {"block":1,"g":42,"op":"guard_begin","skippable":false}
        atStep(st, 263);
        cqUnion(c, st, 180, 176, {"toUnion": {"wp": 179}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 180, "union");
        // step 264: guard_end {"block":1,"g":42,"op":"guard_end"}
        atStep(st, 265);
        cqWorkplaneNew(c, st, 181, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 266);
        cqTransformed(c, st, 182, 181, {"rotate": [0, 0, 40.12788479758574], "offset": [4.054592208838937, 40.099999999999994, 2.05]});
        atStep(st, 267);
        cqBox(c, st, 183, 182, {"length": 8.130960256458732, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 183, "box");
        // step 268: guard_begin {"block":1,"g":43,"op":"guard_begin","skippable":false}
        atStep(st, 269);
        cqUnion(c, st, 184, 180, {"toUnion": {"wp": 183}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 184, "union");
        // step 270: guard_end {"block":1,"g":43,"op":"guard_end"}
        atStep(st, 271);
        cqWorkplaneNew(c, st, 185, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 272);
        cqTransformed(c, st, 186, 185, {"rotate": [0, 0, 45.14659434196286], "offset": [4.708970740324437, 40.7, 2.05]});
        atStep(st, 273);
        cqBox(c, st, 187, 186, {"length": 8.046365438402539, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 187, "box");
        // step 274: guard_begin {"block":1,"g":44,"op":"guard_begin","skippable":false}
        atStep(st, 275);
        cqUnion(c, st, 188, 184, {"toUnion": {"wp": 187}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 188, "union");
        // step 276: guard_end {"block":1,"g":44,"op":"guard_end"}
        atStep(st, 277);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 278);
        cqTransformed(c, st, 190, 189, {"rotate": [0, 0, 52.134518782254226], "offset": [5.240693012858271, 41.3, 2.05]});
        atStep(st, 279);
        cqBox(c, st, 191, 190, {"length": 7.960018923039792, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 191, "box");
        // step 280: guard_begin {"block":1,"g":45,"op":"guard_begin","skippable":false}
        atStep(st, 281);
        cqUnion(c, st, 192, 188, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        // step 282: guard_end {"block":1,"g":45,"op":"guard_end"}
        atStep(st, 283);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 284);
        cqTransformed(c, st, 194, 193, {"rotate": [0, 0, 61.63648997367736], "offset": [5.635909064666036, 41.900000000000006, 2.05]});
        atStep(st, 285);
        cqBox(c, st, 195, 194, {"length": 7.881855912214616, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 195, "box");
        // step 286: guard_begin {"block":1,"g":46,"op":"guard_begin","skippable":false}
        atStep(st, 287);
        cqUnion(c, st, 196, 192, {"toUnion": {"wp": 195}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 196, "union");
        // step 288: guard_end {"block":1,"g":46,"op":"guard_end"}
        atStep(st, 289);
        cqWorkplaneNew(c, st, 197, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 290);
        cqTransformed(c, st, 198, 197, {"rotate": [0, 0, 73.92426442535157], "offset": [5.8843245607294055, 42.5, 2.05]});
        atStep(st, 291);
        cqBox(c, st, 199, 198, {"length": 7.824416879762618, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 199, "box");
        // step 292: guard_begin {"block":1,"g":47,"op":"guard_begin","skippable":false}
        atStep(st, 293);
        cqUnion(c, st, 200, 196, {"toUnion": {"wp": 199}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 200, "union");
        // step 294: guard_end {"block":1,"g":47,"op":"guard_end"}
        atStep(st, 295);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 296);
        cqTransformed(c, st, 202, 201, {"rotate": [0, 0, 88.3405218938875], "offset": [5.979468933042548, 43.099999999999994, 2.05]});
        atStep(st, 297);
        cqBox(c, st, 203, 202, {"length": 7.800251750805568, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 203, "box");
        // step 298: guard_begin {"block":1,"g":48,"op":"guard_begin","skippable":false}
        atStep(st, 299);
        cqUnion(c, st, 204, 200, {"toUnion": {"wp": 203}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 204, "union");
        // step 300: guard_end {"block":1,"g":48,"op":"guard_end"}
        atStep(st, 301);
        cqWorkplaneNew(c, st, 205, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 302);
        cqTransformed(c, st, 206, 205, {"rotate": [0, 0, 103.0065258424082], "offset": [5.918863921830286, 43.7, 2.05]});
        atStep(st, 303);
        cqBox(c, st, 207, 206, {"length": 7.815798661277807, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 207, "box");
        // step 304: guard_begin {"block":1,"g":49,"op":"guard_begin","skippable":false}
        atStep(st, 305);
        cqUnion(c, st, 208, 204, {"toUnion": {"wp": 207}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 208, "union");
        // step 306: guard_end {"block":1,"g":49,"op":"guard_end"}
        atStep(st, 307);
        cqWorkplaneNew(c, st, 209, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 308);
        cqTransformed(c, st, 210, 209, {"rotate": [0, 0, 115.87019160451604], "offset": [5.704088127674136, 44.3, 2.05]});
        atStep(st, 309);
        cqBox(c, st, 211, 210, {"length": 7.866826034113548, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 211, "box");
        // step 310: guard_begin {"block":1,"g":50,"op":"guard_begin","skippable":false}
        atStep(st, 311);
        cqUnion(c, st, 212, 208, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        // step 312: guard_end {"block":1,"g":50,"op":"guard_end"}
        atStep(st, 313);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 314);
        cqTransformed(c, st, 214, 213, {"rotate": [0, 0, 125.98876456778092], "offset": [5.340735893133706, 44.9, 2.05]});
        atStep(st, 315);
        cqBox(c, st, 215, 214, {"length": 7.941535153084672, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 215, "box");
        // step 316: guard_begin {"block":1,"g":51,"op":"guard_begin","skippable":false}
        atStep(st, 317);
        cqUnion(c, st, 216, 212, {"toUnion": {"wp": 215}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 216, "union");
        // step 318: guard_end {"block":1,"g":51,"op":"guard_end"}
        atStep(st, 319);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 320);
        cqTransformed(c, st, 218, 217, {"rotate": [0, 0, 133.49015524312452], "offset": [4.838271584888711, 45.5, 2.05]});
        atStep(st, 321);
        cqBox(c, st, 219, 218, {"length": 8.027024245926357, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 219, "box");
        // step 322: guard_begin {"block":1,"g":52,"op":"guard_begin","skippable":false}
        atStep(st, 323);
        cqUnion(c, st, 220, 216, {"toUnion": {"wp": 219}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 220, "union");
        // step 324: guard_end {"block":1,"g":52,"op":"guard_end"}
        atStep(st, 325);
        cqWorkplaneNew(c, st, 221, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 326);
        cqTransformed(c, st, 222, 221, {"rotate": [0, 0, 138.9000525893691], "offset": [4.209783071967154, 46.099999999999994, 2.05]});
        atStep(st, 327);
        cqBox(c, st, 223, 222, {"length": 8.112721668690325, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 223, "box");
        // step 328: guard_begin {"block":1,"g":53,"op":"guard_begin","skippable":false}
        atStep(st, 329);
        cqUnion(c, st, 224, 220, {"toUnion": {"wp": 223}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 224, "union");
        // step 330: guard_end {"block":1,"g":53,"op":"guard_end"}
        atStep(st, 331);
        cqWorkplaneNew(c, st, 225, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 332);
        cqTransformed(c, st, 226, 225, {"rotate": [0, 0, 142.73075782916942], "offset": [3.471640821301204, 46.7, 2.05]});
        atStep(st, 333);
        cqBox(c, st, 227, 226, {"length": 8.19081631741128, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 227, "box");
        // step 334: guard_begin {"block":1,"g":54,"op":"guard_begin","skippable":false}
        atStep(st, 335);
        cqUnion(c, st, 228, 224, {"toUnion": {"wp": 227}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 228, "union");
        // step 336: guard_end {"block":1,"g":54,"op":"guard_end"}
        atStep(st, 337);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 338);
        cqTransformed(c, st, 230, 229, {"rotate": [0, 0, 145.36606449757966], "offset": [2.643071490271754, 47.3, 2.05]});
        atStep(st, 339);
        cqBox(c, st, 231, 230, {"length": 8.255722479182797, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 231, "box");
        // step 340: guard_begin {"block":1,"g":55,"op":"guard_begin","skippable":false}
        atStep(st, 341);
        cqUnion(c, st, 232, 228, {"toUnion": {"wp": 231}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 232, "union");
        // step 342: guard_end {"block":1,"g":55,"op":"guard_end"}
        atStep(st, 343);
        cqWorkplaneNew(c, st, 233, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 344);
        cqTransformed(c, st, 234, 233, {"rotate": [0, 0, 147.06394706239038], "offset": [1.7456571230303817, 47.9, 2.05]});
        atStep(st, 345);
        cqBox(c, st, 235, 234, {"length": 8.303544338800624, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 235, "box");
        // step 346: guard_begin {"block":1,"g":56,"op":"guard_begin","skippable":false}
        atStep(st, 347);
        cqUnion(c, st, 236, 232, {"toUnion": {"wp": 235}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 236, "union");
        // step 348: guard_end {"block":1,"g":56,"op":"guard_end"}
        atStep(st, 349);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 350);
        cqTransformed(c, st, 238, 237, {"rotate": [0, 0, 147.9835584440287], "offset": [0.8027729952127354, 48.5, 2.05]});
        atStep(st, 351);
        cqBox(c, st, 239, 238, {"length": 8.331728270943609, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 239, "box");
        // step 352: guard_begin {"block":1,"g":57,"op":"guard_begin","skippable":false}
        atStep(st, 353);
        cqUnion(c, st, 240, 236, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        // step 354: guard_end {"block":1,"g":57,"op":"guard_end"}
        atStep(st, 355);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 356);
        cqTransformed(c, st, 242, 241, {"rotate": [0, 0, 148.2079449365075], "offset": [-0.16102125029518471, 49.099999999999994, 2.05]});
        atStep(st, 357);
        cqBox(c, st, 243, 242, {"length": 8.338870143297264, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 243, "box");
        // step 358: guard_begin {"block":1,"g":58,"op":"guard_begin","skippable":false}
        atStep(st, 359);
        cqUnion(c, st, 244, 240, {"toUnion": {"wp": 243}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 244, "union");
        // step 360: guard_end {"block":1,"g":58,"op":"guard_end"}
        atStep(st, 361);
        cqWorkplaneNew(c, st, 245, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 362);
        cqTransformed(c, st, 246, 245, {"rotate": [0, 0, 147.75698856130944], "offset": [-1.1206213172315915, 49.7, 2.05]});
        atStep(st, 363);
        cqBox(c, st, 247, 246, {"length": 8.324624609489737, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 247, "box");
        // step 364: guard_begin {"block":1,"g":59,"op":"guard_begin","skippable":false}
        atStep(st, 365);
        cqUnion(c, st, 248, 244, {"toUnion": {"wp": 247}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 248, "union");
        // step 366: guard_end {"block":1,"g":59,"op":"guard_end"}
        atStep(st, 367);
        cqWorkplaneNew(c, st, 249, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 368);
        cqTransformed(c, st, 250, 249, {"rotate": [0, 0, 146.59057581859648], "offset": [-2.051032156616407, 50.3, 2.05]});
        atStep(st, 369);
        cqBox(c, st, 251, 250, {"length": 8.289684580814917, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 251, "box");
        // step 370: guard_begin {"block":1,"g":60,"op":"guard_begin","skippable":false}
        atStep(st, 371);
        cqUnion(c, st, 252, 248, {"toUnion": {"wp": 251}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 252, "union");
        // step 372: guard_end {"block":1,"g":60,"op":"guard_end"}
        atStep(st, 373);
        cqWorkplaneNew(c, st, 253, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 374);
        cqTransformed(c, st, 254, 253, {"rotate": [0, 0, 144.60222841168326], "offset": [-2.928019021805377, 50.9, 2.05]});
        atStep(st, 375);
        cqBox(c, st, 255, 254, {"length": 8.235823133327651, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 255, "box");
        // step 376: guard_begin {"block":1,"g":61,"op":"guard_begin","skippable":false}
        atStep(st, 377);
        cqUnion(c, st, 256, 252, {"toUnion": {"wp": 255}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 256, "union");
        // step 378: guard_end {"block":1,"g":61,"op":"guard_end"}
        atStep(st, 379);
        cqWorkplaneNew(c, st, 257, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 380);
        cqTransformed(c, st, 258, 257, {"rotate": [0, 0, 141.6028085098047], "offset": [-3.728738719675056, 51.5, 2.05]});
        atStep(st, 381);
        cqBox(c, st, 259, 258, {"length": 8.166013449725613, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 259, "box");
        // step 382: guard_begin {"block":1,"g":62,"op":"guard_begin","skippable":false}
        atStep(st, 383);
        cqUnion(c, st, 260, 256, {"toUnion": {"wp": 259}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 260, "union");
        // step 384: guard_end {"block":1,"g":62,"op":"guard_end"}
        atStep(st, 385);
        cqWorkplaneNew(c, st, 261, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 386);
        cqTransformed(c, st, 262, 261, {"rotate": [0, 0, 137.2951769036805], "offset": [-4.4323346155114915, 52.1, 2.05]});
        atStep(st, 387);
        cqBox(c, st, 263, 262, {"length": 8.084666693534903, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 263, "box");
        // step 388: guard_begin {"block":1,"g":63,"op":"guard_begin","skippable":false}
        atStep(st, 389);
        cqUnion(c, st, 264, 260, {"toUnion": {"wp": 263}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 264, "union");
        // step 390: guard_end {"block":1,"g":63,"op":"guard_end"}
        atStep(st, 391);
        cqWorkplaneNew(c, st, 265, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 392);
        cqTransformed(c, st, 266, 265, {"rotate": [0, 0, 131.25003875092145], "offset": [-5.020479893295638, 52.7, 2.05]});
        atStep(st, 393);
        cqBox(c, st, 267, 266, {"length": 7.998042814317245, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 267, "box");
        // step 394: guard_begin {"block":1,"g":64,"op":"guard_begin","skippable":false}
        atStep(st, 395);
        cqUnion(c, st, 268, 264, {"toUnion": {"wp": 267}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 268, "union");
        // step 396: guard_end {"block":1,"g":64,"op":"guard_end"}
        atStep(st, 397);
        cqWorkplaneNew(c, st, 269, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 398);
        cqTransformed(c, st, 270, 269, {"rotate": [0, 0, 122.92732651847842], "offset": [-5.47785492088892, 53.3, 2.05]});
        atStep(st, 399);
        cqBox(c, st, 271, 270, {"length": 7.914829743404867, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 271, "box");
        // step 400: guard_begin {"block":1,"g":65,"op":"guard_begin","skippable":false}
        atStep(st, 401);
        cqUnion(c, st, 272, 268, {"toUnion": {"wp": 271}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 272, "union");
        // step 402: guard_end {"block":1,"g":65,"op":"guard_end"}
        atStep(st, 403);
        cqWorkplaneNew(c, st, 273, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 404);
        cqTransformed(c, st, 274, 273, {"rotate": [0, 0, 111.8688213305624], "offset": [-5.792546286015886, 53.900000000000006, 2.05]});
        atStep(st, 405);
        cqBox(c, st, 275, 274, {"length": 7.846524493865397, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 275, "box");
        // step 406: guard_begin {"block":1,"g":66,"op":"guard_begin","skippable":false}
        atStep(st, 407);
        cqUnion(c, st, 276, 272, {"toUnion": {"wp": 275}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 276, "union");
        // step 408: guard_end {"block":1,"g":66,"op":"guard_end"}
        atStep(st, 409);
        cqWorkplaneNew(c, st, 277, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 410);
        cqTransformed(c, st, 278, 277, {"rotate": [0, 0, 98.23190323560446], "offset": [-5.956357109209794, 54.5, 2.05]});
        atStep(st, 411);
        cqBox(c, st, 279, 278, {"length": 7.806246362805712, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 279, "box");
        // step 412: guard_begin {"block":1,"g":67,"op":"guard_begin","skippable":false}
        atStep(st, 413);
        cqUnion(c, st, 280, 276, {"toUnion": {"wp": 279}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 280, "union");
        // step 414: guard_end {"block":1,"g":67,"op":"guard_end"}
        atStep(st, 415);
        cqWorkplaneNew(c, st, 281, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 416);
        cqTransformed(c, st, 282, 281, {"rotate": [0, 0, 83.39499413012756], "offset": [-5.965020550887906, 55.099999999999994, 2.05]});
        atStep(st, 417);
        cqBox(c, st, 283, 282, {"length": 7.804008976776674, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 283, "box");
        // step 418: guard_begin {"block":1,"g":68,"op":"guard_begin","skippable":false}
        atStep(st, 419);
        cqUnion(c, st, 284, 280, {"toUnion": {"wp": 283}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 284, "union");
        // step 420: guard_end {"block":1,"g":68,"op":"guard_end"}
        atStep(st, 421);
        cqWorkplaneNew(c, st, 285, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 422);
        cqTransformed(c, st, 286, 285, {"rotate": [0, 0, 69.53243862802537], "offset": [-5.818310951260754, 55.7, 2.05]});
        atStep(st, 423);
        cqBox(c, st, 287, 286, {"length": 7.840430155361062, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 287, "box");
        // step 424: guard_begin {"block":1,"g":69,"op":"guard_begin","skippable":false}
        atStep(st, 425);
        cqUnion(c, st, 288, 284, {"toUnion": {"wp": 287}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 288, "union");
        // step 426: guard_end {"block":1,"g":69,"op":"guard_end"}
        atStep(st, 427);
        cqWorkplaneNew(c, st, 289, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 428);
        cqTransformed(c, st, 290, 289, {"rotate": [0, 0, 58.16118146466221], "offset": [-5.520049708174044, 56.3, 2.05]});
        atStep(st, 429);
        cqBox(c, st, 291, 290, {"length": 7.906268324863513, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 291, "box");
        // step 430: guard_begin {"block":1,"g":70,"op":"guard_begin","skippable":false}
        atStep(st, 431);
        cqUnion(c, st, 292, 288, {"toUnion": {"wp": 291}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 292, "union");
        // step 432: guard_end {"block":1,"g":70,"op":"guard_end"}
        atStep(st, 433);
        cqWorkplaneNew(c, st, 293, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 434);
        cqTransformed(c, st, 294, 293, {"rotate": [0, 0, 49.551901639152746], "offset": [-5.078005739780951, 56.9, 2.05]});
        atStep(st, 435);
        cqBox(c, st, 295, 294, {"length": 7.98844252694848, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 295, "box");
        // step 436: guard_begin {"block":1,"g":71,"op":"guard_begin","skippable":false}
        atStep(st, 437);
        cqUnion(c, st, 296, 292, {"toUnion": {"wp": 295}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 296, "union");
        // step 438: guard_end {"block":1,"g":71,"op":"guard_end"}
        atStep(st, 439);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 440);
        cqTransformed(c, st, 298, 297, {"rotate": [0, 0, 43.28152213747222], "offset": [-4.5036931247294785, 57.5, 2.05]});
        atStep(st, 441);
        cqBox(c, st, 299, 298, {"length": 8.075166778306414, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 299, "box");
        // step 442: guard_begin {"block":1,"g":72,"op":"guard_begin","skippable":false}
        atStep(st, 443);
        cqUnion(c, st, 300, 296, {"toUnion": {"wp": 299}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 300, "union");
        // step 444: guard_end {"block":1,"g":72,"op":"guard_end"}
        atStep(st, 445);
        cqWorkplaneNew(c, st, 301, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 446);
        cqTransformed(c, st, 302, 301, {"rotate": [0, 0, 38.804563483154006], "offset": [-3.8120711908038745, 58.1, 2.05]});
        atStep(st, 447);
        cqBox(c, st, 303, 302, {"length": 8.15744800745522, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 303, "box");
        // step 448: guard_begin {"block":1,"g":73,"op":"guard_begin","skippable":false}
        atStep(st, 449);
        cqUnion(c, st, 304, 300, {"toUnion": {"wp": 303}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 304, "union");
        // step 450: guard_end {"block":1,"g":73,"op":"guard_end"}
        atStep(st, 451);
        cqWorkplaneNew(c, st, 305, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 452);
        cqTransformed(c, st, 306, 305, {"rotate": [0, 0, 35.67671442274206], "offset": [-3.021154863919435, 58.7, 2.05]});
        atStep(st, 453);
        cqBox(c, st, 307, 306, {"length": 8.228786960648543, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 307, "box");
        // step 454: guard_begin {"block":1,"g":74,"op":"guard_begin","skippable":false}
        atStep(st, 455);
        cqUnion(c, st, 308, 304, {"toUnion": {"wp": 307}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 308, "union");
        // step 456: guard_end {"block":1,"g":74,"op":"guard_end"}
        atStep(st, 457);
        cqWorkplaneNew(c, st, 309, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 458);
        cqTransformed(c, st, 310, 309, {"rotate": [0, 0, 33.587095584973284], "offset": [-2.151545426850925, 59.3, 2.05]});
        atStep(st, 459);
        cqBox(c, st, 311, 310, {"length": 8.284590972017698, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 311, "box");
        // step 460: guard_begin {"block":1,"g":75,"op":"guard_begin","skippable":false}
        atStep(st, 461);
        cqUnion(c, st, 312, 308, {"toUnion": {"wp": 311}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 312, "union");
        // step 462: guard_end {"block":1,"g":75,"op":"guard_end"}
        atStep(st, 463);
        cqWorkplaneNew(c, st, 313, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 464);
        cqTransformed(c, st, 314, 313, {"rotate": [0, 0, 32.33594895893903], "offset": [-1.225893910191166, 59.9, 2.05]});
        atStep(st, 465);
        cqBox(c, st, 315, 314, {"length": 8.321741512962516, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 315, "box");
        // step 466: guard_begin {"block":1,"g":76,"op":"guard_begin","skippable":false}
        atStep(st, 467);
        cqUnion(c, st, 316, 312, {"toUnion": {"wp": 315}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 316, "union");
        // step 468: guard_end {"block":1,"g":76,"op":"guard_end"}
        atStep(st, 469);
        cqWorkplaneNew(c, st, 317, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 470);
        cqTransformed(c, st, 318, 317, {"rotate": [0, 0, 31.80860191500336], "offset": [-0.26831109278916376, 60.5, 2.05]});
        atStep(st, 471);
        cqBox(c, st, 319, 318, {"length": 8.338339808445788, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 319, "box");
        // step 472: guard_begin {"block":1,"g":77,"op":"guard_begin","skippable":false}
        atStep(st, 473);
        cqUnion(c, st, 320, 316, {"toUnion": {"wp": 319}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 320, "union");
        // step 474: guard_end {"block":1,"g":77,"op":"guard_end"}
        atStep(st, 475);
        cqWorkplaneNew(c, st, 321, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 476);
        cqTransformed(c, st, 322, 321, {"rotate": [0, 0, 31.958058554159805], "offset": [0.6962605204010313, 61.1, 2.05]});
        atStep(st, 477);
        cqBox(c, st, 323, 322, {"length": 8.333576204120034, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 323, "box");
        // step 478: guard_begin {"block":1,"g":78,"op":"guard_begin","skippable":false}
        atStep(st, 479);
        cqUnion(c, st, 324, 320, {"toUnion": {"wp": 323}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 324, "union");
        // step 480: guard_end {"block":1,"g":78,"op":"guard_end"}
        atStep(st, 481);
        cqWorkplaneNew(c, st, 325, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 482);
        cqTransformed(c, st, 326, 325, {"rotate": [0, 0, 32.79755203230884], "offset": [1.64269638474131, 61.7, 2.05]});
        atStep(st, 483);
        cqBox(c, st, 327, 326, {"length": 8.307680794970727, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 327, "box");
        // step 484: guard_begin {"block":1,"g":79,"op":"guard_begin","skippable":false}
        atStep(st, 485);
        cqUnion(c, st, 328, 324, {"toUnion": {"wp": 327}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 328, "union");
        // step 486: guard_end {"block":1,"g":79,"op":"guard_end"}
        atStep(st, 487);
        cqWorkplaneNew(c, st, 329, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 488);
        cqTransformed(c, st, 330, 329, {"rotate": [0, 0, 34.402655014615235], "offset": [2.5463443439836926, 62.3, 2.05]});
        atStep(st, 489);
        cqBox(c, st, 331, 330, {"length": 8.261937058510048, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 331, "box");
        // step 490: guard_begin {"block":1,"g":80,"op":"guard_begin","skippable":false}
        atStep(st, 491);
        cqUnion(c, st, 332, 328, {"toUnion": {"wp": 331}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 332, "union");
        // step 492: guard_end {"block":1,"g":80,"op":"guard_end"}
        atStep(st, 493);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 494);
        cqTransformed(c, st, 334, 333, {"rotate": [0, 0, 36.92311029348619], "offset": [3.3836667538738032, 62.900000000000006, 2.05]});
        atStep(st, 495);
        cqBox(c, st, 335, 334, {"length": 8.198763647373106, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 335, "box");
        // step 496: guard_begin {"block":1,"g":81,"op":"guard_begin","skippable":false}
        atStep(st, 497);
        cqUnion(c, st, 336, 332, {"toUnion": {"wp": 335}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 336, "union");
        // step 498: guard_end {"block":1,"g":81,"op":"guard_end"}
        atStep(st, 499);
        cqWorkplaneNew(c, st, 337, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 500);
        cqTransformed(c, st, 338, 337, {"rotate": [0, 0, 40.604545642857275], "offset": [4.132853575658584, 63.5, 2.05]});
        atStep(st, 501);
        cqBox(c, st, 339, 338, {"length": 8.121893420278806, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 339, "box");
        // step 502: guard_begin {"block":1,"g":82,"op":"guard_begin","skippable":false}
        atStep(st, 503);
        cqUnion(c, st, 340, 336, {"toUnion": {"wp": 339}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 340, "union");
        // step 504: guard_end {"block":1,"g":82,"op":"guard_end"}
        atStep(st, 505);
        cqWorkplaneNew(c, st, 341, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 506);
        cqTransformed(c, st, 342, 341, {"rotate": [0, 0, 45.81572630690205], "offset": [4.774390470030493, 64.1, 2.05]});
        atStep(st, 507);
        cqBox(c, st, 343, 342, {"length": 8.036701130201367, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 343, "box");
        // step 508: guard_begin {"block":1,"g":83,"op":"guard_begin","skippable":false}
        atStep(st, 509);
        cqUnion(c, st, 344, 340, {"toUnion": {"wp": 343}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 344, "union");
        // step 510: guard_end {"block":1,"g":83,"op":"guard_end"}
        atStep(st, 511);
        cqWorkplaneNew(c, st, 345, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 512);
        cqTransformed(c, st, 346, 345, {"rotate": [0, 0, 53.05689011216569], "offset": [5.291567094160398, 64.7, 2.05]});
        atStep(st, 513);
        cqBox(c, st, 347, 346, {"length": 7.950720062250434, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 347, "box");
        // step 514: guard_begin {"block":1,"g":84,"op":"guard_begin","skippable":false}
        atStep(st, 515);
        cqUnion(c, st, 348, 344, {"toUnion": {"wp": 347}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 348, "union");
        // step 516: guard_end {"block":1,"g":84,"op":"guard_end"}
        atStep(st, 517);
        cqWorkplaneNew(c, st, 349, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 518);
        cqTransformed(c, st, 350, 349, {"rotate": [0, 0, 62.865506747154534], "offset": [5.670912362023104, 65.3, 2.05]});
        atStep(st, 519);
        cqBox(c, st, 351, 350, {"length": 7.874203970569552, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 351, "box");
        // step 520: guard_begin {"block":1,"g":85,"op":"guard_begin","skippable":false}
        atStep(st, 521);
        cqUnion(c, st, 352, 348, {"toUnion": {"wp": 351}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 352, "union");
        // step 522: guard_end {"block":1,"g":85,"op":"guard_end"}
        atStep(st, 523);
        cqWorkplaneNew(c, st, 353, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 524);
        cqTransformed(c, st, 354, 353, {"rotate": [0, 0, 75.44595725851849], "offset": [5.902545330632796, 65.89999999999999, 2.05]});
        atStep(st, 525);
        cqBox(c, st, 355, 354, {"length": 7.819891675859754, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 355, "box");
        // step 526: guard_begin {"block":1,"g":86,"op":"guard_begin","skippable":false}
        atStep(st, 527);
        cqUnion(c, st, 356, 352, {"toUnion": {"wp": 355}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 356, "union");
        // step 528: guard_end {"block":1,"g":86,"op":"guard_end"}
        atStep(st, 529);
        cqWorkplaneNew(c, st, 357, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 530);
        cqTransformed(c, st, 358, 357, {"rotate": [0, 0, 89.99999999999974], "offset": [5.980432572528088, 66.5, 2.05]});
        atStep(st, 531);
        cqBox(c, st, 359, 358, {"length": 7.800000000000008, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 359, "box");
        // step 532: guard_begin {"block":1,"g":87,"op":"guard_begin","skippable":false}
        atStep(st, 533);
        cqUnion(c, st, 360, 356, {"toUnion": {"wp": 359}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 360, "union");
        // step 534: guard_end {"block":1,"g":87,"op":"guard_end"}
        atStep(st, 535);
        cqWorkplaneNew(c, st, 361, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 536);
        cqTransformed(c, st, 362, 361, {"rotate": [0, 0, 104.55404274148118], "offset": [5.902545330632798, 67.1, 2.05]});
        atStep(st, 537);
        cqBox(c, st, 363, 362, {"length": 7.819891675859767, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 363, "box");
        // step 538: guard_begin {"block":1,"g":88,"op":"guard_begin","skippable":false}
        atStep(st, 539);
        cqUnion(c, st, 364, 360, {"toUnion": {"wp": 363}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 364, "union");
        // step 540: guard_end {"block":1,"g":88,"op":"guard_end"}
        atStep(st, 541);
        cqWorkplaneNew(c, st, 365, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 542);
        cqTransformed(c, st, 366, 365, {"rotate": [0, 0, 117.13449325284547], "offset": [5.670912362023107, 67.7, 2.05]});
        atStep(st, 543);
        cqBox(c, st, 367, 366, {"length": 7.874203970569552, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 367, "box");
        // step 544: guard_begin {"block":1,"g":89,"op":"guard_begin","skippable":false}
        atStep(st, 545);
        cqUnion(c, st, 368, 364, {"toUnion": {"wp": 367}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 368, "union");
        // step 546: guard_end {"block":1,"g":89,"op":"guard_end"}
        atStep(st, 547);
        cqWorkplaneNew(c, st, 369, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 548);
        cqTransformed(c, st, 370, 369, {"rotate": [0, 0, 126.94310988783424], "offset": [5.291567094160401, 68.3, 2.05]});
        atStep(st, 549);
        cqBox(c, st, 371, 370, {"length": 7.950720062250433, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 371, "box");
        // step 550: guard_begin {"block":1,"g":90,"op":"guard_begin","skippable":false}
        atStep(st, 551);
        cqUnion(c, st, 372, 368, {"toUnion": {"wp": 371}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 372, "union");
        // step 552: guard_end {"block":1,"g":90,"op":"guard_end"}
        atStep(st, 553);
        cqWorkplaneNew(c, st, 373, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 554);
        cqTransformed(c, st, 374, 373, {"rotate": [0, 0, 134.1842736930979], "offset": [4.774390470030497, 68.9, 2.05]});
        atStep(st, 555);
        cqBox(c, st, 375, 374, {"length": 8.036701130201367, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 375, "box");
        // step 556: guard_begin {"block":1,"g":91,"op":"guard_begin","skippable":false}
        atStep(st, 557);
        cqUnion(c, st, 376, 372, {"toUnion": {"wp": 375}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 376, "union");
        // step 558: guard_end {"block":1,"g":91,"op":"guard_end"}
        atStep(st, 559);
        cqWorkplaneNew(c, st, 377, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 560);
        cqTransformed(c, st, 378, 377, {"rotate": [0, 0, 139.39545435714234], "offset": [4.132853575658585, 69.5, 2.05]});
        atStep(st, 561);
        cqBox(c, st, 379, 378, {"length": 8.12189342027882, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 379, "box");
        // step 562: guard_begin {"block":1,"g":92,"op":"guard_begin","skippable":false}
        atStep(st, 563);
        cqUnion(c, st, 380, 376, {"toUnion": {"wp": 379}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 380, "union");
        // step 564: guard_end {"block":1,"g":92,"op":"guard_end"}
        atStep(st, 565);
        cqWorkplaneNew(c, st, 381, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 566);
        cqTransformed(c, st, 382, 381, {"rotate": [0, 0, 143.07688970651415], "offset": [3.3836667538738, 70.10000000000001, 2.05]});
        atStep(st, 567);
        cqBox(c, st, 383, 382, {"length": 8.198763647373102, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 383, "box");
        // step 568: guard_begin {"block":1,"g":93,"op":"guard_begin","skippable":false}
        atStep(st, 569);
        cqUnion(c, st, 384, 380, {"toUnion": {"wp": 383}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 384, "union");
        // step 570: guard_end {"block":1,"g":93,"op":"guard_end"}
        atStep(st, 571);
        cqWorkplaneNew(c, st, 385, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 572);
        cqTransformed(c, st, 386, 385, {"rotate": [0, 0, 145.5973449853848], "offset": [2.5463443439836935, 70.7, 2.05]});
        atStep(st, 573);
        cqBox(c, st, 387, 386, {"length": 8.261937058510037, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 387, "box");
        // step 574: guard_begin {"block":1,"g":94,"op":"guard_begin","skippable":false}
        atStep(st, 575);
        cqUnion(c, st, 388, 384, {"toUnion": {"wp": 387}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 388, "union");
        // step 576: guard_end {"block":1,"g":94,"op":"guard_end"}
        atStep(st, 577);
        cqWorkplaneNew(c, st, 389, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 578);
        cqTransformed(c, st, 390, 389, {"rotate": [0, 0, 147.20244796769086], "offset": [1.6426963847413265, 71.3, 2.05]});
        atStep(st, 579);
        cqBox(c, st, 391, 390, {"length": 8.307680794970706, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 391, "box");
        // step 580: guard_begin {"block":1,"g":95,"op":"guard_begin","skippable":false}
        atStep(st, 581);
        cqUnion(c, st, 392, 388, {"toUnion": {"wp": 391}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 392, "union");
        // step 582: guard_end {"block":1,"g":95,"op":"guard_end"}
        atStep(st, 583);
        cqWorkplaneNew(c, st, 393, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 584);
        cqTransformed(c, st, 394, 393, {"rotate": [0, 0, 148.0419414458407], "offset": [0.6962605204010376, 71.9, 2.05]});
        atStep(st, 585);
        cqBox(c, st, 395, 394, {"length": 8.333576204120078, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 395, "box");
        // step 586: guard_begin {"block":1,"g":96,"op":"guard_begin","skippable":false}
        atStep(st, 587);
        cqUnion(c, st, 396, 392, {"toUnion": {"wp": 395}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 396, "union");
        // step 588: guard_end {"block":1,"g":96,"op":"guard_end"}
        atStep(st, 589);
        cqWorkplaneNew(c, st, 397, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 590);
        cqTransformed(c, st, 398, 397, {"rotate": [0, 0, 148.1913980849964], "offset": [-0.26831109278916787, 72.5, 2.05]});
        atStep(st, 591);
        cqBox(c, st, 399, 398, {"length": 8.338339808445767, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 399, "box");
        // step 592: guard_begin {"block":1,"g":97,"op":"guard_begin","skippable":false}
        atStep(st, 593);
        cqUnion(c, st, 400, 396, {"toUnion": {"wp": 399}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 400, "union");
        // step 594: guard_end {"block":1,"g":97,"op":"guard_end"}
        atStep(st, 595);
        cqWorkplaneNew(c, st, 401, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 596);
        cqTransformed(c, st, 402, 401, {"rotate": [0, 0, 147.664051041061], "offset": [-1.2258939101911597, 73.1, 2.05]});
        atStep(st, 597);
        cqBox(c, st, 403, 402, {"length": 8.321741512962516, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 403, "box");
        // step 598: guard_begin {"block":1,"g":98,"op":"guard_begin","skippable":false}
        atStep(st, 599);
        cqUnion(c, st, 404, 400, {"toUnion": {"wp": 403}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 404, "union");
        // step 600: guard_end {"block":1,"g":98,"op":"guard_end"}
        atStep(st, 601);
        cqWorkplaneNew(c, st, 405, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 602);
        cqTransformed(c, st, 406, 405, {"rotate": [0, 0, 146.412904415027], "offset": [-2.151545426850923, 73.7, 2.05]});
        atStep(st, 603);
        cqBox(c, st, 407, 406, {"length": 8.284590972017707, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 407, "box");
        // step 604: guard_begin {"block":1,"g":99,"op":"guard_begin","skippable":false}
        atStep(st, 605);
        cqUnion(c, st, 408, 404, {"toUnion": {"wp": 407}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 408, "union");
        // step 606: guard_end {"block":1,"g":99,"op":"guard_end"}
        atStep(st, 607);
        cqWorkplaneNew(c, st, 409, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 608);
        cqTransformed(c, st, 410, 409, {"rotate": [0, 0, 144.32328557725768], "offset": [-3.0211548639194343, 74.3, 2.05]});
        atStep(st, 609);
        cqBox(c, st, 411, 410, {"length": 8.228786960648536, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 411, "box");
        // step 610: guard_begin {"block":1,"g":100,"op":"guard_begin","skippable":false}
        atStep(st, 611);
        cqUnion(c, st, 412, 408, {"toUnion": {"wp": 411}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 412, "union");
        // step 612: guard_end {"block":1,"g":100,"op":"guard_end"}
        atStep(st, 613);
        cqWorkplaneNew(c, st, 413, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 614);
        cqTransformed(c, st, 414, 413, {"rotate": [0, 0, 141.19543651684612], "offset": [-3.812071190803861, 74.89999999999999, 2.05]});
        atStep(st, 615);
        cqBox(c, st, 415, 414, {"length": 8.157448007455198, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 415, "box");
        // step 616: guard_begin {"block":1,"g":101,"op":"guard_begin","skippable":false}
        atStep(st, 617);
        cqUnion(c, st, 416, 412, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        // step 618: guard_end {"block":1,"g":101,"op":"guard_end"}
        atStep(st, 619);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 620);
        cqTransformed(c, st, 418, 417, {"rotate": [0, 0, 136.7184778625279], "offset": [-4.503693124729463, 75.5, 2.05]});
        atStep(st, 621);
        cqBox(c, st, 419, 418, {"length": 8.075166778306427, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 419, "box");
        // step 622: guard_begin {"block":1,"g":102,"op":"guard_begin","skippable":false}
        atStep(st, 623);
        cqUnion(c, st, 420, 416, {"toUnion": {"wp": 419}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 420, "union");
        // step 624: guard_end {"block":1,"g":102,"op":"guard_end"}
        atStep(st, 625);
        cqWorkplaneNew(c, st, 421, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 626);
        cqTransformed(c, st, 422, 421, {"rotate": [0, 0, 130.44809836084704], "offset": [-5.078005739780945, 76.1, 2.05]});
        atStep(st, 627);
        cqBox(c, st, 423, 422, {"length": 7.988442526948496, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 423, "box");
        // step 628: guard_begin {"block":1,"g":103,"op":"guard_begin","skippable":false}
        atStep(st, 629);
        cqUnion(c, st, 424, 420, {"toUnion": {"wp": 423}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 424, "union");
        // step 630: guard_end {"block":1,"g":103,"op":"guard_end"}
        atStep(st, 631);
        cqWorkplaneNew(c, st, 425, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 632);
        cqTransformed(c, st, 426, 425, {"rotate": [0, 0, 121.8388185353381], "offset": [-5.5200497081740405, 76.7, 2.05]});
        atStep(st, 633);
        cqBox(c, st, 427, 426, {"length": 7.906268324863507, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 427, "box");
        // step 634: guard_begin {"block":1,"g":104,"op":"guard_begin","skippable":false}
        atStep(st, 635);
        cqUnion(c, st, 428, 424, {"toUnion": {"wp": 427}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 428, "union");
        // step 636: guard_end {"block":1,"g":104,"op":"guard_end"}
        atStep(st, 637);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 638);
        cqTransformed(c, st, 430, 429, {"rotate": [0, 0, 110.46756137197484], "offset": [-5.818310951260751, 77.3, 2.05]});
        atStep(st, 639);
        cqBox(c, st, 431, 430, {"length": 7.840430155361055, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 431, "box");
        // step 640: guard_begin {"block":1,"g":105,"op":"guard_begin","skippable":false}
        atStep(st, 641);
        cqUnion(c, st, 432, 428, {"toUnion": {"wp": 431}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        // step 642: guard_end {"block":1,"g":105,"op":"guard_end"}
        atStep(st, 643);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 644);
        cqTransformed(c, st, 434, 433, {"rotate": [0, 0, 96.60500586987264], "offset": [-5.965020550887905, 77.9, 2.05]});
        atStep(st, 645);
        cqBox(c, st, 435, 434, {"length": 7.804008976776681, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 435, "box");
        // step 646: guard_begin {"block":1,"g":106,"op":"guard_begin","skippable":false}
        atStep(st, 647);
        cqUnion(c, st, 436, 432, {"toUnion": {"wp": 435}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 436, "union");
        // step 648: guard_end {"block":1,"g":106,"op":"guard_end"}
        atStep(st, 649);
        cqWorkplaneNew(c, st, 437, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 650);
        cqTransformed(c, st, 438, 437, {"rotate": [0, 0, 81.76809676439571], "offset": [-5.956357109209794, 78.5, 2.05]});
        atStep(st, 651);
        cqBox(c, st, 439, 438, {"length": 7.806246362805712, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 439, "box");
        // step 652: guard_begin {"block":1,"g":107,"op":"guard_begin","skippable":false}
        atStep(st, 653);
        cqUnion(c, st, 440, 436, {"toUnion": {"wp": 439}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 440, "union");
        // step 654: guard_end {"block":1,"g":107,"op":"guard_end"}
        atStep(st, 655);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 656);
        cqTransformed(c, st, 442, 441, {"rotate": [0, 0, 68.13117866943755], "offset": [-5.792546286015886, 79.1, 2.05]});
        atStep(st, 657);
        cqBox(c, st, 443, 442, {"length": 7.846524493865405, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 443, "box");
        // step 658: guard_begin {"block":1,"g":108,"op":"guard_begin","skippable":false}
        atStep(st, 659);
        cqUnion(c, st, 444, 440, {"toUnion": {"wp": 443}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 444, "union");
        // step 660: guard_end {"block":1,"g":108,"op":"guard_end"}
        atStep(st, 661);
        cqWorkplaneNew(c, st, 445, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 662);
        cqTransformed(c, st, 446, 445, {"rotate": [0, 0, 57.07267348152162], "offset": [-5.477854920888921, 79.7, 2.05]});
        atStep(st, 663);
        cqBox(c, st, 447, 446, {"length": 7.914829743404858, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 447, "box");
        // step 664: guard_begin {"block":1,"g":109,"op":"guard_begin","skippable":false}
        atStep(st, 665);
        cqUnion(c, st, 448, 444, {"toUnion": {"wp": 447}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 448, "union");
        // step 666: guard_end {"block":1,"g":109,"op":"guard_end"}
        atStep(st, 667);
        cqWorkplaneNew(c, st, 449, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 668);
        cqTransformed(c, st, 450, 449, {"rotate": [0, 0, 48.74996124907855], "offset": [-5.020479893295638, 80.30000000000001, 2.05]});
        atStep(st, 669);
        cqBox(c, st, 451, 450, {"length": 7.998042814317253, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 451, "box");
        // step 670: guard_begin {"block":1,"g":110,"op":"guard_begin","skippable":false}
        atStep(st, 671);
        cqUnion(c, st, 452, 448, {"toUnion": {"wp": 451}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 452, "union");
        // step 672: guard_end {"block":1,"g":110,"op":"guard_end"}
        atStep(st, 673);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 674);
        cqTransformed(c, st, 454, 453, {"rotate": [0, 0, 42.704823096319465], "offset": [-4.432334615511489, 80.9, 2.05]});
        atStep(st, 675);
        cqBox(c, st, 455, 454, {"length": 8.084666693534903, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 455, "box");
        // step 676: guard_begin {"block":1,"g":111,"op":"guard_begin","skippable":false}
        atStep(st, 677);
        cqUnion(c, st, 456, 452, {"toUnion": {"wp": 455}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 456, "union");
        // step 678: guard_end {"block":1,"g":111,"op":"guard_end"}
        atStep(st, 679);
        cqWorkplaneNew(c, st, 457, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 680);
        cqTransformed(c, st, 458, 457, {"rotate": [0, 0, 38.39719149019529], "offset": [-3.7287387196750568, 81.5, 2.05]});
        atStep(st, 681);
        cqBox(c, st, 459, 458, {"length": 8.166013449725602, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 459, "box");
        // step 682: guard_begin {"block":1,"g":112,"op":"guard_begin","skippable":false}
        atStep(st, 683);
        cqUnion(c, st, 460, 456, {"toUnion": {"wp": 459}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 460, "union");
        // step 684: guard_end {"block":1,"g":112,"op":"guard_end"}
        atStep(st, 685);
        cqWorkplaneNew(c, st, 461, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 686);
        cqTransformed(c, st, 462, 461, {"rotate": [0, 0, 35.39777158831675], "offset": [-2.928019021805392, 82.1, 2.05]});
        atStep(st, 687);
        cqBox(c, st, 463, 462, {"length": 8.235823133327628, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 463, "box");
        // step 688: guard_begin {"block":1,"g":113,"op":"guard_begin","skippable":false}
        atStep(st, 689);
        cqUnion(c, st, 464, 460, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 464, "union");
        // step 690: guard_end {"block":1,"g":113,"op":"guard_end"}
        atStep(st, 691);
        cqWorkplaneNew(c, st, 465, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 692);
        cqTransformed(c, st, 466, 465, {"rotate": [0, 0, 33.40942418140391], "offset": [-2.0510321566164285, 82.69999999999999, 2.05]});
        atStep(st, 693);
        cqBox(c, st, 467, 466, {"length": 8.289684580814932, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 467, "box");
        // step 694: guard_begin {"block":1,"g":114,"op":"guard_begin","skippable":false}
        atStep(st, 695);
        cqUnion(c, st, 468, 464, {"toUnion": {"wp": 467}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 468, "union");
        // step 696: guard_end {"block":1,"g":114,"op":"guard_end"}
        atStep(st, 697);
        cqWorkplaneNew(c, st, 469, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 698);
        cqTransformed(c, st, 470, 469, {"rotate": [0, 0, 32.243011438690274], "offset": [-1.1206213172316084, 83.3, 2.05]});
        atStep(st, 699);
        cqBox(c, st, 471, 470, {"length": 8.324624609489733, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 471, "box");
        // step 700: guard_begin {"block":1,"g":115,"op":"guard_begin","skippable":false}
        atStep(st, 701);
        cqUnion(c, st, 472, 468, {"toUnion": {"wp": 471}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 472, "union");
        // step 702: guard_end {"block":1,"g":115,"op":"guard_end"}
        atStep(st, 703);
        cqWorkplaneNew(c, st, 473, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 704);
        cqTransformed(c, st, 474, 473, {"rotate": [0, 0, 42.92901422119717], "offset": [-0.32251078053158333, 83.9, 2.05]});
        atStep(st, 705);
        cqBox(c, st, 475, 474, {"length": 8.080938598448475, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 475, "box");
        // step 706: guard_begin {"block":1,"g":116,"op":"guard_begin","skippable":false}
        atStep(st, 707);
        cqUnion(c, st, 476, 472, {"toUnion": {"wp": 475}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 476, "union");
        // step 708: guard_end {"block":1,"g":116,"op":"guard_end"}
        atStep(st, 709);
        cqWorkplaneNew(c, st, 477, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 710);
        cqTransformed(c, st, 478, 477, {"rotate": [0, 0, 90], "offset": [-4.408728476930472e-15, 84.5, 2.05]});
        atStep(st, 711);
        cqBox(c, st, 479, 478, {"length": 7.799999999999994, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 479, "box");
        // step 712: guard_begin {"block":1,"g":117,"op":"guard_begin","skippable":false}
        atStep(st, 713);
        cqUnion(c, st, 480, 476, {"toUnion": {"wp": 479}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 480, "union");
        // step 714: guard_end {"block":1,"g":117,"op":"guard_end"}
        atStep(st, 715);
        cqWorkplaneNew(c, st, 481, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 716);
        cqTransformed(c, st, 482, 481, {"rotate": [0, 0, 90], "offset": [-4.408728476930472e-15, 85.1, 2.05]});
        atStep(st, 717);
        cqBox(c, st, 483, 482, {"length": 7.800000000000008, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 483, "box");
        // step 718: guard_begin {"block":1,"g":118,"op":"guard_begin","skippable":false}
        atStep(st, 719);
        cqUnion(c, st, 484, 480, {"toUnion": {"wp": 483}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 484, "union");
        // step 720: guard_end {"block":1,"g":118,"op":"guard_end"}
        atStep(st, 721);
        cqWorkplaneNew(c, st, 485, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 722);
        cqTransformed(c, st, 486, 485, {"rotate": [0, 0, 90], "offset": [-4.408728476930472e-15, 85.7, 2.05]});
        atStep(st, 723);
        cqBox(c, st, 487, 486, {"length": 7.799999999999994, "width": 6, "height": 1.8, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 487, "box");
        // step 724: guard_begin {"block":1,"g":119,"op":"guard_begin","skippable":false}
        atStep(st, 725);
        cqUnion(c, st, 488, 484, {"toUnion": {"wp": 487}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 488, "union");
        // step 726: guard_end {"block":1,"g":119,"op":"guard_end"}
        atStep(st, 727);
        cqWorkplaneNew(c, st, 489, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 728);
        cqTransformed(c, st, 490, 489, {"rotate": [0, 0, 0], "offset": [0, 17, 1.9599999999999995]});
        atStep(st, 729);
        cqBox(c, st, 491, 490, {"length": 9, "width": 8.399999999999999, "height": 2.52, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 491, "box");
        // step 730: guard_begin {"block":2,"g":120,"op":"guard_begin","skippable":false}
        atStep(st, 731);
        cqUnion(c, st, 492, 488, {"toUnion": {"wp": 491}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 492, "union");
        // step 732: guard_end {"block":2,"g":120,"op":"guard_end"}
        atStep(st, 733);
        cqWorkplaneNew(c, st, 493, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 734);
        cqTransformed(c, st, 494, 493, {"rotate": [0, 0, 0], "offset": [0, 78, 3.925]});
        atStep(st, 735);
        cqBox(c, st, 495, 494, {"length": 7.199999999999999, "width": 4, "height": 2.25, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 495, "box");
        // step 736: guard_begin {"block":3,"g":121,"op":"guard_begin","skippable":false}
        atStep(st, 737);
        cqUnion(c, st, 496, 492, {"toUnion": {"wp": 495}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 496, "union");
        // step 738: guard_end {"block":3,"g":121,"op":"guard_end"}
        atStep(st, 739);
        cqWorkplaneNew(c, st, 497, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 740);
        cqTransformed(c, st, 498, 497, {"rotate": [0, 0, 0], "offset": [0, 22, 3.4]});
        atStep(st, 741);
        cqBox(c, st, 499, 498, {"length": 8, "width": 1, "height": 2.16, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 499, "box");
        // step 742: guard_begin {"block":5,"g":122,"op":"guard_begin","skippable":false}
        atStep(st, 743);
        cqCut(c, st, 500, 496, {"toCut": {"wp": 499}, "clean": true, "tol": undefined});
        stepVol(c, st, 500, "cut");
        // step 744: guard_end {"block":5,"g":122,"op":"guard_end"}
        atStep(st, 745);
        cqWorkplaneNew(c, st, 501, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 746);
        cqTransformed(c, st, 502, 501, {"rotate": [0, 0, 0], "offset": [0, 34, 3.4]});
        atStep(st, 747);
        cqBox(c, st, 503, 502, {"length": 8, "width": 1, "height": 2.16, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 503, "box");
        // step 748: guard_begin {"block":5,"g":123,"op":"guard_begin","skippable":false}
        atStep(st, 749);
        cqCut(c, st, 504, 500, {"toCut": {"wp": 503}, "clean": true, "tol": undefined});
        stepVol(c, st, 504, "cut");
        // step 750: guard_end {"block":5,"g":123,"op":"guard_end"}
        atStep(st, 751);
        cqWorkplaneNew(c, st, 505, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 752);
        cqTransformed(c, st, 506, 505, {"rotate": [0, 0, 0], "offset": [0, 46, 3.4]});
        atStep(st, 753);
        cqBox(c, st, 507, 506, {"length": 8, "width": 1, "height": 2.16, "centered": [true, true, true], "combine": true, "clean": true});
        stepVol(c, st, 507, "box");
        // step 754: guard_begin {"block":5,"g":124,"op":"guard_begin","skippable":false}
        atStep(st, 755);
        cqCut(c, st, 508, 504, {"toCut": {"wp": 507}, "clean": true, "tol": undefined});
        stepVol(c, st, 508, "cut");
        // step 756: guard_end {"block":5,"g":124,"op":"guard_end"}
        // step 757: guard_begin {"block":6,"g":125,"op":"guard_begin","skippable":false}
        atStep(st, 758);
        cqUnion(c, st, 509, 9, {"toUnion": {"wp": 508}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 509, "union");
        // step 759: guard_end {"block":6,"g":125,"op":"guard_end"}
        atStep(st, 760);
        cqWorkplaneNew(c, st, 510, {"plane": {"origin": [0, 20, 2.5], "xDir": [1, 0, 0], "zDir": [0, 1, 0]}, "obj": undefined});
        atStep(st, 761);
        cqPolyline(c, st, 511, 510, {"listOfXYTuple": [[-20, 0], [-10, 0], [-15, 10]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 762);
        cqClose(c, st, 512, 511, {});
        atStep(st, 763);
        cqExtrude(c, st, 513, 512, {"until": 9, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 513, "extrude");
        atStep(st, 764);
        cqWorkplaneNew(c, st, 514, {"plane": {"origin": [0, 20, 2.5], "xDir": [1, 0, 0], "zDir": [0, 1, 0]}, "obj": undefined});
        atStep(st, 765);
        cqPolyline(c, st, 515, 514, {"listOfXYTuple": [[10, 0], [20, 0], [15, 10]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 766);
        cqClose(c, st, 516, 515, {});
        atStep(st, 767);
        cqExtrude(c, st, 517, 516, {"until": 9, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 517, "extrude");
        // step 768: guard_begin {"block":7,"g":126,"op":"guard_begin","skippable":false}
        atStep(st, 769);
        cqUnion(c, st, 518, 509, {"toUnion": {"wp": 513}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 518, "union");
        atStep(st, 770);
        cqUnion(c, st, 519, 518, {"toUnion": {"wp": 517}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 519, "union");
        // step 771: guard_end {"block":7,"g":126,"op":"guard_end"}
        // step 772: guard_begin {"block":9,"g":127,"op":"guard_begin","skippable":false}
        atStep(st, 773);
        cqSelect("Edges")(c, st, 520, 519, {"selector": undefined, "tag": undefined});
        // step 774: injected_failure {"at":774,"op":"injected_failure","was":"fillet"}
        // step 775: guard_caught {"block":9,"g":127,"op":"guard_caught"}
        // step 776: guard_begin {"before":{"wp":519},"block":8,"g":128,"op":"guard_begin","skippable":true}
        atStep(st, 777);
        cqSelect("Edges")(c, st, 522, 519, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        // step 778: injected_failure {"at":778,"op":"injected_failure","was":"fillet"}
        // step 779: guard_caught {"block":8,"g":128,"op":"guard_caught"}
        return mergeMaps(cqMeasure(c, st, {"wp": 519}), {"i": 5611});
    } catch (e) {
        return {"i": 5611, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c5611"] = case_5611();
    return out;
}
