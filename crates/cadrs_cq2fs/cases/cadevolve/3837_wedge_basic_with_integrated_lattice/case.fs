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

const case_3837 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 2);
        cqPolyline(c, st, 2, 1, {"listOfXYTuple": [[0, 0], [30, 0], [0, 15]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 3);
        cqClose(c, st, 3, 2, {});
        atStep(st, 4);
        cqExtrude(c, st, 4, 3, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 4, "extrude");
        atStep(st, 5);
        cqSelect("Faces")(c, st, 5, 4, {"selector": {"a": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 1, 0]}, "b": {"k": "nth", "key": "center", "max": false, "n": -1, "parallel": false, "v": [0, 1, 0]}, "k": "or"}, "tag": undefined});
        atStep(st, 6);
        cqShell(c, st, 6, 5, {"thickness": -2, "kind": "arc"});
        stepVol(c, st, 6, "shell");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 8);
        cqPolyline(c, st, 8, 7, {"listOfXYTuple": [[-0.7347425009301718, 0.5475477788418415], [0.7347425009301718, 0.5475477788418415], [0.7347425009301718, -0.5475477788418415], [-0.7347425009301718, -0.5475477788418415]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 9);
        cqClose(c, st, 9, 8, {});
        atStep(st, 10);
        cqExtrude(c, st, 10, 9, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 10, "extrude");
        atStep(st, 11);
        cqRotate(c, st, 11, 10, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 66.44257056662921});
        stepVol(c, st, 11, "rotate");
        atStep(st, 12);
        cqTranslate(c, st, 12, 11, {"vec": [4.960997569900783, 0, 4.3041929687088505]});
        stepVol(c, st, 12, "translate");
        atStep(st, 13);
        cqIntersect(c, st, 13, 12, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 13, "intersect");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 14, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 15);
        cqPolyline(c, st, 15, 14, {"listOfXYTuple": [[-0.986933455324264, 0.6128154258234778], [0.986933455324264, 0.6128154258234778], [0.986933455324264, -0.6128154258234778], [-0.986933455324264, -0.6128154258234778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 16);
        cqClose(c, st, 16, 15, {});
        atStep(st, 17);
        cqExtrude(c, st, 17, 16, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 17, "extrude");
        atStep(st, 18);
        cqRotate(c, st, 18, 17, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 145.79721737889372});
        stepVol(c, st, 18, "rotate");
        atStep(st, 19);
        cqTranslate(c, st, 19, 18, {"vec": [3.851097907604376, 0, 4.185462488581922]});
        stepVol(c, st, 19, "translate");
        atStep(st, 20);
        cqIntersect(c, st, 20, 19, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 20, "intersect");
        atStep(st, 21);
        cqUnion(c, st, 21, 13, {"toUnion": {"wp": 20}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 21, "union");
        atStep(st, 22);
        cqWorkplaneNew(c, st, 22, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 23);
        cqPolyline(c, st, 23, 22, {"listOfXYTuple": [[-1.1971088607908178, 0.5662749264527317], [1.1971088607908178, 0.5662749264527317], [1.1971088607908178, -0.5662749264527317], [-1.1971088607908178, -0.5662749264527317]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 24);
        cqClose(c, st, 24, 23, {});
        atStep(st, 25);
        cqExtrude(c, st, 25, 24, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 25, "extrude");
        atStep(st, 26);
        cqRotate(c, st, 26, 25, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -64.62187042173963});
        stepVol(c, st, 26, "rotate");
        atStep(st, 27);
        cqTranslate(c, st, 27, 26, {"vec": [5.1804136871754345, 0, 2.549097349650861]});
        stepVol(c, st, 27, "translate");
        atStep(st, 28);
        cqIntersect(c, st, 28, 27, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 28, "intersect");
        atStep(st, 29);
        cqUnion(c, st, 29, 21, {"toUnion": {"wp": 28}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 29, "union");
        atStep(st, 30);
        cqWorkplaneNew(c, st, 30, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 31);
        cqPolyline(c, st, 31, 30, {"listOfXYTuple": [[-1.416497373708159, 0.4897546723812474], [1.416497373708159, 0.4897546723812474], [1.416497373708159, -0.4897546723812474], [-1.416497373708159, -0.4897546723812474]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 32);
        cqClose(c, st, 32, 31, {});
        atStep(st, 33);
        cqExtrude(c, st, 33, 32, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 33, "extrude");
        atStep(st, 34);
        cqRotate(c, st, 34, 33, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -56.89927448350458});
        stepVol(c, st, 34, "rotate");
        atStep(st, 35);
        cqTranslate(c, st, 35, 34, {"vec": [13.390714444253376, 0, 2.6971346877099687]});
        stepVol(c, st, 35, "translate");
        atStep(st, 36);
        cqIntersect(c, st, 36, 35, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 36, "intersect");
        atStep(st, 37);
        cqUnion(c, st, 37, 29, {"toUnion": {"wp": 36}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 37, "union");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 38, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 39);
        cqPolyline(c, st, 39, 38, {"listOfXYTuple": [[-1.7465391591184665, 0.4826742205018568], [1.7465391591184665, 0.4826742205018568], [1.7465391591184665, -0.4826742205018568], [-1.7465391591184665, -0.4826742205018568]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 40);
        cqClose(c, st, 40, 39, {});
        atStep(st, 41);
        cqExtrude(c, st, 41, 40, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 41, "extrude");
        atStep(st, 42);
        cqRotate(c, st, 42, 41, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 38.83125352304071});
        stepVol(c, st, 42, "rotate");
        atStep(st, 43);
        cqTranslate(c, st, 43, 42, {"vec": [13.9776945329918, 0, 4.978881658075332]});
        stepVol(c, st, 43, "translate");
        atStep(st, 44);
        cqIntersect(c, st, 44, 43, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 44, "intersect");
        atStep(st, 45);
        cqUnion(c, st, 45, 37, {"toUnion": {"wp": 44}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 45, "union");
        atStep(st, 46);
        cqWorkplaneNew(c, st, 46, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 47);
        cqPolyline(c, st, 47, 46, {"listOfXYTuple": [[-1.7939004728244563, 0.5124871343381596], [1.7939004728244563, 0.5124871343381596], [1.7939004728244563, -0.5124871343381596], [-1.7939004728244563, -0.5124871343381596]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 48);
        cqClose(c, st, 48, 47, {});
        atStep(st, 49);
        cqExtrude(c, st, 49, 48, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 49, "extrude");
        atStep(st, 50);
        cqRotate(c, st, 50, 49, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -53.29057368032471});
        stepVol(c, st, 50, "rotate");
        atStep(st, 51);
        cqTranslate(c, st, 51, 50, {"vec": [13.689464074654063, 0, 2.445621945495424]});
        stepVol(c, st, 51, "translate");
        atStep(st, 52);
        cqIntersect(c, st, 52, 51, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 52, "intersect");
        atStep(st, 53);
        cqUnion(c, st, 53, 45, {"toUnion": {"wp": 52}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 53, "union");
        atStep(st, 54);
        cqWorkplaneNew(c, st, 54, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 55);
        cqPolyline(c, st, 55, 54, {"listOfXYTuple": [[-0.20192929868860607, 0.5799681702991859], [0.20192929868860607, 0.5799681702991859], [0.20192929868860607, -0.5799681702991859], [-0.20192929868860607, -0.5799681702991859]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 56);
        cqClose(c, st, 56, 55, {});
        atStep(st, 57);
        cqExtrude(c, st, 57, 56, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 57, "extrude");
        atStep(st, 58);
        cqRotate(c, st, 58, 57, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 25.71895748988999});
        stepVol(c, st, 58, "rotate");
        atStep(st, 59);
        cqTranslate(c, st, 59, 58, {"vec": [15.52016650824379, 0, 6.161640734965373]});
        stepVol(c, st, 59, "translate");
        atStep(st, 60);
        cqIntersect(c, st, 60, 59, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 60, "intersect");
        atStep(st, 61);
        cqUnion(c, st, 61, 53, {"toUnion": {"wp": 60}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 61, "union");
        atStep(st, 62);
        cqWorkplaneNew(c, st, 62, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 63);
        cqPolyline(c, st, 63, 62, {"listOfXYTuple": [[-0.8268599556473917, 0.6144303811730052], [0.8268599556473917, 0.6144303811730052], [0.8268599556473917, -0.6144303811730052], [-0.8268599556473917, -0.6144303811730052]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 64);
        cqClose(c, st, 64, 63, {});
        atStep(st, 65);
        cqExtrude(c, st, 65, 64, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 65, "extrude");
        atStep(st, 66);
        cqRotate(c, st, 66, 65, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -8.250702184680623});
        stepVol(c, st, 66, "rotate");
        atStep(st, 67);
        cqTranslate(c, st, 67, 66, {"vec": [16.15654328872237, 0, 5.955353730049595]});
        stepVol(c, st, 67, "translate");
        atStep(st, 68);
        cqIntersect(c, st, 68, 67, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 68, "intersect");
        atStep(st, 69);
        cqUnion(c, st, 69, 61, {"toUnion": {"wp": 68}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 69, "union");
        atStep(st, 70);
        cqWorkplaneNew(c, st, 70, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 71);
        cqPolyline(c, st, 71, 70, {"listOfXYTuple": [[-1.9804073362036831, 0.6562499851619692], [1.9804073362036831, 0.6562499851619692], [1.9804073362036831, -0.6562499851619692], [-1.9804073362036831, -0.6562499851619692]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 72);
        cqClose(c, st, 72, 71, {});
        atStep(st, 73);
        cqExtrude(c, st, 73, 72, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 73, "extrude");
        atStep(st, 74);
        cqRotate(c, st, 74, 73, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -35.79122545409202});
        stepVol(c, st, 74, "rotate");
        atStep(st, 75);
        cqTranslate(c, st, 75, 74, {"vec": [16.944655770659104, 0, 4.915803591096411]});
        stepVol(c, st, 75, "translate");
        atStep(st, 76);
        cqIntersect(c, st, 76, 75, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 76, "intersect");
        atStep(st, 77);
        cqUnion(c, st, 77, 69, {"toUnion": {"wp": 76}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 77, "union");
        atStep(st, 78);
        cqWorkplaneNew(c, st, 78, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 79);
        cqPolyline(c, st, 79, 78, {"listOfXYTuple": [[-1.039596121501469, 0.5971572222104116], [1.039596121501469, 0.5971572222104116], [1.039596121501469, -0.5971572222104116], [-1.039596121501469, -0.5971572222104116]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 80);
        cqClose(c, st, 80, 79, {});
        atStep(st, 81);
        cqExtrude(c, st, 81, 80, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 81, "extrude");
        atStep(st, 82);
        cqRotate(c, st, 82, 81, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -111.2633420419936});
        stepVol(c, st, 82, "rotate");
        atStep(st, 83);
        cqTranslate(c, st, 83, 82, {"vec": [6.109027537014269, 0, 9.481485109063083]});
        stepVol(c, st, 83, "translate");
        atStep(st, 84);
        cqIntersect(c, st, 84, 83, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 84, "intersect");
        atStep(st, 85);
        cqUnion(c, st, 85, 77, {"toUnion": {"wp": 84}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 85, "union");
        atStep(st, 86);
        cqWorkplaneNew(c, st, 86, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 87);
        cqPolyline(c, st, 87, 86, {"listOfXYTuple": [[-1.1232004853974045, 0.5887201334336553], [1.1232004853974045, 0.5887201334336553], [1.1232004853974045, -0.5887201334336553], [-1.1232004853974045, -0.5887201334336553]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 88);
        cqClose(c, st, 88, 87, {});
        atStep(st, 89);
        cqExtrude(c, st, 89, 88, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 89, "extrude");
        atStep(st, 90);
        cqRotate(c, st, 90, 89, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -105.00039748438664});
        stepVol(c, st, 90, "rotate");
        atStep(st, 91);
        cqTranslate(c, st, 91, 90, {"vec": [6.1953291252803595, 0, 9.365382768641265]});
        stepVol(c, st, 91, "translate");
        atStep(st, 92);
        cqIntersect(c, st, 92, 91, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 92, "intersect");
        atStep(st, 93);
        cqUnion(c, st, 93, 85, {"toUnion": {"wp": 92}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 93, "union");
        atStep(st, 94);
        cqWorkplaneNew(c, st, 94, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 95);
        cqPolyline(c, st, 95, 94, {"listOfXYTuple": [[-1.198931463759764, 0.5645975380879829], [1.198931463759764, 0.5645975380879829], [1.198931463759764, -0.5645975380879829], [-1.198931463759764, -0.5645975380879829]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 96);
        cqClose(c, st, 96, 95, {});
        atStep(st, 97);
        cqExtrude(c, st, 97, 96, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 97, "extrude");
        atStep(st, 98);
        cqRotate(c, st, 98, 97, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -36.689243949644705});
        stepVol(c, st, 98, "rotate");
        atStep(st, 99);
        cqTranslate(c, st, 99, 98, {"vec": [7.447450868419098, 0, 9.733977987726028]});
        stepVol(c, st, 99, "translate");
        atStep(st, 100);
        cqIntersect(c, st, 100, 99, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 100, "intersect");
        atStep(st, 101);
        cqUnion(c, st, 101, 93, {"toUnion": {"wp": 100}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 101, "union");
        atStep(st, 102);
        cqWorkplaneNew(c, st, 102, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 103);
        cqPolyline(c, st, 103, 102, {"listOfXYTuple": [[-0.6689766327230026, 0.6257477015845595], [0.6689766327230026, 0.6257477015845595], [0.6689766327230026, -0.6257477015845595], [-0.6689766327230026, -0.6257477015845595]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 104);
        cqClose(c, st, 104, 103, {});
        atStep(st, 105);
        cqExtrude(c, st, 105, 104, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 105, "extrude");
        atStep(st, 106);
        cqRotate(c, st, 106, 105, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -17.960547627750874});
        stepVol(c, st, 106, "rotate");
        atStep(st, 107);
        cqTranslate(c, st, 107, 106, {"vec": [16.338468155907908, 0, 6.042982403258755]});
        stepVol(c, st, 107, "translate");
        atStep(st, 108);
        cqIntersect(c, st, 108, 107, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 108, "intersect");
        atStep(st, 109);
        cqUnion(c, st, 109, 101, {"toUnion": {"wp": 108}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 109, "union");
        atStep(st, 110);
        cqWorkplaneNew(c, st, 110, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 111);
        cqPolyline(c, st, 111, 110, {"listOfXYTuple": [[-1.892426972905452, 0.6668616780231069], [1.892426972905452, 0.6668616780231069], [1.892426972905452, -0.6668616780231069], [-1.892426972905452, -0.6668616780231069]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 112);
        cqClose(c, st, 112, 111, {});
        atStep(st, 113);
        cqExtrude(c, st, 113, 112, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 113, "extrude");
        atStep(st, 114);
        cqRotate(c, st, 114, 113, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -41.17245996270577});
        stepVol(c, st, 114, "rotate");
        atStep(st, 115);
        cqTranslate(c, st, 115, 114, {"vec": [17.12658063784464, 0, 5.00343226430557]});
        stepVol(c, st, 115, "translate");
        atStep(st, 116);
        cqIntersect(c, st, 116, 115, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 116, "intersect");
        atStep(st, 117);
        cqUnion(c, st, 117, 109, {"toUnion": {"wp": 116}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 117, "union");
        atStep(st, 118);
        cqWorkplaneNew(c, st, 118, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 119);
        cqPolyline(c, st, 119, 118, {"listOfXYTuple": [[-2.1144682296915596, 0.5869971642362466], [2.1144682296915596, 0.5869971642362466], [2.1144682296915596, -0.5869971642362466], [-2.1144682296915596, -0.5869971642362466]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 120);
        cqClose(c, st, 120, 119, {});
        atStep(st, 121);
        cqExtrude(c, st, 121, 120, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 121, "extrude");
        atStep(st, 122);
        cqRotate(c, st, 122, 121, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -90.16779406784308});
        stepVol(c, st, 122, "rotate");
        atStep(st, 123);
        cqTranslate(c, st, 123, 122, {"vec": [15.695899039423908, 0, 4.134810245799965]});
        stepVol(c, st, 123, "translate");
        atStep(st, 124);
        cqIntersect(c, st, 124, 123, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 124, "intersect");
        atStep(st, 125);
        cqUnion(c, st, 125, 117, {"toUnion": {"wp": 124}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 125, "union");
        atStep(st, 126);
        cqWorkplaneNew(c, st, 126, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 127);
        cqPolyline(c, st, 127, 126, {"listOfXYTuple": [[-0.49239071090643494, 0.6913116003336159], [0.49239071090643494, 0.6913116003336159], [0.49239071090643494, -0.6913116003336159], [-0.49239071090643494, -0.6913116003336159]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 128);
        cqClose(c, st, 128, 127, {});
        atStep(st, 129);
        cqExtrude(c, st, 129, 128, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 129, "extrude");
        atStep(st, 130);
        cqRotate(c, st, 130, 129, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 58.46075204092366});
        stepVol(c, st, 130, "rotate");
        atStep(st, 131);
        cqTranslate(c, st, 131, 130, {"vec": [3.0141744110365276, 0, 7.849352927048707]});
        stepVol(c, st, 131, "translate");
        atStep(st, 132);
        cqIntersect(c, st, 132, 131, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 132, "intersect");
        atStep(st, 133);
        cqUnion(c, st, 133, 125, {"toUnion": {"wp": 132}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 133, "union");
        atStep(st, 134);
        cqWorkplaneNew(c, st, 134, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 135);
        cqPolyline(c, st, 135, 134, {"listOfXYTuple": [[-1.3255954284732048, 0.6886112384459778], [1.3255954284732048, 0.6886112384459778], [1.3255954284732048, -0.6886112384459778], [-1.3255954284732048, -0.6886112384459778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 136);
        cqClose(c, st, 136, 135, {});
        atStep(st, 137);
        cqExtrude(c, st, 137, 136, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 137, "extrude");
        atStep(st, 138);
        cqRotate(c, st, 138, 137, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -97.43873150283976});
        stepVol(c, st, 138, "rotate");
        atStep(st, 139);
        cqTranslate(c, st, 139, 138, {"vec": [2.5849940003580034, 0, 6.115258116413666]});
        stepVol(c, st, 139, "translate");
        atStep(st, 140);
        cqIntersect(c, st, 140, 139, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 140, "intersect");
        atStep(st, 141);
        cqUnion(c, st, 141, 133, {"toUnion": {"wp": 140}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 141, "union");
        atStep(st, 142);
        cqWorkplaneNew(c, st, 142, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 143);
        cqPolyline(c, st, 143, 142, {"listOfXYTuple": [[-1.351905199639687, 0.6703746619977509], [1.351905199639687, 0.6703746619977509], [1.351905199639687, -0.6703746619977509], [-1.351905199639687, -0.6703746619977509]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 144);
        cqClose(c, st, 144, 143, {});
        atStep(st, 145);
        cqExtrude(c, st, 145, 144, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 145, "extrude");
        atStep(st, 146);
        cqRotate(c, st, 146, 145, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -84.09347281711705});
        stepVol(c, st, 146, "rotate");
        atStep(st, 147);
        cqTranslate(c, st, 147, 146, {"vec": [2.8957324025500704, 0, 6.084969092003764]});
        stepVol(c, st, 147, "translate");
        atStep(st, 148);
        cqIntersect(c, st, 148, 147, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 148, "intersect");
        atStep(st, 149);
        cqUnion(c, st, 149, 141, {"toUnion": {"wp": 148}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 149, "union");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 151);
        cqPolyline(c, st, 151, 150, {"listOfXYTuple": [[-0.4671922455534294, 0.9038878344540648], [0.4671922455534294, 0.9038878344540648], [0.4671922455534294, -0.9038878344540648], [-0.4671922455534294, -0.9038878344540648]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 152);
        cqClose(c, st, 152, 151, {});
        atStep(st, 153);
        cqExtrude(c, st, 153, 152, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 153, "extrude");
        atStep(st, 154);
        cqRotate(c, st, 154, 153, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 155.358144710877});
        stepVol(c, st, 154, "rotate");
        atStep(st, 155);
        cqTranslate(c, st, 155, 154, {"vec": [21.120218794214452, 0, 3.857730356869855]});
        stepVol(c, st, 155, "translate");
        atStep(st, 156);
        cqIntersect(c, st, 156, 155, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 156, "intersect");
        atStep(st, 157);
        cqUnion(c, st, 157, 149, {"toUnion": {"wp": 156}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 157, "union");
        atStep(st, 158);
        cqWorkplaneNew(c, st, 158, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 159);
        cqPolyline(c, st, 159, 158, {"listOfXYTuple": [[-0.4886808200283755, 0.951172566505192], [0.4886808200283755, 0.951172566505192], [0.4886808200283755, -0.951172566505192], [-0.4886808200283755, -0.951172566505192]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 160);
        cqClose(c, st, 160, 159, {});
        atStep(st, 161);
        cqExtrude(c, st, 161, 160, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 161, "extrude");
        atStep(st, 162);
        cqRotate(c, st, 162, 161, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -49.06528616923722});
        stepVol(c, st, 162, "rotate");
        atStep(st, 163);
        cqTranslate(c, st, 163, 162, {"vec": [21.865047678125052, 0, 3.293759766223153]});
        stepVol(c, st, 163, "translate");
        atStep(st, 164);
        cqIntersect(c, st, 164, 163, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 164, "intersect");
        atStep(st, 165);
        cqUnion(c, st, 165, 157, {"toUnion": {"wp": 164}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 165, "union");
        atStep(st, 166);
        cqWorkplaneNew(c, st, 166, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 167);
        cqPolyline(c, st, 167, 166, {"listOfXYTuple": [[-1.078309795995957, 0.9000838670059209], [1.078309795995957, 0.9000838670059209], [1.078309795995957, -0.9000838670059209], [-1.078309795995957, -0.9000838670059209]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 168);
        cqClose(c, st, 168, 167, {});
        atStep(st, 169);
        cqExtrude(c, st, 169, 168, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 169, "extrude");
        atStep(st, 170);
        cqRotate(c, st, 170, 169, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -127.1035104737427});
        stepVol(c, st, 170, "rotate");
        atStep(st, 171);
        cqTranslate(c, st, 171, 170, {"vec": [20.89436689279359, 0, 2.80293422800591]});
        stepVol(c, st, 171, "translate");
        atStep(st, 172);
        cqIntersect(c, st, 172, 171, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 172, "intersect");
        atStep(st, 173);
        cqUnion(c, st, 173, 165, {"toUnion": {"wp": 172}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 173, "union");
        atStep(st, 174);
        cqWorkplaneNew(c, st, 174, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 175);
        cqPolyline(c, st, 175, 174, {"listOfXYTuple": [[-1.082344228446516, 0.8165377163749615], [1.082344228446516, 0.8165377163749615], [1.082344228446516, -0.8165377163749615], [-1.082344228446516, -0.8165377163749615]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 176);
        cqClose(c, st, 176, 175, {});
        atStep(st, 177);
        cqExtrude(c, st, 177, 176, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 177, "extrude");
        atStep(st, 178);
        cqRotate(c, st, 178, 177, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 7.830636446595912});
        stepVol(c, st, 178, "rotate");
        atStep(st, 179);
        cqTranslate(c, st, 179, 178, {"vec": [19.623321410339983, 0, 3.9050594482673393]});
        stepVol(c, st, 179, "translate");
        atStep(st, 180);
        cqIntersect(c, st, 180, 179, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 180, "intersect");
        atStep(st, 181);
        cqUnion(c, st, 181, 173, {"toUnion": {"wp": 180}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 181, "union");
        atStep(st, 182);
        cqWorkplaneNew(c, st, 182, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 183);
        cqPolyline(c, st, 183, 182, {"listOfXYTuple": [[-1.240823635616512, 0.8138941231834663], [1.240823635616512, 0.8138941231834663], [1.240823635616512, -0.8138941231834663], [-1.240823635616512, -0.8138941231834663]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 184);
        cqClose(c, st, 184, 183, {});
        atStep(st, 185);
        cqExtrude(c, st, 185, 184, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 185, "extrude");
        atStep(st, 186);
        cqRotate(c, st, 186, 185, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -46.98990201873903});
        stepVol(c, st, 186, "rotate");
        atStep(st, 187);
        cqTranslate(c, st, 187, 186, {"vec": [19.397469508919123, 0, 2.8502633194033944]});
        stepVol(c, st, 187, "translate");
        atStep(st, 188);
        cqIntersect(c, st, 188, 187, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 188, "intersect");
        atStep(st, 189);
        cqUnion(c, st, 189, 181, {"toUnion": {"wp": 188}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 189, "union");
        atStep(st, 190);
        cqWorkplaneNew(c, st, 190, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 191);
        cqPolyline(c, st, 191, 190, {"listOfXYTuple": [[-1.3045251149679171, 0.7042408078805908], [1.3045251149679171, 0.7042408078805908], [1.3045251149679171, -0.7042408078805908], [-1.3045251149679171, -0.7042408078805908]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 192);
        cqClose(c, st, 192, 191, {});
        atStep(st, 193);
        cqExtrude(c, st, 193, 192, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 193, "extrude");
        atStep(st, 194);
        cqRotate(c, st, 194, 193, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 127.16679924905836});
        stepVol(c, st, 194, "rotate");
        atStep(st, 195);
        cqTranslate(c, st, 195, 194, {"vec": [17.762957418323218, 0, 4.797145259389792]});
        stepVol(c, st, 195, "translate");
        atStep(st, 196);
        cqIntersect(c, st, 196, 195, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 196, "intersect");
        atStep(st, 197);
        cqUnion(c, st, 197, 189, {"toUnion": {"wp": 196}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 197, "union");
        atStep(st, 198);
        cqWorkplaneNew(c, st, 198, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 199);
        cqPolyline(c, st, 199, 198, {"listOfXYTuple": [[-0.1308527880491518, 0.7573956070771292], [0.1308527880491518, 0.7573956070771292], [0.1308527880491518, -0.7573956070771292], [-0.1308527880491518, -0.7573956070771292]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 200);
        cqClose(c, st, 200, 199, {});
        atStep(st, 201);
        cqExtrude(c, st, 201, 200, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 201, "extrude");
        atStep(st, 202);
        cqRotate(c, st, 202, 201, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 111.97810897247398});
        stepVol(c, st, 202, "rotate");
        atStep(st, 203);
        cqTranslate(c, st, 203, 202, {"vec": [2.658640361907925, 0, 0.3795611714369107]});
        stepVol(c, st, 203, "translate");
        atStep(st, 204);
        cqIntersect(c, st, 204, 203, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 204, "intersect");
        atStep(st, 205);
        cqUnion(c, st, 205, 197, {"toUnion": {"wp": 204}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 205, "union");
        atStep(st, 206);
        cqWorkplaneNew(c, st, 206, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 207);
        cqPolyline(c, st, 207, 206, {"listOfXYTuple": [[-1.3599486151603937, 0.8304832526643229], [1.3599486151603937, 0.8304832526643229], [1.3599486151603937, -0.8304832526643229], [-1.3599486151603937, -0.8304832526643229]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 208);
        cqClose(c, st, 208, 207, {});
        atStep(st, 209);
        cqExtrude(c, st, 209, 208, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 209, "extrude");
        atStep(st, 210);
        cqRotate(c, st, 210, 209, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -174.55232787747127});
        stepVol(c, st, 210, "rotate");
        atStep(st, 211);
        cqTranslate(c, st, 211, 210, {"vec": [1.3538061604763985, 0, 0.1291089297176018]});
        stepVol(c, st, 211, "translate");
        atStep(st, 212);
        cqIntersect(c, st, 212, 211, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 212, "intersect");
        atStep(st, 213);
        cqUnion(c, st, 213, 205, {"toUnion": {"wp": 212}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 213, "union");
        atStep(st, 214);
        cqWorkplaneNew(c, st, 214, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 215);
        cqPolyline(c, st, 215, 214, {"listOfXYTuple": [[-1.6107306194774256, 0.6667031391788454], [1.6107306194774256, 0.6667031391788454], [1.6107306194774256, -0.6667031391788454], [-1.6107306194774256, -0.6667031391788454]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 216);
        cqClose(c, st, 216, 215, {});
        atStep(st, 217);
        cqExtrude(c, st, 217, 216, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 217, "extrude");
        atStep(st, 218);
        cqRotate(c, st, 218, 217, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 22.048199540674});
        stepVol(c, st, 218, "rotate");
        atStep(st, 219);
        cqTranslate(c, st, 219, 218, {"vec": [4.200547620527555, 0, 0.8628643014207316]});
        stepVol(c, st, 219, "translate");
        atStep(st, 220);
        cqIntersect(c, st, 220, 219, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 220, "intersect");
        atStep(st, 221);
        cqUnion(c, st, 221, 213, {"toUnion": {"wp": 220}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 221, "union");
        atStep(st, 222);
        cqWorkplaneNew(c, st, 222, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 223);
        cqPolyline(c, st, 223, 222, {"listOfXYTuple": [[-1.615877442641013, 0.6649798532510771], [1.615877442641013, 0.6649798532510771], [1.615877442641013, -0.6649798532510771], [-1.615877442641013, -0.6649798532510771]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 224);
        cqClose(c, st, 224, 223, {});
        atStep(st, 225);
        cqExtrude(c, st, 225, 224, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 225, "extrude");
        atStep(st, 226);
        cqRotate(c, st, 226, 225, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -162.59666094421408});
        stepVol(c, st, 226, "rotate");
        atStep(st, 227);
        cqTranslate(c, st, 227, 226, {"vec": [4.151575661482683, 0, 0.9842076134224388]});
        stepVol(c, st, 227, "translate");
        atStep(st, 228);
        cqIntersect(c, st, 228, 227, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 228, "intersect");
        atStep(st, 229);
        cqUnion(c, st, 229, 221, {"toUnion": {"wp": 228}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 229, "union");
        atStep(st, 230);
        cqWorkplaneNew(c, st, 230, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 231);
        cqPolyline(c, st, 231, 230, {"listOfXYTuple": [[-1.768757774415827, 0.5344224292199153], [1.768757774415827, 0.5344224292199153], [1.768757774415827, -0.5344224292199153], [-1.768757774415827, -0.5344224292199153]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 232);
        cqClose(c, st, 232, 231, {});
        atStep(st, 233);
        cqExtrude(c, st, 233, 232, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 233, "extrude");
        atStep(st, 234);
        cqRotate(c, st, 234, 233, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 97.12595379856606});
        stepVol(c, st, 234, "rotate");
        atStep(st, 235);
        cqTranslate(c, st, 235, 234, {"vec": [5.474066802827661, 0, 3.2226063624642487]});
        stepVol(c, st, 235, "translate");
        atStep(st, 236);
        cqIntersect(c, st, 236, 235, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 236, "intersect");
        atStep(st, 237);
        cqUnion(c, st, 237, 229, {"toUnion": {"wp": 236}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 237, "union");
        atStep(st, 238);
        cqWorkplaneNew(c, st, 238, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 239);
        cqPolyline(c, st, 239, 238, {"listOfXYTuple": [[-2.1082626282618304, 0.5978195124535013], [2.1082626282618304, 0.5978195124535013], [2.1082626282618304, -0.5978195124535013], [-2.1082626282618304, -0.5978195124535013]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 240);
        cqClose(c, st, 240, 239, {});
        atStep(st, 241);
        cqExtrude(c, st, 241, 240, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 241, "extrude");
        atStep(st, 242);
        cqRotate(c, st, 242, 241, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 129.08898876812947});
        stepVol(c, st, 242, "rotate");
        atStep(st, 243);
        cqTranslate(c, st, 243, 242, {"vec": [4.364167140531254, 0, 3.10387588233732]});
        stepVol(c, st, 243, "translate");
        atStep(st, 244);
        cqIntersect(c, st, 244, 243, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 244, "intersect");
        atStep(st, 245);
        cqUnion(c, st, 245, 237, {"toUnion": {"wp": 244}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 245, "union");
        atStep(st, 246);
        cqWorkplaneNew(c, st, 246, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 247);
        cqPolyline(c, st, 247, 246, {"listOfXYTuple": [[-0.9342552613830444, 0.9250088368869854], [0.9342552613830444, 0.9250088368869854], [0.9342552613830444, -0.9250088368869854], [-0.9342552613830444, -0.9250088368869854]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 248);
        cqClose(c, st, 248, 247, {});
        atStep(st, 249);
        cqExtrude(c, st, 249, 248, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 249, "extrude");
        atStep(st, 250);
        cqRotate(c, st, 250, 249, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -37.1323287540572});
        stepVol(c, st, 250, "rotate");
        atStep(st, 251);
        cqTranslate(c, st, 251, 250, {"vec": [21.44040180433062, 0, 3.4885531854513676]});
        stepVol(c, st, 251, "translate");
        atStep(st, 252);
        cqIntersect(c, st, 252, 251, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 252, "intersect");
        atStep(st, 253);
        cqUnion(c, st, 253, 245, {"toUnion": {"wp": 252}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 253, "union");
        atStep(st, 254);
        cqWorkplaneNew(c, st, 254, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 255);
        cqPolyline(c, st, 255, 254, {"listOfXYTuple": [[-1.0787047579582567, 0.8735137676413471], [1.0787047579582567, 0.8735137676413471], [1.0787047579582567, -0.8735137676413471], [-1.0787047579582567, -0.8735137676413471]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 256);
        cqClose(c, st, 256, 255, {});
        atStep(st, 257);
        cqExtrude(c, st, 257, 256, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 257, "extrude");
        atStep(st, 258);
        cqRotate(c, st, 258, 257, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -102.0856245702574});
        stepVol(c, st, 258, "rotate");
        atStep(st, 259);
        cqTranslate(c, st, 259, 258, {"vec": [20.46972101899916, 0, 2.997727647234125]});
        stepVol(c, st, 259, "translate");
        atStep(st, 260);
        cqIntersect(c, st, 260, 259, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 260, "intersect");
        atStep(st, 261);
        cqUnion(c, st, 261, 253, {"toUnion": {"wp": 260}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 261, "union");
        atStep(st, 262);
        cqWorkplaneNew(c, st, 262, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 263);
        cqPolyline(c, st, 263, 262, {"listOfXYTuple": [[-2.063194435141429, 0.7668161504339777], [2.063194435141429, 0.7668161504339777], [2.063194435141429, -0.7668161504339777], [-2.063194435141429, -0.7668161504339777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 264);
        cqClose(c, st, 264, 263, {});
        atStep(st, 265);
        cqExtrude(c, st, 265, 264, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 265, "extrude");
        atStep(st, 266);
        cqRotate(c, st, 266, 265, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 154.38124851450675});
        stepVol(c, st, 266, "rotate");
        atStep(st, 267);
        cqTranslate(c, st, 267, 266, {"vec": [18.835208928403254, 0, 4.944609587220523]});
        stepVol(c, st, 267, "translate");
        atStep(st, 268);
        cqIntersect(c, st, 268, 267, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 268, "intersect");
        atStep(st, 269);
        cqUnion(c, st, 269, 261, {"toUnion": {"wp": 268}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 269, "union");
        atStep(st, 270);
        cqWorkplaneNew(c, st, 270, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 271);
        cqPolyline(c, st, 271, 270, {"listOfXYTuple": [[-0.3122111138261774, 0.6758067574404814], [0.3122111138261774, 0.6758067574404814], [0.3122111138261774, -0.6758067574404814], [-0.3122111138261774, -0.6758067574404814]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 272);
        cqClose(c, st, 272, 271, {});
        atStep(st, 273);
        cqExtrude(c, st, 273, 272, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 273, "extrude");
        atStep(st, 274);
        cqRotate(c, st, 274, 273, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 174.43271860699753});
        stepVol(c, st, 274, "rotate");
        atStep(st, 275);
        cqTranslate(c, st, 275, 274, {"vec": [2.7241129587681288, 0, 4.770530045678283]});
        stepVol(c, st, 275, "translate");
        atStep(st, 276);
        cqIntersect(c, st, 276, 275, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 276, "intersect");
        atStep(st, 277);
        cqUnion(c, st, 277, 269, {"toUnion": {"wp": 276}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 277, "union");
        atStep(st, 278);
        cqWorkplaneNew(c, st, 278, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 279);
        cqPolyline(c, st, 279, 278, {"listOfXYTuple": [[-1.116232138614925, 0.5925957899375696], [1.116232138614925, 0.5925957899375696], [1.116232138614925, -0.5925957899375696], [-1.116232138614925, -0.5925957899375696]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 280);
        cqClose(c, st, 280, 279, {});
        atStep(st, 281);
        cqExtrude(c, st, 281, 280, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 281, "extrude");
        atStep(st, 282);
        cqRotate(c, st, 282, 281, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 6.105942155053289});
        stepVol(c, st, 282, "rotate");
        atStep(st, 283);
        cqTranslate(c, st, 283, 282, {"vec": [4.144751023256603, 0, 4.85897150139531]});
        stepVol(c, st, 283, "translate");
        atStep(st, 284);
        cqIntersect(c, st, 284, 283, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 284, "intersect");
        atStep(st, 285);
        cqUnion(c, st, 285, 277, {"toUnion": {"wp": 284}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 285, "union");
        atStep(st, 286);
        cqWorkplaneNew(c, st, 286, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 287);
        cqPolyline(c, st, 287, 286, {"listOfXYTuple": [[-1.7683548362085617, 0.6600473263317903], [1.7683548362085617, 0.6600473263317903], [1.7683548362085617, -0.6600473263317903], [-1.7683548362085617, -0.6600473263317903]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 288);
        cqClose(c, st, 288, 287, {});
        atStep(st, 289);
        cqExtrude(c, st, 289, 288, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 289, "extrude");
        atStep(st, 290);
        cqRotate(c, st, 290, 289, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 86.15953126168466});
        stepVol(c, st, 290, "rotate");
        atStep(st, 291);
        cqTranslate(c, st, 291, 290, {"vec": [3.153293369446653, 0, 6.504624856313324]});
        stepVol(c, st, 291, "translate");
        atStep(st, 292);
        cqIntersect(c, st, 292, 291, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 292, "intersect");
        atStep(st, 293);
        cqUnion(c, st, 293, 285, {"toUnion": {"wp": 292}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 293, "union");
        atStep(st, 294);
        cqWorkplaneNew(c, st, 294, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 295);
        cqPolyline(c, st, 295, 294, {"listOfXYTuple": [[-0.3905252887596776, 0.590642023703031], [0.3905252887596776, 0.590642023703031], [0.3905252887596776, -0.590642023703031], [-0.3905252887596776, -0.590642023703031]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 296);
        cqClose(c, st, 296, 295, {});
        atStep(st, 297);
        cqExtrude(c, st, 297, 296, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 297, "extrude");
        atStep(st, 298);
        cqRotate(c, st, 298, 297, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -40.093512689141754});
        stepVol(c, st, 298, "rotate");
        atStep(st, 299);
        cqTranslate(c, st, 299, 298, {"vec": [14.463031093982092, 0, 1.2590053788109428]});
        stepVol(c, st, 299, "translate");
        atStep(st, 300);
        cqIntersect(c, st, 300, 299, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 300, "intersect");
        atStep(st, 301);
        cqUnion(c, st, 301, 293, {"toUnion": {"wp": 300}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 301, "union");
        atStep(st, 302);
        cqWorkplaneNew(c, st, 302, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 303);
        cqPolyline(c, st, 303, 302, {"listOfXYTuple": [[-0.8041846510412752, 0.5947554565425793], [0.8041846510412752, 0.5947554565425793], [0.8041846510412752, -0.5947554565425793], [-0.8041846510412752, -0.5947554565425793]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 304);
        cqClose(c, st, 304, 303, {});
        atStep(st, 305);
        cqExtrude(c, st, 305, 304, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 305, "extrude");
        atStep(st, 306);
        cqRotate(c, st, 306, 305, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 18.48083906353468});
        stepVol(c, st, 306, "rotate");
        atStep(st, 307);
        cqTranslate(c, st, 307, 306, {"vec": [14.926994083499943, 0, 1.7654346022254428]});
        stepVol(c, st, 307, "translate");
        atStep(st, 308);
        cqIntersect(c, st, 308, 307, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 308, "intersect");
        atStep(st, 309);
        cqUnion(c, st, 309, 301, {"toUnion": {"wp": 308}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 309, "union");
        atStep(st, 310);
        cqWorkplaneNew(c, st, 310, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 311);
        cqPolyline(c, st, 311, 310, {"listOfXYTuple": [[-1.3195208336876396, 0.6439508171475317], [1.3195208336876396, 0.6439508171475317], [1.3195208336876396, -0.6439508171475317], [-1.3195208336876396, -0.6439508171475317]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 312);
        cqClose(c, st, 312, 311, {});
        atStep(st, 313);
        cqExtrude(c, st, 313, 312, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 313, "extrude");
        atStep(st, 314);
        cqRotate(c, st, 314, 313, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -29.50913831878057});
        stepVol(c, st, 314, "rotate");
        atStep(st, 315);
        cqTranslate(c, st, 315, 314, {"vec": [15.31263028932454, 0, 0.8605718117428857]});
        stepVol(c, st, 315, "translate");
        atStep(st, 316);
        cqIntersect(c, st, 316, 315, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 316, "intersect");
        atStep(st, 317);
        cqUnion(c, st, 317, 309, {"toUnion": {"wp": 316}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 317, "union");
        atStep(st, 318);
        cqWorkplaneNew(c, st, 318, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 319);
        cqPolyline(c, st, 319, 318, {"listOfXYTuple": [[-2.013458728645799, 0.6256647766322518], [2.013458728645799, 0.6256647766322518], [2.013458728645799, -0.6256647766322518], [-2.013458728645799, -0.6256647766322518]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 320);
        cqClose(c, st, 320, 319, {});
        atStep(st, 321);
        cqExtrude(c, st, 321, 320, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 321, "extrude");
        atStep(st, 322);
        cqRotate(c, st, 322, 321, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -108.61074048229862});
        stepVol(c, st, 322, "rotate");
        atStep(st, 323);
        cqTranslate(c, st, 323, 322, {"vec": [16.332275819902488, 0, 3.928523240884187]});
        stepVol(c, st, 323, "translate");
        atStep(st, 324);
        cqIntersect(c, st, 324, 323, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 324, "intersect");
        atStep(st, 325);
        cqUnion(c, st, 325, 317, {"toUnion": {"wp": 324}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 325, "union");
        atStep(st, 326);
        cqWorkplaneNew(c, st, 326, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 327);
        cqPolyline(c, st, 327, 326, {"listOfXYTuple": [[-2.542042301487014, 0.7577647589574285], [2.542042301487014, 0.7577647589574285], [2.542042301487014, -0.7577647589574285], [-2.542042301487014, -0.7577647589574285]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 328);
        cqClose(c, st, 328, 327, {});
        atStep(st, 329);
        cqExtrude(c, st, 329, 328, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 329, "extrude");
        atStep(st, 330);
        cqRotate(c, st, 330, 329, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -49.98473379868701});
        stepVol(c, st, 330, "rotate");
        atStep(st, 331);
        cqTranslate(c, st, 331, 330, {"vec": [18.60935702698239, 0, 3.889813458356578]});
        stepVol(c, st, 331, "translate");
        atStep(st, 332);
        cqIntersect(c, st, 332, 331, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 332, "intersect");
        atStep(st, 333);
        cqUnion(c, st, 333, 325, {"toUnion": {"wp": 332}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 333, "union");
        atStep(st, 334);
        cqWorkplaneNew(c, st, 334, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 335);
        cqPolyline(c, st, 335, 334, {"listOfXYTuple": [[-2.6560709949336987, 0.605452883986849], [2.6560709949336987, 0.605452883986849], [2.6560709949336987, -0.605452883986849], [-2.6560709949336987, -0.605452883986849]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 336);
        cqClose(c, st, 336, 335, {});
        atStep(st, 337);
        cqExtrude(c, st, 337, 336, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 337, "extrude");
        atStep(st, 338);
        cqRotate(c, st, 338, 337, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -114.62042338881636});
        stepVol(c, st, 338, "rotate");
        atStep(st, 339);
        cqTranslate(c, st, 339, 338, {"vec": [15.868312830384635, 0, 3.422094017469687]});
        stepVol(c, st, 339, "translate");
        atStep(st, 340);
        cqIntersect(c, st, 340, 339, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 340, "intersect");
        atStep(st, 341);
        cqUnion(c, st, 341, 333, {"toUnion": {"wp": 340}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 341, "union");
        atStep(st, 342);
        cqWorkplaneNew(c, st, 342, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 343);
        cqPolyline(c, st, 343, 342, {"listOfXYTuple": [[-1.328652933842272, 0.8295449021042014], [1.328652933842272, 0.8295449021042014], [1.328652933842272, -0.8295449021042014], [-1.328652933842272, -0.8295449021042014]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 344);
        cqClose(c, st, 344, 343, {});
        atStep(st, 345);
        cqExtrude(c, st, 345, 344, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 345, "extrude");
        atStep(st, 346);
        cqRotate(c, st, 346, 345, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -169.134690057778});
        stepVol(c, st, 346, "rotate");
        atStep(st, 347);
        cqTranslate(c, st, 347, 346, {"vec": [1.3048342014315266, 0, 0.2504522417193089]});
        stepVol(c, st, 347, "translate");
        atStep(st, 348);
        cqIntersect(c, st, 348, 347, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 348, "intersect");
        atStep(st, 349);
        cqUnion(c, st, 349, 341, {"toUnion": {"wp": 348}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 349, "union");
        atStep(st, 350);
        cqWorkplaneNew(c, st, 350, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 351);
        cqPolyline(c, st, 351, 350, {"listOfXYTuple": [[-2.1521963622409013, 0.7090776299052017], [2.1521963622409013, 0.7090776299052017], [2.1521963622409013, -0.7090776299052017], [-2.1521963622409013, -0.7090776299052017]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 352);
        cqClose(c, st, 352, 351, {});
        atStep(st, 353);
        cqExtrude(c, st, 353, 352, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 353, "extrude");
        atStep(st, 354);
        cqRotate(c, st, 354, 353, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 92.61377423976384});
        stepVol(c, st, 354, "rotate");
        atStep(st, 355);
        cqTranslate(c, st, 355, 354, {"vec": [2.5115214797195575, 0, 2.650861776763402]});
        stepVol(c, st, 355, "translate");
        atStep(st, 356);
        cqIntersect(c, st, 356, 355, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 356, "intersect");
        atStep(st, 357);
        cqUnion(c, st, 357, 349, {"toUnion": {"wp": 356}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 357, "union");
        atStep(st, 358);
        cqWorkplaneNew(c, st, 358, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 359);
        cqPolyline(c, st, 359, 358, {"listOfXYTuple": [[-2.5998868780483284, 0.62876534579103], [2.5998868780483284, 0.62876534579103], [2.5998868780483284, -0.62876534579103], [-2.5998868780483284, -0.62876534579103]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 360);
        cqClose(c, st, 360, 359, {});
        atStep(st, 361);
        cqExtrude(c, st, 361, 360, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 361, "extrude");
        atStep(st, 362);
        cqRotate(c, st, 362, 361, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 59.42454928097079});
        stepVol(c, st, 362, "rotate");
        atStep(st, 363);
        cqTranslate(c, st, 363, 362, {"vec": [3.932159544208031, 0, 2.7393032324804274]});
        stepVol(c, st, 363, "translate");
        atStep(st, 364);
        cqIntersect(c, st, 364, 363, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 364, "intersect");
        atStep(st, 365);
        cqUnion(c, st, 365, 357, {"toUnion": {"wp": 364}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 365, "union");
        atStep(st, 366);
        cqWorkplaneNew(c, st, 366, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 367);
        cqPolyline(c, st, 367, 366, {"listOfXYTuple": [[-0.6868276450249778, 0.6174476905045864], [0.6868276450249778, 0.6174476905045864], [0.6868276450249778, -0.6174476905045864], [-0.6868276450249778, -0.6174476905045864]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 368);
        cqClose(c, st, 368, 367, {});
        atStep(st, 369);
        cqExtrude(c, st, 369, 368, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 369, "extrude");
        atStep(st, 370);
        cqRotate(c, st, 370, 369, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -132.49422502984558});
        stepVol(c, st, 370, "rotate");
        atStep(st, 371);
        cqTranslate(c, st, 371, 370, {"vec": [15.225743713900636, 0, 1.5139218600108988]});
        stepVol(c, st, 371, "translate");
        atStep(st, 372);
        cqIntersect(c, st, 372, 371, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 372, "intersect");
        atStep(st, 373);
        cqUnion(c, st, 373, 365, {"toUnion": {"wp": 372}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 373, "union");
        atStep(st, 374);
        cqWorkplaneNew(c, st, 374, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 375);
        cqPolyline(c, st, 375, 374, {"listOfXYTuple": [[-0.983611688036834, 0.6718024948673789], [0.983611688036834, 0.6718024948673789], [0.983611688036834, -0.6718024948673789], [-0.983611688036834, -0.6718024948673789]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 376);
        cqClose(c, st, 376, 375, {});
        atStep(st, 377);
        cqExtrude(c, st, 377, 376, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 377, "extrude");
        atStep(st, 378);
        cqRotate(c, st, 378, 377, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -66.91717139706118});
        stepVol(c, st, 378, "rotate");
        atStep(st, 379);
        cqTranslate(c, st, 379, 378, {"vec": [16.075342909243084, 0, 1.1154882929428416]});
        stepVol(c, st, 379, "translate");
        atStep(st, 380);
        cqIntersect(c, st, 380, 379, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 380, "intersect");
        atStep(st, 381);
        cqUnion(c, st, 381, 373, {"toUnion": {"wp": 380}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 381, "union");
        atStep(st, 382);
        cqWorkplaneNew(c, st, 382, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 383);
        cqPolyline(c, st, 383, 382, {"listOfXYTuple": [[-2.277410211380421, 0.7483331696512716], [2.277410211380421, 0.7483331696512716], [2.277410211380421, -0.7483331696512716], [-2.277410211380421, -0.7483331696512716]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 384);
        cqClose(c, st, 384, 383, {});
        atStep(st, 385);
        cqExtrude(c, st, 385, 384, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 385, "extrude");
        atStep(st, 386);
        cqRotate(c, st, 386, 385, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -0.9739193942909496});
        stepVol(c, st, 386, "rotate");
        atStep(st, 387);
        cqTranslate(c, st, 387, 386, {"vec": [17.966787910498393, 0, 1.9816413008977896]});
        stepVol(c, st, 387, "translate");
        atStep(st, 388);
        cqIntersect(c, st, 388, 387, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 388, "intersect");
        atStep(st, 389);
        cqUnion(c, st, 389, 381, {"toUnion": {"wp": 388}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 389, "union");
        atStep(st, 390);
        cqWorkplaneNew(c, st, 390, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 391);
        cqPolyline(c, st, 391, 390, {"listOfXYTuple": [[-1.0877182061443789, 0.9229387444832836], [1.0877182061443789, 0.9229387444832836], [1.0877182061443789, -0.9229387444832836], [-1.0877182061443789, -0.9229387444832836]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 392);
        cqClose(c, st, 392, 391, {});
        atStep(st, 393);
        cqExtrude(c, st, 393, 392, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 393, "extrude");
        atStep(st, 394);
        cqRotate(c, st, 394, 393, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -153.17652005503854});
        stepVol(c, st, 394, "rotate");
        atStep(st, 395);
        cqTranslate(c, st, 395, 394, {"vec": [21.21454990290976, 0, 2.4337570565874227]});
        stepVol(c, st, 395, "translate");
        atStep(st, 396);
        cqIntersect(c, st, 396, 395, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 396, "intersect");
        atStep(st, 397);
        cqUnion(c, st, 397, 389, {"toUnion": {"wp": 396}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 397, "union");
        atStep(st, 398);
        cqWorkplaneNew(c, st, 398, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 399);
        cqPolyline(c, st, 399, 398, {"listOfXYTuple": [[-1.262243360899847, 1.0068958097849026], [1.262243360899847, 1.0068958097849026], [1.262243360899847, -1.0068958097849026], [-1.262243360899847, -1.0068958097849026]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 400);
        cqClose(c, st, 400, 399, {});
        atStep(st, 401);
        cqExtrude(c, st, 401, 400, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 401, "extrude");
        atStep(st, 402);
        cqRotate(c, st, 402, 401, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -74.9317028927121});
        stepVol(c, st, 402, "rotate");
        atStep(st, 403);
        cqTranslate(c, st, 403, 402, {"vec": [22.513376414892303, 0, 1.7057394202262577]});
        stepVol(c, st, 403, "translate");
        atStep(st, 404);
        cqIntersect(c, st, 404, 403, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 404, "intersect");
        atStep(st, 405);
        cqUnion(c, st, 405, 397, {"toUnion": {"wp": 404}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 405, "union");
        atStep(st, 406);
        cqWorkplaneNew(c, st, 406, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 407);
        cqPolyline(c, st, 407, 406, {"listOfXYTuple": [[-3.167515555291959, 0.8311294358115846], [3.167515555291959, 0.8311294358115846], [3.167515555291959, -0.8311294358115846], [-3.167515555291959, -0.8311294358115846]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 408);
        cqClose(c, st, 408, 407, {});
        atStep(st, 409);
        cqExtrude(c, st, 409, 408, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 409, "extrude");
        atStep(st, 410);
        cqRotate(c, st, 410, 409, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -154.63363917327916});
        stepVol(c, st, 410, "rotate");
        atStep(st, 411);
        cqTranslate(c, st, 411, 410, {"vec": [19.32310490165445, 0, 1.5676040486324747]});
        stepVol(c, st, 411, "translate");
        atStep(st, 412);
        cqIntersect(c, st, 412, 411, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 412, "intersect");
        atStep(st, 413);
        cqUnion(c, st, 413, 405, {"toUnion": {"wp": 412}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 413, "union");
        atStep(st, 414);
        cqWorkplaneNew(c, st, 414, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 415);
        cqPolyline(c, st, 415, 414, {"listOfXYTuple": [[-0.9383859014782308, 0.666778702715085], [0.9383859014782308, 0.666778702715085], [0.9383859014782308, -0.666778702715085], [-0.9383859014782308, -0.666778702715085]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 416);
        cqClose(c, st, 416, 415, {});
        atStep(st, 417);
        cqExtrude(c, st, 417, 416, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 417, "extrude");
        atStep(st, 418);
        cqRotate(c, st, 418, 417, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 154.874995694909});
        stepVol(c, st, 418, "rotate");
        atStep(st, 419);
        cqTranslate(c, st, 419, 418, {"vec": [15.61137991972523, 0, 0.6090590695283415]});
        stepVol(c, st, 419, "translate");
        atStep(st, 420);
        cqIntersect(c, st, 420, 419, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 420, "intersect");
        atStep(st, 421);
        cqUnion(c, st, 421, 413, {"toUnion": {"wp": 420}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 421, "union");
        atStep(st, 422);
        cqWorkplaneNew(c, st, 422, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 423);
        cqPolyline(c, st, 423, 422, {"listOfXYTuple": [[-2.0803329603606975, 0.7897776727619125], [2.0803329603606975, 0.7897776727619125], [2.0803329603606975, -0.7897776727619125], [-2.0803329603606975, -0.7897776727619125]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 424);
        cqClose(c, st, 424, 423, {});
        atStep(st, 425);
        cqExtrude(c, st, 425, 424, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 425, "extrude");
        atStep(st, 426);
        cqRotate(c, st, 426, 425, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 24.60455448244012});
        stepVol(c, st, 426, "rotate");
        atStep(st, 427);
        cqTranslate(c, st, 427, 426, {"vec": [18.35242411632299, 0, 1.0767785104152323]});
        stepVol(c, st, 427, "translate");
        atStep(st, 428);
        cqIntersect(c, st, 428, 427, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 428, "intersect");
        atStep(st, 429);
        cqUnion(c, st, 429, 421, {"toUnion": {"wp": 428}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 429, "union");
        atStep(st, 430);
        cqWorkplaneNew(c, st, 430, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 431);
        cqPolyline(c, st, 431, 430, {"listOfXYTuple": [[-3.1932606703904267, 0.8766789181490017], [3.1932606703904267, 0.8766789181490017], [3.1932606703904267, -0.8766789181490017], [-3.1932606703904267, -0.8766789181490017]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 432);
        cqClose(c, st, 432, 431, {});
        atStep(st, 433);
        cqExtrude(c, st, 433, 432, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 433, "extrude");
        atStep(st, 434);
        cqRotate(c, st, 434, 433, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 2.4792978407184454});
        stepVol(c, st, 434, "rotate");
        atStep(st, 435);
        cqTranslate(c, st, 435, 434, {"vec": [19.651250628305537, 0, 0.3487608740540671]});
        stepVol(c, st, 435, "translate");
        atStep(st, 436);
        cqIntersect(c, st, 436, 435, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 436, "intersect");
        atStep(st, 437);
        cqUnion(c, st, 437, 429, {"toUnion": {"wp": 436}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 437, "union");
        atStep(st, 438);
        cqWorkplaneNew(c, st, 438, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 439);
        cqPolyline(c, st, 439, 438, {"listOfXYTuple": [[-1.1319312795164422, 0.4383714673827742], [1.1319312795164422, 0.4383714673827742], [1.1319312795164422, -0.4383714673827742], [-1.1319312795164422, -0.4383714673827742]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 440);
        cqClose(c, st, 440, 439, {});
        atStep(st, 441);
        cqExtrude(c, st, 441, 440, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 441, "extrude");
        atStep(st, 442);
        cqRotate(c, st, 442, 441, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -79.96960432928799});
        stepVol(c, st, 442, "rotate");
        atStep(st, 443);
        cqTranslate(c, st, 443, 442, {"vec": [8.606008555937343, 0, 7.903016598354807]});
        stepVol(c, st, 443, "translate");
        atStep(st, 444);
        cqIntersect(c, st, 444, 443, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 444, "intersect");
        atStep(st, 445);
        cqUnion(c, st, 445, 437, {"toUnion": {"wp": 444}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 445, "union");
        atStep(st, 446);
        cqWorkplaneNew(c, st, 446, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 447);
        cqPolyline(c, st, 447, 446, {"listOfXYTuple": [[-1.305247599182982, 0.5205918737903246], [1.305247599182982, 0.5205918737903246], [1.305247599182982, -0.5205918737903246], [-1.305247599182982, -0.5205918737903246]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 448);
        cqClose(c, st, 448, 447, {});
        atStep(st, 449);
        cqExtrude(c, st, 449, 448, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 449, "extrude");
        atStep(st, 450);
        cqRotate(c, st, 450, 449, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -163.59681015166927});
        stepVol(c, st, 450, "rotate");
        atStep(st, 451);
        cqTranslate(c, st, 451, 450, {"vec": [7.1567376647426375, 0, 8.649051647551207]});
        stepVol(c, st, 451, "translate");
        atStep(st, 452);
        cqIntersect(c, st, 452, 451, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 452, "intersect");
        atStep(st, 453);
        cqUnion(c, st, 453, 445, {"toUnion": {"wp": 452}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 453, "union");
        atStep(st, 454);
        cqWorkplaneNew(c, st, 454, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 455);
        cqPolyline(c, st, 455, 454, {"listOfXYTuple": [[-1.3620314489116263, 0.529051732945929], [1.3620314489116263, 0.529051732945929], [1.3620314489116263, -0.529051732945929], [-1.3620314489116263, -0.529051732945929]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 456);
        cqClose(c, st, 456, 455, {});
        atStep(st, 457);
        cqExtrude(c, st, 457, 456, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 457, "extrude");
        atStep(st, 458);
        cqRotate(c, st, 458, 457, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -169.31673386364193});
        stepVol(c, st, 458, "rotate");
        atStep(st, 459);
        cqTranslate(c, st, 459, 458, {"vec": [7.070436076476547, 0, 8.765153987973022]});
        stepVol(c, st, 459, "translate");
        atStep(st, 460);
        cqIntersect(c, st, 460, 459, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 460, "intersect");
        atStep(st, 461);
        cqUnion(c, st, 461, 453, {"toUnion": {"wp": 460}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 461, "union");
        atStep(st, 462);
        cqWorkplaneNew(c, st, 462, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 463);
        cqPolyline(c, st, 463, 462, {"listOfXYTuple": [[-1.4233883522647302, 0.6107309286791847], [1.4233883522647302, 0.6107309286791847], [1.4233883522647302, -0.6107309286791847], [-1.4233883522647302, -0.6107309286791847]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 464);
        cqClose(c, st, 464, 463, {});
        atStep(st, 465);
        cqExtrude(c, st, 465, 464, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 465, "extrude");
        atStep(st, 466);
        cqRotate(c, st, 466, 465, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -176.4376634273435});
        stepVol(c, st, 466, "rotate");
        atStep(st, 467);
        cqTranslate(c, st, 467, 466, {"vec": [3.834012621064536, 0, 4.889260525805212]});
        stepVol(c, st, 467, "translate");
        atStep(st, 468);
        cqIntersect(c, st, 468, 467, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 468, "intersect");
        atStep(st, 469);
        cqUnion(c, st, 469, 461, {"toUnion": {"wp": 468}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 469, "union");
        atStep(st, 470);
        cqWorkplaneNew(c, st, 470, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 471);
        cqPolyline(c, st, 471, 470, {"listOfXYTuple": [[-1.6830509309646884, 0.5255649746560956], [1.6830509309646884, 0.5255649746560956], [1.6830509309646884, -0.5255649746560956], [-1.6830509309646884, -0.5255649746560956]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 472);
        cqClose(c, st, 472, 471, {});
        atStep(st, 473);
        cqExtrude(c, st, 473, 472, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 473, "extrude");
        atStep(st, 474);
        cqRotate(c, st, 474, 473, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 78.86675146156723});
        stepVol(c, st, 474, "rotate");
        atStep(st, 475);
        cqTranslate(c, st, 475, 474, {"vec": [5.579633303578454, 0, 6.629079204994341]});
        stepVol(c, st, 475, "translate");
        atStep(st, 476);
        cqIntersect(c, st, 476, 475, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 476, "intersect");
        atStep(st, 477);
        cqUnion(c, st, 477, 469, {"toUnion": {"wp": 476}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 477, "union");
        atStep(st, 478);
        cqWorkplaneNew(c, st, 478, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 479);
        cqPolyline(c, st, 479, 478, {"listOfXYTuple": [[-1.7835225377745094, 0.5326879696880833], [1.7835225377745094, 0.5326879696880833], [1.7835225377745094, -0.5326879696880833], [-1.7835225377745094, -0.5326879696880833]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 480);
        cqClose(c, st, 480, 479, {});
        atStep(st, 481);
        cqExtrude(c, st, 481, 480, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 481, "extrude");
        atStep(st, 482);
        cqRotate(c, st, 482, 481, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 82.3092826660455});
        stepVol(c, st, 482, "rotate");
        atStep(st, 483);
        cqTranslate(c, st, 483, 482, {"vec": [5.4933317153123635, 0, 6.745181545416157]});
        stepVol(c, st, 483, "translate");
        atStep(st, 484);
        cqIntersect(c, st, 484, 483, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 484, "intersect");
        atStep(st, 485);
        cqUnion(c, st, 485, 477, {"toUnion": {"wp": 484}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 485, "union");
        atStep(st, 486);
        cqWorkplaneNew(c, st, 486, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 487);
        cqPolyline(c, st, 487, 486, {"listOfXYTuple": [[-1.6755523111840818, 0.7656664699801826], [1.6755523111840818, 0.7656664699801826], [1.6755523111840818, -0.7656664699801826], [-1.6755523111840818, -0.7656664699801826]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 488);
        cqClose(c, st, 488, 487, {});
        atStep(st, 489);
        cqExtrude(c, st, 489, 488, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 489, "extrude");
        atStep(st, 490);
        cqRotate(c, st, 490, 489, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 14.964796097655944});
        stepVol(c, st, 490, "rotate");
        atStep(st, 491);
        cqTranslate(c, st, 491, 490, {"vec": [1.6530099786829793, 0, 7.836338344169069]});
        stepVol(c, st, 491, "translate");
        atStep(st, 492);
        cqIntersect(c, st, 492, 491, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 492, "intersect");
        atStep(st, 493);
        cqUnion(c, st, 493, 485, {"toUnion": {"wp": 492}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 493, "union");
        atStep(st, 494);
        cqWorkplaneNew(c, st, 494, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 495);
        cqPolyline(c, st, 495, 494, {"listOfXYTuple": [[-1.7631570871284048, 0.7673856009146102], [1.7631570871284048, 0.7673856009146102], [1.7631570871284048, -0.7673856009146102], [-1.7631570871284048, -0.7673856009146102]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 496);
        cqClose(c, st, 496, 495, {});
        atStep(st, 497);
        cqExtrude(c, st, 497, 496, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 497, "extrude");
        atStep(st, 498);
        cqRotate(c, st, 498, 497, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -47.57165600571709});
        stepVol(c, st, 498, "rotate");
        atStep(st, 499);
        cqTranslate(c, st, 499, 498, {"vec": [1.2238295680044553, 0, 6.102243533534028]});
        stepVol(c, st, 499, "translate");
        atStep(st, 500);
        cqIntersect(c, st, 500, 499, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 500, "intersect");
        atStep(st, 501);
        cqUnion(c, st, 501, 493, {"toUnion": {"wp": 500}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 501, "union");
        atStep(st, 502);
        cqWorkplaneNew(c, st, 502, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 503);
        cqPolyline(c, st, 503, 502, {"listOfXYTuple": [[-2.784339372466262, 0.7831269918148837], [2.784339372466262, 0.7831269918148837], [2.784339372466262, -0.7831269918148837], [-2.784339372466262, -0.7831269918148837]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 504);
        cqClose(c, st, 504, 503, {});
        atStep(st, 505);
        cqExtrude(c, st, 505, 504, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 505, "extrude");
        atStep(st, 506);
        cqRotate(c, st, 506, 505, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 45.000266962150185});
        stepVol(c, st, 506, "rotate");
        atStep(st, 507);
        cqTranslate(c, st, 507, 506, {"vec": [2.003100657324711, 0, 9.372502421836444]});
        stepVol(c, st, 507, "translate");
        atStep(st, 508);
        cqIntersect(c, st, 508, 507, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 508, "intersect");
        atStep(st, 509);
        cqUnion(c, st, 509, 501, {"toUnion": {"wp": 508}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 509, "union");
        atStep(st, 510);
        cqWorkplaneNew(c, st, 510, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 511);
        cqPolyline(c, st, 511, 510, {"listOfXYTuple": [[-1.5755518261191817, 0.7171126308072302], [1.5755518261191817, 0.7171126308072302], [1.5755518261191817, -0.7171126308072302], [-1.5755518261191817, -0.7171126308072302]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 512);
        cqClose(c, st, 512, 511, {});
        atStep(st, 513);
        cqExtrude(c, st, 513, 512, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 513, "extrude");
        atStep(st, 514);
        cqRotate(c, st, 514, 513, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -102.838396757347});
        stepVol(c, st, 514, "rotate");
        atStep(st, 515);
        cqTranslate(c, st, 515, 514, {"vec": [3.621826056574842, 0, 9.805172769025644]});
        stepVol(c, st, 515, "translate");
        atStep(st, 516);
        cqIntersect(c, st, 516, 515, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 516, "intersect");
        atStep(st, 517);
        cqUnion(c, st, 517, 509, {"toUnion": {"wp": 516}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 517, "union");
        atStep(st, 518);
        cqWorkplaneNew(c, st, 518, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 519);
        cqPolyline(c, st, 519, 518, {"listOfXYTuple": [[-1.6657839289030651, 0.6668182668920943], [1.6657839289030651, 0.6668182668920943], [1.6657839289030651, -0.6668182668920943], [-1.6657839289030651, -0.6668182668920943]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 520);
        cqClose(c, st, 520, 519, {});
        atStep(st, 521);
        cqExtrude(c, st, 521, 520, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 521, "extrude");
        atStep(st, 522);
        cqRotate(c, st, 522, 521, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -58.108753981469675});
        stepVol(c, st, 522, "rotate");
        atStep(st, 523);
        cqTranslate(c, st, 523, 522, {"vec": [4.851964740144146, 0, 9.92699897800155]});
        stepVol(c, st, 523, "translate");
        atStep(st, 524);
        cqIntersect(c, st, 524, 523, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 524, "intersect");
        atStep(st, 525);
        cqUnion(c, st, 525, 517, {"toUnion": {"wp": 524}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 525, "union");
        atStep(st, 526);
        cqWorkplaneNew(c, st, 526, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 527);
        cqPolyline(c, st, 527, 526, {"listOfXYTuple": [[-1.809994135332013, 0.6584780512253962], [1.809994135332013, 0.6584780512253962], [1.809994135332013, -0.6584780512253962], [-1.809994135332013, -0.6584780512253962]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 528);
        cqClose(c, st, 528, 527, {});
        atStep(st, 529);
        cqExtrude(c, st, 529, 528, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 529, "extrude");
        atStep(st, 530);
        cqRotate(c, st, 530, 529, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -57.73089210762522});
        stepVol(c, st, 530, "rotate");
        atStep(st, 531);
        cqTranslate(c, st, 531, 530, {"vec": [4.938266328410236, 0, 9.81089663757973]});
        stepVol(c, st, 531, "translate");
        atStep(st, 532);
        cqIntersect(c, st, 532, 531, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 532, "intersect");
        atStep(st, 533);
        cqUnion(c, st, 533, 525, {"toUnion": {"wp": 532}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 533, "union");
        atStep(st, 534);
        cqWorkplaneNew(c, st, 534, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 535);
        cqPolyline(c, st, 535, 534, {"listOfXYTuple": [[-1.4889459315507894, 0.9605523347871407], [1.4889459315507894, 0.9605523347871407], [1.4889459315507894, -0.9605523347871407], [-1.4889459315507894, -0.9605523347871407]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 536);
        cqClose(c, st, 536, 535, {});
        atStep(st, 537);
        cqExtrude(c, st, 537, 536, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 537, "extrude");
        atStep(st, 538);
        cqRotate(c, st, 538, 537, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -29.27148219693421});
        stepVol(c, st, 538, "rotate");
        atStep(st, 539);
        cqTranslate(c, st, 539, 538, {"vec": [21.542695629560846, 0, 1.2149138820090153]});
        stepVol(c, st, 539, "translate");
        atStep(st, 540);
        cqIntersect(c, st, 540, 539, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 540, "intersect");
        atStep(st, 541);
        cqUnion(c, st, 541, 533, {"toUnion": {"wp": 540}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 541, "union");
        atStep(st, 542);
        cqWorkplaneNew(c, st, 542, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 543);
        cqPolyline(c, st, 543, 542, {"listOfXYTuple": [[-2.78066264819841, 0.7348878153260571], [2.78066264819841, 0.7348878153260571], [2.78066264819841, -0.7348878153260571], [-2.78066264819841, -0.7348878153260571]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 544);
        cqClose(c, st, 544, 543, {});
        atStep(st, 545);
        cqExtrude(c, st, 545, 544, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 545, "extrude");
        atStep(st, 546);
        cqRotate(c, st, 546, 545, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -170.316569794702});
        stepVol(c, st, 546, "rotate");
        atStep(st, 547);
        cqTranslate(c, st, 547, 546, {"vec": [17.50282492098054, 0, 1.4752120774832898]});
        stepVol(c, st, 547, "translate");
        atStep(st, 548);
        cqIntersect(c, st, 548, 547, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 548, "intersect");
        atStep(st, 549);
        cqUnion(c, st, 549, 541, {"toUnion": {"wp": 548}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 549, "union");
        atStep(st, 550);
        cqWorkplaneNew(c, st, 550, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 551);
        cqPolyline(c, st, 551, 550, {"listOfXYTuple": [[-4.973858479088912, 1.165391505644694], [4.973858479088912, 1.165391505644694], [4.973858479088912, -1.165391505644694], [-4.973858479088912, -1.165391505644694]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 552);
        cqClose(c, st, 552, 551, {});
        atStep(st, 553);
        cqExtrude(c, st, 553, 552, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 553, "extrude");
        atStep(st, 554);
        cqRotate(c, st, 554, 553, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -11.263085549870215});
        stepVol(c, st, 554, "rotate");
        atStep(st, 555);
        cqTranslate(c, st, 555, 554, {"vec": [25.12193455878915, 0, 0.9714657591850904]});
        stepVol(c, st, 555, "translate");
        atStep(st, 556);
        cqIntersect(c, st, 556, 555, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 556, "intersect");
        atStep(st, 557);
        cqUnion(c, st, 557, 549, {"toUnion": {"wp": 556}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 557, "union");
        atStep(st, 558);
        cqWorkplaneNew(c, st, 558, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 559);
        cqPolyline(c, st, 559, 558, {"listOfXYTuple": [[-0.14466415447052983, 0.5651248602394965], [0.14466415447052983, 0.5651248602394965], [0.14466415447052983, -0.5651248602394965], [-0.14466415447052983, -0.5651248602394965]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 560);
        cqClose(c, st, 560, 559, {});
        atStep(st, 561);
        cqExtrude(c, st, 561, 560, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 561, "extrude");
        atStep(st, 562);
        cqRotate(c, st, 562, 561, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -53.37571364099226});
        stepVol(c, st, 562, "rotate");
        atStep(st, 563);
        cqTranslate(c, st, 563, 562, {"vec": [5.818314333337808, 0, 8.39655876888826]});
        stepVol(c, st, 563, "translate");
        atStep(st, 564);
        cqIntersect(c, st, 564, 563, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 564, "intersect");
        atStep(st, 565);
        cqUnion(c, st, 565, 557, {"toUnion": {"wp": 564}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 565, "union");
        atStep(st, 566);
        cqWorkplaneNew(c, st, 566, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 567);
        cqPolyline(c, st, 567, 566, {"listOfXYTuple": [[-1.23615646501851, 0.627850569198179], [1.23615646501851, 0.627850569198179], [1.23615646501851, -0.627850569198179], [-1.23615646501851, -0.627850569198179]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 568);
        cqClose(c, st, 568, 567, {});
        atStep(st, 569);
        cqExtrude(c, st, 569, 568, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 569, "extrude");
        atStep(st, 570);
        cqRotate(c, st, 570, 569, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -174.3441816208956});
        stepVol(c, st, 570, "rotate");
        atStep(st, 571);
        cqTranslate(c, st, 571, 570, {"vec": [4.501874061502413, 0, 8.390834900334173]});
        stepVol(c, st, 571, "translate");
        atStep(st, 572);
        cqIntersect(c, st, 572, 571, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 572, "intersect");
        atStep(st, 573);
        cqUnion(c, st, 573, 565, {"toUnion": {"wp": 572}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 573, "union");
        atStep(st, 574);
        cqWorkplaneNew(c, st, 574, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 575);
        cqPolyline(c, st, 575, 574, {"listOfXYTuple": [[-1.761040521468797, 0.47267188648629965], [1.761040521468797, 0.47267188648629965], [1.761040521468797, -0.47267188648629965], [-1.761040521468797, -0.47267188648629965]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 576);
        cqClose(c, st, 576, 575, {});
        atStep(st, 577);
        cqExtrude(c, st, 577, 576, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 577, "extrude");
        atStep(st, 578);
        cqRotate(c, st, 578, 577, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -29.311755003630147});
        stepVol(c, st, 578, "rotate");
        atStep(st, 579);
        cqTranslate(c, st, 579, 578, {"vec": [7.267585224532514, 0, 7.6505237196918605]});
        stepVol(c, st, 579, "translate");
        atStep(st, 580);
        cqIntersect(c, st, 580, 579, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 580, "intersect");
        atStep(st, 581);
        cqUnion(c, st, 581, 573, {"toUnion": {"wp": 580}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 581, "union");
        atStep(st, 582);
        cqWorkplaneNew(c, st, 582, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 583);
        cqPolyline(c, st, 583, 582, {"listOfXYTuple": [[-3.5875086482139475, 1.25], [3.5875086482139475, 1.25], [3.5875086482139475, -1.25], [-3.5875086482139475, -1.25]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 584);
        cqClose(c, st, 584, 583, {});
        atStep(st, 585);
        cqExtrude(c, st, 585, 584, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 585, "extrude");
        atStep(st, 586);
        cqRotate(c, st, 586, 585, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -3.89107846923686});
        stepVol(c, st, 586, "rotate");
        atStep(st, 587);
        cqTranslate(c, st, 587, 586, {"vec": [26.420761070771697, 0, 0.24344812282392492]});
        stepVol(c, st, 587, "translate");
        atStep(st, 588);
        cqIntersect(c, st, 588, 587, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 588, "intersect");
        atStep(st, 589);
        cqUnion(c, st, 589, 581, {"toUnion": {"wp": 588}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 589, "union");
        atStep(st, 590);
        cqWorkplaneNew(c, st, 590, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 591);
        cqPolyline(c, st, 591, 590, {"listOfXYTuple": [[-4.0482478299398, 0.8217924127110544], [4.0482478299398, 0.8217924127110544], [4.0482478299398, -0.8217924127110544], [-4.0482478299398, -0.8217924127110544]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 592);
        cqClose(c, st, 592, 591, {});
        atStep(st, 593);
        cqExtrude(c, st, 593, 592, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 593, "extrude");
        atStep(st, 594);
        cqRotate(c, st, 594, 593, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 176.31339669956822});
        stepVol(c, st, 594, "rotate");
        atStep(st, 595);
        cqTranslate(c, st, 595, 594, {"vec": [18.801651432963087, 0, 0.7471944411221243]});
        stepVol(c, st, 595, "translate");
        atStep(st, 596);
        cqIntersect(c, st, 596, 595, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 596, "intersect");
        atStep(st, 597);
        cqUnion(c, st, 597, 589, {"toUnion": {"wp": 596}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 597, "union");
        atStep(st, 598);
        cqWorkplaneNew(c, st, 598, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 599);
        cqPolyline(c, st, 599, 598, {"listOfXYTuple": [[-7.693901048323721, 0.5997704685338474], [7.693901048323721, 0.5997704685338474], [7.693901048323721, -0.5997704685338474], [-7.693901048323721, -0.5997704685338474]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 600);
        cqClose(c, st, 600, 599, {});
        atStep(st, 601);
        cqExtrude(c, st, 601, 600, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 601, "extrude");
        atStep(st, 602);
        cqRotate(c, st, 602, 601, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 155.82581498040315});
        stepVol(c, st, 602, "rotate");
        atStep(st, 603);
        cqTranslate(c, st, 603, 602, {"vec": [15.822339922768352, 0, 3.637641287860747]});
        stepVol(c, st, 603, "translate");
        atStep(st, 604);
        cqIntersect(c, st, 604, 603, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 604, "intersect");
        atStep(st, 605);
        cqUnion(c, st, 605, 597, {"toUnion": {"wp": 604}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 605, "union");
        atStep(st, 606);
        cqWorkplaneNew(c, st, 606, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 607);
        cqPolyline(c, st, 607, 606, {"listOfXYTuple": [[-1.3164527154369317, 0.6200022013739019], [1.3164527154369317, 0.6200022013739019], [1.3164527154369317, -0.6200022013739019], [-1.3164527154369317, -0.6200022013739019]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 608);
        cqClose(c, st, 608, 607, {});
        atStep(st, 609);
        cqExtrude(c, st, 609, 608, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 609, "extrude");
        atStep(st, 610);
        cqRotate(c, st, 610, 609, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -179.75088011889218});
        stepVol(c, st, 610, "rotate");
        atStep(st, 611);
        cqTranslate(c, st, 611, 610, {"vec": [4.588175649768504, 0, 8.274732559912355]});
        stepVol(c, st, 611, "translate");
        atStep(st, 612);
        cqIntersect(c, st, 612, 611, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 612, "intersect");
        atStep(st, 613);
        cqUnion(c, st, 613, 605, {"toUnion": {"wp": 612}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 613, "union");
        atStep(st, 614);
        cqWorkplaneNew(c, st, 614, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 615);
        cqPolyline(c, st, 615, 614, {"listOfXYTuple": [[-1.6300166903114126, 0.4643241990405982], [1.6300166903114126, 0.4643241990405982], [1.6300166903114126, -0.4643241990405982], [-1.6300166903114126, -0.4643241990405982]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 616);
        cqClose(c, st, 616, 615, {});
        atStep(st, 617);
        cqExtrude(c, st, 617, 616, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 617, "extrude");
        atStep(st, 618);
        cqRotate(c, st, 618, 617, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -27.237859389972964});
        stepVol(c, st, 618, "rotate");
        atStep(st, 619);
        cqTranslate(c, st, 619, 618, {"vec": [7.353886812798605, 0, 7.534421379270044]});
        stepVol(c, st, 619, "translate");
        atStep(st, 620);
        cqIntersect(c, st, 620, 619, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 620, "intersect");
        atStep(st, 621);
        cqUnion(c, st, 621, 613, {"toUnion": {"wp": 620}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 621, "union");
        atStep(st, 622);
        cqWorkplaneNew(c, st, 622, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 623);
        cqPolyline(c, st, 623, 622, {"listOfXYTuple": [[-2.464581222779189, 0.6033483059504132], [2.464581222779189, 0.6033483059504132], [2.464581222779189, -0.6033483059504132], [-2.464581222779189, -0.6033483059504132]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 624);
        cqClose(c, st, 624, 623, {});
        atStep(st, 625);
        cqExtrude(c, st, 625, 624, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 625, "extrude");
        atStep(st, 626);
        cqRotate(c, st, 626, 625, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -135.09537678372104});
        stepVol(c, st, 626, "rotate");
        atStep(st, 627);
        cqTranslate(c, st, 627, 626, {"vec": [4.158995239089981, 0, 6.540637749277315]});
        stepVol(c, st, 627, "translate");
        atStep(st, 628);
        cqIntersect(c, st, 628, 627, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 628, "intersect");
        atStep(st, 629);
        cqUnion(c, st, 629, 621, {"toUnion": {"wp": 628}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 629, "union");
        atStep(st, 630);
        cqWorkplaneNew(c, st, 630, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 631);
        cqPolyline(c, st, 631, 630, {"listOfXYTuple": [[-1.786415583558754, 0.6781857944580079], [1.786415583558754, 0.6781857944580079], [1.786415583558754, -0.6781857944580079], [-1.786415583558754, -0.6781857944580079]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 632);
        cqClose(c, st, 632, 631, {});
        atStep(st, 633);
        cqExtrude(c, st, 633, 632, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 633, "extrude");
        atStep(st, 634);
        cqRotate(c, st, 634, 633, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 76.09890001057805});
        stepVol(c, st, 634, "rotate");
        atStep(st, 635);
        cqTranslate(c, st, 635, 634, {"vec": [2.8425549672545865, 0, 6.534913880723227]});
        stepVol(c, st, 635, "translate");
        atStep(st, 636);
        cqIntersect(c, st, 636, 635, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 636, "intersect");
        atStep(st, 637);
        cqUnion(c, st, 637, 629, {"toUnion": {"wp": 636}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 637, "union");
        atStep(st, 638);
        cqWorkplaneNew(c, st, 638, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 639);
        cqPolyline(c, st, 639, 638, {"listOfXYTuple": [[-2.6866447706224164, 0.7863615486843434], [2.6866447706224164, 0.7863615486843434], [2.6866447706224164, -0.7863615486843434], [-2.6866447706224164, -0.7863615486843434]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 640);
        cqClose(c, st, 640, 639, {});
        atStep(st, 641);
        cqExtrude(c, st, 641, 640, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 641, "extrude");
        atStep(st, 642);
        cqRotate(c, st, 642, 641, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -116.68870312494612});
        stepVol(c, st, 642, "rotate");
        atStep(st, 643);
        cqTranslate(c, st, 643, 642, {"vec": [1.2066872782880311, 0, 2.4004095350440933]});
        stepVol(c, st, 643, "translate");
        atStep(st, 644);
        cqIntersect(c, st, 644, 643, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 644, "intersect");
        atStep(st, 645);
        cqUnion(c, st, 645, 637, {"toUnion": {"wp": 644}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 645, "union");
        atStep(st, 646);
        cqWorkplaneNew(c, st, 646, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 647);
        cqPolyline(c, st, 647, 646, {"listOfXYTuple": [[-3.345883750370878, 0.5110606590067579], [3.345883750370878, 0.5110606590067579], [3.345883750370878, -0.5110606590067579], [-3.345883750370878, -0.5110606590067579]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 648);
        cqClose(c, st, 648, 647, {});
        atStep(st, 649);
        cqExtrude(c, st, 649, 648, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 649, "extrude");
        atStep(st, 650);
        cqRotate(c, st, 650, 649, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 17.27851202876203});
        stepVol(c, st, 650, "rotate");
        atStep(st, 651);
        cqTranslate(c, st, 651, 650, {"vec": [5.608266130284687, 0, 5.794602700080915]});
        stepVol(c, st, 651, "translate");
        atStep(st, 652);
        cqIntersect(c, st, 652, 651, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 652, "intersect");
        atStep(st, 653);
        cqUnion(c, st, 653, 645, {"toUnion": {"wp": 652}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 653, "union");
        atStep(st, 654);
        cqWorkplaneNew(c, st, 654, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 655);
        cqPolyline(c, st, 655, 654, {"listOfXYTuple": [[-4.151021566867617, 0.372582571180696], [4.151021566867617, 0.372582571180696], [4.151021566867617, -0.372582571180696], [-4.151021566867617, -0.372582571180696]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 656);
        cqClose(c, st, 656, 655, {});
        atStep(st, 657);
        cqExtrude(c, st, 657, 656, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 657, "extrude");
        atStep(st, 658);
        cqRotate(c, st, 658, 657, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 135.86735788920237});
        stepVol(c, st, 658, "rotate");
        atStep(st, 659);
        cqTranslate(c, st, 659, 658, {"vec": [11.782469214188048, 0, 3.8979394833350214]});
        stepVol(c, st, 659, "translate");
        atStep(st, 660);
        cqIntersect(c, st, 660, 659, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 660, "intersect");
        atStep(st, 661);
        cqUnion(c, st, 661, 653, {"toUnion": {"wp": 660}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 661, "union");
        atStep(st, 662);
        cqWorkplaneNew(c, st, 662, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 663);
        cqPolyline(c, st, 663, 662, {"listOfXYTuple": [[-6.796152541641376, 0.3112827394666381], [6.796152541641376, 0.3112827394666381], [6.796152541641376, -0.3112827394666381], [-6.796152541641376, -0.3112827394666381]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 664);
        cqClose(c, st, 664, 663, {});
        atStep(st, 665);
        cqExtrude(c, st, 665, 664, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 665, "extrude");
        atStep(st, 666);
        cqRotate(c, st, 666, 665, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 147.70779981174846});
        stepVol(c, st, 666, "rotate");
        atStep(st, 667);
        cqTranslate(c, st, 667, 666, {"vec": [9.016758051157948, 0, 4.638250663977333]});
        stepVol(c, st, 667, "translate");
        atStep(st, 668);
        cqIntersect(c, st, 668, 667, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 668, "intersect");
        atStep(st, 669);
        cqUnion(c, st, 669, 661, {"toUnion": {"wp": 668}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 669, "union");
        atStep(st, 670);
        cqWorkplaneNew(c, st, 670, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 671);
        cqPolyline(c, st, 671, 670, {"listOfXYTuple": [[-7.398060752108539, 0.554370972220543], [7.398060752108539, 0.554370972220543], [7.398060752108539, -0.554370972220543], [-7.398060752108539, -0.554370972220543]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 672);
        cqClose(c, st, 672, 671, {});
        atStep(st, 673);
        cqExtrude(c, st, 673, 672, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 673, "extrude");
        atStep(st, 674);
        cqRotate(c, st, 674, 673, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -176.09561356365137});
        stepVol(c, st, 674, "rotate");
        atStep(st, 675);
        cqTranslate(c, st, 675, 674, {"vec": [7.380890362191392, 0, 0.5037463182981994]});
        stepVol(c, st, 675, "translate");
        atStep(st, 676);
        cqIntersect(c, st, 676, 675, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 676, "intersect");
        atStep(st, 677);
        cqUnion(c, st, 677, 669, {"toUnion": {"wp": 676}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 677, "union");
        atStep(st, 678);
        cqWorkplaneNew(c, st, 678, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 679);
        cqPolyline(c, st, 679, 678, {"listOfXYTuple": [[-2.8630785671185, 0.524959630105905], [2.8630785671185, 0.524959630105905], [2.8630785671185, -0.524959630105905], [-2.8630785671185, -0.524959630105905]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 680);
        cqClose(c, st, 680, 679, {});
        atStep(st, 681);
        cqExtrude(c, st, 681, 680, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 681, "extrude");
        atStep(st, 682);
        cqRotate(c, st, 682, 681, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -14.98533057409192});
        stepVol(c, st, 682, "rotate");
        atStep(st, 683);
        cqTranslate(c, st, 683, 682, {"vec": [6.03744654096321, 0, 7.528697510715956]});
        stepVol(c, st, 683, "translate");
        atStep(st, 684);
        cqIntersect(c, st, 684, 683, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 684, "intersect");
        atStep(st, 685);
        cqUnion(c, st, 685, 677, {"toUnion": {"wp": 684}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 685, "union");
        atStep(st, 686);
        cqWorkplaneNew(c, st, 686, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 687);
        cqPolyline(c, st, 687, 686, {"listOfXYTuple": [[-3.742008029795956, 0.8744766432490042], [3.742008029795956, 0.8744766432490042], [3.742008029795956, -0.8744766432490042], [-3.742008029795956, -0.8744766432490042]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 688);
        cqClose(c, st, 688, 687, {});
        atStep(st, 689);
        cqExtrude(c, st, 689, 688, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 689, "extrude");
        atStep(st, 690);
        cqRotate(c, st, 690, 689, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 115.92301295572751});
        stepVol(c, st, 690, "rotate");
        atStep(st, 691);
        cqTranslate(c, st, 691, 690, {"vec": [1.635867688966555, 0, 11.634504345679131]});
        stepVol(c, st, 691, "translate");
        atStep(st, 692);
        cqIntersect(c, st, 692, 691, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 692, "intersect");
        atStep(st, 693);
        cqUnion(c, st, 693, 685, {"toUnion": {"wp": 692}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 693, "union");
        atStep(st, 694);
        cqWorkplaneNew(c, st, 694, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 695);
        cqPolyline(c, st, 695, 694, {"listOfXYTuple": [[-4.446368099949038, 0.7418632288810317], [4.446368099949038, 0.7418632288810317], [4.446368099949038, -0.7418632288810317], [-4.446368099949038, -0.7418632288810317]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 696);
        cqClose(c, st, 696, 695, {});
        atStep(st, 697);
        cqExtrude(c, st, 697, 696, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 697, "extrude");
        atStep(st, 698);
        cqRotate(c, st, 698, 697, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -111.58684017928104});
        stepVol(c, st, 698, "rotate");
        atStep(st, 699);
        cqTranslate(c, st, 699, 698, {"vec": [1.635867688966555, 0, 4.134504345679134]});
        stepVol(c, st, 699, "translate");
        atStep(st, 700);
        cqIntersect(c, st, 700, 699, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 700, "intersect");
        atStep(st, 701);
        cqUnion(c, st, 701, 693, {"toUnion": {"wp": 700}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 701, "union");
        atStep(st, 702);
        cqWorkplaneNew(c, st, 702, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 703);
        cqPolyline(c, st, 703, 702, {"listOfXYTuple": [[-5.55827703806916, 0.5906784207902656], [5.55827703806916, 0.5906784207902656], [5.55827703806916, -0.5906784207902656], [-5.55827703806916, -0.5906784207902656]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 704);
        cqClose(c, st, 704, 703, {});
        atStep(st, 705);
        cqExtrude(c, st, 705, 704, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 705, "extrude");
        atStep(st, 706);
        cqRotate(c, st, 706, 705, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -142.36307562417704});
        stepVol(c, st, 706, "rotate");
        atStep(st, 707);
        cqTranslate(c, st, 707, 706, {"vec": [4.401578851996655, 0, 3.394193165036822]});
        stepVol(c, st, 707, "translate");
        atStep(st, 708);
        cqIntersect(c, st, 708, 707, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 708, "intersect");
        atStep(st, 709);
        cqUnion(c, st, 709, 701, {"toUnion": {"wp": 708}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 709, "union");
        atStep(st, 710);
        cqWorkplaneNew(c, st, 710, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 711);
        cqPolyline(c, st, 711, 710, {"listOfXYTuple": [[-6.019264586008372, 0.7255085486405215], [6.019264586008372, 0.7255085486405215], [6.019264586008372, -0.7255085486405215], [-6.019264586008372, -0.7255085486405215]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 712);
        cqClose(c, st, 712, 711, {});
        atStep(st, 713);
        cqExtrude(c, st, 713, 712, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 713, "extrude");
        atStep(st, 714);
        cqRotate(c, st, 714, 713, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 136.99117167732314});
        stepVol(c, st, 714, "rotate");
        atStep(st, 715);
        cqTranslate(c, st, 715, 714, {"vec": [4.401578851996655, 0, 10.89419316503682]});
        stepVol(c, st, 715, "translate");
        atStep(st, 716);
        cqIntersect(c, st, 716, 715, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 716, "intersect");
        atStep(st, 717);
        cqUnion(c, st, 717, 709, {"toUnion": {"wp": 716}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 717, "union");
        atStep(st, 718);
        cqWorkplaneNew(c, st, 718, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 719);
        cqPolyline(c, st, 719, 718, {"listOfXYTuple": [[-11.12866021010738, 0.8078992261669871], [11.12866021010738, 0.8078992261669871], [11.12866021010738, -0.8078992261669871], [-11.12866021010738, -0.8078992261669871]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 720);
        cqClose(c, st, 720, 719, {});
        atStep(st, 721);
        cqExtrude(c, st, 721, 720, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 721, "extrude");
        atStep(st, 722);
        cqRotate(c, st, 722, 721, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": -17.75790333907469});
        stepVol(c, st, 722, "rotate");
        atStep(st, 723);
        cqTranslate(c, st, 723, 722, {"vec": [19.401578851996653, 0, 3.394193165036822]});
        stepVol(c, st, 723, "translate");
        atStep(st, 724);
        cqIntersect(c, st, 724, 723, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 724, "intersect");
        atStep(st, 725);
        cqUnion(c, st, 725, 717, {"toUnion": {"wp": 724}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 725, "union");
        atStep(st, 726);
        cqWorkplaneNew(c, st, 726, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 727);
        cqPolyline(c, st, 727, 726, {"listOfXYTuple": [[-7.5, 0.8529415149315798], [7.5, 0.8529415149315798], [7.5, -0.8529415149315798], [-7.5, -0.8529415149315798]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 728);
        cqClose(c, st, 728, 727, {});
        atStep(st, 729);
        cqExtrude(c, st, 729, 728, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 729, "extrude");
        atStep(st, 730);
        cqRotate(c, st, 730, 729, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 90});
        stepVol(c, st, 730, "rotate");
        atStep(st, 731);
        cqTranslate(c, st, 731, 730, {"vec": [0, 0, 7.5]});
        stepVol(c, st, 731, "translate");
        atStep(st, 732);
        cqIntersect(c, st, 732, 731, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 732, "intersect");
        atStep(st, 733);
        cqUnion(c, st, 733, 725, {"toUnion": {"wp": 732}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 733, "union");
        atStep(st, 734);
        cqWorkplaneNew(c, st, 734, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 735);
        cqPolyline(c, st, 735, 734, {"listOfXYTuple": [[-15, 0.6636144669377871], [15, 0.6636144669377871], [15, -0.6636144669377871], [-15, -0.6636144669377871]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 736);
        cqClose(c, st, 736, 735, {});
        atStep(st, 737);
        cqExtrude(c, st, 737, 736, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 737, "extrude");
        atStep(st, 738);
        cqRotate(c, st, 738, 737, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 0});
        stepVol(c, st, 738, "rotate");
        atStep(st, 739);
        cqTranslate(c, st, 739, 738, {"vec": [15, 0, 0]});
        stepVol(c, st, 739, "translate");
        atStep(st, 740);
        cqIntersect(c, st, 740, 739, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 740, "intersect");
        atStep(st, 741);
        cqUnion(c, st, 741, 733, {"toUnion": {"wp": 740}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 741, "union");
        atStep(st, 742);
        cqWorkplaneNew(c, st, 742, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 743);
        cqPolyline(c, st, 743, 742, {"listOfXYTuple": [[-16.77050983124842, 0.5769909471799542], [16.77050983124842, 0.5769909471799542], [16.77050983124842, -0.5769909471799542], [-16.77050983124842, -0.5769909471799542]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 744);
        cqClose(c, st, 744, 743, {});
        atStep(st, 745);
        cqExtrude(c, st, 745, 744, {"until": 20, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 745, "extrude");
        atStep(st, 746);
        cqRotate(c, st, 746, 745, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 1, 0], "angleDegrees": 153.434948822922});
        stepVol(c, st, 746, "rotate");
        atStep(st, 747);
        cqTranslate(c, st, 747, 746, {"vec": [15, 0, 7.5]});
        stepVol(c, st, 747, "translate");
        atStep(st, 748);
        cqIntersect(c, st, 748, 747, {"toIntersect": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 748, "intersect");
        atStep(st, 749);
        cqUnion(c, st, 749, 741, {"toUnion": {"wp": 748}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 749, "union");
        atStep(st, 750);
        cqUnion(c, st, 750, 6, {"toUnion": {"wp": 749}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 750, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 750}), {"i": 3837});
    } catch (e) {
        return {"i": 3837, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_3837(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c3837"] = r; }
    return out;
}
