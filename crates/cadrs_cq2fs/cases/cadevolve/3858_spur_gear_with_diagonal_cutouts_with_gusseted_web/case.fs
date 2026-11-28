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

const case_3858 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqCircle(c, st, 2, 1, {"radius": 13.5, "forConstruction": false});
        atStep(st, 3);
        cqExtrude(c, st, 3, 2, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 3, "extrude");
        atStep(st, 4);
        cqWorkplaneNew(c, st, 4, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 5);
        cqPolyline(c, st, 5, 4, {"listOfXYTuple": [[13.434993810074658, -1.323231394449068], [17.913325080099543, -1.764308525932091], [17.913325080099543, 1.764308525932091], [13.434993810074658, 1.323231394449068]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 6);
        cqClose(c, st, 6, 5, {});
        atStep(st, 7);
        cqExtrude(c, st, 7, 6, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 7, "extrude");
        atStep(st, 8);
        cqUnion(c, st, 8, 3, {"toUnion": {"wp": 7}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 8, "union");
        atStep(st, 9);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 10);
        cqPolyline(c, st, 10, 9, {"listOfXYTuple": [[12.91869453238482, 3.9188431429352417], [17.22492604317976, 5.225124190580322], [15.87458275827039, 8.485141262867957], [11.905937068702793, 6.363855947150968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 11);
        cqClose(c, st, 11, 10, {});
        atStep(st, 12);
        cqExtrude(c, st, 12, 11, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 12, "extrude");
        atStep(st, 13);
        cqUnion(c, st, 13, 8, {"toUnion": {"wp": 12}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 13, "union");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 14, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 15);
        cqPolyline(c, st, 15, 14, {"listOfXYTuple": [[10.43564112039695, 8.564309336209215], [13.914188160529266, 11.41907911494562], [11.41907911494562, 13.914188160529266], [8.564309336209215, 10.43564112039695]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 16);
        cqClose(c, st, 16, 15, {});
        atStep(st, 17);
        cqExtrude(c, st, 17, 16, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 17, "extrude");
        atStep(st, 18);
        cqUnion(c, st, 18, 13, {"toUnion": {"wp": 17}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 18, "union");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 20);
        cqPolyline(c, st, 20, 19, {"listOfXYTuple": [[6.36385594715097, 11.90593706870279], [8.48514126286796, 15.874582758270387], [5.225124190580322, 17.22492604317976], [3.9188431429352417, 12.91869453238482]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 21);
        cqClose(c, st, 21, 20, {});
        atStep(st, 22);
        cqExtrude(c, st, 22, 21, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 22, "extrude");
        atStep(st, 23);
        cqUnion(c, st, 23, 18, {"toUnion": {"wp": 22}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 23, "union");
        atStep(st, 24);
        cqWorkplaneNew(c, st, 24, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 25);
        cqPolyline(c, st, 25, 24, {"listOfXYTuple": [[1.3232313944490703, 13.434993810074657], [1.764308525932094, 17.913325080099543], [-1.7643085259320916, 17.913325080099543], [-1.3232313944490688, 13.434993810074658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 26);
        cqClose(c, st, 26, 25, {});
        atStep(st, 27);
        cqExtrude(c, st, 27, 26, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 27, "extrude");
        atStep(st, 28);
        cqUnion(c, st, 28, 23, {"toUnion": {"wp": 27}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 28, "union");
        atStep(st, 29);
        cqWorkplaneNew(c, st, 29, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 30);
        cqPolyline(c, st, 30, 29, {"listOfXYTuple": [[-3.918843142935239, 12.91869453238482], [-5.225124190580319, 17.22492604317976], [-8.485141262867959, 15.87458275827039], [-6.363855947150969, 11.905937068702793]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 31);
        cqClose(c, st, 31, 30, {});
        atStep(st, 32);
        cqExtrude(c, st, 32, 31, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 32, "extrude");
        atStep(st, 33);
        cqUnion(c, st, 33, 28, {"toUnion": {"wp": 32}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 33, "union");
        atStep(st, 34);
        cqWorkplaneNew(c, st, 34, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 35);
        cqPolyline(c, st, 35, 34, {"listOfXYTuple": [[-8.564309336209213, 10.435641120396951], [-11.419079114945616, 13.914188160529267], [-13.914188160529266, 11.41907911494562], [-10.43564112039695, 8.564309336209215]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 36);
        cqClose(c, st, 36, 35, {});
        atStep(st, 37);
        cqExtrude(c, st, 37, 36, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 37, "extrude");
        atStep(st, 38);
        cqUnion(c, st, 38, 33, {"toUnion": {"wp": 37}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 38, "union");
        atStep(st, 39);
        cqWorkplaneNew(c, st, 39, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 40);
        cqPolyline(c, st, 40, 39, {"listOfXYTuple": [[-11.90593706870279, 6.363855947150971], [-15.874582758270387, 8.48514126286796], [-17.22492604317976, 5.225124190580323], [-12.91869453238482, 3.918843142935242]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 41);
        cqClose(c, st, 41, 40, {});
        atStep(st, 42);
        cqExtrude(c, st, 42, 41, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 42, "extrude");
        atStep(st, 43);
        cqUnion(c, st, 43, 38, {"toUnion": {"wp": 42}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 43, "union");
        atStep(st, 44);
        cqWorkplaneNew(c, st, 44, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 45);
        cqPolyline(c, st, 45, 44, {"listOfXYTuple": [[-13.434993810074657, 1.3232313944490712], [-17.913325080099543, 1.764308525932095], [-17.913325080099543, -1.7643085259320903], [-13.434993810074658, -1.323231394449068]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 46);
        cqClose(c, st, 46, 45, {});
        atStep(st, 47);
        cqExtrude(c, st, 47, 46, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 47, "extrude");
        atStep(st, 48);
        cqUnion(c, st, 48, 43, {"toUnion": {"wp": 47}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 48, "union");
        atStep(st, 49);
        cqWorkplaneNew(c, st, 49, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 50);
        cqPolyline(c, st, 50, 49, {"listOfXYTuple": [[-12.91869453238482, -3.9188431429352386], [-17.22492604317976, -5.225124190580318], [-15.87458275827039, -8.485141262867957], [-11.905937068702793, -6.363855947150968]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 51);
        cqClose(c, st, 51, 50, {});
        atStep(st, 52);
        cqExtrude(c, st, 52, 51, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 52, "extrude");
        atStep(st, 53);
        cqUnion(c, st, 53, 48, {"toUnion": {"wp": 52}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 53, "union");
        atStep(st, 54);
        cqWorkplaneNew(c, st, 54, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 55);
        cqPolyline(c, st, 55, 54, {"listOfXYTuple": [[-10.435641120396951, -8.564309336209211], [-13.914188160529267, -11.419079114945614], [-11.419079114945626, -13.91418816052926], [-8.56430933620922, -10.435641120396944]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 56);
        cqClose(c, st, 56, 55, {});
        atStep(st, 57);
        cqExtrude(c, st, 57, 56, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 57, "extrude");
        atStep(st, 58);
        cqUnion(c, st, 58, 53, {"toUnion": {"wp": 57}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 58, "union");
        atStep(st, 59);
        cqWorkplaneNew(c, st, 59, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 60);
        cqPolyline(c, st, 60, 59, {"listOfXYTuple": [[-6.363855947150971, -11.90593706870279], [-8.48514126286796, -15.874582758270387], [-5.225124190580339, -17.224926043179757], [-3.918843142935254, -12.918694532384816]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 61);
        cqClose(c, st, 61, 60, {});
        atStep(st, 62);
        cqExtrude(c, st, 62, 61, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 62, "extrude");
        atStep(st, 63);
        cqUnion(c, st, 63, 58, {"toUnion": {"wp": 62}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 63, "union");
        atStep(st, 64);
        cqWorkplaneNew(c, st, 64, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 65);
        cqPolyline(c, st, 65, 64, {"listOfXYTuple": [[-1.323231394449066, -13.434993810074658], [-1.764308525932088, -17.913325080099543], [1.7643085259320817, -17.913325080099543], [1.3232313944490612, -13.434993810074658]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 66);
        cqClose(c, st, 66, 65, {});
        atStep(st, 67);
        cqExtrude(c, st, 67, 66, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 67, "extrude");
        atStep(st, 68);
        cqUnion(c, st, 68, 63, {"toUnion": {"wp": 67}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 68, "union");
        atStep(st, 69);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 70);
        cqPolyline(c, st, 70, 69, {"listOfXYTuple": [[3.9188431429352497, -12.918694532384816], [5.225124190580333, -17.224926043179757], [8.485141262867957, -15.87458275827039], [6.3638559471509675, -11.905937068702793]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 71);
        cqClose(c, st, 71, 70, {});
        atStep(st, 72);
        cqExtrude(c, st, 72, 71, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 72, "extrude");
        atStep(st, 73);
        cqUnion(c, st, 73, 68, {"toUnion": {"wp": 72}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 73, "union");
        atStep(st, 74);
        cqWorkplaneNew(c, st, 74, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 75);
        cqPolyline(c, st, 75, 74, {"listOfXYTuple": [[8.564309336209215, -10.435641120396948], [11.41907911494562, -13.914188160529264], [13.91418816052926, -11.419079114945626], [10.435641120396944, -8.56430933620922]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 76);
        cqClose(c, st, 76, 75, {});
        atStep(st, 77);
        cqExtrude(c, st, 77, 76, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 77, "extrude");
        atStep(st, 78);
        cqUnion(c, st, 78, 73, {"toUnion": {"wp": 77}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 78, "union");
        atStep(st, 79);
        cqWorkplaneNew(c, st, 79, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 80);
        cqPolyline(c, st, 80, 79, {"listOfXYTuple": [[11.90593706870279, -6.363855947150972], [15.874582758270387, -8.485141262867963], [17.224926043179757, -5.22512419058034], [12.918694532384816, -3.918843142935255]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 81);
        cqClose(c, st, 81, 80, {});
        atStep(st, 82);
        cqExtrude(c, st, 82, 81, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 82, "extrude");
        atStep(st, 83);
        cqUnion(c, st, 83, 78, {"toUnion": {"wp": 82}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 83, "union");
        atStep(st, 84);
        cqWorkplaneNew(c, st, 84, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 85);
        cqTransformed(c, st, 85, 84, {"rotate": [0, 0, 41.25], "offset": [9.679937521291832, 8.489077369413387, 0]});
        atStep(st, 86);
        cqBox(c, st, 86, 85, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 86, "box");
        // step 87: guard_begin {"before":{"wp":86},"block":1,"g":1,"op":"guard_begin","skippable":true}
        atStep(st, 88);
        cqSelect("Edges")(c, st, 87, 86, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 89);
        cqFillet(c, st, 88, 87, {"radius": 0.225});
        stepVol(c, st, 88, "fillet");
        // step 90: guard_end {"after":{"wp":88},"block":1,"g":1,"op":"guard_end"}
        atStep(st, 91);
        cqCut(c, st, 89, 83, {"toCut": {"wp": 88}, "clean": true, "tol": undefined});
        stepVol(c, st, 89, "cut");
        atStep(st, 92);
        cqWorkplaneNew(c, st, 90, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 93);
        cqTransformed(c, st, 91, 90, {"rotate": [0, 0, 63.75], "offset": [5.694466886569641, 11.547236547233362, 0]});
        atStep(st, 94);
        cqBox(c, st, 92, 91, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 92, "box");
        // step 95: guard_begin {"before":{"wp":92},"block":1,"g":2,"op":"guard_begin","skippable":true}
        atStep(st, 96);
        cqSelect("Edges")(c, st, 93, 92, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 97);
        cqFillet(c, st, 94, 93, {"radius": 0.225});
        stepVol(c, st, 94, "fillet");
        // step 98: guard_end {"after":{"wp":94},"block":1,"g":2,"op":"guard_end"}
        atStep(st, 99);
        cqCut(c, st, 95, 89, {"toCut": {"wp": 94}, "clean": true, "tol": undefined});
        stepVol(c, st, 95, "cut");
        atStep(st, 100);
        cqWorkplaneNew(c, st, 96, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 101);
        cqTransformed(c, st, 97, 96, {"rotate": [0, 0, 86.25], "offset": [0.8420652888380917, 12.84743363669702, 0]});
        atStep(st, 102);
        cqBox(c, st, 98, 97, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 98, "box");
        // step 103: guard_begin {"before":{"wp":98},"block":1,"g":3,"op":"guard_begin","skippable":true}
        atStep(st, 104);
        cqSelect("Edges")(c, st, 99, 98, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 105);
        cqFillet(c, st, 100, 99, {"radius": 0.225});
        stepVol(c, st, 100, "fillet");
        // step 106: guard_end {"after":{"wp":100},"block":1,"g":3,"op":"guard_end"}
        atStep(st, 107);
        cqCut(c, st, 101, 95, {"toCut": {"wp": 100}, "clean": true, "tol": undefined});
        stepVol(c, st, 101, "cut");
        atStep(st, 108);
        cqWorkplaneNew(c, st, 102, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 109);
        cqTransformed(c, st, 103, 102, {"rotate": [0, 0, 108.75], "offset": [-4.1385331157782055, 12.191725417249486, 0]});
        atStep(st, 110);
        cqBox(c, st, 104, 103, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 104, "box");
        // step 111: guard_begin {"before":{"wp":104},"block":1,"g":4,"op":"guard_begin","skippable":true}
        atStep(st, 112);
        cqSelect("Edges")(c, st, 105, 104, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 113);
        cqFillet(c, st, 106, 105, {"radius": 0.225});
        stepVol(c, st, 106, "fillet");
        // step 114: guard_end {"after":{"wp":106},"block":1,"g":4,"op":"guard_end"}
        atStep(st, 115);
        cqCut(c, st, 107, 101, {"toCut": {"wp": 106}, "clean": true, "tol": undefined});
        stepVol(c, st, 107, "cut");
        atStep(st, 116);
        cqWorkplaneNew(c, st, 108, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 117);
        cqTransformed(c, st, 109, 108, {"rotate": [0, 0, 131.25], "offset": [-8.489077369413387, 9.679937521291832, 0]});
        atStep(st, 118);
        cqBox(c, st, 110, 109, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 110, "box");
        // step 119: guard_begin {"before":{"wp":110},"block":1,"g":5,"op":"guard_begin","skippable":true}
        atStep(st, 120);
        cqSelect("Edges")(c, st, 111, 110, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 121);
        cqFillet(c, st, 112, 111, {"radius": 0.225});
        stepVol(c, st, 112, "fillet");
        // step 122: guard_end {"after":{"wp":112},"block":1,"g":5,"op":"guard_end"}
        atStep(st, 123);
        cqCut(c, st, 113, 107, {"toCut": {"wp": 112}, "clean": true, "tol": undefined});
        stepVol(c, st, 113, "cut");
        atStep(st, 124);
        cqWorkplaneNew(c, st, 114, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 125);
        cqTransformed(c, st, 115, 114, {"rotate": [0, 0, 153.75], "offset": [-11.54723654723336, 5.694466886569642, 0]});
        atStep(st, 126);
        cqBox(c, st, 116, 115, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 116, "box");
        // step 127: guard_begin {"before":{"wp":116},"block":1,"g":6,"op":"guard_begin","skippable":true}
        atStep(st, 128);
        cqSelect("Edges")(c, st, 117, 116, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 129);
        cqFillet(c, st, 118, 117, {"radius": 0.225});
        stepVol(c, st, 118, "fillet");
        // step 130: guard_end {"after":{"wp":118},"block":1,"g":6,"op":"guard_end"}
        atStep(st, 131);
        cqCut(c, st, 119, 113, {"toCut": {"wp": 118}, "clean": true, "tol": undefined});
        stepVol(c, st, 119, "cut");
        atStep(st, 132);
        cqWorkplaneNew(c, st, 120, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 133);
        cqTransformed(c, st, 121, 120, {"rotate": [0, 0, 176.25], "offset": [-12.84743363669702, 0.8420652888380926, 0]});
        atStep(st, 134);
        cqBox(c, st, 122, 121, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 122, "box");
        // step 135: guard_begin {"before":{"wp":122},"block":1,"g":7,"op":"guard_begin","skippable":true}
        atStep(st, 136);
        cqSelect("Edges")(c, st, 123, 122, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 137);
        cqFillet(c, st, 124, 123, {"radius": 0.225});
        stepVol(c, st, 124, "fillet");
        // step 138: guard_end {"after":{"wp":124},"block":1,"g":7,"op":"guard_end"}
        atStep(st, 139);
        cqCut(c, st, 125, 119, {"toCut": {"wp": 124}, "clean": true, "tol": undefined});
        stepVol(c, st, 125, "cut");
        atStep(st, 140);
        cqWorkplaneNew(c, st, 126, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 141);
        cqTransformed(c, st, 127, 126, {"rotate": [0, 0, 198.75], "offset": [-12.191725417249486, -4.138533115778205, 0]});
        atStep(st, 142);
        cqBox(c, st, 128, 127, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 128, "box");
        // step 143: guard_begin {"before":{"wp":128},"block":1,"g":8,"op":"guard_begin","skippable":true}
        atStep(st, 144);
        cqSelect("Edges")(c, st, 129, 128, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 145);
        cqFillet(c, st, 130, 129, {"radius": 0.225});
        stepVol(c, st, 130, "fillet");
        // step 146: guard_end {"after":{"wp":130},"block":1,"g":8,"op":"guard_end"}
        atStep(st, 147);
        cqCut(c, st, 131, 125, {"toCut": {"wp": 130}, "clean": true, "tol": undefined});
        stepVol(c, st, 131, "cut");
        atStep(st, 148);
        cqWorkplaneNew(c, st, 132, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 149);
        cqTransformed(c, st, 133, 132, {"rotate": [0, 0, 221.25], "offset": [-9.679937521291835, -8.489077369413387, 0]});
        atStep(st, 150);
        cqBox(c, st, 134, 133, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 134, "box");
        // step 151: guard_begin {"before":{"wp":134},"block":1,"g":9,"op":"guard_begin","skippable":true}
        atStep(st, 152);
        cqSelect("Edges")(c, st, 135, 134, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 153);
        cqFillet(c, st, 136, 135, {"radius": 0.225});
        stepVol(c, st, 136, "fillet");
        // step 154: guard_end {"after":{"wp":136},"block":1,"g":9,"op":"guard_end"}
        atStep(st, 155);
        cqCut(c, st, 137, 131, {"toCut": {"wp": 136}, "clean": true, "tol": undefined});
        stepVol(c, st, 137, "cut");
        atStep(st, 156);
        cqWorkplaneNew(c, st, 138, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 157);
        cqTransformed(c, st, 139, 138, {"rotate": [0, 0, 243.75], "offset": [-5.694466886569643, -11.54723654723336, 0]});
        atStep(st, 158);
        cqBox(c, st, 140, 139, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 140, "box");
        // step 159: guard_begin {"before":{"wp":140},"block":1,"g":10,"op":"guard_begin","skippable":true}
        atStep(st, 160);
        cqSelect("Edges")(c, st, 141, 140, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 161);
        cqFillet(c, st, 142, 141, {"radius": 0.225});
        stepVol(c, st, 142, "fillet");
        // step 162: guard_end {"after":{"wp":142},"block":1,"g":10,"op":"guard_end"}
        atStep(st, 163);
        cqCut(c, st, 143, 137, {"toCut": {"wp": 142}, "clean": true, "tol": undefined});
        stepVol(c, st, 143, "cut");
        atStep(st, 164);
        cqWorkplaneNew(c, st, 144, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 165);
        cqTransformed(c, st, 145, 144, {"rotate": [0, 0, 266.25], "offset": [-0.8420652888380876, -12.84743363669702, 0]});
        atStep(st, 166);
        cqBox(c, st, 146, 145, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 146, "box");
        // step 167: guard_begin {"before":{"wp":146},"block":1,"g":11,"op":"guard_begin","skippable":true}
        atStep(st, 168);
        cqSelect("Edges")(c, st, 147, 146, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 169);
        cqFillet(c, st, 148, 147, {"radius": 0.225});
        stepVol(c, st, 148, "fillet");
        // step 170: guard_end {"after":{"wp":148},"block":1,"g":11,"op":"guard_end"}
        atStep(st, 171);
        cqCut(c, st, 149, 143, {"toCut": {"wp": 148}, "clean": true, "tol": undefined});
        stepVol(c, st, 149, "cut");
        atStep(st, 172);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 173);
        cqTransformed(c, st, 151, 150, {"rotate": [0, 0, 288.75], "offset": [4.138533115778204, -12.191725417249486, 0]});
        atStep(st, 174);
        cqBox(c, st, 152, 151, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 152, "box");
        // step 175: guard_begin {"before":{"wp":152},"block":1,"g":12,"op":"guard_begin","skippable":true}
        atStep(st, 176);
        cqSelect("Edges")(c, st, 153, 152, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 177);
        cqFillet(c, st, 154, 153, {"radius": 0.225});
        stepVol(c, st, 154, "fillet");
        // step 178: guard_end {"after":{"wp":154},"block":1,"g":12,"op":"guard_end"}
        atStep(st, 179);
        cqCut(c, st, 155, 149, {"toCut": {"wp": 154}, "clean": true, "tol": undefined});
        stepVol(c, st, 155, "cut");
        atStep(st, 180);
        cqWorkplaneNew(c, st, 156, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 181);
        cqTransformed(c, st, 157, 156, {"rotate": [0, 0, 311.25], "offset": [8.489077369413389, -9.679937521291832, 0]});
        atStep(st, 182);
        cqBox(c, st, 158, 157, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 158, "box");
        // step 183: guard_begin {"before":{"wp":158},"block":1,"g":13,"op":"guard_begin","skippable":true}
        atStep(st, 184);
        cqSelect("Edges")(c, st, 159, 158, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 185);
        cqFillet(c, st, 160, 159, {"radius": 0.225});
        stepVol(c, st, 160, "fillet");
        // step 186: guard_end {"after":{"wp":160},"block":1,"g":13,"op":"guard_end"}
        atStep(st, 187);
        cqCut(c, st, 161, 155, {"toCut": {"wp": 160}, "clean": true, "tol": undefined});
        stepVol(c, st, 161, "cut");
        atStep(st, 188);
        cqWorkplaneNew(c, st, 162, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 189);
        cqTransformed(c, st, 163, 162, {"rotate": [0, 0, 333.75], "offset": [11.54723654723336, -5.694466886569643, 0]});
        atStep(st, 190);
        cqBox(c, st, 164, 163, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 164, "box");
        // step 191: guard_begin {"before":{"wp":164},"block":1,"g":14,"op":"guard_begin","skippable":true}
        atStep(st, 192);
        cqSelect("Edges")(c, st, 165, 164, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 193);
        cqFillet(c, st, 166, 165, {"radius": 0.225});
        stepVol(c, st, 166, "fillet");
        // step 194: guard_end {"after":{"wp":166},"block":1,"g":14,"op":"guard_end"}
        atStep(st, 195);
        cqCut(c, st, 167, 161, {"toCut": {"wp": 166}, "clean": true, "tol": undefined});
        stepVol(c, st, 167, "cut");
        atStep(st, 196);
        cqWorkplaneNew(c, st, 168, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 197);
        cqTransformed(c, st, 169, 168, {"rotate": [0, 0, 356.25], "offset": [12.84743363669702, -0.8420652888380885, 0]});
        atStep(st, 198);
        cqBox(c, st, 170, 169, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 170, "box");
        // step 199: guard_begin {"before":{"wp":170},"block":1,"g":15,"op":"guard_begin","skippable":true}
        atStep(st, 200);
        cqSelect("Edges")(c, st, 171, 170, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 201);
        cqFillet(c, st, 172, 171, {"radius": 0.225});
        stepVol(c, st, 172, "fillet");
        // step 202: guard_end {"after":{"wp":172},"block":1,"g":15,"op":"guard_end"}
        atStep(st, 203);
        cqCut(c, st, 173, 167, {"toCut": {"wp": 172}, "clean": true, "tol": undefined});
        stepVol(c, st, 173, "cut");
        atStep(st, 204);
        cqWorkplaneNew(c, st, 174, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 205);
        cqTransformed(c, st, 175, 174, {"rotate": [0, 0, 378.75], "offset": [12.191725417249486, 4.138533115778204, 0]});
        atStep(st, 206);
        cqBox(c, st, 176, 175, {"length": 1.25, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 176, "box");
        // step 207: guard_begin {"before":{"wp":176},"block":1,"g":16,"op":"guard_begin","skippable":true}
        atStep(st, 208);
        cqSelect("Edges")(c, st, 177, 176, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 209);
        cqFillet(c, st, 178, 177, {"radius": 0.225});
        stepVol(c, st, 178, "fillet");
        // step 210: guard_end {"after":{"wp":178},"block":1,"g":16,"op":"guard_end"}
        atStep(st, 211);
        cqCut(c, st, 179, 173, {"toCut": {"wp": 178}, "clean": true, "tol": undefined});
        stepVol(c, st, 179, "cut");
        atStep(st, 212);
        cqWorkplaneNew(c, st, 180, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 213);
        cqTransformed(c, st, 181, 180, {"rotate": [0, 0, 41.25], "offset": [9.679937521291832, 8.489077369413387, 0]});
        atStep(st, 214);
        cqBox(c, st, 182, 181, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 182, "box");
        atStep(st, 215);
        cqUnion(c, st, 183, 179, {"toUnion": {"wp": 182}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 183, "union");
        atStep(st, 216);
        cqWorkplaneNew(c, st, 184, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 217);
        cqTransformed(c, st, 185, 184, {"rotate": [0, 0, 41.25], "offset": [9.679937521291832, 8.489077369413387, 0]});
        atStep(st, 218);
        cqPolyline(c, st, 186, 185, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 219);
        cqClose(c, st, 187, 186, {});
        atStep(st, 220);
        cqExtrude(c, st, 188, 187, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 188, "extrude");
        atStep(st, 221);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 222);
        cqTransformed(c, st, 190, 189, {"rotate": [0, 0, 41.25], "offset": [9.679937521291832, 8.489077369413387, 0]});
        atStep(st, 223);
        cqPolyline(c, st, 191, 190, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 224);
        cqClose(c, st, 192, 191, {});
        atStep(st, 225);
        cqExtrude(c, st, 193, 192, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 193, "extrude");
        // step 226: guard_begin {"before":{"wp":188},"block":2,"g":17,"op":"guard_begin","skippable":true}
        atStep(st, 227);
        cqSelect("Edges")(c, st, 194, 188, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        // step 228: injected_failure {"at":228,"op":"injected_failure","was":"fillet"}
        // step 229: guard_caught {"block":2,"g":17,"op":"guard_caught"}
        // step 230: guard_begin {"before":{"wp":193},"block":3,"g":18,"op":"guard_begin","skippable":true}
        atStep(st, 231);
        cqSelect("Edges")(c, st, 196, 193, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        // step 232: injected_failure {"at":232,"op":"injected_failure","was":"fillet"}
        // step 233: guard_caught {"block":3,"g":18,"op":"guard_caught"}
        atStep(st, 234);
        cqUnion(c, st, 198, 183, {"toUnion": {"wp": 188}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 198, "union");
        atStep(st, 235);
        cqUnion(c, st, 199, 198, {"toUnion": {"wp": 193}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 199, "union");
        atStep(st, 236);
        cqWorkplaneNew(c, st, 200, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 237);
        cqTransformed(c, st, 201, 200, {"rotate": [0, 0, 63.75], "offset": [5.694466886569641, 11.547236547233362, 0]});
        atStep(st, 238);
        cqBox(c, st, 202, 201, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 202, "box");
        atStep(st, 239);
        cqUnion(c, st, 203, 199, {"toUnion": {"wp": 202}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 203, "union");
        atStep(st, 240);
        cqWorkplaneNew(c, st, 204, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 241);
        cqTransformed(c, st, 205, 204, {"rotate": [0, 0, 63.75], "offset": [5.694466886569641, 11.547236547233362, 0]});
        atStep(st, 242);
        cqPolyline(c, st, 206, 205, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 243);
        cqClose(c, st, 207, 206, {});
        atStep(st, 244);
        cqExtrude(c, st, 208, 207, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 208, "extrude");
        atStep(st, 245);
        cqWorkplaneNew(c, st, 209, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 246);
        cqTransformed(c, st, 210, 209, {"rotate": [0, 0, 63.75], "offset": [5.694466886569641, 11.547236547233362, 0]});
        atStep(st, 247);
        cqPolyline(c, st, 211, 210, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 248);
        cqClose(c, st, 212, 211, {});
        atStep(st, 249);
        cqExtrude(c, st, 213, 212, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 213, "extrude");
        // step 250: guard_begin {"before":{"wp":208},"block":2,"g":19,"op":"guard_begin","skippable":true}
        atStep(st, 251);
        cqSelect("Edges")(c, st, 214, 208, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 252);
        cqFillet(c, st, 215, 214, {"radius": 0.2});
        stepVol(c, st, 215, "fillet");
        // step 253: guard_end {"after":{"wp":215},"block":2,"g":19,"op":"guard_end"}
        // step 254: guard_begin {"before":{"wp":213},"block":3,"g":20,"op":"guard_begin","skippable":true}
        atStep(st, 255);
        cqSelect("Edges")(c, st, 216, 213, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 256);
        cqFillet(c, st, 217, 216, {"radius": 0.2});
        stepVol(c, st, 217, "fillet");
        // step 257: guard_end {"after":{"wp":217},"block":3,"g":20,"op":"guard_end"}
        atStep(st, 258);
        cqUnion(c, st, 218, 203, {"toUnion": {"wp": 215}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 218, "union");
        atStep(st, 259);
        cqUnion(c, st, 219, 218, {"toUnion": {"wp": 217}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 219, "union");
        atStep(st, 260);
        cqWorkplaneNew(c, st, 220, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 261);
        cqTransformed(c, st, 221, 220, {"rotate": [0, 0, 86.25], "offset": [0.8420652888380917, 12.84743363669702, 0]});
        atStep(st, 262);
        cqBox(c, st, 222, 221, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 222, "box");
        atStep(st, 263);
        cqUnion(c, st, 223, 219, {"toUnion": {"wp": 222}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 223, "union");
        atStep(st, 264);
        cqWorkplaneNew(c, st, 224, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 265);
        cqTransformed(c, st, 225, 224, {"rotate": [0, 0, 86.25], "offset": [0.8420652888380917, 12.84743363669702, 0]});
        atStep(st, 266);
        cqPolyline(c, st, 226, 225, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 267);
        cqClose(c, st, 227, 226, {});
        atStep(st, 268);
        cqExtrude(c, st, 228, 227, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 228, "extrude");
        atStep(st, 269);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 270);
        cqTransformed(c, st, 230, 229, {"rotate": [0, 0, 86.25], "offset": [0.8420652888380917, 12.84743363669702, 0]});
        atStep(st, 271);
        cqPolyline(c, st, 231, 230, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 272);
        cqClose(c, st, 232, 231, {});
        atStep(st, 273);
        cqExtrude(c, st, 233, 232, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 233, "extrude");
        // step 274: guard_begin {"before":{"wp":228},"block":2,"g":21,"op":"guard_begin","skippable":true}
        atStep(st, 275);
        cqSelect("Edges")(c, st, 234, 228, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 276);
        cqFillet(c, st, 235, 234, {"radius": 0.2});
        stepVol(c, st, 235, "fillet");
        // step 277: guard_end {"after":{"wp":235},"block":2,"g":21,"op":"guard_end"}
        // step 278: guard_begin {"before":{"wp":233},"block":3,"g":22,"op":"guard_begin","skippable":true}
        atStep(st, 279);
        cqSelect("Edges")(c, st, 236, 233, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 280);
        cqFillet(c, st, 237, 236, {"radius": 0.2});
        stepVol(c, st, 237, "fillet");
        // step 281: guard_end {"after":{"wp":237},"block":3,"g":22,"op":"guard_end"}
        atStep(st, 282);
        cqUnion(c, st, 238, 223, {"toUnion": {"wp": 235}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 238, "union");
        atStep(st, 283);
        cqUnion(c, st, 239, 238, {"toUnion": {"wp": 237}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 239, "union");
        atStep(st, 284);
        cqWorkplaneNew(c, st, 240, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 285);
        cqTransformed(c, st, 241, 240, {"rotate": [0, 0, 108.75], "offset": [-4.1385331157782055, 12.191725417249486, 0]});
        atStep(st, 286);
        cqBox(c, st, 242, 241, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 242, "box");
        atStep(st, 287);
        cqUnion(c, st, 243, 239, {"toUnion": {"wp": 242}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 243, "union");
        atStep(st, 288);
        cqWorkplaneNew(c, st, 244, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 289);
        cqTransformed(c, st, 245, 244, {"rotate": [0, 0, 108.75], "offset": [-4.1385331157782055, 12.191725417249486, 0]});
        atStep(st, 290);
        cqPolyline(c, st, 246, 245, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 291);
        cqClose(c, st, 247, 246, {});
        atStep(st, 292);
        cqExtrude(c, st, 248, 247, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 248, "extrude");
        atStep(st, 293);
        cqWorkplaneNew(c, st, 249, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 294);
        cqTransformed(c, st, 250, 249, {"rotate": [0, 0, 108.75], "offset": [-4.1385331157782055, 12.191725417249486, 0]});
        atStep(st, 295);
        cqPolyline(c, st, 251, 250, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 296);
        cqClose(c, st, 252, 251, {});
        atStep(st, 297);
        cqExtrude(c, st, 253, 252, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 253, "extrude");
        // step 298: guard_begin {"before":{"wp":248},"block":2,"g":23,"op":"guard_begin","skippable":true}
        atStep(st, 299);
        cqSelect("Edges")(c, st, 254, 248, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 300);
        cqFillet(c, st, 255, 254, {"radius": 0.2});
        stepVol(c, st, 255, "fillet");
        // step 301: guard_end {"after":{"wp":255},"block":2,"g":23,"op":"guard_end"}
        // step 302: guard_begin {"before":{"wp":253},"block":3,"g":24,"op":"guard_begin","skippable":true}
        atStep(st, 303);
        cqSelect("Edges")(c, st, 256, 253, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 304);
        cqFillet(c, st, 257, 256, {"radius": 0.2});
        stepVol(c, st, 257, "fillet");
        // step 305: guard_end {"after":{"wp":257},"block":3,"g":24,"op":"guard_end"}
        atStep(st, 306);
        cqUnion(c, st, 258, 243, {"toUnion": {"wp": 255}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 258, "union");
        atStep(st, 307);
        cqUnion(c, st, 259, 258, {"toUnion": {"wp": 257}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 259, "union");
        atStep(st, 308);
        cqWorkplaneNew(c, st, 260, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 309);
        cqTransformed(c, st, 261, 260, {"rotate": [0, 0, 131.25], "offset": [-8.489077369413387, 9.679937521291832, 0]});
        atStep(st, 310);
        cqBox(c, st, 262, 261, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 262, "box");
        atStep(st, 311);
        cqUnion(c, st, 263, 259, {"toUnion": {"wp": 262}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 263, "union");
        atStep(st, 312);
        cqWorkplaneNew(c, st, 264, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 313);
        cqTransformed(c, st, 265, 264, {"rotate": [0, 0, 131.25], "offset": [-8.489077369413387, 9.679937521291832, 0]});
        atStep(st, 314);
        cqPolyline(c, st, 266, 265, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 315);
        cqClose(c, st, 267, 266, {});
        atStep(st, 316);
        cqExtrude(c, st, 268, 267, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 268, "extrude");
        atStep(st, 317);
        cqWorkplaneNew(c, st, 269, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 318);
        cqTransformed(c, st, 270, 269, {"rotate": [0, 0, 131.25], "offset": [-8.489077369413387, 9.679937521291832, 0]});
        atStep(st, 319);
        cqPolyline(c, st, 271, 270, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 320);
        cqClose(c, st, 272, 271, {});
        atStep(st, 321);
        cqExtrude(c, st, 273, 272, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 273, "extrude");
        // step 322: guard_begin {"before":{"wp":268},"block":2,"g":25,"op":"guard_begin","skippable":true}
        atStep(st, 323);
        cqSelect("Edges")(c, st, 274, 268, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 324);
        cqFillet(c, st, 275, 274, {"radius": 0.2});
        stepVol(c, st, 275, "fillet");
        // step 325: guard_end {"after":{"wp":275},"block":2,"g":25,"op":"guard_end"}
        // step 326: guard_begin {"before":{"wp":273},"block":3,"g":26,"op":"guard_begin","skippable":true}
        atStep(st, 327);
        cqSelect("Edges")(c, st, 276, 273, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 328);
        cqFillet(c, st, 277, 276, {"radius": 0.2});
        stepVol(c, st, 277, "fillet");
        // step 329: guard_end {"after":{"wp":277},"block":3,"g":26,"op":"guard_end"}
        atStep(st, 330);
        cqUnion(c, st, 278, 263, {"toUnion": {"wp": 275}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 278, "union");
        atStep(st, 331);
        cqUnion(c, st, 279, 278, {"toUnion": {"wp": 277}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 279, "union");
        atStep(st, 332);
        cqWorkplaneNew(c, st, 280, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 333);
        cqTransformed(c, st, 281, 280, {"rotate": [0, 0, 153.75], "offset": [-11.54723654723336, 5.694466886569642, 0]});
        atStep(st, 334);
        cqBox(c, st, 282, 281, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 282, "box");
        atStep(st, 335);
        cqUnion(c, st, 283, 279, {"toUnion": {"wp": 282}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 283, "union");
        atStep(st, 336);
        cqWorkplaneNew(c, st, 284, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 337);
        cqTransformed(c, st, 285, 284, {"rotate": [0, 0, 153.75], "offset": [-11.54723654723336, 5.694466886569642, 0]});
        atStep(st, 338);
        cqPolyline(c, st, 286, 285, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 339);
        cqClose(c, st, 287, 286, {});
        atStep(st, 340);
        cqExtrude(c, st, 288, 287, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 288, "extrude");
        atStep(st, 341);
        cqWorkplaneNew(c, st, 289, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 342);
        cqTransformed(c, st, 290, 289, {"rotate": [0, 0, 153.75], "offset": [-11.54723654723336, 5.694466886569642, 0]});
        atStep(st, 343);
        cqPolyline(c, st, 291, 290, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 344);
        cqClose(c, st, 292, 291, {});
        atStep(st, 345);
        cqExtrude(c, st, 293, 292, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 293, "extrude");
        // step 346: guard_begin {"before":{"wp":288},"block":2,"g":27,"op":"guard_begin","skippable":true}
        atStep(st, 347);
        cqSelect("Edges")(c, st, 294, 288, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 348);
        cqFillet(c, st, 295, 294, {"radius": 0.2});
        stepVol(c, st, 295, "fillet");
        // step 349: guard_end {"after":{"wp":295},"block":2,"g":27,"op":"guard_end"}
        // step 350: guard_begin {"before":{"wp":293},"block":3,"g":28,"op":"guard_begin","skippable":true}
        atStep(st, 351);
        cqSelect("Edges")(c, st, 296, 293, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 352);
        cqFillet(c, st, 297, 296, {"radius": 0.2});
        stepVol(c, st, 297, "fillet");
        // step 353: guard_end {"after":{"wp":297},"block":3,"g":28,"op":"guard_end"}
        atStep(st, 354);
        cqUnion(c, st, 298, 283, {"toUnion": {"wp": 295}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 298, "union");
        atStep(st, 355);
        cqUnion(c, st, 299, 298, {"toUnion": {"wp": 297}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 299, "union");
        atStep(st, 356);
        cqWorkplaneNew(c, st, 300, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 357);
        cqTransformed(c, st, 301, 300, {"rotate": [0, 0, 176.25], "offset": [-12.84743363669702, 0.8420652888380926, 0]});
        atStep(st, 358);
        cqBox(c, st, 302, 301, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 302, "box");
        atStep(st, 359);
        cqUnion(c, st, 303, 299, {"toUnion": {"wp": 302}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 303, "union");
        atStep(st, 360);
        cqWorkplaneNew(c, st, 304, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 361);
        cqTransformed(c, st, 305, 304, {"rotate": [0, 0, 176.25], "offset": [-12.84743363669702, 0.8420652888380926, 0]});
        atStep(st, 362);
        cqPolyline(c, st, 306, 305, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 363);
        cqClose(c, st, 307, 306, {});
        atStep(st, 364);
        cqExtrude(c, st, 308, 307, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 308, "extrude");
        atStep(st, 365);
        cqWorkplaneNew(c, st, 309, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 366);
        cqTransformed(c, st, 310, 309, {"rotate": [0, 0, 176.25], "offset": [-12.84743363669702, 0.8420652888380926, 0]});
        atStep(st, 367);
        cqPolyline(c, st, 311, 310, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 368);
        cqClose(c, st, 312, 311, {});
        atStep(st, 369);
        cqExtrude(c, st, 313, 312, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 313, "extrude");
        // step 370: guard_begin {"before":{"wp":308},"block":2,"g":29,"op":"guard_begin","skippable":true}
        atStep(st, 371);
        cqSelect("Edges")(c, st, 314, 308, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 372);
        cqFillet(c, st, 315, 314, {"radius": 0.2});
        stepVol(c, st, 315, "fillet");
        // step 373: guard_end {"after":{"wp":315},"block":2,"g":29,"op":"guard_end"}
        // step 374: guard_begin {"before":{"wp":313},"block":3,"g":30,"op":"guard_begin","skippable":true}
        atStep(st, 375);
        cqSelect("Edges")(c, st, 316, 313, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 376);
        cqFillet(c, st, 317, 316, {"radius": 0.2});
        stepVol(c, st, 317, "fillet");
        // step 377: guard_end {"after":{"wp":317},"block":3,"g":30,"op":"guard_end"}
        atStep(st, 378);
        cqUnion(c, st, 318, 303, {"toUnion": {"wp": 315}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 318, "union");
        atStep(st, 379);
        cqUnion(c, st, 319, 318, {"toUnion": {"wp": 317}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 319, "union");
        atStep(st, 380);
        cqWorkplaneNew(c, st, 320, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 381);
        cqTransformed(c, st, 321, 320, {"rotate": [0, 0, 198.75], "offset": [-12.191725417249486, -4.138533115778205, 0]});
        atStep(st, 382);
        cqBox(c, st, 322, 321, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 322, "box");
        atStep(st, 383);
        cqUnion(c, st, 323, 319, {"toUnion": {"wp": 322}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 323, "union");
        atStep(st, 384);
        cqWorkplaneNew(c, st, 324, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 385);
        cqTransformed(c, st, 325, 324, {"rotate": [0, 0, 198.75], "offset": [-12.191725417249486, -4.138533115778205, 0]});
        atStep(st, 386);
        cqPolyline(c, st, 326, 325, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 387);
        cqClose(c, st, 327, 326, {});
        atStep(st, 388);
        cqExtrude(c, st, 328, 327, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 328, "extrude");
        atStep(st, 389);
        cqWorkplaneNew(c, st, 329, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 390);
        cqTransformed(c, st, 330, 329, {"rotate": [0, 0, 198.75], "offset": [-12.191725417249486, -4.138533115778205, 0]});
        atStep(st, 391);
        cqPolyline(c, st, 331, 330, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 392);
        cqClose(c, st, 332, 331, {});
        atStep(st, 393);
        cqExtrude(c, st, 333, 332, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 333, "extrude");
        // step 394: guard_begin {"before":{"wp":328},"block":2,"g":31,"op":"guard_begin","skippable":true}
        atStep(st, 395);
        cqSelect("Edges")(c, st, 334, 328, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 396);
        cqFillet(c, st, 335, 334, {"radius": 0.2});
        stepVol(c, st, 335, "fillet");
        // step 397: guard_end {"after":{"wp":335},"block":2,"g":31,"op":"guard_end"}
        // step 398: guard_begin {"before":{"wp":333},"block":3,"g":32,"op":"guard_begin","skippable":true}
        atStep(st, 399);
        cqSelect("Edges")(c, st, 336, 333, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 400);
        cqFillet(c, st, 337, 336, {"radius": 0.2});
        stepVol(c, st, 337, "fillet");
        // step 401: guard_end {"after":{"wp":337},"block":3,"g":32,"op":"guard_end"}
        atStep(st, 402);
        cqUnion(c, st, 338, 323, {"toUnion": {"wp": 335}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 338, "union");
        atStep(st, 403);
        cqUnion(c, st, 339, 338, {"toUnion": {"wp": 337}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 339, "union");
        atStep(st, 404);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 405);
        cqTransformed(c, st, 341, 340, {"rotate": [0, 0, 221.25], "offset": [-9.679937521291835, -8.489077369413387, 0]});
        atStep(st, 406);
        cqBox(c, st, 342, 341, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 342, "box");
        atStep(st, 407);
        cqUnion(c, st, 343, 339, {"toUnion": {"wp": 342}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 343, "union");
        atStep(st, 408);
        cqWorkplaneNew(c, st, 344, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 409);
        cqTransformed(c, st, 345, 344, {"rotate": [0, 0, 221.25], "offset": [-9.679937521291835, -8.489077369413387, 0]});
        atStep(st, 410);
        cqPolyline(c, st, 346, 345, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 411);
        cqClose(c, st, 347, 346, {});
        atStep(st, 412);
        cqExtrude(c, st, 348, 347, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 348, "extrude");
        atStep(st, 413);
        cqWorkplaneNew(c, st, 349, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 414);
        cqTransformed(c, st, 350, 349, {"rotate": [0, 0, 221.25], "offset": [-9.679937521291835, -8.489077369413387, 0]});
        atStep(st, 415);
        cqPolyline(c, st, 351, 350, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 416);
        cqClose(c, st, 352, 351, {});
        atStep(st, 417);
        cqExtrude(c, st, 353, 352, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 353, "extrude");
        // step 418: guard_begin {"before":{"wp":348},"block":2,"g":33,"op":"guard_begin","skippable":true}
        atStep(st, 419);
        cqSelect("Edges")(c, st, 354, 348, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 420);
        cqFillet(c, st, 355, 354, {"radius": 0.2});
        stepVol(c, st, 355, "fillet");
        // step 421: guard_end {"after":{"wp":355},"block":2,"g":33,"op":"guard_end"}
        // step 422: guard_begin {"before":{"wp":353},"block":3,"g":34,"op":"guard_begin","skippable":true}
        atStep(st, 423);
        cqSelect("Edges")(c, st, 356, 353, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 424);
        cqFillet(c, st, 357, 356, {"radius": 0.2});
        stepVol(c, st, 357, "fillet");
        // step 425: guard_end {"after":{"wp":357},"block":3,"g":34,"op":"guard_end"}
        atStep(st, 426);
        cqUnion(c, st, 358, 343, {"toUnion": {"wp": 355}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 358, "union");
        atStep(st, 427);
        cqUnion(c, st, 359, 358, {"toUnion": {"wp": 357}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 359, "union");
        atStep(st, 428);
        cqWorkplaneNew(c, st, 360, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 429);
        cqTransformed(c, st, 361, 360, {"rotate": [0, 0, 243.75], "offset": [-5.694466886569643, -11.54723654723336, 0]});
        atStep(st, 430);
        cqBox(c, st, 362, 361, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 362, "box");
        atStep(st, 431);
        cqUnion(c, st, 363, 359, {"toUnion": {"wp": 362}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 363, "union");
        atStep(st, 432);
        cqWorkplaneNew(c, st, 364, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 433);
        cqTransformed(c, st, 365, 364, {"rotate": [0, 0, 243.75], "offset": [-5.694466886569643, -11.54723654723336, 0]});
        atStep(st, 434);
        cqPolyline(c, st, 366, 365, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 435);
        cqClose(c, st, 367, 366, {});
        atStep(st, 436);
        cqExtrude(c, st, 368, 367, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 368, "extrude");
        atStep(st, 437);
        cqWorkplaneNew(c, st, 369, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 438);
        cqTransformed(c, st, 370, 369, {"rotate": [0, 0, 243.75], "offset": [-5.694466886569643, -11.54723654723336, 0]});
        atStep(st, 439);
        cqPolyline(c, st, 371, 370, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 440);
        cqClose(c, st, 372, 371, {});
        atStep(st, 441);
        cqExtrude(c, st, 373, 372, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 373, "extrude");
        // step 442: guard_begin {"before":{"wp":368},"block":2,"g":35,"op":"guard_begin","skippable":true}
        atStep(st, 443);
        cqSelect("Edges")(c, st, 374, 368, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 444);
        cqFillet(c, st, 375, 374, {"radius": 0.2});
        stepVol(c, st, 375, "fillet");
        // step 445: guard_end {"after":{"wp":375},"block":2,"g":35,"op":"guard_end"}
        // step 446: guard_begin {"before":{"wp":373},"block":3,"g":36,"op":"guard_begin","skippable":true}
        atStep(st, 447);
        cqSelect("Edges")(c, st, 376, 373, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 448);
        cqFillet(c, st, 377, 376, {"radius": 0.2});
        stepVol(c, st, 377, "fillet");
        // step 449: guard_end {"after":{"wp":377},"block":3,"g":36,"op":"guard_end"}
        atStep(st, 450);
        cqUnion(c, st, 378, 363, {"toUnion": {"wp": 375}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 378, "union");
        atStep(st, 451);
        cqUnion(c, st, 379, 378, {"toUnion": {"wp": 377}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 379, "union");
        atStep(st, 452);
        cqWorkplaneNew(c, st, 380, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 453);
        cqTransformed(c, st, 381, 380, {"rotate": [0, 0, 266.25], "offset": [-0.8420652888380876, -12.84743363669702, 0]});
        atStep(st, 454);
        cqBox(c, st, 382, 381, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 382, "box");
        atStep(st, 455);
        cqUnion(c, st, 383, 379, {"toUnion": {"wp": 382}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 383, "union");
        atStep(st, 456);
        cqWorkplaneNew(c, st, 384, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 457);
        cqTransformed(c, st, 385, 384, {"rotate": [0, 0, 266.25], "offset": [-0.8420652888380876, -12.84743363669702, 0]});
        atStep(st, 458);
        cqPolyline(c, st, 386, 385, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 459);
        cqClose(c, st, 387, 386, {});
        atStep(st, 460);
        cqExtrude(c, st, 388, 387, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 388, "extrude");
        atStep(st, 461);
        cqWorkplaneNew(c, st, 389, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 462);
        cqTransformed(c, st, 390, 389, {"rotate": [0, 0, 266.25], "offset": [-0.8420652888380876, -12.84743363669702, 0]});
        atStep(st, 463);
        cqPolyline(c, st, 391, 390, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 464);
        cqClose(c, st, 392, 391, {});
        atStep(st, 465);
        cqExtrude(c, st, 393, 392, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 393, "extrude");
        // step 466: guard_begin {"before":{"wp":388},"block":2,"g":37,"op":"guard_begin","skippable":true}
        atStep(st, 467);
        cqSelect("Edges")(c, st, 394, 388, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 468);
        cqFillet(c, st, 395, 394, {"radius": 0.2});
        stepVol(c, st, 395, "fillet");
        // step 469: guard_end {"after":{"wp":395},"block":2,"g":37,"op":"guard_end"}
        // step 470: guard_begin {"before":{"wp":393},"block":3,"g":38,"op":"guard_begin","skippable":true}
        atStep(st, 471);
        cqSelect("Edges")(c, st, 396, 393, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 472);
        cqFillet(c, st, 397, 396, {"radius": 0.2});
        stepVol(c, st, 397, "fillet");
        // step 473: guard_end {"after":{"wp":397},"block":3,"g":38,"op":"guard_end"}
        atStep(st, 474);
        cqUnion(c, st, 398, 383, {"toUnion": {"wp": 395}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 398, "union");
        atStep(st, 475);
        cqUnion(c, st, 399, 398, {"toUnion": {"wp": 397}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 399, "union");
        atStep(st, 476);
        cqWorkplaneNew(c, st, 400, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 477);
        cqTransformed(c, st, 401, 400, {"rotate": [0, 0, 288.75], "offset": [4.138533115778204, -12.191725417249486, 0]});
        atStep(st, 478);
        cqBox(c, st, 402, 401, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 402, "box");
        atStep(st, 479);
        cqUnion(c, st, 403, 399, {"toUnion": {"wp": 402}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 403, "union");
        atStep(st, 480);
        cqWorkplaneNew(c, st, 404, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 481);
        cqTransformed(c, st, 405, 404, {"rotate": [0, 0, 288.75], "offset": [4.138533115778204, -12.191725417249486, 0]});
        atStep(st, 482);
        cqPolyline(c, st, 406, 405, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 483);
        cqClose(c, st, 407, 406, {});
        atStep(st, 484);
        cqExtrude(c, st, 408, 407, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 408, "extrude");
        atStep(st, 485);
        cqWorkplaneNew(c, st, 409, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 486);
        cqTransformed(c, st, 410, 409, {"rotate": [0, 0, 288.75], "offset": [4.138533115778204, -12.191725417249486, 0]});
        atStep(st, 487);
        cqPolyline(c, st, 411, 410, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 488);
        cqClose(c, st, 412, 411, {});
        atStep(st, 489);
        cqExtrude(c, st, 413, 412, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 413, "extrude");
        // step 490: guard_begin {"before":{"wp":408},"block":2,"g":39,"op":"guard_begin","skippable":true}
        atStep(st, 491);
        cqSelect("Edges")(c, st, 414, 408, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 492);
        cqFillet(c, st, 415, 414, {"radius": 0.2});
        stepVol(c, st, 415, "fillet");
        // step 493: guard_end {"after":{"wp":415},"block":2,"g":39,"op":"guard_end"}
        // step 494: guard_begin {"before":{"wp":413},"block":3,"g":40,"op":"guard_begin","skippable":true}
        atStep(st, 495);
        cqSelect("Edges")(c, st, 416, 413, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 496);
        cqFillet(c, st, 417, 416, {"radius": 0.2});
        stepVol(c, st, 417, "fillet");
        // step 497: guard_end {"after":{"wp":417},"block":3,"g":40,"op":"guard_end"}
        atStep(st, 498);
        cqUnion(c, st, 418, 403, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 418, "union");
        atStep(st, 499);
        cqUnion(c, st, 419, 418, {"toUnion": {"wp": 417}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 419, "union");
        atStep(st, 500);
        cqWorkplaneNew(c, st, 420, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 501);
        cqTransformed(c, st, 421, 420, {"rotate": [0, 0, 311.25], "offset": [8.489077369413389, -9.679937521291832, 0]});
        atStep(st, 502);
        cqBox(c, st, 422, 421, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 422, "box");
        atStep(st, 503);
        cqUnion(c, st, 423, 419, {"toUnion": {"wp": 422}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 423, "union");
        atStep(st, 504);
        cqWorkplaneNew(c, st, 424, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 505);
        cqTransformed(c, st, 425, 424, {"rotate": [0, 0, 311.25], "offset": [8.489077369413389, -9.679937521291832, 0]});
        atStep(st, 506);
        cqPolyline(c, st, 426, 425, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 507);
        cqClose(c, st, 427, 426, {});
        atStep(st, 508);
        cqExtrude(c, st, 428, 427, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 428, "extrude");
        atStep(st, 509);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 510);
        cqTransformed(c, st, 430, 429, {"rotate": [0, 0, 311.25], "offset": [8.489077369413389, -9.679937521291832, 0]});
        atStep(st, 511);
        cqPolyline(c, st, 431, 430, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 512);
        cqClose(c, st, 432, 431, {});
        atStep(st, 513);
        cqExtrude(c, st, 433, 432, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 433, "extrude");
        // step 514: guard_begin {"before":{"wp":428},"block":2,"g":41,"op":"guard_begin","skippable":true}
        atStep(st, 515);
        cqSelect("Edges")(c, st, 434, 428, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 516);
        cqFillet(c, st, 435, 434, {"radius": 0.2});
        stepVol(c, st, 435, "fillet");
        // step 517: guard_end {"after":{"wp":435},"block":2,"g":41,"op":"guard_end"}
        // step 518: guard_begin {"before":{"wp":433},"block":3,"g":42,"op":"guard_begin","skippable":true}
        atStep(st, 519);
        cqSelect("Edges")(c, st, 436, 433, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 520);
        cqFillet(c, st, 437, 436, {"radius": 0.2});
        stepVol(c, st, 437, "fillet");
        // step 521: guard_end {"after":{"wp":437},"block":3,"g":42,"op":"guard_end"}
        atStep(st, 522);
        cqUnion(c, st, 438, 423, {"toUnion": {"wp": 435}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 438, "union");
        atStep(st, 523);
        cqUnion(c, st, 439, 438, {"toUnion": {"wp": 437}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 439, "union");
        atStep(st, 524);
        cqWorkplaneNew(c, st, 440, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 525);
        cqTransformed(c, st, 441, 440, {"rotate": [0, 0, 333.75], "offset": [11.54723654723336, -5.694466886569643, 0]});
        atStep(st, 526);
        cqBox(c, st, 442, 441, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 442, "box");
        atStep(st, 527);
        cqUnion(c, st, 443, 439, {"toUnion": {"wp": 442}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 443, "union");
        atStep(st, 528);
        cqWorkplaneNew(c, st, 444, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 529);
        cqTransformed(c, st, 445, 444, {"rotate": [0, 0, 333.75], "offset": [11.54723654723336, -5.694466886569643, 0]});
        atStep(st, 530);
        cqPolyline(c, st, 446, 445, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 531);
        cqClose(c, st, 447, 446, {});
        atStep(st, 532);
        cqExtrude(c, st, 448, 447, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 448, "extrude");
        atStep(st, 533);
        cqWorkplaneNew(c, st, 449, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 534);
        cqTransformed(c, st, 450, 449, {"rotate": [0, 0, 333.75], "offset": [11.54723654723336, -5.694466886569643, 0]});
        atStep(st, 535);
        cqPolyline(c, st, 451, 450, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 536);
        cqClose(c, st, 452, 451, {});
        atStep(st, 537);
        cqExtrude(c, st, 453, 452, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 453, "extrude");
        // step 538: guard_begin {"before":{"wp":448},"block":2,"g":43,"op":"guard_begin","skippable":true}
        atStep(st, 539);
        cqSelect("Edges")(c, st, 454, 448, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 540);
        cqFillet(c, st, 455, 454, {"radius": 0.2});
        stepVol(c, st, 455, "fillet");
        // step 541: guard_end {"after":{"wp":455},"block":2,"g":43,"op":"guard_end"}
        // step 542: guard_begin {"before":{"wp":453},"block":3,"g":44,"op":"guard_begin","skippable":true}
        atStep(st, 543);
        cqSelect("Edges")(c, st, 456, 453, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 544);
        cqFillet(c, st, 457, 456, {"radius": 0.2});
        stepVol(c, st, 457, "fillet");
        // step 545: guard_end {"after":{"wp":457},"block":3,"g":44,"op":"guard_end"}
        atStep(st, 546);
        cqUnion(c, st, 458, 443, {"toUnion": {"wp": 455}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 458, "union");
        atStep(st, 547);
        cqUnion(c, st, 459, 458, {"toUnion": {"wp": 457}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 459, "union");
        atStep(st, 548);
        cqWorkplaneNew(c, st, 460, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 549);
        cqTransformed(c, st, 461, 460, {"rotate": [0, 0, 356.25], "offset": [12.84743363669702, -0.8420652888380885, 0]});
        atStep(st, 550);
        cqBox(c, st, 462, 461, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 462, "box");
        atStep(st, 551);
        cqUnion(c, st, 463, 459, {"toUnion": {"wp": 462}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 463, "union");
        atStep(st, 552);
        cqWorkplaneNew(c, st, 464, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 553);
        cqTransformed(c, st, 465, 464, {"rotate": [0, 0, 356.25], "offset": [12.84743363669702, -0.8420652888380885, 0]});
        atStep(st, 554);
        cqPolyline(c, st, 466, 465, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 555);
        cqClose(c, st, 467, 466, {});
        atStep(st, 556);
        cqExtrude(c, st, 468, 467, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 468, "extrude");
        atStep(st, 557);
        cqWorkplaneNew(c, st, 469, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 558);
        cqTransformed(c, st, 470, 469, {"rotate": [0, 0, 356.25], "offset": [12.84743363669702, -0.8420652888380885, 0]});
        atStep(st, 559);
        cqPolyline(c, st, 471, 470, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 560);
        cqClose(c, st, 472, 471, {});
        atStep(st, 561);
        cqExtrude(c, st, 473, 472, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 473, "extrude");
        // step 562: guard_begin {"before":{"wp":468},"block":2,"g":45,"op":"guard_begin","skippable":true}
        atStep(st, 563);
        cqSelect("Edges")(c, st, 474, 468, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 564);
        cqFillet(c, st, 475, 474, {"radius": 0.2});
        stepVol(c, st, 475, "fillet");
        // step 565: guard_end {"after":{"wp":475},"block":2,"g":45,"op":"guard_end"}
        // step 566: guard_begin {"before":{"wp":473},"block":3,"g":46,"op":"guard_begin","skippable":true}
        atStep(st, 567);
        cqSelect("Edges")(c, st, 476, 473, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 568);
        cqFillet(c, st, 477, 476, {"radius": 0.2});
        stepVol(c, st, 477, "fillet");
        // step 569: guard_end {"after":{"wp":477},"block":3,"g":46,"op":"guard_end"}
        atStep(st, 570);
        cqUnion(c, st, 478, 463, {"toUnion": {"wp": 475}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 478, "union");
        atStep(st, 571);
        cqUnion(c, st, 479, 478, {"toUnion": {"wp": 477}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 479, "union");
        atStep(st, 572);
        cqWorkplaneNew(c, st, 480, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 573);
        cqTransformed(c, st, 481, 480, {"rotate": [0, 0, 378.75], "offset": [12.191725417249486, 4.138533115778204, 0]});
        atStep(st, 574);
        cqBox(c, st, 482, 481, {"length": 0.75, "width": 0.5, "height": 6, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 482, "box");
        atStep(st, 575);
        cqUnion(c, st, 483, 479, {"toUnion": {"wp": 482}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 483, "union");
        atStep(st, 576);
        cqWorkplaneNew(c, st, 484, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 577);
        cqTransformed(c, st, 485, 484, {"rotate": [0, 0, 378.75], "offset": [12.191725417249486, 4.138533115778204, 0]});
        atStep(st, 578);
        cqPolyline(c, st, 486, 485, {"listOfXYTuple": [[0.375, -0.375], [0.375, 0.375], [0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 579);
        cqClose(c, st, 487, 486, {});
        atStep(st, 580);
        cqExtrude(c, st, 488, 487, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 488, "extrude");
        atStep(st, 581);
        cqWorkplaneNew(c, st, 489, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 582);
        cqTransformed(c, st, 490, 489, {"rotate": [0, 0, 378.75], "offset": [12.191725417249486, 4.138533115778204, 0]});
        atStep(st, 583);
        cqPolyline(c, st, 491, 490, {"listOfXYTuple": [[-0.375, -0.375], [-0.375, 0.375], [-0.625, 0]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 584);
        cqClose(c, st, 492, 491, {});
        atStep(st, 585);
        cqExtrude(c, st, 493, 492, {"until": 6, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 493, "extrude");
        // step 586: guard_begin {"before":{"wp":488},"block":2,"g":47,"op":"guard_begin","skippable":true}
        atStep(st, 587);
        cqSelect("Edges")(c, st, 494, 488, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 588);
        cqFillet(c, st, 495, 494, {"radius": 0.2});
        stepVol(c, st, 495, "fillet");
        // step 589: guard_end {"after":{"wp":495},"block":2,"g":47,"op":"guard_end"}
        // step 590: guard_begin {"before":{"wp":493},"block":3,"g":48,"op":"guard_begin","skippable":true}
        atStep(st, 591);
        cqSelect("Edges")(c, st, 496, 493, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 592);
        cqFillet(c, st, 497, 496, {"radius": 0.2});
        stepVol(c, st, 497, "fillet");
        // step 593: guard_end {"after":{"wp":497},"block":3,"g":48,"op":"guard_end"}
        atStep(st, 594);
        cqUnion(c, st, 498, 483, {"toUnion": {"wp": 495}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 498, "union");
        atStep(st, 595);
        cqUnion(c, st, 499, 498, {"toUnion": {"wp": 497}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 499, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 499}), {"i": 3858});
    } catch (e) {
        return {"i": 3858, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_3858(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c3858"] = r; }
    return out;
}
