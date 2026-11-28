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

const case_744 = function() {
    var c = newContextWithDefaults();
    var st = newState();
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 2);
        cqMoveTo(c, st, 2, 1, {"x": -12, "y": 0});
        atStep(st, 3);
        cqThreePointArc(c, st, 3, 2, {"point1": [0, 12], "point2": [12, 0], "forConstruction": false});
        atStep(st, 4);
        cqClose(c, st, 4, 3, {});
        atStep(st, 5);
        cqRevolve(c, st, 5, 4, {"angleDegrees": 180, "axisStart": [0, 0, 0], "axisEnd": [1, 0, 0], "combine": true, "clean": true});
        stepVol(c, st, 5, "revolve");
        atStep(st, 6);
        cqWorkplaneNew(c, st, 6, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 7);
        cqMoveTo(c, st, 7, 6, {"x": -10.5, "y": 0});
        atStep(st, 8);
        cqThreePointArc(c, st, 8, 7, {"point1": [0, 10.5], "point2": [10.5, 0], "forConstruction": false});
        atStep(st, 9);
        cqClose(c, st, 9, 8, {});
        atStep(st, 10);
        cqRevolve(c, st, 10, 9, {"angleDegrees": 180, "axisStart": [0, 0, 0], "axisEnd": [1, 0, 0], "combine": true, "clean": true});
        stepVol(c, st, 10, "revolve");
        atStep(st, 11);
        cqCut(c, st, 11, 5, {"toCut": {"wp": 10}, "clean": true, "tol": undefined});
        stepVol(c, st, 11, "cut");
        atStep(st, 12);
        cqWorkplaneNew(c, st, 12, {"plane": {"origin": [7.713451316238471, 0, 9.192533317427737], "xDir": [-0.7660444431189781, 0, 0.6427876096865394], "zDir": [-0.6427876096865394, 0, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 13);
        cqCircle(c, st, 13, 12, {"radius": 0.5, "forConstruction": false});
        atStep(st, 14);
        cqExtrude(c, st, 14, 13, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 14, "extrude");
        atStep(st, 15);
        cqCut(c, st, 15, 11, {"toCut": {"wp": 14}, "clean": true, "tol": undefined});
        stepVol(c, st, 15, "cut");
        atStep(st, 16);
        cqWorkplaneNew(c, st, 16, {"plane": {"origin": [7.450621836178147, 1.9963881041136229, 9.192533317427737], "xDir": [-0.7768708163454219, 0, 0.6296600151754898], "zDir": [-0.6208851530148456, -0.16636567534280192, -0.766044443118978]}, "obj": undefined});
        atStep(st, 17);
        cqCircle(c, st, 17, 16, {"radius": 0.5, "forConstruction": false});
        atStep(st, 18);
        cqExtrude(c, st, 18, 17, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 18, "extrude");
        atStep(st, 19);
        cqCut(c, st, 19, 15, {"toCut": {"wp": 18}, "clean": true, "tol": undefined});
        stepVol(c, st, 19, "cut");
        atStep(st, 20);
        cqWorkplaneNew(c, st, 20, {"plane": {"origin": [6.680044790717032, 3.856725658119235, 9.192533317427737], "xDir": [-0.8089634933040122, -2.77555756156289e-17, 0.5878588831525551], "zDir": [-0.5566703992264195, -0.3213938048432696, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 21);
        cqCircle(c, st, 21, 20, {"radius": 0.5, "forConstruction": false});
        atStep(st, 22);
        cqExtrude(c, st, 22, 21, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 22, "extrude");
        atStep(st, 23);
        cqCut(c, st, 23, 19, {"toCut": {"wp": 22}, "clean": true, "tol": undefined});
        stepVol(c, st, 23, "cut");
        atStep(st, 24);
        cqWorkplaneNew(c, st, 24, {"plane": {"origin": [5.454233732064524, 5.454233732064523, 9.192533317427737], "xDir": [-0.8600121206459054, -2.7755575615628907e-17, 0.5102736053747369], "zDir": [-0.4545194776720437, -0.4545194776720437, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 25);
        cqCircle(c, st, 25, 24, {"radius": 0.5, "forConstruction": false});
        atStep(st, 26);
        cqExtrude(c, st, 26, 25, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 26, "extrude");
        atStep(st, 27);
        cqCut(c, st, 27, 23, {"toCut": {"wp": 26}, "clean": true, "tol": undefined});
        stepVol(c, st, 27, "cut");
        atStep(st, 28);
        cqWorkplaneNew(c, st, 28, {"plane": {"origin": [3.856725658119237, 6.680044790717031, 9.192533317427737], "xDir": [-2.7755575615628907e-17, -0.8089634933040123, 0.587858883152555], "zDir": [-0.32139380484326974, -0.5566703992264194, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 29);
        cqCircle(c, st, 29, 28, {"radius": 0.5, "forConstruction": false});
        atStep(st, 30);
        cqExtrude(c, st, 30, 29, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 30, "extrude");
        atStep(st, 31);
        cqCut(c, st, 31, 27, {"toCut": {"wp": 30}, "clean": true, "tol": undefined});
        stepVol(c, st, 31, "cut");
        atStep(st, 32);
        cqWorkplaneNew(c, st, 32, {"plane": {"origin": [1.9963881041136229, 7.450621836178147, 9.192533317427737], "xDir": [0, -0.7768708163454219, 0.6296600151754898], "zDir": [-0.16636567534280192, -0.6208851530148456, -0.766044443118978]}, "obj": undefined});
        atStep(st, 33);
        cqCircle(c, st, 33, 32, {"radius": 0.5, "forConstruction": false});
        atStep(st, 34);
        cqExtrude(c, st, 34, 33, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 34, "extrude");
        atStep(st, 35);
        cqCut(c, st, 35, 31, {"toCut": {"wp": 34}, "clean": true, "tol": undefined});
        stepVol(c, st, 35, "cut");
        atStep(st, 36);
        cqWorkplaneNew(c, st, 36, {"plane": {"origin": [4.723126732405191e-16, 7.713451316238471, 9.192533317427737], "xDir": [-3.081487911019578e-33, -0.7660444431189781, 0.6427876096865394], "zDir": [-3.935938943670993e-17, -0.6427876096865394, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 37);
        cqCircle(c, st, 37, 36, {"radius": 0.5, "forConstruction": false});
        atStep(st, 38);
        cqExtrude(c, st, 38, 37, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 38, "extrude");
        atStep(st, 39);
        cqCut(c, st, 39, 35, {"toCut": {"wp": 38}, "clean": true, "tol": undefined});
        stepVol(c, st, 39, "cut");
        atStep(st, 40);
        cqWorkplaneNew(c, st, 40, {"plane": {"origin": [-1.996388104113622, 7.450621836178147, 9.192533317427737], "xDir": [0, -0.7768708163454218, 0.6296600151754896], "zDir": [0.16636567534280186, -0.6208851530148457, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 41);
        cqCircle(c, st, 41, 40, {"radius": 0.5, "forConstruction": false});
        atStep(st, 42);
        cqExtrude(c, st, 42, 41, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 42, "extrude");
        atStep(st, 43);
        cqCut(c, st, 43, 39, {"toCut": {"wp": 42}, "clean": true, "tol": undefined});
        stepVol(c, st, 43, "cut");
        atStep(st, 44);
        cqWorkplaneNew(c, st, 44, {"plane": {"origin": [-3.8567256581192337, 6.680044790717032, 9.192533317427737], "xDir": [0, -0.8089634933040123, 0.5878588831525551], "zDir": [0.3213938048432695, -0.5566703992264195, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 45);
        cqCircle(c, st, 45, 44, {"radius": 0.5, "forConstruction": false});
        atStep(st, 46);
        cqExtrude(c, st, 46, 45, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 46, "extrude");
        atStep(st, 47);
        cqCut(c, st, 47, 43, {"toCut": {"wp": 46}, "clean": true, "tol": undefined});
        stepVol(c, st, 47, "cut");
        atStep(st, 48);
        cqWorkplaneNew(c, st, 48, {"plane": {"origin": [-5.454233732064523, 5.454233732064524, 9.192533317427737], "xDir": [2.7755575615628907e-17, -0.8600121206459054, 0.5102736053747369], "zDir": [0.4545194776720437, -0.4545194776720437, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 49);
        cqCircle(c, st, 49, 48, {"radius": 0.5, "forConstruction": false});
        atStep(st, 50);
        cqExtrude(c, st, 50, 49, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 50, "extrude");
        atStep(st, 51);
        cqCut(c, st, 51, 47, {"toCut": {"wp": 50}, "clean": true, "tol": undefined});
        stepVol(c, st, 51, "cut");
        atStep(st, 52);
        cqWorkplaneNew(c, st, 52, {"plane": {"origin": [-6.680044790717032, 3.856725658119235, 9.192533317427737], "xDir": [-0.8089634933040122, 2.77555756156289e-17, -0.5878588831525551], "zDir": [0.5566703992264195, -0.3213938048432696, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 53);
        cqCircle(c, st, 53, 52, {"radius": 0.5, "forConstruction": false});
        atStep(st, 54);
        cqExtrude(c, st, 54, 53, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 54, "extrude");
        atStep(st, 55);
        cqCut(c, st, 55, 51, {"toCut": {"wp": 54}, "clean": true, "tol": undefined});
        stepVol(c, st, 55, "cut");
        atStep(st, 56);
        cqWorkplaneNew(c, st, 56, {"plane": {"origin": [-7.450621836178146, 1.9963881041136249, 9.192533317427737], "xDir": [-0.7768708163454219, -1.3877787807814457e-17, -0.6296600151754896], "zDir": [0.6208851530148456, -0.1663656753428021, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 57);
        cqCircle(c, st, 57, 56, {"radius": 0.5, "forConstruction": false});
        atStep(st, 58);
        cqExtrude(c, st, 58, 57, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 58, "extrude");
        atStep(st, 59);
        cqCut(c, st, 59, 55, {"toCut": {"wp": 58}, "clean": true, "tol": undefined});
        stepVol(c, st, 59, "cut");
        atStep(st, 60);
        cqWorkplaneNew(c, st, 60, {"plane": {"origin": [-7.713451316238471, 9.446253464810382e-16, 9.192533317427737], "xDir": [-0.7660444431189781, 6.162975822039155e-33, -0.6427876096865394], "zDir": [0.6427876096865394, -7.871877887341985e-17, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 61);
        cqCircle(c, st, 61, 60, {"radius": 0.5, "forConstruction": false});
        atStep(st, 62);
        cqExtrude(c, st, 62, 61, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 62, "extrude");
        atStep(st, 63);
        cqCut(c, st, 63, 59, {"toCut": {"wp": 62}, "clean": true, "tol": undefined});
        stepVol(c, st, 63, "cut");
        atStep(st, 64);
        cqWorkplaneNew(c, st, 64, {"plane": {"origin": [-7.450621836178147, -1.996388104113623, 9.192533317427737], "xDir": [-0.7768708163454219, 0, -0.6296600151754898], "zDir": [0.6208851530148456, 0.16636567534280194, -0.766044443118978]}, "obj": undefined});
        atStep(st, 65);
        cqCircle(c, st, 65, 64, {"radius": 0.5, "forConstruction": false});
        atStep(st, 66);
        cqExtrude(c, st, 66, 65, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 66, "extrude");
        atStep(st, 67);
        cqCut(c, st, 67, 63, {"toCut": {"wp": 66}, "clean": true, "tol": undefined});
        stepVol(c, st, 67, "cut");
        atStep(st, 68);
        cqWorkplaneNew(c, st, 68, {"plane": {"origin": [-6.680044790717033, -3.8567256581192337, 9.192533317427737], "xDir": [-0.8089634933040123, 0, -0.5878588831525551], "zDir": [0.5566703992264195, 0.3213938048432695, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 69);
        cqCircle(c, st, 69, 68, {"radius": 0.5, "forConstruction": false});
        atStep(st, 70);
        cqExtrude(c, st, 70, 69, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 70, "extrude");
        atStep(st, 71);
        cqCut(c, st, 71, 67, {"toCut": {"wp": 70}, "clean": true, "tol": undefined});
        stepVol(c, st, 71, "cut");
        atStep(st, 72);
        cqWorkplaneNew(c, st, 72, {"plane": {"origin": [-5.454233732064527, -5.45423373206452, 9.192533317427737], "xDir": [-0.8600121206459052, -5.5511151231257815e-17, -0.5102736053747371], "zDir": [0.4545194776720439, 0.4545194776720434, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 73);
        cqCircle(c, st, 73, 72, {"radius": 0.5, "forConstruction": false});
        atStep(st, 74);
        cqExtrude(c, st, 74, 73, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 74, "extrude");
        atStep(st, 75);
        cqCut(c, st, 75, 71, {"toCut": {"wp": 74}, "clean": true, "tol": undefined});
        stepVol(c, st, 75, "cut");
        atStep(st, 76);
        cqWorkplaneNew(c, st, 76, {"plane": {"origin": [-3.856725658119239, -6.680044790717029, 9.192533317427737], "xDir": [0, -0.8089634933040124, -0.5878588831525549], "zDir": [0.32139380484326996, 0.5566703992264193, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 77);
        cqCircle(c, st, 77, 76, {"radius": 0.5, "forConstruction": false});
        atStep(st, 78);
        cqExtrude(c, st, 78, 77, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 78, "extrude");
        atStep(st, 79);
        cqCut(c, st, 79, 75, {"toCut": {"wp": 78}, "clean": true, "tol": undefined});
        stepVol(c, st, 79, "cut");
        atStep(st, 80);
        cqWorkplaneNew(c, st, 80, {"plane": {"origin": [-1.996388104113622, -7.450621836178147, 9.192533317427737], "xDir": [0, -0.7768708163454218, -0.6296600151754896], "zDir": [0.16636567534280186, 0.6208851530148457, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 81);
        cqCircle(c, st, 81, 80, {"radius": 0.5, "forConstruction": false});
        atStep(st, 82);
        cqExtrude(c, st, 82, 81, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 82, "extrude");
        atStep(st, 83);
        cqCut(c, st, 83, 79, {"toCut": {"wp": 82}, "clean": true, "tol": undefined});
        stepVol(c, st, 83, "cut");
        atStep(st, 84);
        cqWorkplaneNew(c, st, 84, {"plane": {"origin": [-1.4169380197215572e-15, -7.713451316238471, 9.192533317427737], "xDir": [0, -0.7660444431189781, -0.6427876096865394], "zDir": [1.180781683101298e-16, 0.6427876096865394, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 85);
        cqCircle(c, st, 85, 84, {"radius": 0.5, "forConstruction": false});
        atStep(st, 86);
        cqExtrude(c, st, 86, 85, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 86, "extrude");
        atStep(st, 87);
        cqCut(c, st, 87, 83, {"toCut": {"wp": 86}, "clean": true, "tol": undefined});
        stepVol(c, st, 87, "cut");
        atStep(st, 88);
        cqWorkplaneNew(c, st, 88, {"plane": {"origin": [1.996388104113619, -7.450621836178147, 9.192533317427737], "xDir": [0, -0.7768708163454218, -0.6296600151754898], "zDir": [-0.16636567534280164, 0.6208851530148457, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 89);
        cqCircle(c, st, 89, 88, {"radius": 0.5, "forConstruction": false});
        atStep(st, 90);
        cqExtrude(c, st, 90, 89, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 90, "extrude");
        atStep(st, 91);
        cqCut(c, st, 91, 87, {"toCut": {"wp": 90}, "clean": true, "tol": undefined});
        stepVol(c, st, 91, "cut");
        atStep(st, 92);
        cqWorkplaneNew(c, st, 92, {"plane": {"origin": [3.856725658119237, -6.680044790717031, 9.192533317427737], "xDir": [2.7755575615628907e-17, -0.8089634933040123, -0.587858883152555], "zDir": [-0.32139380484326974, 0.5566703992264194, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 93);
        cqCircle(c, st, 93, 92, {"radius": 0.5, "forConstruction": false});
        atStep(st, 94);
        cqExtrude(c, st, 94, 93, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 94, "extrude");
        atStep(st, 95);
        cqCut(c, st, 95, 91, {"toCut": {"wp": 94}, "clean": true, "tol": undefined});
        stepVol(c, st, 95, "cut");
        atStep(st, 96);
        cqWorkplaneNew(c, st, 96, {"plane": {"origin": [5.454233732064522, -5.454233732064525, 9.192533317427737], "xDir": [2.7755575615628907e-17, -0.8600121206459053, -0.5102736053747371], "zDir": [-0.4545194776720436, 0.4545194776720438, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 97);
        cqCircle(c, st, 97, 96, {"radius": 0.5, "forConstruction": false});
        atStep(st, 98);
        cqExtrude(c, st, 98, 97, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 98, "extrude");
        atStep(st, 99);
        cqCut(c, st, 99, 95, {"toCut": {"wp": 98}, "clean": true, "tol": undefined});
        stepVol(c, st, 99, "cut");
        atStep(st, 100);
        cqWorkplaneNew(c, st, 100, {"plane": {"origin": [6.680044790717029, -3.856725658119239, 9.192533317427737], "xDir": [-0.8089634933040124, 0, 0.5878588831525549], "zDir": [-0.5566703992264193, 0.32139380484326996, -0.7660444431189781]}, "obj": undefined});
        atStep(st, 101);
        cqCircle(c, st, 101, 100, {"radius": 0.5, "forConstruction": false});
        atStep(st, 102);
        cqExtrude(c, st, 102, 101, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 102, "extrude");
        atStep(st, 103);
        cqCut(c, st, 103, 99, {"toCut": {"wp": 102}, "clean": true, "tol": undefined});
        stepVol(c, st, 103, "cut");
        atStep(st, 104);
        cqWorkplaneNew(c, st, 104, {"plane": {"origin": [7.450621836178145, -1.996388104113629, 9.192533317427737], "xDir": [-0.7768708163454219, -1.387778780781446e-17, 0.6296600151754898], "zDir": [-0.6208851530148455, 0.16636567534280244, -0.766044443118978]}, "obj": undefined});
        atStep(st, 105);
        cqCircle(c, st, 105, 104, {"radius": 0.5, "forConstruction": false});
        atStep(st, 106);
        cqExtrude(c, st, 106, 105, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 106, "extrude");
        atStep(st, 107);
        cqCut(c, st, 107, 103, {"toCut": {"wp": 106}, "clean": true, "tol": undefined});
        stepVol(c, st, 107, "cut");
        atStep(st, 108);
        cqWorkplaneNew(c, st, 108, {"plane": {"origin": [9.625478313060524, 0, 7.165903100433434], "xDir": [0.5971585917027862, 0, -0.8021231927550437], "zDir": [-0.8021231927550437, 0, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 109);
        cqCircle(c, st, 109, 108, {"radius": 0.5, "forConstruction": false});
        atStep(st, 110);
        cqExtrude(c, st, 110, 109, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 110, "extrude");
        atStep(st, 111);
        cqCut(c, st, 111, 107, {"toCut": {"wp": 110}, "clean": true, "tol": undefined});
        stepVol(c, st, 111, "cut");
        atStep(st, 112);
        cqWorkplaneNew(c, st, 112, {"plane": {"origin": [9.297498092970494, 2.4912571056413473, 7.165903100433434], "xDir": [0.6104587849467163, 0, -0.7920480237216547], "zDir": [-0.7747915077475412, -0.2076047588034456, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 113);
        cqCircle(c, st, 113, 112, {"radius": 0.5, "forConstruction": false});
        atStep(st, 114);
        cqExtrude(c, st, 114, 113, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 114, "extrude");
        atStep(st, 115);
        cqCut(c, st, 115, 111, {"toCut": {"wp": 114}, "clean": true, "tol": undefined});
        stepVol(c, st, 115, "cut");
        atStep(st, 116);
        cqWorkplaneNew(c, st, 116, {"plane": {"origin": [8.335908742686598, 4.812739156530261, 7.165903100433434], "xDir": [0.6518835024542823, 0, -0.7583191275630186], "zDir": [-0.6946590618905499, -0.4010615963775218, -0.5971585917027863]}, "obj": undefined});
        atStep(st, 117);
        cqCircle(c, st, 117, 116, {"radius": 0.5, "forConstruction": false});
        atStep(st, 118);
        cqExtrude(c, st, 118, 117, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 118, "extrude");
        atStep(st, 119);
        cqCut(c, st, 119, 115, {"toCut": {"wp": 118}, "clean": true, "tol": undefined});
        stepVol(c, st, 119, "cut");
        atStep(st, 120);
        cqWorkplaneNew(c, st, 120, {"plane": {"origin": [6.806240987329147, 6.806240987329146, 7.165903100433434], "xDir": [-0.7250684956212247, 0, 0.6886767577445706], "zDir": [-0.5671867489440957, -0.5671867489440956, -0.5971585917027863]}, "obj": undefined});
        atStep(st, 121);
        cqCircle(c, st, 121, 120, {"radius": 0.5, "forConstruction": false});
        atStep(st, 122);
        cqExtrude(c, st, 122, 121, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 122, "extrude");
        atStep(st, 123);
        cqCut(c, st, 123, 119, {"toCut": {"wp": 122}, "clean": true, "tol": undefined});
        stepVol(c, st, 123, "cut");
        atStep(st, 124);
        cqWorkplaneNew(c, st, 124, {"plane": {"origin": [4.812739156530263, 8.335908742686598, 7.165903100433434], "xDir": [-2.775557561562892e-17, 0.6518835024542824, -0.7583191275630187], "zDir": [-0.4010615963775219, -0.6946590618905498, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 125);
        cqCircle(c, st, 125, 124, {"radius": 0.5, "forConstruction": false});
        atStep(st, 126);
        cqExtrude(c, st, 126, 125, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 126, "extrude");
        atStep(st, 127);
        cqCut(c, st, 127, 123, {"toCut": {"wp": 126}, "clean": true, "tol": undefined});
        stepVol(c, st, 127, "cut");
        atStep(st, 128);
        cqWorkplaneNew(c, st, 128, {"plane": {"origin": [2.4912571056413473, 9.297498092970494, 7.165903100433434], "xDir": [0, 0.6104587849467163, -0.7920480237216547], "zDir": [-0.2076047588034456, -0.7747915077475412, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 129);
        cqCircle(c, st, 129, 128, {"radius": 0.5, "forConstruction": false});
        atStep(st, 130);
        cqExtrude(c, st, 130, 129, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 130, "extrude");
        atStep(st, 131);
        cqCut(c, st, 131, 127, {"toCut": {"wp": 130}, "clean": true, "tol": undefined});
        stepVol(c, st, 131, "cut");
        atStep(st, 132);
        cqWorkplaneNew(c, st, 132, {"plane": {"origin": [5.893905603175918e-16, 9.625478313060524, 7.165903100433434], "xDir": [0, 0.5971585917027862, -0.8021231927550437], "zDir": [-4.911588002646599e-17, -0.8021231927550437, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 133);
        cqCircle(c, st, 133, 132, {"radius": 0.5, "forConstruction": false});
        atStep(st, 134);
        cqExtrude(c, st, 134, 133, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 134, "extrude");
        atStep(st, 135);
        cqCut(c, st, 135, 131, {"toCut": {"wp": 134}, "clean": true, "tol": undefined});
        stepVol(c, st, 135, "cut");
        atStep(st, 136);
        cqWorkplaneNew(c, st, 136, {"plane": {"origin": [-2.491257105641346, 9.297498092970494, 7.165903100433434], "xDir": [1.387778780781446e-17, 0.6104587849467163, -0.7920480237216547], "zDir": [0.2076047588034455, -0.7747915077475412, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 137);
        cqCircle(c, st, 137, 136, {"radius": 0.5, "forConstruction": false});
        atStep(st, 138);
        cqExtrude(c, st, 138, 137, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 138, "extrude");
        atStep(st, 139);
        cqCut(c, st, 139, 135, {"toCut": {"wp": 138}, "clean": true, "tol": undefined});
        stepVol(c, st, 139, "cut");
        atStep(st, 140);
        cqWorkplaneNew(c, st, 140, {"plane": {"origin": [-4.81273915653026, 8.335908742686598, 7.165903100433434], "xDir": [0, 0.6518835024542823, -0.7583191275630187], "zDir": [0.40106159637752176, -0.6946590618905499, -0.5971585917027863]}, "obj": undefined});
        atStep(st, 141);
        cqCircle(c, st, 141, 140, {"radius": 0.5, "forConstruction": false});
        atStep(st, 142);
        cqExtrude(c, st, 142, 141, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 142, "extrude");
        atStep(st, 143);
        cqCut(c, st, 143, 139, {"toCut": {"wp": 142}, "clean": true, "tol": undefined});
        stepVol(c, st, 143, "cut");
        atStep(st, 144);
        cqWorkplaneNew(c, st, 144, {"plane": {"origin": [-6.806240987329146, 6.806240987329147, 7.165903100433434], "xDir": [0, -0.7250684956212247, 0.6886767577445706], "zDir": [0.5671867489440956, -0.5671867489440957, -0.5971585917027863]}, "obj": undefined});
        atStep(st, 145);
        cqCircle(c, st, 145, 144, {"radius": 0.5, "forConstruction": false});
        atStep(st, 146);
        cqExtrude(c, st, 146, 145, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 146, "extrude");
        atStep(st, 147);
        cqCut(c, st, 147, 143, {"toCut": {"wp": 146}, "clean": true, "tol": undefined});
        stepVol(c, st, 147, "cut");
        atStep(st, 148);
        cqWorkplaneNew(c, st, 148, {"plane": {"origin": [-8.335908742686598, 4.812739156530261, 7.165903100433434], "xDir": [0.6518835024542823, 0, 0.7583191275630186], "zDir": [0.6946590618905499, -0.4010615963775218, -0.5971585917027863]}, "obj": undefined});
        atStep(st, 149);
        cqCircle(c, st, 149, 148, {"radius": 0.5, "forConstruction": false});
        atStep(st, 150);
        cqExtrude(c, st, 150, 149, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 150, "extrude");
        atStep(st, 151);
        cqCut(c, st, 151, 147, {"toCut": {"wp": 150}, "clean": true, "tol": undefined});
        stepVol(c, st, 151, "cut");
        atStep(st, 152);
        cqWorkplaneNew(c, st, 152, {"plane": {"origin": [-9.297498092970493, 2.49125710564135, 7.165903100433434], "xDir": [0.6104587849467163, 0, 0.7920480237216546], "zDir": [0.7747915077475411, -0.2076047588034459, -0.5971585917027863]}, "obj": undefined});
        atStep(st, 153);
        cqCircle(c, st, 153, 152, {"radius": 0.5, "forConstruction": false});
        atStep(st, 154);
        cqExtrude(c, st, 154, 153, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 154, "extrude");
        atStep(st, 155);
        cqCut(c, st, 155, 151, {"toCut": {"wp": 154}, "clean": true, "tol": undefined});
        stepVol(c, st, 155, "cut");
        atStep(st, 156);
        cqWorkplaneNew(c, st, 156, {"plane": {"origin": [-9.625478313060524, 1.1787811206351837e-15, 7.165903100433434], "xDir": [0.5971585917027862, 0, 0.8021231927550437], "zDir": [0.8021231927550437, -9.823176005293195e-17, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 157);
        cqCircle(c, st, 157, 156, {"radius": 0.5, "forConstruction": false});
        atStep(st, 158);
        cqExtrude(c, st, 158, 157, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 158, "extrude");
        atStep(st, 159);
        cqCut(c, st, 159, 155, {"toCut": {"wp": 158}, "clean": true, "tol": undefined});
        stepVol(c, st, 159, "cut");
        atStep(st, 160);
        cqWorkplaneNew(c, st, 160, {"plane": {"origin": [-9.297498092970494, -2.4912571056413477, 7.165903100433434], "xDir": [0.6104587849467162, 0, 0.7920480237216547], "zDir": [0.7747915077475412, 0.20760475880344564, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 161);
        cqCircle(c, st, 161, 160, {"radius": 0.5, "forConstruction": false});
        atStep(st, 162);
        cqExtrude(c, st, 162, 161, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 162, "extrude");
        atStep(st, 163);
        cqCut(c, st, 163, 159, {"toCut": {"wp": 162}, "clean": true, "tol": undefined});
        stepVol(c, st, 163, "cut");
        atStep(st, 164);
        cqWorkplaneNew(c, st, 164, {"plane": {"origin": [-8.3359087426866, -4.8127391565302595, 7.165903100433434], "xDir": [0.6518835024542822, 2.7755575615628914e-17, 0.7583191275630188], "zDir": [0.69465906189055, 0.4010615963775217, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 165);
        cqCircle(c, st, 165, 164, {"radius": 0.5, "forConstruction": false});
        atStep(st, 166);
        cqExtrude(c, st, 166, 165, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 166, "extrude");
        atStep(st, 167);
        cqCut(c, st, 167, 163, {"toCut": {"wp": 166}, "clean": true, "tol": undefined});
        stepVol(c, st, 167, "cut");
        atStep(st, 168);
        cqWorkplaneNew(c, st, 168, {"plane": {"origin": [-6.806240987329151, -6.806240987329143, 7.165903100433434], "xDir": [-0.7250684956212246, -2.7755575615628914e-17, -0.6886767577445709], "zDir": [0.5671867489440959, 0.5671867489440953, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 169);
        cqCircle(c, st, 169, 168, {"radius": 0.5, "forConstruction": false});
        atStep(st, 170);
        cqExtrude(c, st, 170, 169, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 170, "extrude");
        atStep(st, 171);
        cqCut(c, st, 171, 167, {"toCut": {"wp": 170}, "clean": true, "tol": undefined});
        stepVol(c, st, 171, "cut");
        atStep(st, 172);
        cqWorkplaneNew(c, st, 172, {"plane": {"origin": [-4.812739156530267, -8.335908742686595, 7.165903100433434], "xDir": [0, 0.6518835024542825, 0.7583191275630186], "zDir": [0.40106159637752226, 0.6946590618905497, -0.5971585917027863]}, "obj": undefined});
        atStep(st, 173);
        cqCircle(c, st, 173, 172, {"radius": 0.5, "forConstruction": false});
        atStep(st, 174);
        cqExtrude(c, st, 174, 173, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 174, "extrude");
        atStep(st, 175);
        cqCut(c, st, 175, 171, {"toCut": {"wp": 174}, "clean": true, "tol": undefined});
        stepVol(c, st, 175, "cut");
        atStep(st, 176);
        cqWorkplaneNew(c, st, 176, {"plane": {"origin": [-2.491257105641346, -9.297498092970494, 7.165903100433434], "xDir": [-1.387778780781446e-17, 0.6104587849467163, 0.7920480237216547], "zDir": [0.2076047588034455, 0.7747915077475412, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 177);
        cqCircle(c, st, 177, 176, {"radius": 0.5, "forConstruction": false});
        atStep(st, 178);
        cqExtrude(c, st, 178, 177, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 178, "extrude");
        atStep(st, 179);
        cqCut(c, st, 179, 175, {"toCut": {"wp": 178}, "clean": true, "tol": undefined});
        stepVol(c, st, 179, "cut");
        atStep(st, 180);
        cqWorkplaneNew(c, st, 180, {"plane": {"origin": [-1.7681716809527753e-15, -9.625478313060524, 7.165903100433434], "xDir": [-1.232595164407831e-32, 0.5971585917027862, 0.8021231927550437], "zDir": [1.4734764007939794e-16, 0.8021231927550437, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 181);
        cqCircle(c, st, 181, 180, {"radius": 0.5, "forConstruction": false});
        atStep(st, 182);
        cqExtrude(c, st, 182, 181, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 182, "extrude");
        atStep(st, 183);
        cqCut(c, st, 183, 179, {"toCut": {"wp": 182}, "clean": true, "tol": undefined});
        stepVol(c, st, 183, "cut");
        atStep(st, 184);
        cqWorkplaneNew(c, st, 184, {"plane": {"origin": [2.491257105641343, -9.297498092970496, 7.165903100433434], "xDir": [2.7755575615628914e-17, 0.6104587849467162, 0.7920480237216547], "zDir": [-0.20760475880344523, 0.7747915077475414, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 185);
        cqCircle(c, st, 185, 184, {"radius": 0.5, "forConstruction": false});
        atStep(st, 186);
        cqExtrude(c, st, 186, 185, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 186, "extrude");
        atStep(st, 187);
        cqCut(c, st, 187, 183, {"toCut": {"wp": 186}, "clean": true, "tol": undefined});
        stepVol(c, st, 187, "cut");
        atStep(st, 188);
        cqWorkplaneNew(c, st, 188, {"plane": {"origin": [4.812739156530263, -8.335908742686598, 7.165903100433434], "xDir": [2.775557561562892e-17, 0.6518835024542824, 0.7583191275630187], "zDir": [-0.4010615963775219, 0.6946590618905498, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 189);
        cqCircle(c, st, 189, 188, {"radius": 0.5, "forConstruction": false});
        atStep(st, 190);
        cqExtrude(c, st, 190, 189, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 190, "extrude");
        atStep(st, 191);
        cqCut(c, st, 191, 187, {"toCut": {"wp": 190}, "clean": true, "tol": undefined});
        stepVol(c, st, 191, "cut");
        atStep(st, 192);
        cqWorkplaneNew(c, st, 192, {"plane": {"origin": [6.806240987329145, -6.806240987329148, 7.165903100433434], "xDir": [0, -0.7250684956212247, -0.6886767577445708], "zDir": [-0.5671867489440955, 0.5671867489440957, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 193);
        cqCircle(c, st, 193, 192, {"radius": 0.5, "forConstruction": false});
        atStep(st, 194);
        cqExtrude(c, st, 194, 193, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 194, "extrude");
        atStep(st, 195);
        cqCut(c, st, 195, 191, {"toCut": {"wp": 194}, "clean": true, "tol": undefined});
        stepVol(c, st, 195, "cut");
        atStep(st, 196);
        cqWorkplaneNew(c, st, 196, {"plane": {"origin": [8.335908742686595, -4.812739156530267, 7.165903100433434], "xDir": [0.6518835024542825, 0, -0.7583191275630186], "zDir": [-0.6946590618905497, 0.40106159637752226, -0.5971585917027863]}, "obj": undefined});
        atStep(st, 197);
        cqCircle(c, st, 197, 196, {"radius": 0.5, "forConstruction": false});
        atStep(st, 198);
        cqExtrude(c, st, 198, 197, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 198, "extrude");
        atStep(st, 199);
        cqCut(c, st, 199, 195, {"toCut": {"wp": 198}, "clean": true, "tol": undefined});
        stepVol(c, st, 199, "cut");
        atStep(st, 200);
        cqWorkplaneNew(c, st, 200, {"plane": {"origin": [9.297498092970493, -2.4912571056413553, 7.165903100433434], "xDir": [0.6104587849467163, 0, -0.7920480237216547], "zDir": [-0.774791507747541, 0.20760475880344628, -0.5971585917027862]}, "obj": undefined});
        atStep(st, 201);
        cqCircle(c, st, 201, 200, {"radius": 0.5, "forConstruction": false});
        atStep(st, 202);
        cqExtrude(c, st, 202, 201, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 202, "extrude");
        atStep(st, 203);
        cqCut(c, st, 203, 199, {"toCut": {"wp": 202}, "clean": true, "tol": undefined});
        stepVol(c, st, 203, "cut");
        atStep(st, 204);
        cqWorkplaneNew(c, st, 204, {"plane": {"origin": [11.01859328256329, 0, 4.75295719246988], "xDir": [0.3960797660391567, 0, -0.918216106880274], "zDir": [-0.918216106880274, 0, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 205);
        cqCircle(c, st, 205, 204, {"radius": 0.5, "forConstruction": false});
        atStep(st, 206);
        cqExtrude(c, st, 206, 205, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 206, "extrude");
        atStep(st, 207);
        cqCut(c, st, 207, 203, {"toCut": {"wp": 206}, "clean": true, "tol": undefined});
        stepVol(c, st, 207, "cut");
        atStep(st, 208);
        cqWorkplaneNew(c, st, 208, {"plane": {"origin": [10.643143821003124, 2.85182179176608, 4.75295719246988], "xDir": [0.4077619805753993, 0, -0.9130882581641426], "zDir": [-0.8869286517502603, -0.23765181598050664, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 209);
        cqCircle(c, st, 209, 208, {"radius": 0.5, "forConstruction": false});
        atStep(st, 210);
        cqExtrude(c, st, 210, 209, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 210, "extrude");
        atStep(st, 211);
        cqCut(c, st, 211, 207, {"toCut": {"wp": 210}, "clean": true, "tol": undefined});
        stepVol(c, st, 211, "cut");
        atStep(st, 212);
        cqWorkplaneNew(c, st, 212, {"plane": {"origin": [9.542381696668375, 5.509296641281644, 4.75295719246988], "xDir": [0.49999999999999994, -0.8660254037844387, 0], "zDir": [-0.7951984747223647, -0.459108053440137, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 213);
        cqCircle(c, st, 213, 212, {"radius": 0.5, "forConstruction": false});
        atStep(st, 214);
        cqExtrude(c, st, 214, 213, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 214, "extrude");
        atStep(st, 215);
        cqCut(c, st, 215, 211, {"toCut": {"wp": 214}, "clean": true, "tol": undefined});
        stepVol(c, st, 215, "cut");
        atStep(st, 216);
        cqWorkplaneNew(c, st, 216, {"plane": {"origin": [7.791322029237043, 7.791322029237041, 4.75295719246988], "xDir": [0.7071067811865475, -0.7071067811865476, 0], "zDir": [-0.6492768357697536, -0.6492768357697535, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 217);
        cqCircle(c, st, 217, 216, {"radius": 0.5, "forConstruction": false});
        atStep(st, 218);
        cqExtrude(c, st, 218, 217, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 218, "extrude");
        atStep(st, 219);
        cqCut(c, st, 219, 215, {"toCut": {"wp": 218}, "clean": true, "tol": undefined});
        stepVol(c, st, 219, "cut");
        atStep(st, 220);
        cqWorkplaneNew(c, st, 220, {"plane": {"origin": [5.5092966412816455, 9.542381696668375, 4.75295719246988], "xDir": [-0.8660254037844387, 0.5000000000000001, -2.7755575615628914e-17], "zDir": [-0.4591080534401371, -0.7951984747223646, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 221);
        cqCircle(c, st, 221, 220, {"radius": 0.5, "forConstruction": false});
        atStep(st, 222);
        cqExtrude(c, st, 222, 221, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 222, "extrude");
        atStep(st, 223);
        cqCut(c, st, 223, 219, {"toCut": {"wp": 222}, "clean": true, "tol": undefined});
        stepVol(c, st, 223, "cut");
        atStep(st, 224);
        cqWorkplaneNew(c, st, 224, {"plane": {"origin": [2.85182179176608, 10.643143821003124, 4.75295719246988], "xDir": [0, 0.4077619805753993, -0.9130882581641426], "zDir": [-0.23765181598050664, -0.8869286517502603, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 225);
        cqCircle(c, st, 225, 224, {"radius": 0.5, "forConstruction": false});
        atStep(st, 226);
        cqExtrude(c, st, 226, 225, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 226, "extrude");
        atStep(st, 227);
        cqCut(c, st, 227, 223, {"toCut": {"wp": 226}, "clean": true, "tol": undefined});
        stepVol(c, st, 227, "cut");
        atStep(st, 228);
        cqWorkplaneNew(c, st, 228, {"plane": {"origin": [6.74694249729883e-16, 11.01859328256329, 4.75295719246988], "xDir": [0, 0.3960797660391567, -0.918216106880274], "zDir": [-5.622452081082359e-17, -0.918216106880274, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 229);
        cqCircle(c, st, 229, 228, {"radius": 0.5, "forConstruction": false});
        atStep(st, 230);
        cqExtrude(c, st, 230, 229, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 230, "extrude");
        atStep(st, 231);
        cqCut(c, st, 231, 227, {"toCut": {"wp": 230}, "clean": true, "tol": undefined});
        stepVol(c, st, 231, "cut");
        atStep(st, 232);
        cqWorkplaneNew(c, st, 232, {"plane": {"origin": [-2.851821791766078, 10.643143821003124, 4.75295719246988], "xDir": [1.3877787807814457e-17, 0.4077619805753993, -0.9130882581641424], "zDir": [0.23765181598050655, -0.8869286517502603, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 233);
        cqCircle(c, st, 233, 232, {"radius": 0.5, "forConstruction": false});
        atStep(st, 234);
        cqExtrude(c, st, 234, 233, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 234, "extrude");
        atStep(st, 235);
        cqCut(c, st, 235, 231, {"toCut": {"wp": 234}, "clean": true, "tol": undefined});
        stepVol(c, st, 235, "cut");
        atStep(st, 236);
        cqWorkplaneNew(c, st, 236, {"plane": {"origin": [-5.509296641281642, 9.542381696668375, 4.75295719246988], "xDir": [-0.8660254037844388, -0.49999999999999983, 0], "zDir": [0.4591080534401368, -0.7951984747223647, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 237);
        cqCircle(c, st, 237, 236, {"radius": 0.5, "forConstruction": false});
        atStep(st, 238);
        cqExtrude(c, st, 238, 237, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 238, "extrude");
        atStep(st, 239);
        cqCut(c, st, 239, 235, {"toCut": {"wp": 238}, "clean": true, "tol": undefined});
        stepVol(c, st, 239, "cut");
        atStep(st, 240);
        cqWorkplaneNew(c, st, 240, {"plane": {"origin": [-7.791322029237041, 7.791322029237043, 4.75295719246988], "xDir": [-0.7071067811865476, -0.7071067811865475, 0], "zDir": [0.6492768357697535, -0.6492768357697536, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 241);
        cqCircle(c, st, 241, 240, {"radius": 0.5, "forConstruction": false});
        atStep(st, 242);
        cqExtrude(c, st, 242, 241, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 242, "extrude");
        atStep(st, 243);
        cqCut(c, st, 243, 239, {"toCut": {"wp": 242}, "clean": true, "tol": undefined});
        stepVol(c, st, 243, "cut");
        atStep(st, 244);
        cqWorkplaneNew(c, st, 244, {"plane": {"origin": [-9.542381696668375, 5.509296641281644, 4.75295719246988], "xDir": [0.49999999999999994, 0.8660254037844387, 0], "zDir": [0.7951984747223647, -0.459108053440137, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 245);
        cqCircle(c, st, 245, 244, {"radius": 0.5, "forConstruction": false});
        atStep(st, 246);
        cqExtrude(c, st, 246, 245, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 246, "extrude");
        atStep(st, 247);
        cqCut(c, st, 247, 243, {"toCut": {"wp": 246}, "clean": true, "tol": undefined});
        stepVol(c, st, 247, "cut");
        atStep(st, 248);
        cqWorkplaneNew(c, st, 248, {"plane": {"origin": [-10.643143821003122, 2.851821791766083, 4.75295719246988], "xDir": [0.4077619805753992, 0, 0.9130882581641424], "zDir": [0.8869286517502601, -0.23765181598050691, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 249);
        cqCircle(c, st, 249, 248, {"radius": 0.5, "forConstruction": false});
        atStep(st, 250);
        cqExtrude(c, st, 250, 249, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 250, "extrude");
        atStep(st, 251);
        cqCut(c, st, 251, 247, {"toCut": {"wp": 250}, "clean": true, "tol": undefined});
        stepVol(c, st, 251, "cut");
        atStep(st, 252);
        cqWorkplaneNew(c, st, 252, {"plane": {"origin": [-11.01859328256329, 1.349388499459766e-15, 4.75295719246988], "xDir": [0.3960797660391567, 0, 0.918216106880274], "zDir": [0.918216106880274, -1.1244904162164715e-16, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 253);
        cqCircle(c, st, 253, 252, {"radius": 0.5, "forConstruction": false});
        atStep(st, 254);
        cqExtrude(c, st, 254, 253, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        atStep(st, 255);
        cqCut(c, st, 255, 251, {"toCut": {"wp": 254}, "clean": true, "tol": undefined});
        stepVol(c, st, 255, "cut");
        atStep(st, 256);
        cqWorkplaneNew(c, st, 256, {"plane": {"origin": [-10.643143821003124, -2.8518217917660804, 4.75295719246988], "xDir": [0.4077619805753992, -1.3877787807814457e-17, 0.9130882581641424], "zDir": [0.8869286517502603, 0.2376518159805067, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 257);
        cqCircle(c, st, 257, 256, {"radius": 0.5, "forConstruction": false});
        atStep(st, 258);
        cqExtrude(c, st, 258, 257, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 258, "extrude");
        atStep(st, 259);
        cqCut(c, st, 259, 255, {"toCut": {"wp": 258}, "clean": true, "tol": undefined});
        stepVol(c, st, 259, "cut");
        atStep(st, 260);
        cqWorkplaneNew(c, st, 260, {"plane": {"origin": [-9.542381696668375, -5.509296641281642, 4.75295719246988], "xDir": [-0.49999999999999983, 0.8660254037844388, 0], "zDir": [0.7951984747223647, 0.4591080534401368, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 261);
        cqCircle(c, st, 261, 260, {"radius": 0.5, "forConstruction": false});
        atStep(st, 262);
        cqExtrude(c, st, 262, 261, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 262, "extrude");
        atStep(st, 263);
        cqCut(c, st, 263, 259, {"toCut": {"wp": 262}, "clean": true, "tol": undefined});
        stepVol(c, st, 263, "cut");
        atStep(st, 264);
        cqWorkplaneNew(c, st, 264, {"plane": {"origin": [-7.791322029237047, -7.791322029237038, 4.75295719246988], "xDir": [-0.7071067811865471, 0.707106781186548, 2.7755575615628907e-17], "zDir": [0.6492768357697539, 0.6492768357697531, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 265);
        cqCircle(c, st, 265, 264, {"radius": 0.5, "forConstruction": false});
        atStep(st, 266);
        cqExtrude(c, st, 266, 265, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 266, "extrude");
        atStep(st, 267);
        cqCut(c, st, 267, 263, {"toCut": {"wp": 266}, "clean": true, "tol": undefined});
        stepVol(c, st, 267, "cut");
        atStep(st, 268);
        cqWorkplaneNew(c, st, 268, {"plane": {"origin": [-5.50929664128165, -9.542381696668372, 4.75295719246988], "xDir": [0.8660254037844384, -0.5000000000000004, 0], "zDir": [0.4591080534401375, 0.7951984747223645, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 269);
        cqCircle(c, st, 269, 268, {"radius": 0.5, "forConstruction": false});
        atStep(st, 270);
        cqExtrude(c, st, 270, 269, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 270, "extrude");
        atStep(st, 271);
        cqCut(c, st, 271, 267, {"toCut": {"wp": 270}, "clean": true, "tol": undefined});
        stepVol(c, st, 271, "cut");
        atStep(st, 272);
        cqWorkplaneNew(c, st, 272, {"plane": {"origin": [-2.851821791766078, -10.643143821003124, 4.75295719246988], "xDir": [-1.3877787807814457e-17, 0.4077619805753993, 0.9130882581641424], "zDir": [0.23765181598050655, 0.8869286517502603, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 273);
        cqCircle(c, st, 273, 272, {"radius": 0.5, "forConstruction": false});
        atStep(st, 274);
        cqExtrude(c, st, 274, 273, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 274, "extrude");
        atStep(st, 275);
        cqCut(c, st, 275, 271, {"toCut": {"wp": 274}, "clean": true, "tol": undefined});
        stepVol(c, st, 275, "cut");
        atStep(st, 276);
        cqWorkplaneNew(c, st, 276, {"plane": {"origin": [-2.0240827491896488e-15, -11.01859328256329, 4.75295719246988], "xDir": [0, 0.3960797660391567, 0.918216106880274], "zDir": [1.686735624324707e-16, 0.918216106880274, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 277);
        cqCircle(c, st, 277, 276, {"radius": 0.5, "forConstruction": false});
        atStep(st, 278);
        cqExtrude(c, st, 278, 277, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 278, "extrude");
        atStep(st, 279);
        cqCut(c, st, 279, 275, {"toCut": {"wp": 278}, "clean": true, "tol": undefined});
        stepVol(c, st, 279, "cut");
        atStep(st, 280);
        cqWorkplaneNew(c, st, 280, {"plane": {"origin": [2.851821791766075, -10.643143821003124, 4.75295719246988], "xDir": [1.387778780781446e-17, 0.4077619805753993, 0.9130882581641424], "zDir": [-0.23765181598050625, 0.8869286517502603, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 281);
        cqCircle(c, st, 281, 280, {"radius": 0.5, "forConstruction": false});
        atStep(st, 282);
        cqExtrude(c, st, 282, 281, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 282, "extrude");
        atStep(st, 283);
        cqCut(c, st, 283, 279, {"toCut": {"wp": 282}, "clean": true, "tol": undefined});
        stepVol(c, st, 283, "cut");
        atStep(st, 284);
        cqWorkplaneNew(c, st, 284, {"plane": {"origin": [5.5092966412816455, -9.542381696668375, 4.75295719246988], "xDir": [0.8660254037844387, 0.5000000000000001, 2.7755575615628914e-17], "zDir": [-0.4591080534401371, 0.7951984747223646, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 285);
        cqCircle(c, st, 285, 284, {"radius": 0.5, "forConstruction": false});
        atStep(st, 286);
        cqExtrude(c, st, 286, 285, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 286, "extrude");
        atStep(st, 287);
        cqCut(c, st, 287, 283, {"toCut": {"wp": 286}, "clean": true, "tol": undefined});
        stepVol(c, st, 287, "cut");
        atStep(st, 288);
        cqWorkplaneNew(c, st, 288, {"plane": {"origin": [7.79132202923704, -7.791322029237044, 4.75295719246988], "xDir": [0.7071067811865477, 0.7071067811865475, 0], "zDir": [-0.6492768357697534, 0.6492768357697537, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 289);
        cqCircle(c, st, 289, 288, {"radius": 0.5, "forConstruction": false});
        atStep(st, 290);
        cqExtrude(c, st, 290, 289, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 290, "extrude");
        atStep(st, 291);
        cqCut(c, st, 291, 287, {"toCut": {"wp": 290}, "clean": true, "tol": undefined});
        stepVol(c, st, 291, "cut");
        atStep(st, 292);
        cqWorkplaneNew(c, st, 292, {"plane": {"origin": [9.542381696668372, -5.50929664128165, 4.75295719246988], "xDir": [-0.5000000000000004, -0.8660254037844384, 0], "zDir": [-0.7951984747223645, 0.4591080534401375, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 293);
        cqCircle(c, st, 293, 292, {"radius": 0.5, "forConstruction": false});
        atStep(st, 294);
        cqExtrude(c, st, 294, 293, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 294, "extrude");
        atStep(st, 295);
        cqCut(c, st, 295, 291, {"toCut": {"wp": 294}, "clean": true, "tol": undefined});
        stepVol(c, st, 295, "cut");
        atStep(st, 296);
        cqWorkplaneNew(c, st, 296, {"plane": {"origin": [10.64314382100312, -2.8518217917660893, 4.75295719246988], "xDir": [0.4077619805753993, 1.3877787807814457e-17, -0.9130882581641424], "zDir": [-0.88692865175026, 0.23765181598050744, -0.3960797660391567]}, "obj": undefined});
        atStep(st, 297);
        cqCircle(c, st, 297, 296, {"radius": 0.5, "forConstruction": false});
        atStep(st, 298);
        cqExtrude(c, st, 298, 297, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 298, "extrude");
        atStep(st, 299);
        cqCut(c, st, 299, 295, {"toCut": {"wp": 298}, "clean": true, "tol": undefined});
        stepVol(c, st, 299, "cut");
        atStep(st, 300);
        cqWorkplaneNew(c, st, 300, {"plane": {"origin": [11.817693036146496, 0, 2.083778132003165], "xDir": [0.17364817766693044, 0, -0.984807753012208], "zDir": [-0.984807753012208, 0, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 301);
        cqCircle(c, st, 301, 300, {"radius": 0.5, "forConstruction": false});
        atStep(st, 302);
        cqExtrude(c, st, 302, 301, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 302, "extrude");
        atStep(st, 303);
        cqCut(c, st, 303, 299, {"toCut": {"wp": 302}, "clean": true, "tol": undefined});
        stepVol(c, st, 303, "cut");
        atStep(st, 304);
        cqWorkplaneNew(c, st, 304, {"plane": {"origin": [11.415014910770372, 3.058644026930146, 2.083778132003165], "xDir": [0.2588190451025208, -0.9659258262890684, 6.938893903907228e-18], "zDir": [-0.9512512425641978, -0.2548870022441788, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 305);
        cqCircle(c, st, 305, 304, {"radius": 0.5, "forConstruction": false});
        atStep(st, 306);
        cqExtrude(c, st, 306, 305, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 306, "extrude");
        atStep(st, 307);
        cqCut(c, st, 307, 303, {"toCut": {"wp": 306}, "clean": true, "tol": undefined});
        stepVol(c, st, 307, "cut");
        atStep(st, 308);
        cqWorkplaneNew(c, st, 308, {"plane": {"origin": [10.23442238342932, 5.908846518073247, 2.083778132003165], "xDir": [0.4999999999999999, -0.8660254037844388, 0], "zDir": [-0.8528685319524434, -0.4924038765061039, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 309);
        cqCircle(c, st, 309, 308, {"radius": 0.5, "forConstruction": false});
        atStep(st, 310);
        cqExtrude(c, st, 310, 309, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 310, "extrude");
        atStep(st, 311);
        cqCut(c, st, 311, 307, {"toCut": {"wp": 310}, "clean": true, "tol": undefined});
        stepVol(c, st, 311, "cut");
        atStep(st, 312);
        cqWorkplaneNew(c, st, 312, {"plane": {"origin": [8.356370883840228, 8.356370883840226, 2.083778132003165], "xDir": [0.7071067811865475, -0.7071067811865476, 0], "zDir": [-0.696364240320019, -0.6963642403200189, -0.1736481776669304]}, "obj": undefined});
        atStep(st, 313);
        cqCircle(c, st, 313, 312, {"radius": 0.5, "forConstruction": false});
        atStep(st, 314);
        cqExtrude(c, st, 314, 313, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 314, "extrude");
        atStep(st, 315);
        cqCut(c, st, 315, 311, {"toCut": {"wp": 314}, "clean": true, "tol": undefined});
        stepVol(c, st, 315, "cut");
        atStep(st, 316);
        cqWorkplaneNew(c, st, 316, {"plane": {"origin": [5.908846518073249, 10.234422383429315, 2.083778132003165], "xDir": [-0.8660254037844386, 0.5000000000000001, 1.3877787807814457e-17], "zDir": [-0.4924038765061041, -0.8528685319524432, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 317);
        cqCircle(c, st, 317, 316, {"radius": 0.5, "forConstruction": false});
        atStep(st, 318);
        cqExtrude(c, st, 318, 317, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 318, "extrude");
        atStep(st, 319);
        cqCut(c, st, 319, 315, {"toCut": {"wp": 318}, "clean": true, "tol": undefined});
        stepVol(c, st, 319, "cut");
        atStep(st, 320);
        cqWorkplaneNew(c, st, 320, {"plane": {"origin": [3.058644026930146, 11.415014910770372, 2.083778132003165], "xDir": [-0.9659258262890684, 0.2588190451025208, 6.938893903907228e-18], "zDir": [-0.2548870022441788, -0.9512512425641978, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 321);
        cqCircle(c, st, 321, 320, {"radius": 0.5, "forConstruction": false});
        atStep(st, 322);
        cqExtrude(c, st, 322, 321, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 322, "extrude");
        atStep(st, 323);
        cqCut(c, st, 323, 319, {"toCut": {"wp": 322}, "clean": true, "tol": undefined});
        stepVol(c, st, 323, "cut");
        atStep(st, 324);
        cqWorkplaneNew(c, st, 324, {"plane": {"origin": [7.236249975011386e-16, 11.817693036146496, 2.083778132003165], "xDir": [0, 0.17364817766693044, -0.984807753012208], "zDir": [-6.030208312509488e-17, -0.984807753012208, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 325);
        cqCircle(c, st, 325, 324, {"radius": 0.5, "forConstruction": false});
        atStep(st, 326);
        cqExtrude(c, st, 326, 325, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 326, "extrude");
        atStep(st, 327);
        cqCut(c, st, 327, 323, {"toCut": {"wp": 326}, "clean": true, "tol": undefined});
        stepVol(c, st, 327, "cut");
        atStep(st, 328);
        cqWorkplaneNew(c, st, 328, {"plane": {"origin": [-3.058644026930144, 11.415014910770372, 2.083778132003165], "xDir": [-0.9659258262890684, -0.2588190451025207, 0], "zDir": [0.2548870022441787, -0.9512512425641978, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 329);
        cqCircle(c, st, 329, 328, {"radius": 0.5, "forConstruction": false});
        atStep(st, 330);
        cqExtrude(c, st, 330, 329, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 330, "extrude");
        atStep(st, 331);
        cqCut(c, st, 331, 327, {"toCut": {"wp": 330}, "clean": true, "tol": undefined});
        stepVol(c, st, 331, "cut");
        atStep(st, 332);
        cqWorkplaneNew(c, st, 332, {"plane": {"origin": [-5.908846518073245, 10.23442238342932, 2.083778132003165], "xDir": [-0.8660254037844389, -0.49999999999999983, -1.3877787807814457e-17], "zDir": [0.49240387650610384, -0.8528685319524434, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 333);
        cqCircle(c, st, 333, 332, {"radius": 0.5, "forConstruction": false});
        atStep(st, 334);
        cqExtrude(c, st, 334, 333, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 334, "extrude");
        atStep(st, 335);
        cqCut(c, st, 335, 331, {"toCut": {"wp": 334}, "clean": true, "tol": undefined});
        stepVol(c, st, 335, "cut");
        atStep(st, 336);
        cqWorkplaneNew(c, st, 336, {"plane": {"origin": [-8.356370883840226, 8.356370883840228, 2.083778132003165], "xDir": [-0.7071067811865476, -0.7071067811865475, 0], "zDir": [0.6963642403200189, -0.696364240320019, -0.1736481776669304]}, "obj": undefined});
        atStep(st, 337);
        cqCircle(c, st, 337, 336, {"radius": 0.5, "forConstruction": false});
        atStep(st, 338);
        cqExtrude(c, st, 338, 337, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 338, "extrude");
        atStep(st, 339);
        cqCut(c, st, 339, 335, {"toCut": {"wp": 338}, "clean": true, "tol": undefined});
        stepVol(c, st, 339, "cut");
        atStep(st, 340);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [-10.23442238342932, 5.908846518073247, 2.083778132003165], "xDir": [0.4999999999999999, 0.8660254037844388, 0], "zDir": [0.8528685319524434, -0.4924038765061039, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 341);
        cqCircle(c, st, 341, 340, {"radius": 0.5, "forConstruction": false});
        atStep(st, 342);
        cqExtrude(c, st, 342, 341, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 342, "extrude");
        atStep(st, 343);
        cqCut(c, st, 343, 339, {"toCut": {"wp": 342}, "clean": true, "tol": undefined});
        stepVol(c, st, 343, "cut");
        atStep(st, 344);
        cqWorkplaneNew(c, st, 344, {"plane": {"origin": [-11.41501491077037, 3.0586440269301485, 2.083778132003165], "xDir": [0.258819045102521, 0.9659258262890682, -6.9388939039072276e-18], "zDir": [0.9512512425641976, -0.2548870022441791, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 345);
        cqCircle(c, st, 345, 344, {"radius": 0.5, "forConstruction": false});
        atStep(st, 346);
        cqExtrude(c, st, 346, 345, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 346, "extrude");
        atStep(st, 347);
        cqCut(c, st, 347, 343, {"toCut": {"wp": 346}, "clean": true, "tol": undefined});
        stepVol(c, st, 347, "cut");
        atStep(st, 348);
        cqWorkplaneNew(c, st, 348, {"plane": {"origin": [-11.817693036146496, 1.4472499950022772e-15, 2.083778132003165], "xDir": [0.17364817766693044, 0, 0.984807753012208], "zDir": [0.984807753012208, -1.2060416625018979e-16, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 349);
        cqCircle(c, st, 349, 348, {"radius": 0.5, "forConstruction": false});
        atStep(st, 350);
        cqExtrude(c, st, 350, 349, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 350, "extrude");
        atStep(st, 351);
        cqCut(c, st, 351, 347, {"toCut": {"wp": 350}, "clean": true, "tol": undefined});
        stepVol(c, st, 351, "cut");
        atStep(st, 352);
        cqWorkplaneNew(c, st, 352, {"plane": {"origin": [-11.415014910770372, -3.058644026930146, 2.083778132003165], "xDir": [-0.2588190451025208, 0.9659258262890684, 1.3877787807814457e-17], "zDir": [0.9512512425641978, 0.2548870022441789, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 353);
        cqCircle(c, st, 353, 352, {"radius": 0.5, "forConstruction": false});
        atStep(st, 354);
        cqExtrude(c, st, 354, 353, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 354, "extrude");
        atStep(st, 355);
        cqCut(c, st, 355, 351, {"toCut": {"wp": 354}, "clean": true, "tol": undefined});
        stepVol(c, st, 355, "cut");
        atStep(st, 356);
        cqWorkplaneNew(c, st, 356, {"plane": {"origin": [-10.23442238342932, -5.908846518073244, 2.083778132003165], "xDir": [-0.49999999999999967, 0.8660254037844389, 0], "zDir": [0.8528685319524435, 0.4924038765061038, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 357);
        cqCircle(c, st, 357, 356, {"radius": 0.5, "forConstruction": false});
        atStep(st, 358);
        cqExtrude(c, st, 358, 357, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 358, "extrude");
        atStep(st, 359);
        cqCut(c, st, 359, 355, {"toCut": {"wp": 358}, "clean": true, "tol": undefined});
        stepVol(c, st, 359, "cut");
        atStep(st, 360);
        cqWorkplaneNew(c, st, 360, {"plane": {"origin": [-8.356370883840231, -8.356370883840222, 2.083778132003165], "xDir": [-0.7071067811865472, 0.7071067811865479, 0], "zDir": [0.6963642403200193, 0.6963642403200185, -0.1736481776669304]}, "obj": undefined});
        atStep(st, 361);
        cqCircle(c, st, 361, 360, {"radius": 0.5, "forConstruction": false});
        atStep(st, 362);
        cqExtrude(c, st, 362, 361, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        atStep(st, 363);
        cqCut(c, st, 363, 359, {"toCut": {"wp": 362}, "clean": true, "tol": undefined});
        stepVol(c, st, 363, "cut");
        atStep(st, 364);
        cqWorkplaneNew(c, st, 364, {"plane": {"origin": [-5.908846518073253, -10.234422383429315, 2.083778132003165], "xDir": [0.8660254037844385, -0.5000000000000006, -1.3877787807814457e-17], "zDir": [0.4924038765061045, 0.852868531952443, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 365);
        cqCircle(c, st, 365, 364, {"radius": 0.5, "forConstruction": false});
        atStep(st, 366);
        cqExtrude(c, st, 366, 365, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 366, "extrude");
        atStep(st, 367);
        cqCut(c, st, 367, 363, {"toCut": {"wp": 366}, "clean": true, "tol": undefined});
        stepVol(c, st, 367, "cut");
        atStep(st, 368);
        cqWorkplaneNew(c, st, 368, {"plane": {"origin": [-3.058644026930144, -11.415014910770372, 2.083778132003165], "xDir": [0.9659258262890684, -0.2588190451025207, 0], "zDir": [0.2548870022441787, 0.9512512425641978, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 369);
        cqCircle(c, st, 369, 368, {"radius": 0.5, "forConstruction": false});
        atStep(st, 370);
        cqExtrude(c, st, 370, 369, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 370, "extrude");
        atStep(st, 371);
        cqCut(c, st, 371, 367, {"toCut": {"wp": 370}, "clean": true, "tol": undefined});
        stepVol(c, st, 371, "cut");
        atStep(st, 372);
        cqWorkplaneNew(c, st, 372, {"plane": {"origin": [-2.1708749925034158e-15, -11.817693036146496, 2.083778132003165], "xDir": [0, 0.17364817766693044, 0.984807753012208], "zDir": [1.8090624937528468e-16, 0.984807753012208, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 373);
        cqCircle(c, st, 373, 372, {"radius": 0.5, "forConstruction": false});
        atStep(st, 374);
        cqExtrude(c, st, 374, 373, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 374, "extrude");
        atStep(st, 375);
        cqCut(c, st, 375, 371, {"toCut": {"wp": 374}, "clean": true, "tol": undefined});
        stepVol(c, st, 375, "cut");
        atStep(st, 376);
        cqWorkplaneNew(c, st, 376, {"plane": {"origin": [3.05864402693014, -11.415014910770374, 2.083778132003165], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [-0.2548870022441784, 0.951251242564198, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 377);
        cqCircle(c, st, 377, 376, {"radius": 0.5, "forConstruction": false});
        atStep(st, 378);
        cqExtrude(c, st, 378, 377, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 378, "extrude");
        atStep(st, 379);
        cqCut(c, st, 379, 375, {"toCut": {"wp": 378}, "clean": true, "tol": undefined});
        stepVol(c, st, 379, "cut");
        atStep(st, 380);
        cqWorkplaneNew(c, st, 380, {"plane": {"origin": [5.908846518073249, -10.234422383429315, 2.083778132003165], "xDir": [0.8660254037844386, 0.5000000000000001, -1.3877787807814457e-17], "zDir": [-0.4924038765061041, 0.8528685319524432, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 381);
        cqCircle(c, st, 381, 380, {"radius": 0.5, "forConstruction": false});
        atStep(st, 382);
        cqExtrude(c, st, 382, 381, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 382, "extrude");
        atStep(st, 383);
        cqCut(c, st, 383, 379, {"toCut": {"wp": 382}, "clean": true, "tol": undefined});
        stepVol(c, st, 383, "cut");
        atStep(st, 384);
        cqWorkplaneNew(c, st, 384, {"plane": {"origin": [8.356370883840224, -8.356370883840228, 2.083778132003165], "xDir": [0.7071067811865477, 0.7071067811865474, 0], "zDir": [-0.6963642403200188, 0.6963642403200191, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 385);
        cqCircle(c, st, 385, 384, {"radius": 0.5, "forConstruction": false});
        atStep(st, 386);
        cqExtrude(c, st, 386, 385, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 386, "extrude");
        atStep(st, 387);
        cqCut(c, st, 387, 383, {"toCut": {"wp": 386}, "clean": true, "tol": undefined});
        stepVol(c, st, 387, "cut");
        atStep(st, 388);
        cqWorkplaneNew(c, st, 388, {"plane": {"origin": [10.234422383429315, -5.908846518073253, 2.083778132003165], "xDir": [-0.5000000000000006, -0.8660254037844385, 1.3877787807814457e-17], "zDir": [-0.852868531952443, 0.4924038765061045, -0.17364817766693044]}, "obj": undefined});
        atStep(st, 389);
        cqCircle(c, st, 389, 388, {"radius": 0.5, "forConstruction": false});
        atStep(st, 390);
        cqExtrude(c, st, 390, 389, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 390, "extrude");
        atStep(st, 391);
        cqCut(c, st, 391, 387, {"toCut": {"wp": 390}, "clean": true, "tol": undefined});
        stepVol(c, st, 391, "cut");
        atStep(st, 392);
        cqWorkplaneNew(c, st, 392, {"plane": {"origin": [11.41501491077037, -3.058644026930155, 2.083778132003165], "xDir": [-0.25881904510252157, -0.965925826289068, 0], "zDir": [-0.9512512425641976, 0.2548870022441796, -0.1736481776669304]}, "obj": undefined});
        atStep(st, 393);
        cqCircle(c, st, 393, 392, {"radius": 0.5, "forConstruction": false});
        atStep(st, 394);
        cqExtrude(c, st, 394, 393, {"until": 3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 394, "extrude");
        atStep(st, 395);
        cqCut(c, st, 395, 391, {"toCut": {"wp": 394}, "clean": true, "tol": undefined});
        stepVol(c, st, 395, "cut");
        return mergeMaps(cqMeasure(c, st, {"wp": 395}), {"i": 744});
    } catch (e) {
        return {"i": 744, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    out["c744"] = case_744();
    return out;
}
