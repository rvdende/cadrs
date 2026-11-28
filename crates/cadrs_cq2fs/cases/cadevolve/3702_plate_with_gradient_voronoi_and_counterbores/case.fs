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
    return s.base + (prefix ~ s.n);   // under the case's own id (a feature's ops must be under its id)
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
const cqSkSpline = function(sk, nm, e, to2, P, st) {
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
            } else if (e.k == "spline") cqSkSpline(sk, nm, e, to2, P, st);
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
            else if (e.k == "spline") cqSkSpline(sk, "p" ~ j, e, to2, planar, st);
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
    return {"status": "ok", "result_q": B,
            "vol": evVolume(c, {"entities": B, "accuracy": "HIGH"}) / (MM ^ 3),
            "area": evArea(c, {"entities": faces}) / (MM ^ 2),
            "solids": size(ents(c, qBodyType(qEntityFilter(B, EntityType.BODY), BodyType.SOLID))),
            "faces": size(ents(c, faces)),
            "edges": size(ents(c, qOwnedByBody(B, EntityType.EDGE))),
            "vertices": size(ents(c, qOwnedByBody(B, EntityType.VERTEX))),
            "bbox": [bb.minCorner[0] / MM, bb.minCorner[1] / MM, bb.minCorner[2] / MM, bb.maxCorner[0] / MM, bb.maxCorner[1] / MM, bb.maxCorner[2] / MM],
            "other": other, "warn": st[].warn, "log": st[].log};
};
const newState = function(base) { return new box({"base": base, "wps": {}, "ctx": [], "shapes": {}, "n": 0, "log": [], "warn": [], "cur": 0}); };
const atStep = function(st, i) { var s = st[]; s.cur = i; st[] = s; };
const stepVol = function(c, st, out, op) {   // after a solid-producing step: its solids and volume
    const w = st[].wps[out];
    if (w == undefined) return;
    const q = solidsQ(c, w.objs);
    if (q == undefined) return;
    logStep(c, st, out, op, {"solids": size(ents(c, q)), "vol": evVolume(c, {"entities": q}) / (MM ^ 3)});
};

