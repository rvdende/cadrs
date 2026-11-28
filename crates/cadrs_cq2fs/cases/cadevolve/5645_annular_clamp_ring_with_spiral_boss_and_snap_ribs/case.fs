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

const case_5645 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 2);
        cqPolyline(c, st, 2, 1, {"listOfXYTuple": [[24, 0], [24, 8], [24.6, 8.05], [29.99, 8.05], [36, 10.5], [36, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 3);
        cqClose(c, st, 3, 2, {});
        atStep(st, 4);
        cqRevolve(c, st, 4, 3, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 4, "revolve");
        atStep(st, 5);
        cqWorkplaneNew(c, st, 5, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 6);
        cqCircle(c, st, 6, 5, {"radius": 28.5, "forConstruction": false});
        atStep(st, 7);
        cqExtrude(c, st, 7, 6, {"until": 15.5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 7, "extrude");
        atStep(st, 8);
        cqCut(c, st, 8, 4, {"toCut": {"wp": 7}, "clean": true, "tol": undefined});
        stepVol(c, st, 8, "cut");
        atStep(st, 9);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 10);
        cqPolyline(c, st, 10, 9, {"listOfXYTuple": [[0, 0], [41.361925626512736, -7.293223462011074], [41.361925626512736, 7.293223462011074]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 11);
        cqClose(c, st, 11, 10, {});
        atStep(st, 12);
        cqExtrude(c, st, 12, 11, {"until": 15.5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 12, "extrude");
        atStep(st, 13);
        cqCut(c, st, 13, 8, {"toCut": {"wp": 12}, "clean": true, "tol": undefined});
        stepVol(c, st, 13, "cut");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 14, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 15);
        cqWorkplane(c, st, 15, 14, {"offset": 10.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 16);
        cqCircle(c, st, 16, 15, {"radius": 3, "forConstruction": false});
        atStep(st, 17);
        cqExtrude(c, st, 17, 16, {"until": -1.5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 17, "extrude");
        atStep(st, 18);
        cqCut(c, st, 18, 13, {"toCut": {"wp": 17}, "clean": true, "tol": undefined});
        stepVol(c, st, 18, "cut");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 20);
        cqWorkplane(c, st, 20, 19, {"offset": 7.55, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 21);
        cqTransformed(c, st, 21, 20, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 22);
        cqCenter(c, st, 22, 21, {"x": 28.03, "y": 0});
        atStep(st, 23);
        cqRect(c, st, 23, 22, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 24);
        cqExtrude(c, st, 24, 23, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 24, "extrude");
        // step 25: guard_begin {"block":1,"g":1,"op":"guard_begin","skippable":false}
        // step 26: guard_caught {"block":1,"g":1,"op":"guard_caught"}
        atStep(st, 27);
        cqWorkplaneNew(c, st, 25, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 28);
        cqWorkplane(c, st, 26, 25, {"offset": 7.464893617021277, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 29);
        cqTransformed(c, st, 27, 26, {"rotate": [0, 0, 7.659574468085107], "offset": [0, 0, 0]});
        atStep(st, 30);
        cqCenter(c, st, 28, 27, {"x": 28.03, "y": 0});
        atStep(st, 31);
        cqRect(c, st, 29, 28, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 32);
        cqExtrude(c, st, 30, 29, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 30, "extrude");
        // step 33: guard_begin {"block":1,"g":2,"op":"guard_begin","skippable":false}
        // step 34: guard_caught {"block":1,"g":2,"op":"guard_caught"}
        atStep(st, 35);
        cqUnion(c, st, 31, 24, {"toUnion": {"wp": 30}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 31, "union");
        atStep(st, 36);
        cqWorkplaneNew(c, st, 32, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 37);
        cqWorkplane(c, st, 33, 32, {"offset": 7.379787234042553, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 38);
        cqTransformed(c, st, 34, 33, {"rotate": [0, 0, 15.319148936170214], "offset": [0, 0, 0]});
        atStep(st, 39);
        cqCenter(c, st, 35, 34, {"x": 28.03, "y": 0});
        atStep(st, 40);
        cqRect(c, st, 36, 35, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 41);
        cqExtrude(c, st, 37, 36, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 37, "extrude");
        // step 42: guard_begin {"block":1,"g":3,"op":"guard_begin","skippable":false}
        // step 43: guard_caught {"block":1,"g":3,"op":"guard_caught"}
        atStep(st, 44);
        cqUnion(c, st, 38, 31, {"toUnion": {"wp": 37}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 38, "union");
        atStep(st, 45);
        cqWorkplaneNew(c, st, 39, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 46);
        cqWorkplane(c, st, 40, 39, {"offset": 7.29468085106383, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 47);
        cqTransformed(c, st, 41, 40, {"rotate": [0, 0, 22.97872340425532], "offset": [0, 0, 0]});
        atStep(st, 48);
        cqCenter(c, st, 42, 41, {"x": 28.03, "y": 0});
        atStep(st, 49);
        cqRect(c, st, 43, 42, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 50);
        cqExtrude(c, st, 44, 43, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 44, "extrude");
        // step 51: guard_begin {"block":1,"g":4,"op":"guard_begin","skippable":false}
        // step 52: guard_caught {"block":1,"g":4,"op":"guard_caught"}
        atStep(st, 53);
        cqUnion(c, st, 45, 38, {"toUnion": {"wp": 44}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 45, "union");
        atStep(st, 54);
        cqWorkplaneNew(c, st, 46, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 55);
        cqWorkplane(c, st, 47, 46, {"offset": 7.209574468085107, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 56);
        cqTransformed(c, st, 48, 47, {"rotate": [0, 0, 30.638297872340427], "offset": [0, 0, 0]});
        atStep(st, 57);
        cqCenter(c, st, 49, 48, {"x": 28.03, "y": 0});
        atStep(st, 58);
        cqRect(c, st, 50, 49, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 59);
        cqExtrude(c, st, 51, 50, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 51, "extrude");
        // step 60: guard_begin {"block":1,"g":5,"op":"guard_begin","skippable":false}
        // step 61: guard_caught {"block":1,"g":5,"op":"guard_caught"}
        atStep(st, 62);
        cqUnion(c, st, 52, 45, {"toUnion": {"wp": 51}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 52, "union");
        atStep(st, 63);
        cqWorkplaneNew(c, st, 53, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 64);
        cqWorkplane(c, st, 54, 53, {"offset": 7.124468085106382, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 65);
        cqTransformed(c, st, 55, 54, {"rotate": [0, 0, 38.297872340425535], "offset": [0, 0, 0]});
        atStep(st, 66);
        cqCenter(c, st, 56, 55, {"x": 28.03, "y": 0});
        atStep(st, 67);
        cqRect(c, st, 57, 56, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 68);
        cqExtrude(c, st, 58, 57, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 58, "extrude");
        // step 69: guard_begin {"block":1,"g":6,"op":"guard_begin","skippable":false}
        // step 70: guard_caught {"block":1,"g":6,"op":"guard_caught"}
        atStep(st, 71);
        cqUnion(c, st, 59, 52, {"toUnion": {"wp": 58}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 59, "union");
        atStep(st, 72);
        cqWorkplaneNew(c, st, 60, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 73);
        cqWorkplane(c, st, 61, 60, {"offset": 7.0393617021276595, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 74);
        cqTransformed(c, st, 62, 61, {"rotate": [0, 0, 45.95744680851064], "offset": [0, 0, 0]});
        atStep(st, 75);
        cqCenter(c, st, 63, 62, {"x": 28.03, "y": 0});
        atStep(st, 76);
        cqRect(c, st, 64, 63, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 77);
        cqExtrude(c, st, 65, 64, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 65, "extrude");
        // step 78: guard_begin {"block":1,"g":7,"op":"guard_begin","skippable":false}
        // step 79: guard_caught {"block":1,"g":7,"op":"guard_caught"}
        atStep(st, 80);
        cqUnion(c, st, 66, 59, {"toUnion": {"wp": 65}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 66, "union");
        atStep(st, 81);
        cqWorkplaneNew(c, st, 67, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 82);
        cqWorkplane(c, st, 68, 67, {"offset": 6.9542553191489365, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 83);
        cqTransformed(c, st, 69, 68, {"rotate": [0, 0, 53.61702127659575], "offset": [0, 0, 0]});
        atStep(st, 84);
        cqCenter(c, st, 70, 69, {"x": 28.03, "y": 0});
        atStep(st, 85);
        cqRect(c, st, 71, 70, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 86);
        cqExtrude(c, st, 72, 71, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 72, "extrude");
        // step 87: guard_begin {"block":1,"g":8,"op":"guard_begin","skippable":false}
        // step 88: guard_caught {"block":1,"g":8,"op":"guard_caught"}
        atStep(st, 89);
        cqUnion(c, st, 73, 66, {"toUnion": {"wp": 72}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 73, "union");
        atStep(st, 90);
        cqWorkplaneNew(c, st, 74, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 91);
        cqWorkplane(c, st, 75, 74, {"offset": 6.8691489361702125, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 92);
        cqTransformed(c, st, 76, 75, {"rotate": [0, 0, 61.276595744680854], "offset": [0, 0, 0]});
        atStep(st, 93);
        cqCenter(c, st, 77, 76, {"x": 28.03, "y": 0});
        atStep(st, 94);
        cqRect(c, st, 78, 77, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 95);
        cqExtrude(c, st, 79, 78, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 79, "extrude");
        // step 96: guard_begin {"block":1,"g":9,"op":"guard_begin","skippable":false}
        // step 97: guard_caught {"block":1,"g":9,"op":"guard_caught"}
        atStep(st, 98);
        cqUnion(c, st, 80, 73, {"toUnion": {"wp": 79}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 80, "union");
        atStep(st, 99);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 100);
        cqWorkplane(c, st, 82, 81, {"offset": 6.784042553191489, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 101);
        cqTransformed(c, st, 83, 82, {"rotate": [0, 0, 68.93617021276596], "offset": [0, 0, 0]});
        atStep(st, 102);
        cqCenter(c, st, 84, 83, {"x": 28.03, "y": 0});
        atStep(st, 103);
        cqRect(c, st, 85, 84, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 104);
        cqExtrude(c, st, 86, 85, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 86, "extrude");
        // step 105: guard_begin {"block":1,"g":10,"op":"guard_begin","skippable":false}
        // step 106: guard_caught {"block":1,"g":10,"op":"guard_caught"}
        atStep(st, 107);
        cqUnion(c, st, 87, 80, {"toUnion": {"wp": 86}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 87, "union");
        atStep(st, 108);
        cqWorkplaneNew(c, st, 88, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 109);
        cqWorkplane(c, st, 89, 88, {"offset": 6.698936170212766, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 110);
        cqTransformed(c, st, 90, 89, {"rotate": [0, 0, 76.59574468085107], "offset": [0, 0, 0]});
        atStep(st, 111);
        cqCenter(c, st, 91, 90, {"x": 28.03, "y": 0});
        atStep(st, 112);
        cqRect(c, st, 92, 91, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 113);
        cqExtrude(c, st, 93, 92, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 93, "extrude");
        // step 114: guard_begin {"block":1,"g":11,"op":"guard_begin","skippable":false}
        // step 115: guard_caught {"block":1,"g":11,"op":"guard_caught"}
        atStep(st, 116);
        cqUnion(c, st, 94, 87, {"toUnion": {"wp": 93}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 94, "union");
        atStep(st, 117);
        cqWorkplaneNew(c, st, 95, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 118);
        cqWorkplane(c, st, 96, 95, {"offset": 6.613829787234042, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 119);
        cqTransformed(c, st, 97, 96, {"rotate": [0, 0, 84.25531914893618], "offset": [0, 0, 0]});
        atStep(st, 120);
        cqCenter(c, st, 98, 97, {"x": 28.03, "y": 0});
        atStep(st, 121);
        cqRect(c, st, 99, 98, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 122);
        cqExtrude(c, st, 100, 99, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 100, "extrude");
        // step 123: guard_begin {"block":1,"g":12,"op":"guard_begin","skippable":false}
        // step 124: guard_caught {"block":1,"g":12,"op":"guard_caught"}
        atStep(st, 125);
        cqUnion(c, st, 101, 94, {"toUnion": {"wp": 100}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 101, "union");
        atStep(st, 126);
        cqWorkplaneNew(c, st, 102, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 127);
        cqWorkplane(c, st, 103, 102, {"offset": 6.528723404255319, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 128);
        cqTransformed(c, st, 104, 103, {"rotate": [0, 0, 91.91489361702128], "offset": [0, 0, 0]});
        atStep(st, 129);
        cqCenter(c, st, 105, 104, {"x": 28.03, "y": 0});
        atStep(st, 130);
        cqRect(c, st, 106, 105, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 131);
        cqExtrude(c, st, 107, 106, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 107, "extrude");
        // step 132: guard_begin {"block":1,"g":13,"op":"guard_begin","skippable":false}
        // step 133: guard_caught {"block":1,"g":13,"op":"guard_caught"}
        atStep(st, 134);
        cqUnion(c, st, 108, 101, {"toUnion": {"wp": 107}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 108, "union");
        atStep(st, 135);
        cqWorkplaneNew(c, st, 109, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 136);
        cqWorkplane(c, st, 110, 109, {"offset": 6.443617021276595, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 137);
        cqTransformed(c, st, 111, 110, {"rotate": [0, 0, 99.5744680851064], "offset": [0, 0, 0]});
        atStep(st, 138);
        cqCenter(c, st, 112, 111, {"x": 28.03, "y": 0});
        atStep(st, 139);
        cqRect(c, st, 113, 112, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 140);
        cqExtrude(c, st, 114, 113, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 114, "extrude");
        // step 141: guard_begin {"block":1,"g":14,"op":"guard_begin","skippable":false}
        // step 142: guard_caught {"block":1,"g":14,"op":"guard_caught"}
        atStep(st, 143);
        cqUnion(c, st, 115, 108, {"toUnion": {"wp": 114}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 115, "union");
        atStep(st, 144);
        cqWorkplaneNew(c, st, 116, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 145);
        cqWorkplane(c, st, 117, 116, {"offset": 6.358510638297872, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 146);
        cqTransformed(c, st, 118, 117, {"rotate": [0, 0, 107.2340425531915], "offset": [0, 0, 0]});
        atStep(st, 147);
        cqCenter(c, st, 119, 118, {"x": 28.03, "y": 0});
        atStep(st, 148);
        cqRect(c, st, 120, 119, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 149);
        cqExtrude(c, st, 121, 120, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 121, "extrude");
        // step 150: guard_begin {"block":1,"g":15,"op":"guard_begin","skippable":false}
        // step 151: guard_caught {"block":1,"g":15,"op":"guard_caught"}
        atStep(st, 152);
        cqUnion(c, st, 122, 115, {"toUnion": {"wp": 121}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 122, "union");
        atStep(st, 153);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 154);
        cqWorkplane(c, st, 124, 123, {"offset": 6.273404255319149, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 155);
        cqTransformed(c, st, 125, 124, {"rotate": [0, 0, 114.8936170212766], "offset": [0, 0, 0]});
        atStep(st, 156);
        cqCenter(c, st, 126, 125, {"x": 28.03, "y": 0});
        atStep(st, 157);
        cqRect(c, st, 127, 126, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 158);
        cqExtrude(c, st, 128, 127, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 128, "extrude");
        // step 159: guard_begin {"block":1,"g":16,"op":"guard_begin","skippable":false}
        // step 160: guard_caught {"block":1,"g":16,"op":"guard_caught"}
        atStep(st, 161);
        cqUnion(c, st, 129, 122, {"toUnion": {"wp": 128}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 129, "union");
        atStep(st, 162);
        cqWorkplaneNew(c, st, 130, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 163);
        cqWorkplane(c, st, 131, 130, {"offset": 6.188297872340425, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 164);
        cqTransformed(c, st, 132, 131, {"rotate": [0, 0, 122.55319148936172], "offset": [0, 0, 0]});
        atStep(st, 165);
        cqCenter(c, st, 133, 132, {"x": 28.03, "y": 0});
        atStep(st, 166);
        cqRect(c, st, 134, 133, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 167);
        cqExtrude(c, st, 135, 134, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 135, "extrude");
        // step 168: guard_begin {"block":1,"g":17,"op":"guard_begin","skippable":false}
        // step 169: guard_caught {"block":1,"g":17,"op":"guard_caught"}
        atStep(st, 170);
        cqUnion(c, st, 136, 129, {"toUnion": {"wp": 135}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 136, "union");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 137, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 172);
        cqWorkplane(c, st, 138, 137, {"offset": 6.103191489361702, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 173);
        cqTransformed(c, st, 139, 138, {"rotate": [0, 0, 130.2127659574468], "offset": [0, 0, 0]});
        atStep(st, 174);
        cqCenter(c, st, 140, 139, {"x": 28.03, "y": 0});
        atStep(st, 175);
        cqRect(c, st, 141, 140, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 176);
        cqExtrude(c, st, 142, 141, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 142, "extrude");
        // step 177: guard_begin {"block":1,"g":18,"op":"guard_begin","skippable":false}
        // step 178: guard_caught {"block":1,"g":18,"op":"guard_caught"}
        atStep(st, 179);
        cqUnion(c, st, 143, 136, {"toUnion": {"wp": 142}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 143, "union");
        atStep(st, 180);
        cqWorkplaneNew(c, st, 144, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 181);
        cqWorkplane(c, st, 145, 144, {"offset": 6.018085106382979, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 182);
        cqTransformed(c, st, 146, 145, {"rotate": [0, 0, 137.87234042553192], "offset": [0, 0, 0]});
        atStep(st, 183);
        cqCenter(c, st, 147, 146, {"x": 28.03, "y": 0});
        atStep(st, 184);
        cqRect(c, st, 148, 147, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 185);
        cqExtrude(c, st, 149, 148, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 149, "extrude");
        // step 186: guard_begin {"block":1,"g":19,"op":"guard_begin","skippable":false}
        // step 187: guard_caught {"block":1,"g":19,"op":"guard_caught"}
        atStep(st, 188);
        cqUnion(c, st, 150, 143, {"toUnion": {"wp": 149}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 150, "union");
        atStep(st, 189);
        cqWorkplaneNew(c, st, 151, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 190);
        cqWorkplane(c, st, 152, 151, {"offset": 5.932978723404255, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 191);
        cqTransformed(c, st, 153, 152, {"rotate": [0, 0, 145.53191489361703], "offset": [0, 0, 0]});
        atStep(st, 192);
        cqCenter(c, st, 154, 153, {"x": 28.03, "y": 0});
        atStep(st, 193);
        cqRect(c, st, 155, 154, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 194);
        cqExtrude(c, st, 156, 155, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 156, "extrude");
        // step 195: guard_begin {"block":1,"g":20,"op":"guard_begin","skippable":false}
        // step 196: guard_caught {"block":1,"g":20,"op":"guard_caught"}
        atStep(st, 197);
        cqUnion(c, st, 157, 150, {"toUnion": {"wp": 156}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 157, "union");
        atStep(st, 198);
        cqWorkplaneNew(c, st, 158, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 199);
        cqWorkplane(c, st, 159, 158, {"offset": 5.847872340425532, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 200);
        cqTransformed(c, st, 160, 159, {"rotate": [0, 0, 153.19148936170214], "offset": [0, 0, 0]});
        atStep(st, 201);
        cqCenter(c, st, 161, 160, {"x": 28.03, "y": 0});
        atStep(st, 202);
        cqRect(c, st, 162, 161, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 203);
        cqExtrude(c, st, 163, 162, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 163, "extrude");
        // step 204: guard_begin {"block":1,"g":21,"op":"guard_begin","skippable":false}
        // step 205: guard_caught {"block":1,"g":21,"op":"guard_caught"}
        atStep(st, 206);
        cqUnion(c, st, 164, 157, {"toUnion": {"wp": 163}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 164, "union");
        atStep(st, 207);
        cqWorkplaneNew(c, st, 165, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 208);
        cqWorkplane(c, st, 166, 165, {"offset": 5.762765957446809, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 209);
        cqTransformed(c, st, 167, 166, {"rotate": [0, 0, 160.85106382978725], "offset": [0, 0, 0]});
        atStep(st, 210);
        cqCenter(c, st, 168, 167, {"x": 28.03, "y": 0});
        atStep(st, 211);
        cqRect(c, st, 169, 168, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 212);
        cqExtrude(c, st, 170, 169, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 170, "extrude");
        // step 213: guard_begin {"block":1,"g":22,"op":"guard_begin","skippable":false}
        // step 214: guard_caught {"block":1,"g":22,"op":"guard_caught"}
        atStep(st, 215);
        cqUnion(c, st, 171, 164, {"toUnion": {"wp": 170}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 171, "union");
        atStep(st, 216);
        cqWorkplaneNew(c, st, 172, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 217);
        cqWorkplane(c, st, 173, 172, {"offset": 5.677659574468085, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 218);
        cqTransformed(c, st, 174, 173, {"rotate": [0, 0, 168.51063829787236], "offset": [0, 0, 0]});
        atStep(st, 219);
        cqCenter(c, st, 175, 174, {"x": 28.03, "y": 0});
        atStep(st, 220);
        cqRect(c, st, 176, 175, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 221);
        cqExtrude(c, st, 177, 176, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 177, "extrude");
        // step 222: guard_begin {"block":1,"g":23,"op":"guard_begin","skippable":false}
        // step 223: guard_caught {"block":1,"g":23,"op":"guard_caught"}
        atStep(st, 224);
        cqUnion(c, st, 178, 171, {"toUnion": {"wp": 177}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 178, "union");
        atStep(st, 225);
        cqWorkplaneNew(c, st, 179, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 226);
        cqWorkplane(c, st, 180, 179, {"offset": 5.592553191489362, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 227);
        cqTransformed(c, st, 181, 180, {"rotate": [0, 0, 176.17021276595744], "offset": [0, 0, 0]});
        atStep(st, 228);
        cqCenter(c, st, 182, 181, {"x": 28.03, "y": 0});
        atStep(st, 229);
        cqRect(c, st, 183, 182, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 230);
        cqExtrude(c, st, 184, 183, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 184, "extrude");
        // step 231: guard_begin {"block":1,"g":24,"op":"guard_begin","skippable":false}
        // step 232: guard_caught {"block":1,"g":24,"op":"guard_caught"}
        atStep(st, 233);
        cqUnion(c, st, 185, 178, {"toUnion": {"wp": 184}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 185, "union");
        atStep(st, 234);
        cqWorkplaneNew(c, st, 186, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 235);
        cqWorkplane(c, st, 187, 186, {"offset": 5.507446808510639, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 236);
        cqTransformed(c, st, 188, 187, {"rotate": [0, 0, 183.82978723404256], "offset": [0, 0, 0]});
        atStep(st, 237);
        cqCenter(c, st, 189, 188, {"x": 28.03, "y": 0});
        atStep(st, 238);
        cqRect(c, st, 190, 189, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 239);
        cqExtrude(c, st, 191, 190, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 191, "extrude");
        // step 240: guard_begin {"block":1,"g":25,"op":"guard_begin","skippable":false}
        // step 241: guard_caught {"block":1,"g":25,"op":"guard_caught"}
        atStep(st, 242);
        cqUnion(c, st, 192, 185, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        atStep(st, 243);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 244);
        cqWorkplane(c, st, 194, 193, {"offset": 5.422340425531915, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 245);
        cqTransformed(c, st, 195, 194, {"rotate": [0, 0, 191.48936170212767], "offset": [0, 0, 0]});
        atStep(st, 246);
        cqCenter(c, st, 196, 195, {"x": 28.03, "y": 0});
        atStep(st, 247);
        cqRect(c, st, 197, 196, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 248);
        cqExtrude(c, st, 198, 197, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 198, "extrude");
        // step 249: guard_begin {"block":1,"g":26,"op":"guard_begin","skippable":false}
        // step 250: guard_caught {"block":1,"g":26,"op":"guard_caught"}
        atStep(st, 251);
        cqUnion(c, st, 199, 192, {"toUnion": {"wp": 198}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 199, "union");
        atStep(st, 252);
        cqWorkplaneNew(c, st, 200, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 253);
        cqWorkplane(c, st, 201, 200, {"offset": 5.337234042553191, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 254);
        cqTransformed(c, st, 202, 201, {"rotate": [0, 0, 199.1489361702128], "offset": [0, 0, 0]});
        atStep(st, 255);
        cqCenter(c, st, 203, 202, {"x": 28.03, "y": 0});
        atStep(st, 256);
        cqRect(c, st, 204, 203, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 257);
        cqExtrude(c, st, 205, 204, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 205, "extrude");
        // step 258: guard_begin {"block":1,"g":27,"op":"guard_begin","skippable":false}
        // step 259: guard_caught {"block":1,"g":27,"op":"guard_caught"}
        atStep(st, 260);
        cqUnion(c, st, 206, 199, {"toUnion": {"wp": 205}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 206, "union");
        atStep(st, 261);
        cqWorkplaneNew(c, st, 207, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 262);
        cqWorkplane(c, st, 208, 207, {"offset": 5.252127659574468, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 263);
        cqTransformed(c, st, 209, 208, {"rotate": [0, 0, 206.8085106382979], "offset": [0, 0, 0]});
        atStep(st, 264);
        cqCenter(c, st, 210, 209, {"x": 28.03, "y": 0});
        atStep(st, 265);
        cqRect(c, st, 211, 210, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 266);
        cqExtrude(c, st, 212, 211, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 212, "extrude");
        // step 267: guard_begin {"block":1,"g":28,"op":"guard_begin","skippable":false}
        // step 268: guard_caught {"block":1,"g":28,"op":"guard_caught"}
        atStep(st, 269);
        cqUnion(c, st, 213, 206, {"toUnion": {"wp": 212}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 213, "union");
        atStep(st, 270);
        cqWorkplaneNew(c, st, 214, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 271);
        cqWorkplane(c, st, 215, 214, {"offset": 5.167021276595745, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 272);
        cqTransformed(c, st, 216, 215, {"rotate": [0, 0, 214.468085106383], "offset": [0, 0, 0]});
        atStep(st, 273);
        cqCenter(c, st, 217, 216, {"x": 28.03, "y": 0});
        atStep(st, 274);
        cqRect(c, st, 218, 217, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 275);
        cqExtrude(c, st, 219, 218, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 219, "extrude");
        // step 276: guard_begin {"block":1,"g":29,"op":"guard_begin","skippable":false}
        // step 277: guard_caught {"block":1,"g":29,"op":"guard_caught"}
        atStep(st, 278);
        cqUnion(c, st, 220, 213, {"toUnion": {"wp": 219}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 220, "union");
        atStep(st, 279);
        cqWorkplaneNew(c, st, 221, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 280);
        cqWorkplane(c, st, 222, 221, {"offset": 5.081914893617021, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 281);
        cqTransformed(c, st, 223, 222, {"rotate": [0, 0, 222.12765957446808], "offset": [0, 0, 0]});
        atStep(st, 282);
        cqCenter(c, st, 224, 223, {"x": 28.03, "y": 0});
        atStep(st, 283);
        cqRect(c, st, 225, 224, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 284);
        cqExtrude(c, st, 226, 225, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 226, "extrude");
        // step 285: guard_begin {"block":1,"g":30,"op":"guard_begin","skippable":false}
        // step 286: guard_caught {"block":1,"g":30,"op":"guard_caught"}
        atStep(st, 287);
        cqUnion(c, st, 227, 220, {"toUnion": {"wp": 226}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 227, "union");
        atStep(st, 288);
        cqWorkplaneNew(c, st, 228, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 289);
        cqWorkplane(c, st, 229, 228, {"offset": 4.996808510638298, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 290);
        cqTransformed(c, st, 230, 229, {"rotate": [0, 0, 229.7872340425532], "offset": [0, 0, 0]});
        atStep(st, 291);
        cqCenter(c, st, 231, 230, {"x": 28.03, "y": 0});
        atStep(st, 292);
        cqRect(c, st, 232, 231, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 293);
        cqExtrude(c, st, 233, 232, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 233, "extrude");
        // step 294: guard_begin {"block":1,"g":31,"op":"guard_begin","skippable":false}
        // step 295: guard_caught {"block":1,"g":31,"op":"guard_caught"}
        atStep(st, 296);
        cqUnion(c, st, 234, 227, {"toUnion": {"wp": 233}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 234, "union");
        atStep(st, 297);
        cqWorkplaneNew(c, st, 235, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 298);
        cqWorkplane(c, st, 236, 235, {"offset": 4.9117021276595745, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 299);
        cqTransformed(c, st, 237, 236, {"rotate": [0, 0, 237.4468085106383], "offset": [0, 0, 0]});
        atStep(st, 300);
        cqCenter(c, st, 238, 237, {"x": 28.03, "y": 0});
        atStep(st, 301);
        cqRect(c, st, 239, 238, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 302);
        cqExtrude(c, st, 240, 239, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 240, "extrude");
        // step 303: guard_begin {"block":1,"g":32,"op":"guard_begin","skippable":false}
        // step 304: guard_caught {"block":1,"g":32,"op":"guard_caught"}
        atStep(st, 305);
        cqUnion(c, st, 241, 234, {"toUnion": {"wp": 240}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 241, "union");
        atStep(st, 306);
        cqWorkplaneNew(c, st, 242, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 307);
        cqWorkplane(c, st, 243, 242, {"offset": 4.8265957446808505, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 308);
        cqTransformed(c, st, 244, 243, {"rotate": [0, 0, 245.10638297872345], "offset": [0, 0, 0]});
        atStep(st, 309);
        cqCenter(c, st, 245, 244, {"x": 28.03, "y": 0});
        atStep(st, 310);
        cqRect(c, st, 246, 245, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 311);
        cqExtrude(c, st, 247, 246, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 247, "extrude");
        // step 312: guard_begin {"block":1,"g":33,"op":"guard_begin","skippable":false}
        // step 313: guard_caught {"block":1,"g":33,"op":"guard_caught"}
        atStep(st, 314);
        cqUnion(c, st, 248, 241, {"toUnion": {"wp": 247}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 248, "union");
        atStep(st, 315);
        cqWorkplaneNew(c, st, 249, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 316);
        cqWorkplane(c, st, 250, 249, {"offset": 4.741489361702127, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 317);
        cqTransformed(c, st, 251, 250, {"rotate": [0, 0, 252.76595744680853], "offset": [0, 0, 0]});
        atStep(st, 318);
        cqCenter(c, st, 252, 251, {"x": 28.03, "y": 0});
        atStep(st, 319);
        cqRect(c, st, 253, 252, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 320);
        cqExtrude(c, st, 254, 253, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        // step 321: guard_begin {"block":1,"g":34,"op":"guard_begin","skippable":false}
        // step 322: guard_caught {"block":1,"g":34,"op":"guard_caught"}
        atStep(st, 323);
        cqUnion(c, st, 255, 248, {"toUnion": {"wp": 254}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 255, "union");
        atStep(st, 324);
        cqWorkplaneNew(c, st, 256, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 325);
        cqWorkplane(c, st, 257, 256, {"offset": 4.656382978723404, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 326);
        cqTransformed(c, st, 258, 257, {"rotate": [0, 0, 260.4255319148936], "offset": [0, 0, 0]});
        atStep(st, 327);
        cqCenter(c, st, 259, 258, {"x": 28.03, "y": 0});
        atStep(st, 328);
        cqRect(c, st, 260, 259, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 329);
        cqExtrude(c, st, 261, 260, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 261, "extrude");
        // step 330: guard_begin {"block":1,"g":35,"op":"guard_begin","skippable":false}
        // step 331: guard_caught {"block":1,"g":35,"op":"guard_caught"}
        atStep(st, 332);
        cqUnion(c, st, 262, 255, {"toUnion": {"wp": 261}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 262, "union");
        atStep(st, 333);
        cqWorkplaneNew(c, st, 263, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 334);
        cqWorkplane(c, st, 264, 263, {"offset": 4.57127659574468, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 335);
        cqTransformed(c, st, 265, 264, {"rotate": [0, 0, 268.0851063829787], "offset": [0, 0, 0]});
        atStep(st, 336);
        cqCenter(c, st, 266, 265, {"x": 28.03, "y": 0});
        atStep(st, 337);
        cqRect(c, st, 267, 266, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 338);
        cqExtrude(c, st, 268, 267, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 268, "extrude");
        // step 339: guard_begin {"block":1,"g":36,"op":"guard_begin","skippable":false}
        // step 340: guard_caught {"block":1,"g":36,"op":"guard_caught"}
        atStep(st, 341);
        cqUnion(c, st, 269, 262, {"toUnion": {"wp": 268}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 269, "union");
        atStep(st, 342);
        cqWorkplaneNew(c, st, 270, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 343);
        cqWorkplane(c, st, 271, 270, {"offset": 4.486170212765957, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 344);
        cqTransformed(c, st, 272, 271, {"rotate": [0, 0, 275.74468085106383], "offset": [0, 0, 0]});
        atStep(st, 345);
        cqCenter(c, st, 273, 272, {"x": 28.03, "y": 0});
        atStep(st, 346);
        cqRect(c, st, 274, 273, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 347);
        cqExtrude(c, st, 275, 274, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 275, "extrude");
        // step 348: guard_begin {"block":1,"g":37,"op":"guard_begin","skippable":false}
        // step 349: guard_caught {"block":1,"g":37,"op":"guard_caught"}
        atStep(st, 350);
        cqUnion(c, st, 276, 269, {"toUnion": {"wp": 275}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 276, "union");
        atStep(st, 351);
        cqWorkplaneNew(c, st, 277, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 352);
        cqWorkplane(c, st, 278, 277, {"offset": 4.401063829787234, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 353);
        cqTransformed(c, st, 279, 278, {"rotate": [0, 0, 283.40425531914894], "offset": [0, 0, 0]});
        atStep(st, 354);
        cqCenter(c, st, 280, 279, {"x": 28.03, "y": 0});
        atStep(st, 355);
        cqRect(c, st, 281, 280, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 356);
        cqExtrude(c, st, 282, 281, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 282, "extrude");
        // step 357: guard_begin {"block":1,"g":38,"op":"guard_begin","skippable":false}
        // step 358: guard_caught {"block":1,"g":38,"op":"guard_caught"}
        atStep(st, 359);
        cqUnion(c, st, 283, 276, {"toUnion": {"wp": 282}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 283, "union");
        atStep(st, 360);
        cqWorkplaneNew(c, st, 284, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 361);
        cqWorkplane(c, st, 285, 284, {"offset": 4.31595744680851, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 362);
        cqTransformed(c, st, 286, 285, {"rotate": [0, 0, 291.06382978723406], "offset": [0, 0, 0]});
        atStep(st, 363);
        cqCenter(c, st, 287, 286, {"x": 28.03, "y": 0});
        atStep(st, 364);
        cqRect(c, st, 288, 287, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 365);
        cqExtrude(c, st, 289, 288, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 289, "extrude");
        // step 366: guard_begin {"block":1,"g":39,"op":"guard_begin","skippable":false}
        // step 367: guard_caught {"block":1,"g":39,"op":"guard_caught"}
        atStep(st, 368);
        cqUnion(c, st, 290, 283, {"toUnion": {"wp": 289}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 290, "union");
        atStep(st, 369);
        cqWorkplaneNew(c, st, 291, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 370);
        cqWorkplane(c, st, 292, 291, {"offset": 4.230851063829787, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 371);
        cqTransformed(c, st, 293, 292, {"rotate": [0, 0, 298.72340425531917], "offset": [0, 0, 0]});
        atStep(st, 372);
        cqCenter(c, st, 294, 293, {"x": 28.03, "y": 0});
        atStep(st, 373);
        cqRect(c, st, 295, 294, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 374);
        cqExtrude(c, st, 296, 295, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 296, "extrude");
        // step 375: guard_begin {"block":1,"g":40,"op":"guard_begin","skippable":false}
        // step 376: guard_caught {"block":1,"g":40,"op":"guard_caught"}
        atStep(st, 377);
        cqUnion(c, st, 297, 290, {"toUnion": {"wp": 296}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 297, "union");
        atStep(st, 378);
        cqWorkplaneNew(c, st, 298, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 379);
        cqWorkplane(c, st, 299, 298, {"offset": 4.145744680851064, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 380);
        cqTransformed(c, st, 300, 299, {"rotate": [0, 0, 306.3829787234043], "offset": [0, 0, 0]});
        atStep(st, 381);
        cqCenter(c, st, 301, 300, {"x": 28.03, "y": 0});
        atStep(st, 382);
        cqRect(c, st, 302, 301, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 383);
        cqExtrude(c, st, 303, 302, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 303, "extrude");
        // step 384: guard_begin {"block":1,"g":41,"op":"guard_begin","skippable":false}
        // step 385: guard_caught {"block":1,"g":41,"op":"guard_caught"}
        atStep(st, 386);
        cqUnion(c, st, 304, 297, {"toUnion": {"wp": 303}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 304, "union");
        atStep(st, 387);
        cqWorkplaneNew(c, st, 305, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 388);
        cqWorkplane(c, st, 306, 305, {"offset": 4.06063829787234, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 389);
        cqTransformed(c, st, 307, 306, {"rotate": [0, 0, 314.0425531914894], "offset": [0, 0, 0]});
        atStep(st, 390);
        cqCenter(c, st, 308, 307, {"x": 28.03, "y": 0});
        atStep(st, 391);
        cqRect(c, st, 309, 308, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 392);
        cqExtrude(c, st, 310, 309, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 310, "extrude");
        // step 393: guard_begin {"block":1,"g":42,"op":"guard_begin","skippable":false}
        // step 394: guard_caught {"block":1,"g":42,"op":"guard_caught"}
        atStep(st, 395);
        cqUnion(c, st, 311, 304, {"toUnion": {"wp": 310}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 311, "union");
        atStep(st, 396);
        cqWorkplaneNew(c, st, 312, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 397);
        cqWorkplane(c, st, 313, 312, {"offset": 3.975531914893617, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 398);
        cqTransformed(c, st, 314, 313, {"rotate": [0, 0, 321.7021276595745], "offset": [0, 0, 0]});
        atStep(st, 399);
        cqCenter(c, st, 315, 314, {"x": 28.03, "y": 0});
        atStep(st, 400);
        cqRect(c, st, 316, 315, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 401);
        cqExtrude(c, st, 317, 316, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 317, "extrude");
        // step 402: guard_begin {"block":1,"g":43,"op":"guard_begin","skippable":false}
        // step 403: guard_caught {"block":1,"g":43,"op":"guard_caught"}
        atStep(st, 404);
        cqUnion(c, st, 318, 311, {"toUnion": {"wp": 317}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 318, "union");
        atStep(st, 405);
        cqWorkplaneNew(c, st, 319, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 406);
        cqWorkplane(c, st, 320, 319, {"offset": 3.890425531914893, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 407);
        cqTransformed(c, st, 321, 320, {"rotate": [0, 0, 329.3617021276596], "offset": [0, 0, 0]});
        atStep(st, 408);
        cqCenter(c, st, 322, 321, {"x": 28.03, "y": 0});
        atStep(st, 409);
        cqRect(c, st, 323, 322, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 410);
        cqExtrude(c, st, 324, 323, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 324, "extrude");
        // step 411: guard_begin {"block":1,"g":44,"op":"guard_begin","skippable":false}
        // step 412: guard_caught {"block":1,"g":44,"op":"guard_caught"}
        atStep(st, 413);
        cqUnion(c, st, 325, 318, {"toUnion": {"wp": 324}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 325, "union");
        atStep(st, 414);
        cqWorkplaneNew(c, st, 326, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 415);
        cqWorkplane(c, st, 327, 326, {"offset": 3.80531914893617, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 416);
        cqTransformed(c, st, 328, 327, {"rotate": [0, 0, 337.0212765957447], "offset": [0, 0, 0]});
        atStep(st, 417);
        cqCenter(c, st, 329, 328, {"x": 28.03, "y": 0});
        atStep(st, 418);
        cqRect(c, st, 330, 329, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 419);
        cqExtrude(c, st, 331, 330, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 331, "extrude");
        // step 420: guard_begin {"block":1,"g":45,"op":"guard_begin","skippable":false}
        // step 421: guard_caught {"block":1,"g":45,"op":"guard_caught"}
        atStep(st, 422);
        cqUnion(c, st, 332, 325, {"toUnion": {"wp": 331}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 332, "union");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 424);
        cqWorkplane(c, st, 334, 333, {"offset": 3.720212765957447, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 425);
        cqTransformed(c, st, 335, 334, {"rotate": [0, 0, 344.6808510638298], "offset": [0, 0, 0]});
        atStep(st, 426);
        cqCenter(c, st, 336, 335, {"x": 28.03, "y": 0});
        atStep(st, 427);
        cqRect(c, st, 337, 336, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 428);
        cqExtrude(c, st, 338, 337, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 338, "extrude");
        // step 429: guard_begin {"block":1,"g":46,"op":"guard_begin","skippable":false}
        // step 430: guard_caught {"block":1,"g":46,"op":"guard_caught"}
        atStep(st, 431);
        cqUnion(c, st, 339, 332, {"toUnion": {"wp": 338}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 339, "union");
        atStep(st, 432);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 433);
        cqWorkplane(c, st, 341, 340, {"offset": 3.6351063829787233, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 434);
        cqTransformed(c, st, 342, 341, {"rotate": [0, 0, 352.3404255319149], "offset": [0, 0, 0]});
        atStep(st, 435);
        cqCenter(c, st, 343, 342, {"x": 28.03, "y": 0});
        atStep(st, 436);
        cqRect(c, st, 344, 343, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 437);
        cqExtrude(c, st, 345, 344, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 345, "extrude");
        // step 438: guard_begin {"block":1,"g":47,"op":"guard_begin","skippable":false}
        // step 439: guard_caught {"block":1,"g":47,"op":"guard_caught"}
        atStep(st, 440);
        cqUnion(c, st, 346, 339, {"toUnion": {"wp": 345}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 346, "union");
        atStep(st, 441);
        cqWorkplaneNew(c, st, 347, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 442);
        cqWorkplane(c, st, 348, 347, {"offset": 3.55, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 443);
        cqTransformed(c, st, 349, 348, {"rotate": [0, 0, 360], "offset": [0, 0, 0]});
        atStep(st, 444);
        cqCenter(c, st, 350, 349, {"x": 28.03, "y": 0});
        atStep(st, 445);
        cqRect(c, st, 351, 350, {"xLen": 0.9, "yLen": 1.4, "centered": true, "forConstruction": false});
        atStep(st, 446);
        cqExtrude(c, st, 352, 351, {"until": 0.6, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 352, "extrude");
        // step 447: guard_begin {"block":1,"g":48,"op":"guard_begin","skippable":false}
        // step 448: guard_caught {"block":1,"g":48,"op":"guard_caught"}
        atStep(st, 449);
        cqUnion(c, st, 353, 346, {"toUnion": {"wp": 352}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 353, "union");
        // step 450: guard_begin {"before":{"wp":353},"block":2,"g":49,"op":"guard_begin","skippable":true}
        // step 451: guard_caught {"block":2,"g":49,"op":"guard_caught"}
        atStep(st, 452);
        cqUnion(c, st, 354, 18, {"toUnion": {"wp": 353}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 354, "union");
        atStep(st, 453);
        cqWorkplaneNew(c, st, 355, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 454);
        cqWorkplane(c, st, 356, 355, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 455);
        cqTransformed(c, st, 357, 356, {"rotate": [0, 0, -105], "offset": [0, 0, 0]});
        atStep(st, 456);
        cqCenter(c, st, 358, 357, {"x": 33, "y": 0});
        atStep(st, 457);
        cqRect(c, st, 359, 358, {"xLen": 1.2, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 458);
        cqExtrude(c, st, 360, 359, {"until": 1, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 360, "extrude");
        // step 459: guard_begin {"block":3,"g":50,"op":"guard_begin","skippable":false}
        // step 460: guard_caught {"block":3,"g":50,"op":"guard_caught"}
        atStep(st, 461);
        cqWorkplaneNew(c, st, 361, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 462);
        cqWorkplane(c, st, 362, 361, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 463);
        cqTransformed(c, st, 363, 362, {"rotate": [0, 0, -97.5], "offset": [0, 0, 0]});
        atStep(st, 464);
        cqCenter(c, st, 364, 363, {"x": 33, "y": 0});
        atStep(st, 465);
        cqRect(c, st, 365, 364, {"xLen": 1.2, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 466);
        cqExtrude(c, st, 366, 365, {"until": 1, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 366, "extrude");
        // step 467: guard_begin {"block":3,"g":51,"op":"guard_begin","skippable":false}
        // step 468: guard_caught {"block":3,"g":51,"op":"guard_caught"}
        atStep(st, 469);
        cqUnion(c, st, 367, 360, {"toUnion": {"wp": 366}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 367, "union");
        atStep(st, 470);
        cqWorkplaneNew(c, st, 368, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 471);
        cqWorkplane(c, st, 369, 368, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 472);
        cqTransformed(c, st, 370, 369, {"rotate": [0, 0, -90], "offset": [0, 0, 0]});
        atStep(st, 473);
        cqCenter(c, st, 371, 370, {"x": 33, "y": 0});
        atStep(st, 474);
        cqRect(c, st, 372, 371, {"xLen": 1.2, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 475);
        cqExtrude(c, st, 373, 372, {"until": 1, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 373, "extrude");
        // step 476: guard_begin {"block":3,"g":52,"op":"guard_begin","skippable":false}
        // step 477: guard_caught {"block":3,"g":52,"op":"guard_caught"}
        atStep(st, 478);
        cqUnion(c, st, 374, 367, {"toUnion": {"wp": 373}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 374, "union");
        atStep(st, 479);
        cqWorkplaneNew(c, st, 375, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 480);
        cqWorkplane(c, st, 376, 375, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 481);
        cqTransformed(c, st, 377, 376, {"rotate": [0, 0, -82.5], "offset": [0, 0, 0]});
        atStep(st, 482);
        cqCenter(c, st, 378, 377, {"x": 33, "y": 0});
        atStep(st, 483);
        cqRect(c, st, 379, 378, {"xLen": 1.2, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 484);
        cqExtrude(c, st, 380, 379, {"until": 1, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 380, "extrude");
        // step 485: guard_begin {"block":3,"g":53,"op":"guard_begin","skippable":false}
        // step 486: guard_caught {"block":3,"g":53,"op":"guard_caught"}
        atStep(st, 487);
        cqUnion(c, st, 381, 374, {"toUnion": {"wp": 380}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 381, "union");
        atStep(st, 488);
        cqWorkplaneNew(c, st, 382, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 489);
        cqWorkplane(c, st, 383, 382, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 490);
        cqTransformed(c, st, 384, 383, {"rotate": [0, 0, -75], "offset": [0, 0, 0]});
        atStep(st, 491);
        cqCenter(c, st, 385, 384, {"x": 33, "y": 0});
        atStep(st, 492);
        cqRect(c, st, 386, 385, {"xLen": 1.2, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 493);
        cqExtrude(c, st, 387, 386, {"until": 1, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 387, "extrude");
        // step 494: guard_begin {"block":3,"g":54,"op":"guard_begin","skippable":false}
        // step 495: guard_caught {"block":3,"g":54,"op":"guard_caught"}
        atStep(st, 496);
        cqUnion(c, st, 388, 381, {"toUnion": {"wp": 387}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 388, "union");
        // step 497: guard_begin {"before":{"wp":388},"block":4,"g":55,"op":"guard_begin","skippable":true}
        // step 498: guard_caught {"block":4,"g":55,"op":"guard_caught"}
        atStep(st, 499);
        cqUnion(c, st, 389, 354, {"toUnion": {"wp": 388}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 389, "union");
        atStep(st, 500);
        cqWorkplaneNew(c, st, 390, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 501);
        cqWorkplane(c, st, 391, 390, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 502);
        cqTransformed(c, st, 392, 391, {"rotate": [0, 0, 0], "offset": [0, 0, 0]});
        atStep(st, 503);
        cqCenter(c, st, 393, 392, {"x": 35.5, "y": 0});
        atStep(st, 504);
        cqRect(c, st, 394, 393, {"xLen": 1, "yLen": 6, "centered": true, "forConstruction": false});
        atStep(st, 505);
        cqExtrude(c, st, 395, 394, {"until": -4.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 395, "extrude");
        atStep(st, 506);
        cqWorkplaneNew(c, st, 396, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 507);
        cqWorkplane(c, st, 397, 396, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 508);
        cqTransformed(c, st, 398, 397, {"rotate": [0, 0, 45], "offset": [0, 0, 0]});
        atStep(st, 509);
        cqCenter(c, st, 399, 398, {"x": 35.5, "y": 0});
        atStep(st, 510);
        cqRect(c, st, 400, 399, {"xLen": 1, "yLen": 6, "centered": true, "forConstruction": false});
        atStep(st, 511);
        cqExtrude(c, st, 401, 400, {"until": -4.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 401, "extrude");
        atStep(st, 512);
        cqUnion(c, st, 402, 395, {"toUnion": {"wp": 401}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 402, "union");
        atStep(st, 513);
        cqWorkplaneNew(c, st, 403, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 514);
        cqWorkplane(c, st, 404, 403, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 515);
        cqTransformed(c, st, 405, 404, {"rotate": [0, 0, 90], "offset": [0, 0, 0]});
        atStep(st, 516);
        cqCenter(c, st, 406, 405, {"x": 35.5, "y": 0});
        atStep(st, 517);
        cqRect(c, st, 407, 406, {"xLen": 1, "yLen": 6, "centered": true, "forConstruction": false});
        atStep(st, 518);
        cqExtrude(c, st, 408, 407, {"until": -4.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 408, "extrude");
        atStep(st, 519);
        cqUnion(c, st, 409, 402, {"toUnion": {"wp": 408}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 409, "union");
        atStep(st, 520);
        cqWorkplaneNew(c, st, 410, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 521);
        cqWorkplane(c, st, 411, 410, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 522);
        cqTransformed(c, st, 412, 411, {"rotate": [0, 0, 135], "offset": [0, 0, 0]});
        atStep(st, 523);
        cqCenter(c, st, 413, 412, {"x": 35.5, "y": 0});
        atStep(st, 524);
        cqRect(c, st, 414, 413, {"xLen": 1, "yLen": 6, "centered": true, "forConstruction": false});
        atStep(st, 525);
        cqExtrude(c, st, 415, 414, {"until": -4.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 415, "extrude");
        atStep(st, 526);
        cqUnion(c, st, 416, 409, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        atStep(st, 527);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 528);
        cqWorkplane(c, st, 418, 417, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 529);
        cqTransformed(c, st, 419, 418, {"rotate": [0, 0, 180], "offset": [0, 0, 0]});
        atStep(st, 530);
        cqCenter(c, st, 420, 419, {"x": 35.5, "y": 0});
        atStep(st, 531);
        cqRect(c, st, 421, 420, {"xLen": 1, "yLen": 6, "centered": true, "forConstruction": false});
        atStep(st, 532);
        cqExtrude(c, st, 422, 421, {"until": -4.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 422, "extrude");
        atStep(st, 533);
        cqUnion(c, st, 423, 416, {"toUnion": {"wp": 422}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 423, "union");
        atStep(st, 534);
        cqWorkplaneNew(c, st, 424, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 535);
        cqWorkplane(c, st, 425, 424, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 536);
        cqTransformed(c, st, 426, 425, {"rotate": [0, 0, 225], "offset": [0, 0, 0]});
        atStep(st, 537);
        cqCenter(c, st, 427, 426, {"x": 35.5, "y": 0});
        atStep(st, 538);
        cqRect(c, st, 428, 427, {"xLen": 1, "yLen": 6, "centered": true, "forConstruction": false});
        atStep(st, 539);
        cqExtrude(c, st, 429, 428, {"until": -4.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 429, "extrude");
        atStep(st, 540);
        cqUnion(c, st, 430, 423, {"toUnion": {"wp": 429}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 430, "union");
        atStep(st, 541);
        cqWorkplaneNew(c, st, 431, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 542);
        cqWorkplane(c, st, 432, 431, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 543);
        cqTransformed(c, st, 433, 432, {"rotate": [0, 0, 270], "offset": [0, 0, 0]});
        atStep(st, 544);
        cqCenter(c, st, 434, 433, {"x": 35.5, "y": 0});
        atStep(st, 545);
        cqRect(c, st, 435, 434, {"xLen": 1, "yLen": 6, "centered": true, "forConstruction": false});
        atStep(st, 546);
        cqExtrude(c, st, 436, 435, {"until": -4.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 436, "extrude");
        atStep(st, 547);
        cqUnion(c, st, 437, 430, {"toUnion": {"wp": 436}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 437, "union");
        atStep(st, 548);
        cqWorkplaneNew(c, st, 438, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 549);
        cqWorkplane(c, st, 439, 438, {"offset": 10, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 550);
        cqTransformed(c, st, 440, 439, {"rotate": [0, 0, 315], "offset": [0, 0, 0]});
        atStep(st, 551);
        cqCenter(c, st, 441, 440, {"x": 35.5, "y": 0});
        atStep(st, 552);
        cqRect(c, st, 442, 441, {"xLen": 1, "yLen": 6, "centered": true, "forConstruction": false});
        atStep(st, 553);
        cqExtrude(c, st, 443, 442, {"until": -4.5, "combine": false, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 443, "extrude");
        atStep(st, 554);
        cqUnion(c, st, 444, 437, {"toUnion": {"wp": 443}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 444, "union");
        atStep(st, 555);
        cqCut(c, st, 445, 389, {"toCut": {"wp": 444}, "clean": true, "tol": undefined});
        stepVol(c, st, 445, "cut");
        // step 556: guard_begin {"block":5,"g":56,"op":"guard_begin","skippable":false}
        atStep(st, 557);
        cqSelect("Faces")(c, st, 446, 445, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 558);
        cqWorkplane(c, st, 447, 446, {"offset": 0, "invert": false, "centerOption": "CenterOfMass", "origin": undefined});
        atStep(st, 559);
        cqPushPoints(c, st, 448, 447, {"pntList": [[33, 0], [16.500000000000004, 28.578838324886473], [-16.499999999999993, 28.578838324886476], [-33, 4.0413344371862654e-15], [-16.500000000000014, -28.578838324886465], [16.500000000000004, -28.578838324886473]]});
        atStep(st, 560);
        cqCboreHole(c, st, 449, 448, {"diameter": 3, "cboreDiameter": 6, "cboreDepth": 1.5, "depth": undefined, "clean": true});
        stepVol(c, st, 449, "cboreHole");
        // step 561: guard_end {"block":5,"g":56,"op":"guard_end"}
        // step 562: guard_begin {"before":{"wp":449},"block":6,"g":57,"op":"guard_begin","skippable":true}
        // step 563: guard_caught {"block":6,"g":57,"op":"guard_caught"}
        // step 564: guard_begin {"block":7,"g":58,"op":"guard_begin","skippable":false}
        atStep(st, 565);
        cqSelect("Edges")(c, st, 450, 449, {"selector": {"a": {"a": {"k": "par", "v": [0, 0, 1]}, "b": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "k": "or"}, "b": {"k": "perp", "v": [0, 0, 1]}, "k": "or"}, "tag": undefined});
        atStep(st, 566);
        cqFillet(c, st, 451, 450, {"radius": 0.4});
        stepVol(c, st, 451, "fillet");
        // step 567: guard_end {"block":7,"g":58,"op":"guard_end"}
        // step 568: guard_begin {"block":9,"g":59,"op":"guard_begin","skippable":false}
        atStep(st, 569);
        cqSelect("Edges")(c, st, 452, 451, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        // step 570: injected_failure {"at":570,"op":"injected_failure","was":"chamfer"}
        // step 571: guard_caught {"block":9,"g":59,"op":"guard_caught"}
        // step 572: guard_begin {"before":{"wp":451},"block":8,"g":60,"op":"guard_begin","skippable":true}
        atStep(st, 573);
        cqSelect("Edges")(c, st, 454, 451, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        // step 574: injected_failure {"at":574,"op":"injected_failure","was":"fillet"}
        // step 575: guard_caught {"block":8,"g":60,"op":"guard_caught"}
        // step 576: guard_begin {"before":{"wp":451},"block":10,"g":61,"op":"guard_begin","skippable":true}
        // step 577: guard_caught {"block":10,"g":61,"op":"guard_caught"}
        // step 578: guard_begin {"block":11,"g":62,"op":"guard_begin","skippable":false}
        atStep(st, 579);
        cqVal(c, st, 1, 451);
        atStep(st, 580);
        cqWorkplaneNew(c, st, 456, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 581);
        cqAdd(c, st, 456, 456, {"obj": {"shape": 1}});
        // step 582: guard_end {"block":11,"g":62,"op":"guard_end"}
        return mergeMaps(cqMeasure(c, st, {"wp": 456}), {"i": 5645});
    } catch (e) {
        return {"i": 5645, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c5645"] = case_5645();
    return out;
}