const case_3702 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqBox(c, st, 2, 1, {"length": 100, "width": 60, "height": 5, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "box");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqWorkplane(c, st, 4, 3, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 5);
        cqPolyline(c, st, 5, 4, {"listOfXYTuple": [[24.012382026247696, 10.087284013095008], [27.826013026247697, 18.23847901309501], [28.731780973752304, 17.814704986904992], [24.918149973752303, 9.663509986904993]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 6);
        cqClose(c, st, 6, 5, {});
        atStep(st, 7);
        cqExtrude(c, st, 7, 6, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 7, "extrude");
        atStep(st, 8);
        cqVal(c, st, 1, 7);
        atStep(st, 9);
        cqCut(c, st, 8, 2, {"toCut": {"shape": 1}, "clean": true, "tol": undefined});
        stepVol(c, st, 8, "cut");
        atStep(st, 10);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 11);
        cqWorkplane(c, st, 10, 9, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 12);
        cqPolyline(c, st, 11, 10, {"listOfXYTuple": [[28.61670563577755, 18.561096403375792], [28.74885263577755, 18.197301403375793], [27.808941364222452, 17.85588259662421], [27.676794364222452, 18.21967759662421]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 13);
        cqClose(c, st, 12, 11, {});
        atStep(st, 14);
        cqExtrude(c, st, 13, 12, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 13, "extrude");
        atStep(st, 15);
        cqVal(c, st, 2, 13);
        atStep(st, 16);
        cqCut(c, st, 14, 8, {"toCut": {"shape": 2}, "clean": true, "tol": undefined});
        stepVol(c, st, 14, "cut");
        atStep(st, 17);
        cqWorkplaneNew(c, st, 15, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 18);
        cqWorkplane(c, st, 16, 15, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 19);
        cqPolyline(c, st, 17, 16, {"listOfXYTuple": [[23.690653214172357, 23.291229177899787], [28.490244214172357, 18.753721177899788], [27.803255785827645, 18.027052822100213], [23.003664785827645, 22.564560822100212]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 20);
        cqClose(c, st, 18, 17, {});
        atStep(st, 21);
        cqExtrude(c, st, 19, 18, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 19, "extrude");
        atStep(st, 22);
        cqVal(c, st, 3, 19);
        atStep(st, 23);
        cqCut(c, st, 20, 14, {"toCut": {"shape": 3}, "clean": true, "tol": undefined});
        stepVol(c, st, 20, "cut");
        atStep(st, 24);
        cqWorkplaneNew(c, st, 21, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 25);
        cqWorkplane(c, st, 22, 21, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 26);
        cqPolyline(c, st, 23, 22, {"listOfXYTuple": [[15.23915356946617, 15.613397257369003], [22.99559156946617, 23.283423257369005], [23.69872643053383, 22.572366742630994], [15.94228843053383, 14.902340742630996]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 27);
        cqClose(c, st, 24, 23, {});
        atStep(st, 28);
        cqExtrude(c, st, 25, 24, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 25, "extrude");
        atStep(st, 29);
        cqVal(c, st, 4, 25);
        atStep(st, 30);
        cqCut(c, st, 26, 20, {"toCut": {"shape": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 26, "cut");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 27, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 32);
        cqWorkplane(c, st, 28, 27, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 33);
        cqPolyline(c, st, 29, 28, {"listOfXYTuple": [[15.850011358854513, 15.685383338712864], [24.72455635885451, 10.302911338712864], [24.20597564114549, 9.447882661287135], [15.331430641145488, 14.830354661287137]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 34);
        cqClose(c, st, 30, 29, {});
        atStep(st, 35);
        cqExtrude(c, st, 31, 30, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 31, "extrude");
        atStep(st, 36);
        cqVal(c, st, 5, 31);
        atStep(st, 37);
        cqCut(c, st, 32, 26, {"toCut": {"shape": 5}, "clean": true, "tol": undefined});
        stepVol(c, st, 32, "cut");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 39);
        cqWorkplane(c, st, 34, 33, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 40);
        cqPolyline(c, st, 35, 34, {"listOfXYTuple": [[-34.310111308658335, -3.3849537453115586], [-24.24881530865834, -13.09149974531156], [-24.943120691341655, -13.81118025468844], [-35.00441669134166, -4.104634254688442]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 41);
        cqClose(c, st, 36, 35, {});
        atStep(st, 42);
        cqExtrude(c, st, 37, 36, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 37, "extrude");
        atStep(st, 43);
        cqVal(c, st, 6, 37);
        atStep(st, 44);
        cqCut(c, st, 38, 32, {"toCut": {"shape": 6}, "clean": true, "tol": undefined});
        stepVol(c, st, 38, "cut");
        atStep(st, 45);
        cqWorkplaneNew(c, st, 39, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 46);
        cqWorkplane(c, st, 40, 39, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 47);
        cqPolyline(c, st, 41, 40, {"listOfXYTuple": [[-37.188469503418055, -3.1047038038939094], [-34.629782503418056, -3.2455498038939097], [-34.68474549658194, -4.244038196106091], [-37.24343249658194, -4.10319219610609]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 48);
        cqClose(c, st, 42, 41, {});
        atStep(st, 49);
        cqExtrude(c, st, 43, 42, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 43, "extrude");
        atStep(st, 50);
        cqVal(c, st, 7, 43);
        atStep(st, 51);
        cqCut(c, st, 44, 38, {"toCut": {"shape": 7}, "clean": true, "tol": undefined});
        stepVol(c, st, 44, "cut");
        atStep(st, 52);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 53);
        cqWorkplane(c, st, 46, 45, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 54);
        cqPolyline(c, st, 47, 46, {"listOfXYTuple": [[-39.51764978032209, -4.721500493231886], [-37.51802178032209, -3.205509493231886], [-36.91388021967791, -4.002386506768113], [-38.91350821967791, -5.518377506768113]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 55);
        cqClose(c, st, 48, 47, {});
        atStep(st, 56);
        cqExtrude(c, st, 49, 48, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 49, "extrude");
        atStep(st, 57);
        cqVal(c, st, 8, 49);
        atStep(st, 58);
        cqCut(c, st, 50, 44, {"toCut": {"shape": 8}, "clean": true, "tol": undefined});
        stepVol(c, st, 50, "cut");
        atStep(st, 59);
        cqWorkplaneNew(c, st, 51, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 60);
        cqWorkplane(c, st, 52, 51, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 61);
        cqPolyline(c, st, 53, 52, {"listOfXYTuple": [[-41.22176214269435, -12.063682862902349], [-39.70438214269435, -5.014717862902346], [-38.72677585730565, -5.225160137097653], [-40.24415585730565, -12.274125137097652]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 62);
        cqClose(c, st, 54, 53, {});
        atStep(st, 63);
        cqExtrude(c, st, 55, 54, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 55, "extrude");
        atStep(st, 64);
        cqVal(c, st, 9, 55);
        atStep(st, 65);
        cqCut(c, st, 56, 50, {"toCut": {"shape": 9}, "clean": true, "tol": undefined});
        stepVol(c, st, 56, "cut");
        atStep(st, 66);
        cqWorkplaneNew(c, st, 57, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 67);
        cqWorkplane(c, st, 58, 57, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 68);
        cqPolyline(c, st, 59, 58, {"listOfXYTuple": [[-40.25499735402159, -12.022095740785444], [-36.98207335402159, -22.67770974078545], [-37.93799664597841, -22.971326259214557], [-41.21092064597841, -12.315712259214555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 69);
        cqClose(c, st, 60, 59, {});
        atStep(st, 70);
        cqExtrude(c, st, 61, 60, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 61, "extrude");
        atStep(st, 71);
        cqVal(c, st, 10, 61);
        atStep(st, 72);
        cqCut(c, st, 62, 56, {"toCut": {"shape": 10}, "clean": true, "tol": undefined});
        stepVol(c, st, 62, "cut");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 63, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 74);
        cqWorkplane(c, st, 64, 63, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 75);
        cqPolyline(c, st, 65, 64, {"listOfXYTuple": [[-37.678217617565934, -22.374633186528737], [-24.45312161756594, -15.960800186528736], [-24.01675638243406, -16.860569813471265], [-37.24185238243406, -23.274402813471266]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 76);
        cqClose(c, st, 66, 65, {});
        atStep(st, 77);
        cqExtrude(c, st, 67, 66, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 67, "extrude");
        atStep(st, 78);
        cqVal(c, st, 11, 67);
        atStep(st, 79);
        cqCut(c, st, 68, 62, {"toCut": {"shape": 11}, "clean": true, "tol": undefined});
        stepVol(c, st, 68, "cut");
        atStep(st, 80);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 81);
        cqWorkplane(c, st, 70, 69, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 82);
        cqPolyline(c, st, 71, 70, {"listOfXYTuple": [[-24.09964774747871, -13.390790789128166], [-23.73861874747871, -16.350135789128167], [-24.73125925252129, -16.471234210871835], [-25.09228825252129, -13.511889210871834]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 83);
        cqClose(c, st, 72, 71, {});
        atStep(st, 84);
        cqExtrude(c, st, 73, 72, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 73, "extrude");
        atStep(st, 85);
        cqVal(c, st, 12, 73);
        atStep(st, 86);
        cqCut(c, st, 74, 68, {"toCut": {"shape": 12}, "clean": true, "tol": undefined});
        stepVol(c, st, 74, "cut");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 75, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 88);
        cqWorkplane(c, st, 76, 75, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 89);
        cqPolyline(c, st, 77, 76, {"listOfXYTuple": [[-24.952589649690996, -13.10088173005667], [-24.498096649690996, -12.63839573005667], [-23.784853350309003, -13.33931226994333], [-24.239346350309003, -13.80179826994333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 90);
        cqClose(c, st, 78, 77, {});
        atStep(st, 91);
        cqExtrude(c, st, 79, 78, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 79, "extrude");
        atStep(st, 92);
        cqVal(c, st, 13, 79);
        atStep(st, 93);
        cqCut(c, st, 80, 74, {"toCut": {"shape": 13}, "clean": true, "tol": undefined});
        stepVol(c, st, 80, "cut");
        atStep(st, 94);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 95);
        cqWorkplane(c, st, 82, 81, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 96);
        cqPolyline(c, st, 83, 82, {"listOfXYTuple": [[-24.62475925908213, -12.860649768699156], [-21.303727259082127, -0.3415427686991548], [-20.337158740917875, -0.5979512313008453], [-23.65819074091787, -13.117058231300843]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 97);
        cqClose(c, st, 84, 83, {});
        atStep(st, 98);
        cqExtrude(c, st, 85, 84, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 85, "extrude");
        atStep(st, 99);
        cqVal(c, st, 14, 85);
        atStep(st, 100);
        cqCut(c, st, 86, 80, {"toCut": {"shape": 14}, "clean": true, "tol": undefined});
        stepVol(c, st, 86, "cut");
        atStep(st, 101);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 102);
        cqWorkplane(c, st, 88, 87, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 103);
        cqPolyline(c, st, 89, 88, {"listOfXYTuple": [[-20.770078689810436, 1.1700074371021318], [-20.339777689810436, -0.3320485628978681], [-21.30110831018957, -0.607445437102132], [-21.73140931018957, 0.894610562897868]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 104);
        cqClose(c, st, 90, 89, {});
        atStep(st, 105);
        cqExtrude(c, st, 91, 90, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 91, "extrude");
        atStep(st, 106);
        cqVal(c, st, 15, 91);
        atStep(st, 107);
        cqCut(c, st, 92, 86, {"toCut": {"shape": 15}, "clean": true, "tol": undefined});
        stepVol(c, st, 92, "cut");
        atStep(st, 108);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 109);
        cqWorkplane(c, st, 94, 93, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 110);
        cqPolyline(c, st, 95, 94, {"listOfXYTuple": [[-33.00336143651447, -2.2030307101851943], [-21.403112436514466, 1.5085272898148054], [-21.09837556348554, 0.5560907101851944], [-32.69862456348554, -3.1554672898148057]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 111);
        cqClose(c, st, 96, 95, {});
        atStep(st, 112);
        cqExtrude(c, st, 97, 96, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 97, "extrude");
        atStep(st, 113);
        cqVal(c, st, 16, 97);
        atStep(st, 114);
        cqCut(c, st, 98, 92, {"toCut": {"shape": 16}, "clean": true, "tol": undefined});
        stepVol(c, st, 98, "cut");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 99, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 116);
        cqWorkplane(c, st, 100, 99, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 117);
        cqPolyline(c, st, 101, 100, {"listOfXYTuple": [[-34.91131103722201, -3.314143210056784], [-33.10504003722202, -2.248598210056784], [-32.59694596277799, -3.109899789943216], [-34.40321696277798, -4.175444789943216]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 118);
        cqClose(c, st, 102, 101, {});
        atStep(st, 119);
        cqExtrude(c, st, 103, 102, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 103, "extrude");
        atStep(st, 120);
        cqVal(c, st, 17, 103);
        atStep(st, 121);
        cqCut(c, st, 104, 98, {"toCut": {"shape": 17}, "clean": true, "tol": undefined});
        stepVol(c, st, 104, "cut");
        atStep(st, 122);
        cqWorkplaneNew(c, st, 105, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 123);
        cqWorkplane(c, st, 106, 105, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 124);
        cqPolyline(c, st, 107, 106, {"listOfXYTuple": [[16.769436130589895, -17.730292293274907], [18.864167130589895, -9.596457293274906], [19.832568869410107, -9.845852706725092], [17.737837869410107, -17.979687706725095]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 125);
        cqClose(c, st, 108, 107, {});
        atStep(st, 126);
        cqExtrude(c, st, 109, 108, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 109, "extrude");
        atStep(st, 127);
        cqVal(c, st, 18, 109);
        atStep(st, 128);
        cqCut(c, st, 110, 104, {"toCut": {"shape": 18}, "clean": true, "tol": undefined});
        stepVol(c, st, 110, "cut");
        atStep(st, 129);
        cqWorkplaneNew(c, st, 111, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 130);
        cqWorkplane(c, st, 112, 111, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 131);
        cqPolyline(c, st, 113, 112, {"listOfXYTuple": [[8.613253794962029, -5.386510225165855], [19.51537579496203, -9.249871225165856], [19.181360205037972, -10.192438774834145], [8.279238205037972, -6.329077774834145]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 132);
        cqClose(c, st, 114, 113, {});
        atStep(st, 133);
        cqExtrude(c, st, 115, 114, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 115, "extrude");
        atStep(st, 134);
        cqVal(c, st, 19, 115);
        atStep(st, 135);
        cqCut(c, st, 116, 110, {"toCut": {"shape": 19}, "clean": true, "tol": undefined});
        stepVol(c, st, 116, "cut");
        atStep(st, 136);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 137);
        cqWorkplane(c, st, 118, 117, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 138);
        cqPolyline(c, st, 119, 118, {"listOfXYTuple": [[4.932865039599938, -10.787594423948654], [8.01649403959994, -5.602229423948655], [8.875997960400062, -6.113358576051345], [5.792368960400062, -11.298723576051344]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 139);
        cqClose(c, st, 120, 119, {});
        atStep(st, 140);
        cqExtrude(c, st, 121, 120, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 121, "extrude");
        atStep(st, 141);
        cqVal(c, st, 20, 121);
        atStep(st, 142);
        cqCut(c, st, 122, 116, {"toCut": {"shape": 20}, "clean": true, "tol": undefined});
        stepVol(c, st, 122, "cut");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 144);
        cqWorkplane(c, st, 124, 123, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 145);
        cqPolyline(c, st, 125, 124, {"listOfXYTuple": [[5.857647935144055, -10.972842762540912], [6.998297935144055, -19.003093762540914], [6.008236064855946, -19.143726237459084], [4.867586064855946, -11.113475237459086]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 146);
        cqClose(c, st, 126, 125, {});
        atStep(st, 147);
        cqExtrude(c, st, 127, 126, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 127, "extrude");
        atStep(st, 148);
        cqVal(c, st, 21, 127);
        atStep(st, 149);
        cqCut(c, st, 128, 122, {"toCut": {"shape": 21}, "clean": true, "tol": undefined});
        stepVol(c, st, 128, "cut");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 151);
        cqWorkplane(c, st, 130, 129, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 152);
        cqPolyline(c, st, 131, 130, {"listOfXYTuple": [[6.44695874945187, -18.576590736162323], [17.19732874945187, -17.358170736162325], [17.309945250548132, -18.351809263837676], [6.55957525054813, -19.570229263837675]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 153);
        cqClose(c, st, 132, 131, {});
        atStep(st, 154);
        cqExtrude(c, st, 133, 132, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 133, "extrude");
        atStep(st, 155);
        cqVal(c, st, 22, 133);
        atStep(st, 156);
        cqCut(c, st, 134, 128, {"toCut": {"shape": 22}, "clean": true, "tol": undefined});
        stepVol(c, st, 134, "cut");
        atStep(st, 157);
        cqWorkplaneNew(c, st, 135, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 158);
        cqWorkplane(c, st, 136, 135, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 159);
        cqPolyline(c, st, 137, 136, {"listOfXYTuple": [[-33.31080995711751, -2.482860696002038], [-29.071571957117506, 7.442748303997963], [-28.15193804288249, 7.0499716960020375], [-32.39117604288249, -2.875637303997962]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 160);
        cqClose(c, st, 138, 137, {});
        atStep(st, 161);
        cqExtrude(c, st, 139, 138, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 139, "extrude");
        atStep(st, 162);
        cqVal(c, st, 23, 139);
        atStep(st, 163);
        cqCut(c, st, 140, 134, {"toCut": {"shape": 23}, "clean": true, "tol": undefined});
        stepVol(c, st, 140, "cut");
        atStep(st, 164);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 165);
        cqWorkplane(c, st, 142, 141, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 166);
        cqPolyline(c, st, 143, 142, {"listOfXYTuple": [[-21.659820203383767, 1.319809712738465], [-19.47381620338377, 4.430209712738465], [-18.655663796616235, 3.8552082872615343], [-20.841667796616235, 0.7448082872615349]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 167);
        cqClose(c, st, 144, 143, {});
        atStep(st, 168);
        cqExtrude(c, st, 145, 144, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 145, "extrude");
        atStep(st, 169);
        cqVal(c, st, 24, 145);
        atStep(st, 170);
        cqCut(c, st, 146, 140, {"toCut": {"shape": 24}, "clean": true, "tol": undefined});
        stepVol(c, st, 146, "cut");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 147, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 172);
        cqWorkplane(c, st, 148, 147, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 173);
        cqPolyline(c, st, 149, 148, {"listOfXYTuple": [[-19.530244284005303, 4.3252076618381405], [-19.471587284005302, 4.474825661838141], [-18.540578715994695, 4.10982833816186], [-18.5992357159947, 3.960210338161859]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 174);
        cqClose(c, st, 150, 149, {});
        atStep(st, 175);
        cqExtrude(c, st, 151, 150, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 151, "extrude");
        atStep(st, 176);
        cqVal(c, st, 25, 151);
        atStep(st, 177);
        cqCut(c, st, 152, 146, {"toCut": {"shape": 25}, "clean": true, "tol": undefined});
        stepVol(c, st, 152, "cut");
        atStep(st, 178);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 179);
        cqWorkplane(c, st, 154, 153, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 180);
        cqPolyline(c, st, 155, 154, {"listOfXYTuple": [[-20.57456547520727, 11.344090704029275], [-18.526694475207268, 4.434406704029276], [-19.485471524792732, 4.150247295970725], [-21.533342524792733, 11.059931295970726]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 181);
        cqClose(c, st, 156, 155, {});
        atStep(st, 182);
        cqExtrude(c, st, 157, 156, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 157, "extrude");
        atStep(st, 183);
        cqVal(c, st, 26, 157);
        atStep(st, 184);
        cqCut(c, st, 158, 152, {"toCut": {"shape": 26}, "clean": true, "tol": undefined});
        stepVol(c, st, 158, "cut");
        atStep(st, 185);
        cqWorkplaneNew(c, st, 159, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 186);
        cqWorkplane(c, st, 160, 159, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 187);
        cqPolyline(c, st, 161, 160, {"listOfXYTuple": [[-28.843611434388773, 7.68935276950366], [-21.285810434388775, 11.645003769503662], [-20.822097565611227, 10.75901823049634], [-28.379898565611224, 6.80336723049634]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 188);
        cqClose(c, st, 162, 161, {});
        atStep(st, 189);
        cqExtrude(c, st, 163, 162, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 163, "extrude");
        atStep(st, 190);
        cqVal(c, st, 27, 163);
        atStep(st, 191);
        cqCut(c, st, 164, 158, {"toCut": {"shape": 27}, "clean": true, "tol": undefined});
        stepVol(c, st, 164, "cut");
        atStep(st, 192);
        cqWorkplaneNew(c, st, 165, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 193);
        cqWorkplane(c, st, 166, 165, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 194);
        cqPolyline(c, st, 167, 166, {"listOfXYTuple": [[23.31423174135621, -3.0516600385969244], [23.666755741356205, -0.3891200385969244], [24.658104258643796, -0.5203759614030756], [24.305580258643797, -3.1829159614030753]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 195);
        cqClose(c, st, 168, 167, {});
        atStep(st, 196);
        cqExtrude(c, st, 169, 168, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 169, "extrude");
        atStep(st, 197);
        cqVal(c, st, 28, 169);
        atStep(st, 198);
        cqCut(c, st, 170, 164, {"toCut": {"shape": 28}, "clean": true, "tol": undefined});
        stepVol(c, st, 170, "cut");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 200);
        cqWorkplane(c, st, 172, 171, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 201);
        cqPolyline(c, st, 173, 172, {"listOfXYTuple": [[24.30116247021172, -3.024190843893465], [24.736767470211717, -5.322797843893465], [23.754254529788284, -5.508992156106535], [23.318649529788285, -3.210385156106535]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 202);
        cqClose(c, st, 174, 173, {});
        atStep(st, 203);
        cqExtrude(c, st, 175, 174, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 175, "extrude");
        atStep(st, 204);
        cqVal(c, st, 29, 175);
        atStep(st, 205);
        cqCut(c, st, 176, 170, {"toCut": {"shape": 29}, "clean": true, "tol": undefined});
        stepVol(c, st, 176, "cut");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 177, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 207);
        cqWorkplane(c, st, 178, 177, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 208);
        cqPolyline(c, st, 179, 178, {"listOfXYTuple": [[24.454368587176617, -4.961605985958539], [47.26638758717662, -15.44934198595854], [46.84867241282338, -16.35792001404146], [24.036653412823384, -5.8701840140414605]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 209);
        cqClose(c, st, 180, 179, {});
        atStep(st, 210);
        cqExtrude(c, st, 181, 180, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 181, "extrude");
        atStep(st, 211);
        cqVal(c, st, 30, 181);
        atStep(st, 212);
        cqCut(c, st, 182, 176, {"toCut": {"shape": 30}, "clean": true, "tol": undefined});
        stepVol(c, st, 182, "cut");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 183, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 214);
        cqWorkplane(c, st, 184, 183, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 215);
        cqPolyline(c, st, 185, 184, {"listOfXYTuple": [[35.560951569419565, 4.858879740719215], [47.489749569419565, -15.652262259280786], [46.625310430580434, -16.154999740719216], [34.696512430580434, 4.356142259280785]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 216);
        cqClose(c, st, 186, 185, {});
        atStep(st, 217);
        cqExtrude(c, st, 187, 186, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 187, "extrude");
        atStep(st, 218);
        cqVal(c, st, 31, 187);
        atStep(st, 219);
        cqCut(c, st, 188, 182, {"toCut": {"shape": 31}, "clean": true, "tol": undefined});
        stepVol(c, st, 188, "cut");
        atStep(st, 220);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 221);
        cqWorkplane(c, st, 190, 189, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 222);
        cqPolyline(c, st, 191, 190, {"listOfXYTuple": [[23.952870578254487, -0.0007823088319499072], [34.91917257825448, 5.06147669116805], [35.33829142174552, 4.15354530883195], [24.37198942174551, -0.90871369116805]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 223);
        cqClose(c, st, 192, 191, {});
        atStep(st, 224);
        cqExtrude(c, st, 193, 192, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 193, "extrude");
        atStep(st, 225);
        cqVal(c, st, 32, 193);
        atStep(st, 226);
        cqCut(c, st, 194, 188, {"toCut": {"shape": 32}, "clean": true, "tol": undefined});
        stepVol(c, st, 194, "cut");
        atStep(st, 227);
        cqWorkplaneNew(c, st, 195, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 228);
        cqWorkplane(c, st, 196, 195, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 229);
        cqPolyline(c, st, 197, 196, {"listOfXYTuple": [[-49.822409057800925, 12.36943660638308], [-45.16623405780093, 10.60029460638308], [-45.52141594219908, 9.66549739361692], [-50.177590942199075, 11.43463939361692]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 230);
        cqClose(c, st, 198, 197, {});
        atStep(st, 231);
        cqExtrude(c, st, 199, 198, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 199, "extrude");
        atStep(st, 232);
        cqVal(c, st, 33, 199);
        atStep(st, 233);
        cqCut(c, st, 200, 194, {"toCut": {"shape": 33}, "clean": true, "tol": undefined});
        stepVol(c, st, 200, "cut");
        atStep(st, 234);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 235);
        cqWorkplane(c, st, 202, 201, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 236);
        cqPolyline(c, st, 203, 202, {"listOfXYTuple": [[-45.70998636611552, 10.473374859790765], [-38.56574536611552, 18.15650985979076], [-37.83342263388448, 17.47555214020924], [-44.97766363388448, 9.792417140209238]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 237);
        cqClose(c, st, 204, 203, {});
        atStep(st, 238);
        cqExtrude(c, st, 205, 204, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 205, "extrude");
        atStep(st, 239);
        cqVal(c, st, 34, 205);
        atStep(st, 240);
        cqCut(c, st, 206, 200, {"toCut": {"shape": 34}, "clean": true, "tol": undefined});
        stepVol(c, st, 206, "cut");
        atStep(st, 241);
        cqWorkplaneNew(c, st, 207, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 242);
        cqWorkplane(c, st, 208, 207, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 243);
        cqPolyline(c, st, 209, 208, {"listOfXYTuple": [[-38.98613987280747, 18.652346162460944], [-38.02090087280747, 18.283013162460943], [-38.37826712719253, 17.349048837539055], [-39.343506127192526, 17.718381837539056]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 244);
        cqClose(c, st, 210, 209, {});
        atStep(st, 245);
        cqExtrude(c, st, 211, 210, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 211, "extrude");
        atStep(st, 246);
        cqVal(c, st, 35, 211);
        atStep(st, 247);
        cqCut(c, st, 212, 206, {"toCut": {"shape": 35}, "clean": true, "tol": undefined});
        stepVol(c, st, 212, "cut");
        atStep(st, 248);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 249);
        cqWorkplane(c, st, 214, 213, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 250);
        cqPolyline(c, st, 215, 214, {"listOfXYTuple": [[-50.18246586143731, 14.40388414190773], [-39.347288861437306, 18.65088114190773], [-38.98235713856269, 17.71984685809227], [-49.81753413856269, 13.47284985809227]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 251);
        cqClose(c, st, 216, 215, {});
        atStep(st, 252);
        cqExtrude(c, st, 217, 216, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 217, "extrude");
        atStep(st, 253);
        cqVal(c, st, 36, 217);
        atStep(st, 254);
        cqCut(c, st, 218, 212, {"toCut": {"shape": 36}, "clean": true, "tol": undefined});
        stepVol(c, st, 218, "cut");
        atStep(st, 255);
        cqWorkplaneNew(c, st, 219, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 256);
        cqWorkplane(c, st, 220, 219, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 257);
        cqPolyline(c, st, 221, 220, {"listOfXYTuple": [[-13.11890526925106, -10.416101252207614], [-6.875503269251061, -8.339362252207614], [-6.55987673074894, -9.288245747792384], [-12.80327873074894, -11.364984747792384]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 258);
        cqClose(c, st, 222, 221, {});
        atStep(st, 259);
        cqExtrude(c, st, 223, 222, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 223, "extrude");
        atStep(st, 260);
        cqVal(c, st, 37, 223);
        atStep(st, 261);
        cqCut(c, st, 224, 218, {"toCut": {"shape": 37}, "clean": true, "tol": undefined});
        stepVol(c, st, 224, "cut");
        atStep(st, 262);
        cqWorkplaneNew(c, st, 225, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 263);
        cqWorkplane(c, st, 226, 225, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 264);
        cqPolyline(c, st, 227, 226, {"listOfXYTuple": [[-18.146783408058894, -20.78174690268971], [-13.413873408058896, -10.67843690268971], [-12.508310591941104, -11.10264909731029], [-17.241220591941104, -21.20595909731029]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 265);
        cqClose(c, st, 228, 227, {});
        atStep(st, 266);
        cqExtrude(c, st, 229, 228, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 229, "extrude");
        atStep(st, 267);
        cqVal(c, st, 38, 229);
        atStep(st, 268);
        cqCut(c, st, 230, 224, {"toCut": {"shape": 38}, "clean": true, "tol": undefined});
        stepVol(c, st, 230, "cut");
        atStep(st, 269);
        cqWorkplaneNew(c, st, 231, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 270);
        cqWorkplane(c, st, 232, 231, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 271);
        cqPolyline(c, st, 233, 232, {"listOfXYTuple": [[-17.445389437314567, -20.560042219042472], [-3.8519524373145666, -28.35030321904247], [-4.3491775626854325, -29.217924780957528], [-17.942614562685435, -21.42766378095753]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 272);
        cqClose(c, st, 234, 233, {});
        atStep(st, 273);
        cqExtrude(c, st, 235, 234, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 235, "extrude");
        atStep(st, 274);
        cqVal(c, st, 39, 235);
        atStep(st, 275);
        cqCut(c, st, 236, 230, {"toCut": {"shape": 39}, "clean": true, "tol": undefined});
        stepVol(c, st, 236, "cut");
        atStep(st, 276);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 277);
        cqWorkplane(c, st, 238, 237, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 278);
        cqPolyline(c, st, 239, 238, {"listOfXYTuple": [[-6.221929053082022, -8.748834133823525], [-3.6048040530820207, -28.719144133823523], [-4.596325946917978, -28.849083866176475], [-7.213450946917979, -8.878773866176473]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 279);
        cqClose(c, st, 240, 239, {});
        atStep(st, 280);
        cqExtrude(c, st, 241, 240, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 241, "extrude");
        atStep(st, 281);
        cqVal(c, st, 40, 241);
        atStep(st, 282);
        cqCut(c, st, 242, 236, {"toCut": {"shape": 40}, "clean": true, "tol": undefined});
        stepVol(c, st, 242, "cut");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 243, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 284);
        cqWorkplane(c, st, 244, 243, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 285);
        cqPolyline(c, st, 245, 244, {"listOfXYTuple": [[27.135062309290863, 30.43174655946048], [31.56721830929087, 27.842953559460483], [31.062857690709137, 26.97946044053952], [26.63070169070913, 29.56825344053952]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 286);
        cqClose(c, st, 246, 245, {});
        atStep(st, 287);
        cqExtrude(c, st, 247, 246, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 247, "extrude");
        atStep(st, 288);
        cqVal(c, st, 41, 247);
        atStep(st, 289);
        cqCut(c, st, 248, 242, {"toCut": {"shape": 41}, "clean": true, "tol": undefined});
        stepVol(c, st, 248, "cut");
        atStep(st, 290);
        cqWorkplaneNew(c, st, 249, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 291);
        cqWorkplane(c, st, 250, 249, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 292);
        cqPolyline(c, st, 251, 250, {"listOfXYTuple": [[31.05371882735551, 27.837484245473412], [35.27669082735551, 30.42627724547341], [35.79932917264449, 29.57372275452659], [31.576357172644492, 26.98492975452659]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 293);
        cqClose(c, st, 252, 251, {});
        atStep(st, 294);
        cqExtrude(c, st, 253, 252, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 253, "extrude");
        atStep(st, 295);
        cqVal(c, st, 42, 253);
        atStep(st, 296);
        cqCut(c, st, 254, 248, {"toCut": {"shape": 42}, "clean": true, "tol": undefined});
        stepVol(c, st, 254, "cut");
        atStep(st, 297);
        cqWorkplaneNew(c, st, 255, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 298);
        cqWorkplane(c, st, 256, 255, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 299);
        cqPolyline(c, st, 257, 256, {"listOfXYTuple": [[10.787409788210406, 30.09082411101804], [12.217629788210406, 22.34824211101804], [11.234266211789596, 22.16659388898196], [9.804046211789595, 29.90917588898196]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 300);
        cqClose(c, st, 258, 257, {});
        atStep(st, 301);
        cqExtrude(c, st, 259, 258, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 259, "extrude");
        atStep(st, 302);
        cqVal(c, st, 43, 259);
        atStep(st, 303);
        cqCut(c, st, 260, 254, {"toCut": {"shape": 43}, "clean": true, "tol": undefined});
        stepVol(c, st, 260, "cut");
        atStep(st, 304);
        cqWorkplaneNew(c, st, 261, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 305);
        cqWorkplane(c, st, 262, 261, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 306);
        cqPolyline(c, st, 263, 262, {"listOfXYTuple": [[11.68983034512678, 22.756111808871196], [23.261037345126777, 23.594150808871195], [23.333272654873223, 22.596763191128804], [11.762065654873222, 21.75872419112881]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 307);
        cqClose(c, st, 264, 263, {});
        atStep(st, 308);
        cqExtrude(c, st, 265, 264, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 265, "extrude");
        atStep(st, 309);
        cqVal(c, st, 44, 265);
        atStep(st, 310);
        cqCut(c, st, 266, 260, {"toCut": {"shape": 44}, "clean": true, "tol": undefined});
        stepVol(c, st, 266, "cut");
        atStep(st, 311);
        cqWorkplaneNew(c, st, 267, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 312);
        cqWorkplane(c, st, 268, 267, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 313);
        cqPolyline(c, st, 269, 268, {"listOfXYTuple": [[23.21875619376135, 30.041614861359427], [23.795420193761352, 23.137071861359427], [22.79888980623865, 23.053842138640572], [22.222225806238647, 29.958385138640573]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 314);
        cqClose(c, st, 270, 269, {});
        atStep(st, 315);
        cqExtrude(c, st, 271, 270, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 271, "extrude");
        atStep(st, 316);
        cqVal(c, st, 45, 271);
        atStep(st, 317);
        cqCut(c, st, 272, 266, {"toCut": {"shape": 45}, "clean": true, "tol": undefined});
        stepVol(c, st, 272, "cut");
        atStep(st, 318);
        cqWorkplaneNew(c, st, 273, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 319);
        cqWorkplane(c, st, 274, 273, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 320);
        cqPolyline(c, st, 275, 274, {"listOfXYTuple": [[-45.0825673952154, 10.55921098207571], [-38.629658395215394, 6.604689982075708], [-39.1521736047846, 5.752060017924292], [-45.605082604784606, 9.706581017924291]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 321);
        cqClose(c, st, 276, 275, {});
        atStep(st, 322);
        cqExtrude(c, st, 277, 276, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 277, "extrude");
        atStep(st, 323);
        cqVal(c, st, 46, 277);
        atStep(st, 324);
        cqCut(c, st, 278, 272, {"toCut": {"shape": 46}, "clean": true, "tol": undefined});
        stepVol(c, st, 278, "cut");
        atStep(st, 325);
        cqWorkplaneNew(c, st, 279, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 326);
        cqWorkplane(c, st, 280, 279, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 327);
        cqPolyline(c, st, 281, 280, {"listOfXYTuple": [[-39.286488887762246, 6.484188816671902], [-35.864813887762246, 10.910155816671905], [-35.07366811223775, 10.2985281833281], [-38.49534311223775, 5.872561183328098]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 328);
        cqClose(c, st, 282, 281, {});
        atStep(st, 329);
        cqExtrude(c, st, 283, 282, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 283, "extrude");
        atStep(st, 330);
        cqVal(c, st, 47, 283);
        atStep(st, 331);
        cqCut(c, st, 284, 278, {"toCut": {"shape": 47}, "clean": true, "tol": undefined});
        stepVol(c, st, 284, "cut");
        atStep(st, 332);
        cqWorkplaneNew(c, st, 285, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 333);
        cqWorkplane(c, st, 286, 285, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 334);
        cqPolyline(c, st, 287, 286, {"listOfXYTuple": [[-35.93492978882238, 10.786369338510298], [-33.557146788822386, 16.869559338510296], [-32.62576921117762, 16.505504661489706], [-35.00355221117761, 10.422314661489704]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 335);
        cqClose(c, st, 288, 287, {});
        atStep(st, 336);
        cqExtrude(c, st, 289, 288, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 289, "extrude");
        atStep(st, 337);
        cqVal(c, st, 48, 289);
        atStep(st, 338);
        cqCut(c, st, 290, 284, {"toCut": {"shape": 48}, "clean": true, "tol": undefined});
        stepVol(c, st, 290, "cut");
        atStep(st, 339);
        cqWorkplaneNew(c, st, 291, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 340);
        cqWorkplane(c, st, 292, 291, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 341);
        cqPolyline(c, st, 293, 292, {"listOfXYTuple": [[-33.03195818338891, 17.541513599065954], [-32.66629218338891, 16.950655599065954], [-33.516623816611094, 16.42440840093405], [-33.88228981661109, 17.01526640093405]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 342);
        cqClose(c, st, 294, 293, {});
        atStep(st, 343);
        cqExtrude(c, st, 295, 294, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 295, "extrude");
        atStep(st, 344);
        cqVal(c, st, 49, 295);
        atStep(st, 345);
        cqCut(c, st, 296, 290, {"toCut": {"shape": 49}, "clean": true, "tol": undefined});
        stepVol(c, st, 296, "cut");
        atStep(st, 346);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 347);
        cqWorkplane(c, st, 298, 297, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 348);
        cqPolyline(c, st, 299, 298, {"listOfXYTuple": [[-38.14326101407883, 18.31284859354609], [-33.40080101407883, 17.77520759354609], [-33.51344698592117, 16.781572406453915], [-38.25590698592117, 17.319213406453912]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 349);
        cqClose(c, st, 300, 299, {});
        atStep(st, 350);
        cqExtrude(c, st, 301, 300, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 301, "extrude");
        atStep(st, 351);
        cqVal(c, st, 50, 301);
        atStep(st, 352);
        cqCut(c, st, 302, 296, {"toCut": {"shape": 50}, "clean": true, "tol": undefined});
        stepVol(c, st, 302, "cut");
        atStep(st, 353);
        cqWorkplaneNew(c, st, 303, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 354);
        cqWorkplane(c, st, 304, 303, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 355);
        cqPolyline(c, st, 305, 304, {"listOfXYTuple": [[24.963885128749833, 9.912531410521185], [25.034526128749835, 8.964005410521185], [24.037287871250168, 8.889736589478815], [23.96664687125017, 9.838262589478814]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 356);
        cqClose(c, st, 306, 305, {});
        atStep(st, 357);
        cqExtrude(c, st, 307, 306, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 307, "extrude");
        atStep(st, 358);
        cqVal(c, st, 51, 307);
        atStep(st, 359);
        cqCut(c, st, 308, 302, {"toCut": {"shape": 51}, "clean": true, "tol": undefined});
        stepVol(c, st, 308, "cut");
        atStep(st, 360);
        cqWorkplaneNew(c, st, 309, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 361);
        cqWorkplane(c, st, 310, 309, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 362);
        cqPolyline(c, st, 311, 310, {"listOfXYTuple": [[24.595777119096123, 9.423273627752328], [35.36998111909612, 8.123818627752328], [35.25024088090388, 7.131013372247672], [24.47603688090388, 8.430468372247672]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 363);
        cqClose(c, st, 312, 311, {});
        atStep(st, 364);
        cqExtrude(c, st, 313, 312, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 313, "extrude");
        atStep(st, 365);
        cqVal(c, st, 52, 313);
        atStep(st, 366);
        cqCut(c, st, 314, 308, {"toCut": {"shape": 52}, "clean": true, "tol": undefined});
        stepVol(c, st, 314, "cut");
        atStep(st, 367);
        cqWorkplaneNew(c, st, 315, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 368);
        cqWorkplane(c, st, 316, 315, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 369);
        cqPolyline(c, st, 317, 316, {"listOfXYTuple": [[34.89370110458992, 7.9041881066230655], [37.75896410458992, 12.215041106623064], [38.591783895410074, 11.661496893376937], [35.726520895410076, 7.350643893376935]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 370);
        cqClose(c, st, 318, 317, {});
        atStep(st, 371);
        cqExtrude(c, st, 319, 318, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 319, "extrude");
        atStep(st, 372);
        cqVal(c, st, 53, 319);
        atStep(st, 373);
        cqCut(c, st, 320, 314, {"toCut": {"shape": 53}, "clean": true, "tol": undefined});
        stepVol(c, st, 320, "cut");
        atStep(st, 374);
        cqWorkplaneNew(c, st, 321, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 375);
        cqWorkplane(c, st, 322, 321, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 376);
        cqPolyline(c, st, 323, 322, {"listOfXYTuple": [[37.711200584999695, 12.12412658205399], [38.1942705849997, 13.33057858205399], [39.122617415000306, 12.958863417946011], [38.6395474150003, 11.75241141794601]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 377);
        cqClose(c, st, 324, 323, {});
        atStep(st, 378);
        cqExtrude(c, st, 325, 324, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 325, "extrude");
        atStep(st, 379);
        cqVal(c, st, 54, 325);
        atStep(st, 380);
        cqCut(c, st, 326, 320, {"toCut": {"shape": 54}, "clean": true, "tol": undefined});
        stepVol(c, st, 326, "cut");
        atStep(st, 381);
        cqWorkplaneNew(c, st, 327, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 382);
        cqWorkplane(c, st, 328, 327, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 383);
        cqPolyline(c, st, 329, 328, {"listOfXYTuple": [[28.4917019509577, 18.479045370910075], [38.8712489509577, 13.597174370910077], [38.445639049042306, 12.692267629089924], [28.0660920490423, 17.574138629089926]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 384);
        cqClose(c, st, 330, 329, {});
        atStep(st, 385);
        cqExtrude(c, st, 331, 330, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 331, "extrude");
        atStep(st, 386);
        cqVal(c, st, 55, 331);
        atStep(st, 387);
        cqCut(c, st, 332, 326, {"toCut": {"shape": 55}, "clean": true, "tol": undefined});
        stepVol(c, st, 332, "cut");
        atStep(st, 388);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 389);
        cqWorkplane(c, st, 334, 333, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 390);
        cqPolyline(c, st, 335, 334, {"listOfXYTuple": [[-36.72907830493446, -29.57846396831807], [-20.864444304934462, -19.45832096831807], [-20.326643695065535, -20.301393031681933], [-36.19127769506554, -30.42153603168193]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 391);
        cqClose(c, st, 336, 335, {});
        atStep(st, 392);
        cqExtrude(c, st, 337, 336, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 337, "extrude");
        atStep(st, 393);
        cqVal(c, st, 56, 337);
        atStep(st, 394);
        cqCut(c, st, 338, 332, {"toCut": {"shape": 56}, "clean": true, "tol": undefined});
        stepVol(c, st, 338, "cut");
        atStep(st, 395);
        cqWorkplaneNew(c, st, 339, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 396);
        cqWorkplane(c, st, 340, 339, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 397);
        cqPolyline(c, st, 341, 340, {"listOfXYTuple": [[-23.88995075859227, -16.04876910743599], [-20.250555758592274, -19.51794110743599], [-20.940532241407727, -20.24177289256401], [-24.579927241407727, -16.77260089256401]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 398);
        cqClose(c, st, 342, 341, {});
        atStep(st, 399);
        cqExtrude(c, st, 343, 342, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 343, "extrude");
        atStep(st, 400);
        cqVal(c, st, 57, 343);
        atStep(st, 401);
        cqCut(c, st, 344, 338, {"toCut": {"shape": 57}, "clean": true, "tol": undefined});
        stepVol(c, st, 344, "cut");
        atStep(st, 402);
        cqWorkplaneNew(c, st, 345, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 403);
        cqWorkplane(c, st, 346, 345, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 404);
        cqPolyline(c, st, 347, 346, {"listOfXYTuple": [[-39.922119016682785, -29.86699457269232], [-37.94202001668278, -22.69151257269232], [-36.97804998331721, -22.95752342730768], [-38.958148983317216, -30.13300542730768]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 405);
        cqClose(c, st, 348, 347, {});
        atStep(st, 406);
        cqExtrude(c, st, 349, 348, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 349, "extrude");
        atStep(st, 407);
        cqVal(c, st, 58, 349);
        atStep(st, 408);
        cqCut(c, st, 350, 344, {"toCut": {"shape": 58}, "clean": true, "tol": undefined});
        stepVol(c, st, 350, "cut");
        atStep(st, 409);
        cqWorkplaneNew(c, st, 351, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 410);
        cqWorkplane(c, st, 352, 351, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 411);
        cqPolyline(c, st, 353, 352, {"listOfXYTuple": [[-38.39808806345764, 6.262758795621005], [-36.72312306345764, -3.5195642043789945], [-37.70877893654235, -3.6883317956210058], [-39.38374393654235, 6.0939912043789946]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 412);
        cqClose(c, st, 354, 353, {});
        atStep(st, 413);
        cqExtrude(c, st, 355, 354, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 355, "extrude");
        atStep(st, 414);
        cqVal(c, st, 59, 355);
        atStep(st, 415);
        cqCut(c, st, 356, 350, {"toCut": {"shape": 59}, "clean": true, "tol": undefined});
        stepVol(c, st, 356, "cut");
        atStep(st, 416);
        cqWorkplaneNew(c, st, 357, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 417);
        cqWorkplane(c, st, 358, 357, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 418);
        cqPolyline(c, st, 359, 358, {"listOfXYTuple": [[-35.24934882241791, 11.05339370107484], [-28.39186282241791, 7.695411701074839], [-28.831647177582088, 6.797308298925161], [-35.689133177582086, 10.15529029892516]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 419);
        cqClose(c, st, 360, 359, {});
        atStep(st, 420);
        cqExtrude(c, st, 361, 360, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 361, "extrude");
        atStep(st, 421);
        cqVal(c, st, 60, 361);
        atStep(st, 422);
        cqCut(c, st, 362, 356, {"toCut": {"shape": 60}, "clean": true, "tol": undefined});
        stepVol(c, st, 362, "cut");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 363, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 424);
        cqWorkplane(c, st, 364, 363, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 425);
        cqPolyline(c, st, 365, 364, {"listOfXYTuple": [[31.67362460957463, 27.759654475860813], [34.71324660957463, 24.631585475860813], [33.99607339042537, 23.93469052413919], [30.95645139042537, 27.06275952413919]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 426);
        cqClose(c, st, 366, 365, {});
        atStep(st, 427);
        cqExtrude(c, st, 367, 366, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 367, "extrude");
        atStep(st, 428);
        cqVal(c, st, 61, 367);
        atStep(st, 429);
        cqCut(c, st, 368, 362, {"toCut": {"shape": 61}, "clean": true, "tol": undefined});
        stepVol(c, st, 368, "cut");
        atStep(st, 430);
        cqWorkplaneNew(c, st, 369, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 431);
        cqWorkplane(c, st, 370, 369, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 432);
        cqPolyline(c, st, 371, 370, {"listOfXYTuple": [[34.23081461562534, 24.76755757100131], [41.094241615625336, 26.522242571001307], [41.34193238437466, 25.553403428998692], [34.478505384374664, 23.798718428998693]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 433);
        cqClose(c, st, 372, 371, {});
        atStep(st, 434);
        cqExtrude(c, st, 373, 372, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 373, "extrude");
        atStep(st, 435);
        cqVal(c, st, 62, 373);
        atStep(st, 436);
        cqCut(c, st, 374, 368, {"toCut": {"shape": 62}, "clean": true, "tol": undefined});
        stepVol(c, st, 374, "cut");
        atStep(st, 437);
        cqWorkplaneNew(c, st, 375, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 438);
        cqWorkplane(c, st, 376, 375, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 439);
        cqPolyline(c, st, 377, 376, {"listOfXYTuple": [[40.72664964425923, 26.12996053514463], [41.46950164425924, 30.09213753514463], [42.45237635574077, 29.90786246485537], [41.70952435574076, 25.94568546485537]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 440);
        cqClose(c, st, 378, 377, {});
        atStep(st, 441);
        cqExtrude(c, st, 379, 378, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 379, "extrude");
        atStep(st, 442);
        cqVal(c, st, 63, 379);
        atStep(st, 443);
        cqCut(c, st, 380, 374, {"toCut": {"shape": 63}, "clean": true, "tol": undefined});
        stepVol(c, st, 380, "cut");
        atStep(st, 444);
        cqWorkplaneNew(c, st, 381, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 445);
        cqWorkplane(c, st, 382, 381, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 446);
        cqPolyline(c, st, 383, 382, {"listOfXYTuple": [[-33.956962416780556, 17.29110051180351], [-33.645515416780555, 29.53869351180351], [-32.645838583219444, 29.513272488196492], [-32.957285583219445, 17.265679488196493]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 447);
        cqClose(c, st, 384, 383, {});
        atStep(st, 448);
        cqExtrude(c, st, 385, 384, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 385, "extrude");
        atStep(st, 449);
        cqVal(c, st, 64, 385);
        atStep(st, 450);
        cqCut(c, st, 386, 380, {"toCut": {"shape": 64}, "clean": true, "tol": undefined});
        stepVol(c, st, 386, "cut");
        atStep(st, 451);
        cqWorkplaneNew(c, st, 387, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 452);
        cqWorkplane(c, st, 388, 387, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 453);
        cqPolyline(c, st, 389, 388, {"listOfXYTuple": [[-32.993693231623105, 17.17788090645765], [-20.9177952316231, 14.770212906457648], [-21.113324768376906, 13.789515093542352], [-33.1892227683769, 16.197183093542353]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 454);
        cqClose(c, st, 390, 389, {});
        atStep(st, 455);
        cqExtrude(c, st, 391, 390, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 391, "extrude");
        atStep(st, 456);
        cqVal(c, st, 65, 391);
        atStep(st, 457);
        cqCut(c, st, 392, 386, {"toCut": {"shape": 65}, "clean": true, "tol": undefined});
        stepVol(c, st, 392, "cut");
        atStep(st, 458);
        cqWorkplaneNew(c, st, 393, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 459);
        cqWorkplane(c, st, 394, 393, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 460);
        cqPolyline(c, st, 395, 394, {"listOfXYTuple": [[-21.397253614575153, 14.602834563043375], [-18.230051614575153, 18.345902563043374], [-17.46666438542485, 17.699961436956624], [-20.63386638542485, 13.956893436956625]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 461);
        cqClose(c, st, 396, 395, {});
        atStep(st, 462);
        cqExtrude(c, st, 397, 396, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 397, "extrude");
        atStep(st, 463);
        cqVal(c, st, 66, 397);
        atStep(st, 464);
        cqCut(c, st, 398, 392, {"toCut": {"shape": 66}, "clean": true, "tol": undefined});
        stepVol(c, st, 398, "cut");
        atStep(st, 465);
        cqWorkplaneNew(c, st, 399, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 466);
        cqWorkplane(c, st, 400, 399, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 467);
        cqPolyline(c, st, 401, 400, {"listOfXYTuple": [[-32.84517462918114, 29.92560572849807], [-17.547855629181136, 18.42255472849807], [-18.148860370818863, 17.62330927150193], [-33.44617937081886, 29.12636027150193]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 468);
        cqClose(c, st, 402, 401, {});
        atStep(st, 469);
        cqExtrude(c, st, 403, 402, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 403, "extrude");
        atStep(st, 470);
        cqVal(c, st, 67, 403);
        atStep(st, 471);
        cqCut(c, st, 404, 398, {"toCut": {"shape": 67}, "clean": true, "tol": undefined});
        stepVol(c, st, 404, "cut");
        atStep(st, 472);
        cqWorkplaneNew(c, st, 405, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 473);
        cqWorkplane(c, st, 406, 405, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 474);
        cqPolyline(c, st, 407, 406, {"listOfXYTuple": [[-43.92908410398749, 23.479007382504893], [-38.817575103987494, 18.545112382504893], [-39.5120708960125, 17.825615617495107], [-44.6235798960125, 22.759510617495103]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 475);
        cqClose(c, st, 408, 407, {});
        atStep(st, 476);
        cqExtrude(c, st, 409, 408, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 409, "extrude");
        atStep(st, 477);
        cqVal(c, st, 68, 409);
        atStep(st, 478);
        cqCut(c, st, 410, 404, {"toCut": {"shape": 68}, "clean": true, "tol": undefined});
        stepVol(c, st, 410, "cut");
        atStep(st, 479);
        cqWorkplaneNew(c, st, 411, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 480);
        cqWorkplane(c, st, 412, 411, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 481);
        cqPolyline(c, st, 413, 412, {"listOfXYTuple": [[-50.055641165199326, 22.975229416083803], [-44.33197316519932, 23.6161534160838], [-44.22069083480067, 22.6223645839162], [-49.944358834800674, 21.9814405839162]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 482);
        cqClose(c, st, 414, 413, {});
        atStep(st, 483);
        cqExtrude(c, st, 415, 414, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 415, "extrude");
        atStep(st, 484);
        cqVal(c, st, 69, 415);
        atStep(st, 485);
        cqCut(c, st, 416, 410, {"toCut": {"shape": 69}, "clean": true, "tol": undefined});
        stepVol(c, st, 416, "cut");
        atStep(st, 486);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 487);
        cqWorkplane(c, st, 418, 417, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 488);
        cqPolyline(c, st, 419, 418, {"listOfXYTuple": [[-3.7100070294833527, -28.47192099612913], [-2.7380870294833533, -29.68780699612913], [-3.519202970516647, -30.31219300387087], [-4.491122970516646, -29.09630700387087]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 489);
        cqClose(c, st, 420, 419, {});
        atStep(st, 490);
        cqExtrude(c, st, 421, 420, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 421, "extrude");
        atStep(st, 491);
        cqVal(c, st, 70, 421);
        atStep(st, 492);
        cqCut(c, st, 422, 416, {"toCut": {"shape": 70}, "clean": true, "tol": undefined});
        stepVol(c, st, 422, "cut");
        atStep(st, 493);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 494);
        cqWorkplane(c, st, 424, 423, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 495);
        cqPolyline(c, st, 425, 424, {"listOfXYTuple": [[-19.65256506812305, -20.12754322196825], [-17.60409206812305, -20.502003221968252], [-17.78391193187695, -21.48570277803175], [-19.83238493187695, -21.111242778031748]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 496);
        cqClose(c, st, 426, 425, {});
        atStep(st, 497);
        cqExtrude(c, st, 427, 426, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 427, "extrude");
        atStep(st, 498);
        cqVal(c, st, 71, 427);
        atStep(st, 499);
        cqCut(c, st, 428, 422, {"toCut": {"shape": 71}, "clean": true, "tol": undefined});
        stepVol(c, st, 428, "cut");
        atStep(st, 500);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 501);
        cqWorkplane(c, st, 430, 429, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 502);
        cqPolyline(c, st, 431, 430, {"listOfXYTuple": [[-25.933261868954787, -29.738210994259457], [-20.168463868954785, -20.357603994259453], [-19.316486131045213, -20.881182005740545], [-25.081284131045216, -30.261789005740543]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 503);
        cqClose(c, st, 432, 431, {});
        atStep(st, 504);
        cqExtrude(c, st, 433, 432, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 433, "extrude");
        atStep(st, 505);
        cqVal(c, st, 72, 433);
        atStep(st, 506);
        cqCut(c, st, 434, 428, {"toCut": {"shape": 72}, "clean": true, "tol": undefined});
        stepVol(c, st, 434, "cut");
        atStep(st, 507);
        cqWorkplaneNew(c, st, 435, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 508);
        cqWorkplane(c, st, 436, 435, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 509);
        cqPolyline(c, st, 437, 436, {"listOfXYTuple": [[7.641529953637628, -0.1758755478087057], [8.933313953637628, -5.7448125478087055], [7.959178046362372, -5.970775452191295], [6.6673940463623715, -0.4018384521912942]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 510);
        cqClose(c, st, 438, 437, {});
        atStep(st, 511);
        cqExtrude(c, st, 439, 438, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 439, "extrude");
        atStep(st, 512);
        cqVal(c, st, 73, 439);
        atStep(st, 513);
        cqCut(c, st, 440, 434, {"toCut": {"shape": 73}, "clean": true, "tol": undefined});
        stepVol(c, st, 440, "cut");
        atStep(st, 514);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 515);
        cqWorkplane(c, st, 442, 441, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 516);
        cqPolyline(c, st, 443, 442, {"listOfXYTuple": [[-5.217190822670689, -2.378752356668697], [7.051598177329311, 0.2004476433313025], [7.257325822670689, -0.7781616433313024], [-5.011463177329311, -3.3573616433313025]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 517);
        cqClose(c, st, 444, 443, {});
        atStep(st, 518);
        cqExtrude(c, st, 445, 444, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 445, "extrude");
        atStep(st, 519);
        cqVal(c, st, 74, 445);
        atStep(st, 520);
        cqCut(c, st, 446, 440, {"toCut": {"shape": 74}, "clean": true, "tol": undefined});
        stepVol(c, st, 446, "cut");
        atStep(st, 521);
        cqWorkplaneNew(c, st, 447, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 522);
        cqWorkplane(c, st, 448, 447, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 523);
        cqPolyline(c, st, 449, 448, {"listOfXYTuple": [[-6.807956031255932, -6.013495222023141], [-5.583410031255932, -2.69496422202314], [-4.645243968744069, -3.0411497779768597], [-5.869789968744069, -6.35968077797686]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 524);
        cqClose(c, st, 450, 449, {});
        atStep(st, 525);
        cqExtrude(c, st, 451, 450, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 451, "extrude");
        atStep(st, 526);
        cqVal(c, st, 75, 451);
        atStep(st, 527);
        cqCut(c, st, 452, 446, {"toCut": {"shape": 75}, "clean": true, "tol": undefined});
        stepVol(c, st, 452, "cut");
        atStep(st, 528);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 529);
        cqWorkplane(c, st, 454, 453, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 530);
        cqPolyline(c, st, 455, 454, {"listOfXYTuple": [[-5.83953300813823, -6.160905855862743], [-5.755880008138229, -7.787377855862743], [-6.75455999186177, -7.838742144137258], [-6.838212991861771, -6.212270144137258]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 531);
        cqClose(c, st, 456, 455, {});
        atStep(st, 532);
        cqExtrude(c, st, 457, 456, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 457, "extrude");
        atStep(st, 533);
        cqVal(c, st, 76, 457);
        atStep(st, 534);
        cqCut(c, st, 458, 452, {"toCut": {"shape": 76}, "clean": true, "tol": undefined});
        stepVol(c, st, 458, "cut");
        atStep(st, 535);
        cqWorkplaneNew(c, st, 459, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 536);
        cqWorkplane(c, st, 460, 459, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 537);
        cqPolyline(c, st, 461, 460, {"listOfXYTuple": [[-6.121285589402388, -7.33133230341419], [5.496551410597612, -10.561431303414189], [5.228682589402388, -11.52488669658581], [-6.3891544105976115, -8.29478769658581]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 538);
        cqClose(c, st, 462, 461, {});
        atStep(st, 539);
        cqExtrude(c, st, 463, 462, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 463, "extrude");
        atStep(st, 540);
        cqVal(c, st, 77, 463);
        atStep(st, 541);
        cqCut(c, st, 464, 458, {"toCut": {"shape": 77}, "clean": true, "tol": undefined});
        stepVol(c, st, 464, "cut");
        atStep(st, 542);
        cqWorkplaneNew(c, st, 465, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 543);
        cqWorkplane(c, st, 466, 465, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 544);
        cqPolyline(c, st, 467, 466, {"listOfXYTuple": [[10.06860846983452, 14.669922298957804], [15.49323446983452, 15.748273298957804], [15.68820753016548, 14.767464701042195], [10.26358153016548, 13.689113701042196]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 545);
        cqClose(c, st, 468, 467, {});
        atStep(st, 546);
        cqExtrude(c, st, 469, 468, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 469, "extrude");
        atStep(st, 547);
        cqVal(c, st, 78, 469);
        atStep(st, 548);
        cqCut(c, st, 470, 464, {"toCut": {"shape": 78}, "clean": true, "tol": undefined});
        stepVol(c, st, 470, "cut");
        atStep(st, 549);
        cqWorkplaneNew(c, st, 471, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 550);
        cqWorkplane(c, st, 472, 471, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 551);
        cqPolyline(c, st, 473, 472, {"listOfXYTuple": [[6.655716755892195, 11.851642479109632], [9.842251755892196, 14.560471479109632], [10.489938244107805, 13.798564520890368], [7.303403244107805, 11.089735520890368]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 552);
        cqClose(c, st, 474, 473, {});
        atStep(st, 553);
        cqExtrude(c, st, 475, 474, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 475, "extrude");
        atStep(st, 554);
        cqVal(c, st, 79, 475);
        atStep(st, 555);
        cqCut(c, st, 476, 470, {"toCut": {"shape": 79}, "clean": true, "tol": undefined});
        stepVol(c, st, 476, "cut");
        atStep(st, 556);
        cqWorkplaneNew(c, st, 477, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 557);
        cqWorkplane(c, st, 478, 477, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 558);
        cqPolyline(c, st, 479, 478, {"listOfXYTuple": [[7.443823734975938, 11.656320851755543], [8.739980734975939, 8.414642851755543], [7.8114532650240625, 8.043379148244457], [6.5152962650240624, 11.285057148244457]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 559);
        cqClose(c, st, 480, 479, {});
        atStep(st, 560);
        cqExtrude(c, st, 481, 480, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 481, "extrude");
        atStep(st, 561);
        cqVal(c, st, 80, 481);
        atStep(st, 562);
        cqCut(c, st, 482, 476, {"toCut": {"shape": 80}, "clean": true, "tol": undefined});
        stepVol(c, st, 482, "cut");
        atStep(st, 563);
        cqWorkplaneNew(c, st, 483, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 564);
        cqWorkplane(c, st, 484, 483, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 565);
        cqPolyline(c, st, 485, 484, {"listOfXYTuple": [[8.383962061016799, 8.717153404187005], [23.1015340610168, 5.453547404187004], [22.8850439389832, 4.477262595812995], [8.167471938983201, 7.740868595812994]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 566);
        cqClose(c, st, 486, 485, {});
        atStep(st, 567);
        cqExtrude(c, st, 487, 486, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 487, "extrude");
        atStep(st, 568);
        cqVal(c, st, 81, 487);
        atStep(st, 569);
        cqCut(c, st, 488, 482, {"toCut": {"shape": 81}, "clean": true, "tol": undefined});
        stepVol(c, st, 488, "cut");
        atStep(st, 570);
        cqWorkplaneNew(c, st, 489, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 571);
        cqWorkplane(c, st, 490, 489, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 572);
        cqPolyline(c, st, 491, 490, {"listOfXYTuple": [[22.52736807371409, 5.146837330219499], [24.06998607371409, 9.1083033302195], [25.001827926285912, 8.7454386697805], [23.45920992628591, 4.7839726697805]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 573);
        cqClose(c, st, 492, 491, {});
        atStep(st, 574);
        cqExtrude(c, st, 493, 492, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 493, "extrude");
        atStep(st, 575);
        cqVal(c, st, 82, 493);
        atStep(st, 576);
        cqCut(c, st, 494, 488, {"toCut": {"shape": 82}, "clean": true, "tol": undefined});
        stepVol(c, st, 494, "cut");
        atStep(st, 577);
        cqWorkplaneNew(c, st, 495, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 578);
        cqWorkplane(c, st, 496, 495, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 579);
        cqPolyline(c, st, 497, 496, {"listOfXYTuple": [[-50.03101299372126, -5.112101732633679], [-39.92351499372126, -4.483964732633679], [-39.86148900627874, -5.4820392673663205], [-49.96898700627874, -6.11017626736632]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 580);
        cqClose(c, st, 498, 497, {});
        atStep(st, 581);
        cqExtrude(c, st, 499, 498, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 499, "extrude");
        atStep(st, 582);
        cqVal(c, st, 83, 499);
        atStep(st, 583);
        cqCut(c, st, 500, 494, {"toCut": {"shape": 83}, "clean": true, "tol": undefined});
        stepVol(c, st, 500, "cut");
        atStep(st, 584);
        cqWorkplaneNew(c, st, 501, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 585);
        cqWorkplane(c, st, 502, 501, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 586);
        cqPolyline(c, st, 503, 502, {"listOfXYTuple": [[-45.95828532553403, 0.656304038079101], [-39.574784325534026, -4.596924961920899], [-40.210219674465975, -5.369079038079101], [-46.59372067446598, -0.11585003807910103]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 587);
        cqClose(c, st, 504, 503, {});
        atStep(st, 588);
        cqExtrude(c, st, 505, 504, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 505, "extrude");
        atStep(st, 589);
        cqVal(c, st, 84, 505);
        atStep(st, 590);
        cqCut(c, st, 506, 500, {"toCut": {"shape": 84}, "clean": true, "tol": undefined});
        stepVol(c, st, 506, "cut");
        atStep(st, 591);
        cqWorkplaneNew(c, st, 507, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 592);
        cqWorkplane(c, st, 508, 507, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 593);
        cqPolyline(c, st, 509, 508, {"listOfXYTuple": [[-49.83753495037868, 2.0225610174366926], [-46.113537950378685, 0.7430960174366923], [-46.43846804962132, -0.2026420174366923], [-50.16246504962132, 1.0768229825633078]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 594);
        cqClose(c, st, 510, 509, {});
        atStep(st, 595);
        cqExtrude(c, st, 511, 510, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 511, "extrude");
        atStep(st, 596);
        cqVal(c, st, 85, 511);
        atStep(st, 597);
        cqCut(c, st, 512, 506, {"toCut": {"shape": 85}, "clean": true, "tol": undefined});
        stepVol(c, st, 512, "cut");
        atStep(st, 598);
        cqWorkplaneNew(c, st, 513, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 599);
        cqWorkplane(c, st, 514, 513, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 600);
        cqPolyline(c, st, 515, 514, {"listOfXYTuple": [[47.3552484520226, -15.501929962753062], [50.2977184520226, -17.68272496275306], [49.7022815479774, -18.48612703724694], [46.7598115479774, -16.30533203724694]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 601);
        cqClose(c, st, 516, 515, {});
        atStep(st, 602);
        cqExtrude(c, st, 517, 516, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 517, "extrude");
        atStep(st, 603);
        cqVal(c, st, 86, 517);
        atStep(st, 604);
        cqCut(c, st, 518, 512, {"toCut": {"shape": 86}, "clean": true, "tol": undefined});
        stepVol(c, st, 518, "cut");
        atStep(st, 605);
        cqWorkplaneNew(c, st, 519, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 606);
        cqWorkplane(c, st, 520, 519, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 607);
        cqPolyline(c, st, 521, 520, {"listOfXYTuple": [[19.018236711851447, -9.345637580183851], [23.915379711851447, -5.040377580183852], [24.57564228814855, -5.791412419816147], [19.67849928814855, -10.096672419816148]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 608);
        cqClose(c, st, 522, 521, {});
        atStep(st, 609);
        cqExtrude(c, st, 523, 522, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 523, "extrude");
        atStep(st, 610);
        cqVal(c, st, 87, 523);
        atStep(st, 611);
        cqCut(c, st, 524, 518, {"toCut": {"shape": 87}, "clean": true, "tol": undefined});
        stepVol(c, st, 524, "cut");
        atStep(st, 612);
        cqWorkplaneNew(c, st, 525, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 613);
        cqWorkplane(c, st, 526, 525, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 614);
        cqPolyline(c, st, 527, 526, {"listOfXYTuple": [[17.646812805931948, -17.546100398961435], [27.188262805931945, -29.691110398961435], [26.401911194068052, -30.308889601038565], [16.860461194068055, -18.16387960103857]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 615);
        cqClose(c, st, 528, 527, {});
        atStep(st, 616);
        cqExtrude(c, st, 529, 528, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 529, "extrude");
        atStep(st, 617);
        cqVal(c, st, 88, 529);
        atStep(st, 618);
        cqCut(c, st, 530, 524, {"toCut": {"shape": 88}, "clean": true, "tol": undefined});
        stepVol(c, st, 530, "cut");
        atStep(st, 619);
        cqWorkplaneNew(c, st, 531, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 620);
        cqWorkplane(c, st, 532, 531, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 621);
        cqPolyline(c, st, 533, 532, {"listOfXYTuple": [[-44.59753260852104, 23.50244324952719], [-36.38898160852104, 30.38318424952719], [-35.74658039147896, 29.61681575047281], [-43.95513139147896, 22.73607475047281]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 622);
        cqClose(c, st, 534, 533, {});
        atStep(st, 623);
        cqExtrude(c, st, 535, 534, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 535, "extrude");
        atStep(st, 624);
        cqVal(c, st, 89, 535);
        atStep(st, 625);
        cqCut(c, st, 536, 530, {"toCut": {"shape": 89}, "clean": true, "tol": undefined});
        stepVol(c, st, 536, "cut");
        atStep(st, 626);
        cqWorkplaneNew(c, st, 537, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 627);
        cqWorkplane(c, st, 538, 537, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 628);
        cqPolyline(c, st, 539, 538, {"listOfXYTuple": [[-32.776626535836435, 30.119334599972273], [-32.66012653583643, 29.645317599972273], [-33.631227464163565, 29.406648400027727], [-33.74772746416357, 29.880665400027727]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 629);
        cqClose(c, st, 540, 539, {});
        atStep(st, 630);
        cqExtrude(c, st, 541, 540, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 541, "extrude");
        atStep(st, 631);
        cqVal(c, st, 90, 541);
        atStep(st, 632);
        cqCut(c, st, 542, 536, {"toCut": {"shape": 90}, "clean": true, "tol": undefined});
        stepVol(c, st, 542, "cut");
        atStep(st, 633);
        cqWorkplaneNew(c, st, 543, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 634);
        cqWorkplane(c, st, 544, 543, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 635);
        cqPolyline(c, st, 545, 544, {"listOfXYTuple": [[-49.535118956825606, 9.85897545984593], [-45.81112195682561, 0.45430745984593024], [-46.7408840431744, 0.08614654015406978], [-50.464881043174394, 9.49081454015407]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 636);
        cqClose(c, st, 546, 545, {});
        atStep(st, 637);
        cqExtrude(c, st, 547, 546, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 547, "extrude");
        atStep(st, 638);
        cqVal(c, st, 91, 547);
        atStep(st, 639);
        cqCut(c, st, 548, 542, {"toCut": {"shape": 91}, "clean": true, "tol": undefined});
        stepVol(c, st, 548, "cut");
        atStep(st, 640);
        cqWorkplaneNew(c, st, 549, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 641);
        cqWorkplane(c, st, 550, 549, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 642);
        cqPolyline(c, st, 551, 550, {"listOfXYTuple": [[-39.79336350381719, -4.492928986486083], [-39.11644050381719, -4.629865986486083], [-39.31471749618281, -5.610012013513916], [-39.99164049618281, -5.473075013513917]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 643);
        cqClose(c, st, 552, 551, {});
        atStep(st, 644);
        cqExtrude(c, st, 553, 552, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 553, "extrude");
        atStep(st, 645);
        cqVal(c, st, 92, 553);
        atStep(st, 646);
        cqCut(c, st, 554, 548, {"toCut": {"shape": 92}, "clean": true, "tol": undefined});
        stepVol(c, st, 554, "cut");
        atStep(st, 647);
        cqWorkplaneNew(c, st, 555, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 648);
        cqWorkplane(c, st, 556, 555, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 649);
        cqPolyline(c, st, 557, 556, {"listOfXYTuple": [[8.270763533563844, 2.5317192027769124], [23.966446533563843, -2.642424797223087], [23.65336546643616, -3.5921512027769125], [7.957682466436157, 1.5819927972230872]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 650);
        cqClose(c, st, 558, 557, {});
        atStep(st, 651);
        cqExtrude(c, st, 559, 558, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 559, "extrude");
        atStep(st, 652);
        cqVal(c, st, 93, 559);
        atStep(st, 653);
        cqCut(c, st, 560, 554, {"toCut": {"shape": 93}, "clean": true, "tol": undefined});
        stepVol(c, st, 560, "cut");
        atStep(st, 654);
        cqWorkplaneNew(c, st, 561, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 655);
        cqWorkplane(c, st, 562, 561, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 656);
        cqPolyline(c, st, 563, 562, {"listOfXYTuple": [[6.691698971475994, -0.09951503573615816], [7.651459971475995, 2.2461979642638417], [8.576986028524008, 1.867514035736158], [7.617225028524006, -0.4781989642638418]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 657);
        cqClose(c, st, 564, 563, {});
        atStep(st, 658);
        cqExtrude(c, st, 565, 564, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 565, "extrude");
        atStep(st, 659);
        cqVal(c, st, 94, 565);
        atStep(st, 660);
        cqCut(c, st, 566, 560, {"toCut": {"shape": 94}, "clean": true, "tol": undefined});
        stepVol(c, st, 566, "cut");
        atStep(st, 661);
        cqWorkplaneNew(c, st, 567, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 662);
        cqWorkplane(c, st, 568, 567, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 663);
        cqPolyline(c, st, 569, 568, {"listOfXYTuple": [[-50.17246873986479, -15.105155168543783], [-40.90542773986479, -11.699591168543783], [-40.56049026013521, -12.638216831456216], [-49.82753126013521, -16.043780831456218]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 664);
        cqClose(c, st, 570, 569, {});
        atStep(st, 665);
        cqExtrude(c, st, 571, 570, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 571, "extrude");
        atStep(st, 666);
        cqVal(c, st, 95, 571);
        atStep(st, 667);
        cqCut(c, st, 572, 566, {"toCut": {"shape": 95}, "clean": true, "tol": undefined});
        stepVol(c, st, 572, "cut");
        atStep(st, 668);
        cqWorkplaneNew(c, st, 573, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 669);
        cqWorkplane(c, st, 574, 573, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 670);
        cqPolyline(c, st, 575, 574, {"listOfXYTuple": [[-18.13755209333223, 18.43081283600698], [-8.640998093332227, 25.16402283600698], [-8.062609906667772, 24.348261163993023], [-17.559163906667774, 17.615051163993023]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 671);
        cqClose(c, st, 576, 575, {});
        atStep(st, 672);
        cqExtrude(c, st, 577, 576, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 577, "extrude");
        atStep(st, 673);
        cqVal(c, st, 96, 577);
        atStep(st, 674);
        cqCut(c, st, 578, 572, {"toCut": {"shape": 96}, "clean": true, "tol": undefined});
        stepVol(c, st, 578, "cut");
        atStep(st, 675);
        cqWorkplaneNew(c, st, 579, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 676);
        cqWorkplane(c, st, 580, 579, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 677);
        cqPolyline(c, st, 581, 580, {"listOfXYTuple": [[-8.848994258430423, 24.80907447511585], [-8.290716258430423, 30.05293247511585], [-7.296335741569577, 29.94706752488415], [-7.854613741569577, 24.703209524884155]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 678);
        cqClose(c, st, 582, 581, {});
        atStep(st, 679);
        cqExtrude(c, st, 583, 582, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 583, "extrude");
        atStep(st, 680);
        cqVal(c, st, 97, 583);
        atStep(st, 681);
        cqCut(c, st, 584, 578, {"toCut": {"shape": 97}, "clean": true, "tol": undefined});
        stepVol(c, st, 584, "cut");
        atStep(st, 682);
        cqWorkplaneNew(c, st, 585, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 683);
        cqWorkplane(c, st, 586, 585, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 684);
        cqPolyline(c, st, 587, 586, {"listOfXYTuple": [[-7.674638435201366, 4.527736170410779], [-2.1363324352013664, 14.98535717041078], [-1.2526115647986338, 14.517342829589222], [-6.790917564798633, 4.059721829589221]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 685);
        cqClose(c, st, 588, 587, {});
        atStep(st, 686);
        cqExtrude(c, st, 589, 588, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 589, "extrude");
        atStep(st, 687);
        cqVal(c, st, 98, 589);
        atStep(st, 688);
        cqCut(c, st, 590, 584, {"toCut": {"shape": 98}, "clean": true, "tol": undefined});
        stepVol(c, st, 590, "cut");
        atStep(st, 689);
        cqWorkplaneNew(c, st, 591, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 690);
        cqWorkplane(c, st, 592, 591, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 691);
        cqPolyline(c, st, 593, 592, {"listOfXYTuple": [[-6.7533140817787185, 4.435554072268257], [-4.634863081778719, -2.726231927731743], [-5.5937909182212815, -3.009882072268257], [-7.712241918221281, 4.151903927731743]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 692);
        cqClose(c, st, 594, 593, {});
        atStep(st, 693);
        cqExtrude(c, st, 595, 594, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 595, "extrude");
        atStep(st, 694);
        cqVal(c, st, 99, 595);
        atStep(st, 695);
        cqCut(c, st, 596, 590, {"toCut": {"shape": 99}, "clean": true, "tol": undefined});
        stepVol(c, st, 596, "cut");
        atStep(st, 696);
        cqWorkplaneNew(c, st, 597, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 697);
        cqWorkplane(c, st, 598, 597, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 698);
        cqPolyline(c, st, 599, 598, {"listOfXYTuple": [[7.614394063055237, 2.069933988861744], [7.775888063055237, 8.242088988861743], [8.775545936944763, 8.215933011138256], [8.614051936944763, 2.0437780111382557]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 699);
        cqClose(c, st, 600, 599, {});
        atStep(st, 700);
        cqExtrude(c, st, 601, 600, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 601, "extrude");
        atStep(st, 701);
        cqVal(c, st, 100, 601);
        atStep(st, 702);
        cqCut(c, st, 602, 596, {"toCut": {"shape": 100}, "clean": true, "tol": undefined});
        stepVol(c, st, 602, "cut");
        atStep(st, 703);
        cqWorkplaneNew(c, st, 603, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 704);
        cqWorkplane(c, st, 604, 603, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 705);
        cqPolyline(c, st, 605, 604, {"listOfXYTuple": [[-1.5175922254192535, 15.219018200056691], [7.156439774580747, 11.938357200056693], [6.8026802254192535, 11.00302079994331], [-1.8713517745807464, 14.28368179994331]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 706);
        cqClose(c, st, 606, 605, {});
        atStep(st, 707);
        cqExtrude(c, st, 607, 606, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 607, "extrude");
        atStep(st, 708);
        cqVal(c, st, 101, 607);
        atStep(st, 709);
        cqCut(c, st, 608, 602, {"toCut": {"shape": 101}, "clean": true, "tol": undefined});
        stepVol(c, st, 608, "cut");
        atStep(st, 710);
        cqWorkplaneNew(c, st, 609, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 711);
        cqWorkplane(c, st, 610, 609, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 712);
        cqPolyline(c, st, 611, 610, {"listOfXYTuple": [[23.48204787923141, 5.07083155250202], [24.651188879231405, -0.34932144749797955], [23.673671120768596, -0.5601745525020204], [22.504530120768596, 4.859978447497979]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 713);
        cqClose(c, st, 612, 611, {});
        atStep(st, 714);
        cqExtrude(c, st, 613, 612, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 613, "extrude");
        atStep(st, 715);
        cqVal(c, st, 102, 613);
        atStep(st, 716);
        cqCut(c, st, 614, 608, {"toCut": {"shape": 102}, "clean": true, "tol": undefined});
        stepVol(c, st, 614, "cut");
        atStep(st, 717);
        cqWorkplaneNew(c, st, 615, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 718);
        cqWorkplane(c, st, 616, 615, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 719);
        cqPolyline(c, st, 617, 616, {"listOfXYTuple": [[44.52676660500769, 16.65437195597267], [50.15991060500769, 14.75290395597267], [49.84008939499231, 13.805426044027332], [44.20694539499231, 15.706894044027331]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 720);
        cqClose(c, st, 618, 617, {});
        atStep(st, 721);
        cqExtrude(c, st, 619, 618, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 619, "extrude");
        atStep(st, 722);
        cqVal(c, st, 103, 619);
        atStep(st, 723);
        cqCut(c, st, 620, 614, {"toCut": {"shape": 103}, "clean": true, "tol": undefined});
        stepVol(c, st, 620, "cut");
        atStep(st, 724);
        cqWorkplaneNew(c, st, 621, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 725);
        cqWorkplane(c, st, 622, 621, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 726);
        cqPolyline(c, st, 623, 622, {"listOfXYTuple": [[38.42366628873119, 13.586172499364745], [44.132078288731186, 16.622084499364743], [44.60163371126881, 15.739181500635258], [38.893221711268815, 12.703269500635258]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 727);
        cqClose(c, st, 624, 623, {});
        atStep(st, 728);
        cqExtrude(c, st, 625, 624, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 625, "extrude");
        atStep(st, 729);
        cqVal(c, st, 104, 625);
        atStep(st, 730);
        cqCut(c, st, 626, 620, {"toCut": {"shape": 104}, "clean": true, "tol": undefined});
        stepVol(c, st, 626, "cut");
        atStep(st, 731);
        cqWorkplaneNew(c, st, 627, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 732);
        cqWorkplane(c, st, 628, 627, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 733);
        cqPolyline(c, st, 629, 628, {"listOfXYTuple": [[38.61837471811696, 12.17011024686486], [50.44300071811696, -10.42430675313514], [49.55699928188304, -10.887989246864858], [37.732373281883035, 11.706427753135142]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 734);
        cqClose(c, st, 630, 629, {});
        atStep(st, 735);
        cqExtrude(c, st, 631, 630, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 631, "extrude");
        atStep(st, 736);
        cqVal(c, st, 105, 631);
        atStep(st, 737);
        cqCut(c, st, 632, 626, {"toCut": {"shape": 105}, "clean": true, "tol": undefined});
        stepVol(c, st, 632, "cut");
        atStep(st, 738);
        cqWorkplaneNew(c, st, 633, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 739);
        cqWorkplane(c, st, 634, 633, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 740);
        cqPolyline(c, st, 635, 634, {"listOfXYTuple": [[-20.393083423956543, -0.2102016421837695], [-17.129555423956543, -5.58382864218377], [-17.984274576043457, -6.10291935781623], [-21.24780257604346, -0.7292923578162306]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 741);
        cqClose(c, st, 636, 635, {});
        atStep(st, 742);
        cqExtrude(c, st, 637, 636, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 637, "extrude");
        atStep(st, 743);
        cqVal(c, st, 106, 637);
        atStep(st, 744);
        cqCut(c, st, 638, 632, {"toCut": {"shape": 106}, "clean": true, "tol": undefined});
        stepVol(c, st, 638, "cut");
        atStep(st, 745);
        cqWorkplaneNew(c, st, 639, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 746);
        cqWorkplane(c, st, 640, 639, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 747);
        cqPolyline(c, st, 641, 640, {"listOfXYTuple": [[-17.693824530571103, -5.362483388281701], [-12.254543530571103, -3.813920388281701], [-11.980724469428896, -4.775701611718299], [-17.420005469428897, -6.324264611718299]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 748);
        cqClose(c, st, 642, 641, {});
        atStep(st, 749);
        cqExtrude(c, st, 643, 642, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 643, "extrude");
        atStep(st, 750);
        cqVal(c, st, 107, 643);
        atStep(st, 751);
        cqCut(c, st, 644, 638, {"toCut": {"shape": 107}, "clean": true, "tol": undefined});
        stepVol(c, st, 644, "cut");
        atStep(st, 752);
        cqWorkplaneNew(c, st, 645, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 753);
        cqWorkplane(c, st, 646, 645, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 754);
        cqPolyline(c, st, 647, 646, {"listOfXYTuple": [[-18.67874255968575, 4.460523373606433], [-11.731636559685754, -3.976996626393567], [-12.503631440314248, -4.612625373606432], [-19.45073744031425, 3.8248946263935673]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 755);
        cqClose(c, st, 648, 647, {});
        atStep(st, 756);
        cqExtrude(c, st, 649, 648, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 649, "extrude");
        atStep(st, 757);
        cqVal(c, st, 108, 649);
        atStep(st, 758);
        cqCut(c, st, 650, 644, {"toCut": {"shape": 108}, "clean": true, "tol": undefined});
        stepVol(c, st, 650, "cut");
        atStep(st, 759);
        cqWorkplaneNew(c, st, 651, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 760);
        cqWorkplane(c, st, 652, 651, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 761);
        cqPolyline(c, st, 653, 652, {"listOfXYTuple": [[-17.543339817208437, -8.394065663310258], [-12.765053817208438, -10.430576663310257], [-13.157130182791564, -11.350509336689742], [-17.93541618279156, -9.313998336689744]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 762);
        cqClose(c, st, 654, 653, {});
        atStep(st, 763);
        cqExtrude(c, st, 655, 654, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 655, "extrude");
        atStep(st, 764);
        cqVal(c, st, 109, 655);
        atStep(st, 765);
        cqCut(c, st, 656, 650, {"toCut": {"shape": 109}, "clean": true, "tol": undefined});
        stepVol(c, st, 656, "cut");
        atStep(st, 766);
        cqWorkplaneNew(c, st, 657, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 767);
        cqWorkplane(c, st, 658, 657, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 768);
        cqPolyline(c, st, 659, 658, {"listOfXYTuple": [[-7.171568143215208, -8.604055028342176], [-6.709098143215208, -7.603311028342175], [-5.801341856784791, -8.022808971657824], [-6.263811856784792, -9.023552971657825]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 769);
        cqClose(c, st, 660, 659, {});
        atStep(st, 770);
        cqExtrude(c, st, 661, 660, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 661, "extrude");
        atStep(st, 771);
        cqVal(c, st, 110, 661);
        atStep(st, 772);
        cqCut(c, st, 662, 656, {"toCut": {"shape": 110}, "clean": true, "tol": undefined});
        stepVol(c, st, 662, "cut");
        atStep(st, 773);
        cqWorkplaneNew(c, st, 663, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 774);
        cqWorkplane(c, st, 664, 663, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 775);
        cqPolyline(c, st, 665, 664, {"listOfXYTuple": [[-11.962073907146172, -3.819625712442285], [-6.183312907146174, -5.711402712442285], [-6.494433092853827, -6.661773287557716], [-12.273194092853828, -4.769996287557715]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 776);
        cqClose(c, st, 666, 665, {});
        atStep(st, 777);
        cqExtrude(c, st, 667, 666, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 667, "extrude");
        atStep(st, 778);
        cqVal(c, st, 111, 667);
        atStep(st, 779);
        cqCut(c, st, 668, 662, {"toCut": {"shape": 111}, "clean": true, "tol": undefined});
        stepVol(c, st, 668, "cut");
        atStep(st, 780);
        cqWorkplaneNew(c, st, 669, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 781);
        cqWorkplane(c, st, 670, 669, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 782);
        cqPolyline(c, st, 671, 670, {"listOfXYTuple": [[-18.23846225953895, -8.823784654984973], [-18.05599925953895, -5.813126654984972], [-17.05783074046105, -5.873621345015028], [-17.24029374046105, -8.884279345015027]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 783);
        cqClose(c, st, 672, 671, {});
        atStep(st, 784);
        cqExtrude(c, st, 673, 672, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 673, "extrude");
        atStep(st, 785);
        cqVal(c, st, 112, 673);
        atStep(st, 786);
        cqCut(c, st, 674, 668, {"toCut": {"shape": 112}, "clean": true, "tol": undefined});
        stepVol(c, st, 674, "cut");
        atStep(st, 787);
        cqWorkplaneNew(c, st, 675, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 788);
        cqWorkplane(c, st, 676, 675, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 789);
        cqPolyline(c, st, 677, 676, {"listOfXYTuple": [[-24.412743935755195, -12.568838328275428], [-18.010646935755194, -8.434016328275428], [-17.468109064244803, -9.274047671724572], [-23.870206064244805, -13.408869671724572]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 790);
        cqClose(c, st, 678, 677, {});
        atStep(st, 791);
        cqExtrude(c, st, 679, 678, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 679, "extrude");
        atStep(st, 792);
        cqVal(c, st, 113, 679);
        atStep(st, 793);
        cqCut(c, st, 680, 674, {"toCut": {"shape": 113}, "clean": true, "tol": undefined});
        stepVol(c, st, 680, "cut");
        atStep(st, 794);
        cqWorkplaneNew(c, st, 681, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 795);
        cqWorkplane(c, st, 682, 681, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 796);
        cqPolyline(c, st, 683, 682, {"listOfXYTuple": [[-20.268025741364543, -19.502058389282073], [-19.41495674136454, -20.24159438928207], [-20.069993258635456, -20.997191610717927], [-20.923062258635458, -20.25765561071793]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 797);
        cqClose(c, st, 684, 683, {});
        atStep(st, 798);
        cqExtrude(c, st, 685, 684, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 685, "extrude");
        atStep(st, 799);
        cqVal(c, st, 114, 685);
        atStep(st, 800);
        cqCut(c, st, 686, 680, {"toCut": {"shape": 114}, "clean": true, "tol": undefined});
        stepVol(c, st, 686, "cut");
        atStep(st, 801);
        cqWorkplaneNew(c, st, 687, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 802);
        cqWorkplane(c, st, 688, 687, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 803);
        cqPolyline(c, st, 689, 688, {"listOfXYTuple": [[-21.553915102624355, 11.208247654763616], [-21.515521102624355, 14.286100654763615], [-20.515598897375646, 14.273627345236385], [-20.553992897375647, 11.195774345236384]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 804);
        cqClose(c, st, 690, 689, {});
        atStep(st, 805);
        cqExtrude(c, st, 691, 690, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 691, "extrude");
        atStep(st, 806);
        cqVal(c, st, 115, 691);
        atStep(st, 807);
        cqCut(c, st, 692, 686, {"toCut": {"shape": 115}, "clean": true, "tol": undefined});
        stepVol(c, st, 692, "cut");
        atStep(st, 808);
        cqWorkplaneNew(c, st, 693, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 809);
        cqWorkplane(c, st, 694, 693, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 810);
        cqPolyline(c, st, 695, 694, {"listOfXYTuple": [[-19.00614254147922, 4.792326996454812], [-7.232837541479221, 4.793728996454812], [-7.232718458520778, 3.793729003545187], [-19.00602345852078, 3.792327003545188]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 811);
        cqClose(c, st, 696, 695, {});
        atStep(st, 812);
        cqExtrude(c, st, 697, 696, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 697, "extrude");
        atStep(st, 813);
        cqVal(c, st, 116, 697);
        atStep(st, 814);
        cqCut(c, st, 698, 692, {"toCut": {"shape": 116}, "clean": true, "tol": undefined});
        stepVol(c, st, 698, "cut");
        atStep(st, 815);
        cqWorkplaneNew(c, st, 699, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 816);
        cqWorkplane(c, st, 700, 699, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 817);
        cqPolyline(c, st, 701, 700, {"listOfXYTuple": [[-7.935538407883075, 25.03313109151724], [-1.2782064078830753, 15.02833909151724], [-2.1107375921169247, 14.47436090848276], [-8.768069592116925, 24.47915290848276]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 818);
        cqClose(c, st, 702, 701, {});
        atStep(st, 819);
        cqExtrude(c, st, 703, 702, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 703, "extrude");
        atStep(st, 820);
        cqVal(c, st, 117, 703);
        atStep(st, 821);
        cqCut(c, st, 704, 698, {"toCut": {"shape": 117}, "clean": true, "tol": undefined});
        stepVol(c, st, 704, "cut");
        atStep(st, 822);
        cqWorkplaneNew(c, st, 705, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 823);
        cqWorkplane(c, st, 706, 705, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 824);
        cqPolyline(c, st, 707, 706, {"listOfXYTuple": [[9.675164172626143, 14.27431738150653], [11.235017172626144, 22.35221738150653], [12.216878827373858, 22.16261861849347], [10.657025827373856, 14.08471861849347]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 825);
        cqClose(c, st, 708, 707, {});
        atStep(st, 826);
        cqExtrude(c, st, 709, 708, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 709, "extrude");
        atStep(st, 827);
        cqVal(c, st, 118, 709);
        atStep(st, 828);
        cqCut(c, st, 710, 704, {"toCut": {"shape": 118}, "clean": true, "tol": undefined});
        stepVol(c, st, 710, "cut");
        atStep(st, 829);
        cqWorkplaneNew(c, st, 711, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 830);
        cqWorkplane(c, st, 712, 711, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 831);
        cqPolyline(c, st, 713, 712, {"listOfXYTuple": [[27.802521558621716, 18.75302563576056], [34.01043155862172, 24.64577663576056], [34.69888844137829, 23.920499364239443], [28.490978441378285, 18.027748364239443]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 832);
        cqClose(c, st, 714, 713, {});
        atStep(st, 833);
        cqExtrude(c, st, 715, 714, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 715, "extrude");
        atStep(st, 834);
        cqVal(c, st, 119, 715);
        atStep(st, 835);
        cqCut(c, st, 716, 710, {"toCut": {"shape": 119}, "clean": true, "tol": undefined});
        stepVol(c, st, 716, "cut");
        atStep(st, 836);
        cqWorkplaneNew(c, st, 717, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 837);
        cqWorkplane(c, st, 718, 717, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 838);
        cqPolyline(c, st, 719, 718, {"listOfXYTuple": [[41.69437659999225, 26.189968380932903], [44.843145599992255, 16.332778380932904], [43.89056640000774, 16.028487619067096], [40.74179740000774, 25.885677619067096]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 839);
        cqClose(c, st, 720, 719, {});
        atStep(st, 840);
        cqExtrude(c, st, 721, 720, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 721, "extrude");
        atStep(st, 841);
        cqVal(c, st, 120, 721);
        atStep(st, 842);
        cqCut(c, st, 722, 716, {"toCut": {"shape": 120}, "clean": true, "tol": undefined});
        stepVol(c, st, 722, "cut");
        atStep(st, 843);
        cqWorkplaneNew(c, st, 723, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 844);
        cqWorkplane(c, st, 724, 723, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 845);
        cqPolyline(c, st, 725, 724, {"listOfXYTuple": [[23.776275874802245, 23.238436674530096], [23.82627987480225, 23.070874674530096], [22.868038125197756, 22.784915325469903], [22.818034125197755, 22.952477325469903]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 846);
        cqClose(c, st, 726, 725, {});
        atStep(st, 847);
        cqExtrude(c, st, 727, 726, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 727, "extrude");
        atStep(st, 848);
        cqVal(c, st, 121, 727);
        atStep(st, 849);
        cqCut(c, st, 728, 722, {"toCut": {"shape": 121}, "clean": true, "tol": undefined});
        stepVol(c, st, 728, "cut");
        atStep(st, 850);
        cqWorkplaneNew(c, st, 729, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 851);
        cqWorkplane(c, st, 730, 729, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 852);
        cqPolyline(c, st, 731, 730, {"listOfXYTuple": [[-3.475235186461262, -29.66992760463676], [6.127697813538738, -18.74333760463676], [6.878836186461262, -19.40348239536324], [-2.724096813538738, -30.33007239536324]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 853);
        cqClose(c, st, 732, 731, {});
        atStep(st, 854);
        cqExtrude(c, st, 733, 732, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 733, "extrude");
        atStep(st, 855);
        cqVal(c, st, 122, 733);
        atStep(st, 856);
        cqCut(c, st, 734, 728, {"toCut": {"shape": 122}, "clean": true, "tol": undefined});
        stepVol(c, st, 734, "cut");
        atStep(st, 857);
        cqWorkplaneNew(c, st, 735, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 858);
        cqWorkplane(c, st, 736, 735, {"offset": -2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 859);
        cqPolyline(c, st, 737, 736, {"listOfXYTuple": [[34.629631403149496, 4.637487561234922], [34.811010403149496, 7.657392561234922], [35.8092115968505, 7.597439438765078], [35.6278325968505, 4.577534438765078]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 860);
        cqClose(c, st, 738, 737, {});
        atStep(st, 861);
        cqExtrude(c, st, 739, 738, {"until": 10, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 739, "extrude");
        atStep(st, 862);
        cqVal(c, st, 123, 739);
        atStep(st, 863);
        cqCut(c, st, 740, 734, {"toCut": {"shape": 123}, "clean": true, "tol": undefined});
        stepVol(c, st, 740, "cut");
        atStep(st, 864);
        cqWorkplaneNew(c, st, 741, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 865);
        cqWorkplane(c, st, 742, 741, {"offset": 2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 866);
        cqCenter(c, st, 743, 742, {"x": -45, "y": -25});
        atStep(st, 867);
        cqCircle(c, st, 744, 743, {"radius": 5, "forConstruction": false});
        atStep(st, 868);
        cqExtrude(c, st, 745, 744, {"until": -1.5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 745, "extrude");
        atStep(st, 869);
        cqVal(c, st, 124, 745);
        atStep(st, 870);
        cqCut(c, st, 746, 740, {"toCut": {"shape": 124}, "clean": true, "tol": undefined});
        stepVol(c, st, 746, "cut");
        atStep(st, 871);
        cqWorkplaneNew(c, st, 747, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 872);
        cqWorkplane(c, st, 748, 747, {"offset": 2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 873);
        cqCenter(c, st, 749, 748, {"x": -45, "y": -25});
        atStep(st, 874);
        cqCircle(c, st, 750, 749, {"radius": 2.5, "forConstruction": false});
        atStep(st, 875);
        cqExtrude(c, st, 751, 750, {"until": -5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 751, "extrude");
        atStep(st, 876);
        cqVal(c, st, 125, 751);
        atStep(st, 877);
        cqCut(c, st, 752, 746, {"toCut": {"shape": 125}, "clean": true, "tol": undefined});
        stepVol(c, st, 752, "cut");
        atStep(st, 878);
        cqWorkplaneNew(c, st, 753, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 879);
        cqWorkplane(c, st, 754, 753, {"offset": 2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 880);
        cqCenter(c, st, 755, 754, {"x": -45, "y": 25});
        atStep(st, 881);
        cqCircle(c, st, 756, 755, {"radius": 5, "forConstruction": false});
        atStep(st, 882);
        cqExtrude(c, st, 757, 756, {"until": -1.5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 757, "extrude");
        atStep(st, 883);
        cqVal(c, st, 126, 757);
        atStep(st, 884);
        cqCut(c, st, 758, 752, {"toCut": {"shape": 126}, "clean": true, "tol": undefined});
        stepVol(c, st, 758, "cut");
        atStep(st, 885);
        cqWorkplaneNew(c, st, 759, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 886);
        cqWorkplane(c, st, 760, 759, {"offset": 2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 887);
        cqCenter(c, st, 761, 760, {"x": -45, "y": 25});
        atStep(st, 888);
        cqCircle(c, st, 762, 761, {"radius": 2.5, "forConstruction": false});
        atStep(st, 889);
        cqExtrude(c, st, 763, 762, {"until": -5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 763, "extrude");
        atStep(st, 890);
        cqVal(c, st, 127, 763);
        atStep(st, 891);
        cqCut(c, st, 764, 758, {"toCut": {"shape": 127}, "clean": true, "tol": undefined});
        stepVol(c, st, 764, "cut");
        atStep(st, 892);
        cqWorkplaneNew(c, st, 765, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 893);
        cqWorkplane(c, st, 766, 765, {"offset": 2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 894);
        cqCenter(c, st, 767, 766, {"x": 45, "y": -25});
        atStep(st, 895);
        cqCircle(c, st, 768, 767, {"radius": 5, "forConstruction": false});
        atStep(st, 896);
        cqExtrude(c, st, 769, 768, {"until": -1.5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 769, "extrude");
        atStep(st, 897);
        cqVal(c, st, 128, 769);
        atStep(st, 898);
        cqCut(c, st, 770, 764, {"toCut": {"shape": 128}, "clean": true, "tol": undefined});
        stepVol(c, st, 770, "cut");
        atStep(st, 899);
        cqWorkplaneNew(c, st, 771, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 900);
        cqWorkplane(c, st, 772, 771, {"offset": 2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 901);
        cqCenter(c, st, 773, 772, {"x": 45, "y": -25});
        atStep(st, 902);
        cqCircle(c, st, 774, 773, {"radius": 2.5, "forConstruction": false});
        atStep(st, 903);
        cqExtrude(c, st, 775, 774, {"until": -5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 775, "extrude");
        atStep(st, 904);
        cqVal(c, st, 129, 775);
        atStep(st, 905);
        cqCut(c, st, 776, 770, {"toCut": {"shape": 129}, "clean": true, "tol": undefined});
        stepVol(c, st, 776, "cut");
        atStep(st, 906);
        cqWorkplaneNew(c, st, 777, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 907);
        cqWorkplane(c, st, 778, 777, {"offset": 2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 908);
        cqCenter(c, st, 779, 778, {"x": 45, "y": 25});
        atStep(st, 909);
        cqCircle(c, st, 780, 779, {"radius": 5, "forConstruction": false});
        atStep(st, 910);
        cqExtrude(c, st, 781, 780, {"until": -1.5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 781, "extrude");
        atStep(st, 911);
        cqVal(c, st, 130, 781);
        atStep(st, 912);
        cqCut(c, st, 782, 776, {"toCut": {"shape": 130}, "clean": true, "tol": undefined});
        stepVol(c, st, 782, "cut");
        atStep(st, 913);
        cqWorkplaneNew(c, st, 783, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 914);
        cqWorkplane(c, st, 784, 783, {"offset": 2.5, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 915);
        cqCenter(c, st, 785, 784, {"x": 45, "y": 25});
        atStep(st, 916);
        cqCircle(c, st, 786, 785, {"radius": 2.5, "forConstruction": false});
        atStep(st, 917);
        cqExtrude(c, st, 787, 786, {"until": -5, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 787, "extrude");
        atStep(st, 918);
        cqVal(c, st, 131, 787);
        atStep(st, 919);
        cqCut(c, st, 788, 782, {"toCut": {"shape": 131}, "clean": true, "tol": undefined});
        stepVol(c, st, 788, "cut");
        return mergeMaps(cqMeasure(c, st, {"wp": 788}), {"i": 3702});
    } catch (e) {
        return {"i": 3702, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_3702(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c3702"] = r; }
    return out;
}
