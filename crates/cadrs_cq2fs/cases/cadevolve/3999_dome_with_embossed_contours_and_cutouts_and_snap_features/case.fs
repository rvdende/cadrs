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

const case_3999 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqSphere(c, st, 2, 1, {"radius": 12, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "sphere");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqRect(c, st, 4, 3, {"xLen": 24, "yLen": 24, "centered": true, "forConstruction": false});
        atStep(st, 5);
        cqExtrude(c, st, 5, 4, {"until": -24, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 5, "extrude");
        atStep(st, 6);
        cqCut(c, st, 6, 2, {"toCut": {"wp": 5}, "clean": true, "tol": undefined});
        stepVol(c, st, 6, "cut");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [11.61895003862225, 0, 3], "xDir": [0, 1, 0], "zDir": [0.9682458365518544, 0, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 8);
        cqRect(c, st, 8, 7, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 9);
        cqExtrude(c, st, 9, 8, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 9, "extrude");
        atStep(st, 10);
        cqWorkplaneNew(c, st, 10, {"plane": {"origin": [11.61895003862225, 0, 3], "xDir": [0, 1, 0], "zDir": [0.9682458365518544, 0, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 11);
        cqRect(c, st, 11, 10, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 12);
        cqExtrude(c, st, 12, 11, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 12, "extrude");
        atStep(st, 13);
        cqWorkplaneNew(c, st, 13, {"plane": {"origin": [11.61895003862225, 0, 3], "xDir": [0, 1, 0], "zDir": [0.9682458365518544, 0, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 14);
        cqRect(c, st, 14, 13, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 15);
        cqExtrude(c, st, 15, 14, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 15, "extrude");
        atStep(st, 16);
        cqCut(c, st, 16, 12, {"toCut": {"wp": 15}, "clean": true, "tol": undefined});
        stepVol(c, st, 16, "cut");
        atStep(st, 17);
        cqWorkplaneNew(c, st, 17, {"plane": {"origin": [10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [0.8385254915624212, 0.484122918275927, 0.25]}, "obj": undefined});
        atStep(st, 18);
        cqRect(c, st, 18, 17, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 19);
        cqExtrude(c, st, 19, 18, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 19, "extrude");
        atStep(st, 20);
        cqWorkplaneNew(c, st, 20, {"plane": {"origin": [10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [0.8385254915624212, 0.484122918275927, 0.25]}, "obj": undefined});
        atStep(st, 21);
        cqRect(c, st, 21, 20, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 22);
        cqExtrude(c, st, 22, 21, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 22, "extrude");
        atStep(st, 23);
        cqWorkplaneNew(c, st, 23, {"plane": {"origin": [10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [0.8385254915624212, 0.484122918275927, 0.25]}, "obj": undefined});
        atStep(st, 24);
        cqRect(c, st, 24, 23, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 25);
        cqExtrude(c, st, 25, 24, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 25, "extrude");
        atStep(st, 26);
        cqCut(c, st, 26, 22, {"toCut": {"wp": 25}, "clean": true, "tol": undefined});
        stepVol(c, st, 26, "cut");
        atStep(st, 27);
        cqWorkplaneNew(c, st, 27, {"plane": {"origin": [5.809475019311126, 10.062305898749052, 3], "xDir": [-0.8660254037844386, 0.5000000000000001, -1.3877787807814457e-17], "zDir": [0.4841229182759272, 0.8385254915624211, 0.25]}, "obj": undefined});
        atStep(st, 28);
        cqRect(c, st, 28, 27, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 29);
        cqExtrude(c, st, 29, 28, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 29, "extrude");
        atStep(st, 30);
        cqWorkplaneNew(c, st, 30, {"plane": {"origin": [5.809475019311126, 10.062305898749052, 3], "xDir": [-0.8660254037844386, 0.5000000000000001, -1.3877787807814457e-17], "zDir": [0.4841229182759272, 0.8385254915624211, 0.25]}, "obj": undefined});
        atStep(st, 31);
        cqRect(c, st, 31, 30, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 32);
        cqExtrude(c, st, 32, 31, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 32, "extrude");
        atStep(st, 33);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [5.809475019311126, 10.062305898749052, 3], "xDir": [-0.8660254037844386, 0.5000000000000001, -1.3877787807814457e-17], "zDir": [0.4841229182759272, 0.8385254915624211, 0.25]}, "obj": undefined});
        atStep(st, 34);
        cqRect(c, st, 34, 33, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 35);
        cqExtrude(c, st, 35, 34, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 35, "extrude");
        atStep(st, 36);
        cqCut(c, st, 36, 32, {"toCut": {"wp": 35}, "clean": true, "tol": undefined});
        stepVol(c, st, 36, "cut");
        atStep(st, 37);
        cqWorkplaneNew(c, st, 37, {"plane": {"origin": [7.114554987125877e-16, 11.61895003862225, 3], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.928795822604899e-17, 0.9682458365518544, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 38);
        cqRect(c, st, 38, 37, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 39);
        cqExtrude(c, st, 39, 38, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 39, "extrude");
        atStep(st, 40);
        cqWorkplaneNew(c, st, 40, {"plane": {"origin": [7.114554987125877e-16, 11.61895003862225, 3], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.928795822604899e-17, 0.9682458365518544, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 41);
        cqRect(c, st, 41, 40, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 42);
        cqExtrude(c, st, 42, 41, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 42, "extrude");
        atStep(st, 43);
        cqWorkplaneNew(c, st, 43, {"plane": {"origin": [7.114554987125877e-16, 11.61895003862225, 3], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.928795822604899e-17, 0.9682458365518544, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 44);
        cqRect(c, st, 44, 43, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 45);
        cqExtrude(c, st, 45, 44, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 45, "extrude");
        atStep(st, 46);
        cqCut(c, st, 46, 42, {"toCut": {"wp": 45}, "clean": true, "tol": undefined});
        stepVol(c, st, 46, "cut");
        atStep(st, 47);
        cqWorkplaneNew(c, st, 47, {"plane": {"origin": [-5.809475019311122, 10.062305898749054, 3], "xDir": [-0.8660254037844387, -0.4999999999999998, 0], "zDir": [-0.4841229182759269, 0.8385254915624213, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 48);
        cqRect(c, st, 48, 47, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 49);
        cqExtrude(c, st, 49, 48, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 49, "extrude");
        atStep(st, 50);
        cqWorkplaneNew(c, st, 50, {"plane": {"origin": [-5.809475019311122, 10.062305898749054, 3], "xDir": [-0.8660254037844387, -0.4999999999999998, 0], "zDir": [-0.4841229182759269, 0.8385254915624213, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 51);
        cqRect(c, st, 51, 50, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 52);
        cqExtrude(c, st, 52, 51, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 52, "extrude");
        atStep(st, 53);
        cqWorkplaneNew(c, st, 53, {"plane": {"origin": [-5.809475019311122, 10.062305898749054, 3], "xDir": [-0.8660254037844387, -0.4999999999999998, 0], "zDir": [-0.4841229182759269, 0.8385254915624213, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 54);
        cqRect(c, st, 54, 53, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 55);
        cqExtrude(c, st, 55, 54, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 55, "extrude");
        atStep(st, 56);
        cqCut(c, st, 56, 52, {"toCut": {"wp": 55}, "clean": true, "tol": undefined});
        stepVol(c, st, 56, "cut");
        atStep(st, 57);
        cqWorkplaneNew(c, st, 57, {"plane": {"origin": [-10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [-0.8385254915624212, 0.484122918275927, 0.25]}, "obj": undefined});
        atStep(st, 58);
        cqRect(c, st, 58, 57, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 59);
        cqExtrude(c, st, 59, 58, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 59, "extrude");
        atStep(st, 60);
        cqWorkplaneNew(c, st, 60, {"plane": {"origin": [-10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [-0.8385254915624212, 0.484122918275927, 0.25]}, "obj": undefined});
        atStep(st, 61);
        cqRect(c, st, 61, 60, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 62);
        cqExtrude(c, st, 62, 61, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 62, "extrude");
        atStep(st, 63);
        cqWorkplaneNew(c, st, 63, {"plane": {"origin": [-10.062305898749054, 5.809475019311124, 3], "xDir": [-0.49999999999999994, -0.8660254037844387, 0], "zDir": [-0.8385254915624212, 0.484122918275927, 0.25]}, "obj": undefined});
        atStep(st, 64);
        cqRect(c, st, 64, 63, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 65);
        cqExtrude(c, st, 65, 64, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 65, "extrude");
        atStep(st, 66);
        cqCut(c, st, 66, 62, {"toCut": {"wp": 65}, "clean": true, "tol": undefined});
        stepVol(c, st, 66, "cut");
        atStep(st, 67);
        cqWorkplaneNew(c, st, 67, {"plane": {"origin": [-11.61895003862225, 1.4229109974251757e-15, 3], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.9682458365518544, 1.1857591645209795e-16, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 68);
        cqRect(c, st, 68, 67, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 69);
        cqExtrude(c, st, 69, 68, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 69, "extrude");
        atStep(st, 70);
        cqWorkplaneNew(c, st, 70, {"plane": {"origin": [-11.61895003862225, 1.4229109974251757e-15, 3], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.9682458365518544, 1.1857591645209795e-16, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 71);
        cqRect(c, st, 71, 70, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 72);
        cqExtrude(c, st, 72, 71, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 72, "extrude");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [-11.61895003862225, 1.4229109974251757e-15, 3], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.9682458365518544, 1.1857591645209795e-16, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 74);
        cqRect(c, st, 74, 73, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 75);
        cqExtrude(c, st, 75, 74, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 75, "extrude");
        atStep(st, 76);
        cqCut(c, st, 76, 72, {"toCut": {"wp": 75}, "clean": true, "tol": undefined});
        stepVol(c, st, 76, "cut");
        atStep(st, 77);
        cqWorkplaneNew(c, st, 77, {"plane": {"origin": [-10.062305898749056, -5.8094750193111215, 3], "xDir": [0.4999999999999997, -0.8660254037844388, -1.3877787807814457e-17], "zDir": [-0.8385254915624214, -0.4841229182759269, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 78);
        cqRect(c, st, 78, 77, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 79);
        cqExtrude(c, st, 79, 78, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 79, "extrude");
        atStep(st, 80);
        cqWorkplaneNew(c, st, 80, {"plane": {"origin": [-10.062305898749056, -5.8094750193111215, 3], "xDir": [0.4999999999999997, -0.8660254037844388, -1.3877787807814457e-17], "zDir": [-0.8385254915624214, -0.4841229182759269, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 81);
        cqRect(c, st, 81, 80, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 82);
        cqExtrude(c, st, 82, 81, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 82, "extrude");
        atStep(st, 83);
        cqWorkplaneNew(c, st, 83, {"plane": {"origin": [-10.062305898749056, -5.8094750193111215, 3], "xDir": [0.4999999999999997, -0.8660254037844388, -1.3877787807814457e-17], "zDir": [-0.8385254915624214, -0.4841229182759269, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 84);
        cqRect(c, st, 84, 83, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 85);
        cqExtrude(c, st, 85, 84, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 85, "extrude");
        atStep(st, 86);
        cqCut(c, st, 86, 82, {"toCut": {"wp": 85}, "clean": true, "tol": undefined});
        stepVol(c, st, 86, "cut");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [-5.80947501931113, -10.06230589874905, 3], "xDir": [0.8660254037844385, -0.5000000000000004, 0], "zDir": [-0.4841229182759275, -0.8385254915624208, 0.25]}, "obj": undefined});
        atStep(st, 88);
        cqRect(c, st, 88, 87, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 89);
        cqExtrude(c, st, 89, 88, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 89, "extrude");
        atStep(st, 90);
        cqWorkplaneNew(c, st, 90, {"plane": {"origin": [-5.80947501931113, -10.06230589874905, 3], "xDir": [0.8660254037844385, -0.5000000000000004, 0], "zDir": [-0.4841229182759275, -0.8385254915624208, 0.25]}, "obj": undefined});
        atStep(st, 91);
        cqRect(c, st, 91, 90, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 92);
        cqExtrude(c, st, 92, 91, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 92, "extrude");
        atStep(st, 93);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [-5.80947501931113, -10.06230589874905, 3], "xDir": [0.8660254037844385, -0.5000000000000004, 0], "zDir": [-0.4841229182759275, -0.8385254915624208, 0.25]}, "obj": undefined});
        atStep(st, 94);
        cqRect(c, st, 94, 93, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 95);
        cqExtrude(c, st, 95, 94, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 95, "extrude");
        atStep(st, 96);
        cqCut(c, st, 96, 92, {"toCut": {"wp": 95}, "clean": true, "tol": undefined});
        stepVol(c, st, 96, "cut");
        atStep(st, 97);
        cqWorkplaneNew(c, st, 97, {"plane": {"origin": [-2.134366496137763e-15, -11.61895003862225, 3], "xDir": [1, -1.8369701987210294e-16, 6.162975822039154e-33], "zDir": [-1.778638746781469e-16, -0.9682458365518544, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 98);
        cqRect(c, st, 98, 97, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 99);
        cqExtrude(c, st, 99, 98, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 99, "extrude");
        atStep(st, 100);
        cqWorkplaneNew(c, st, 100, {"plane": {"origin": [-2.134366496137763e-15, -11.61895003862225, 3], "xDir": [1, -1.8369701987210294e-16, 6.162975822039154e-33], "zDir": [-1.778638746781469e-16, -0.9682458365518544, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 101);
        cqRect(c, st, 101, 100, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 102);
        cqExtrude(c, st, 102, 101, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 102, "extrude");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [-2.134366496137763e-15, -11.61895003862225, 3], "xDir": [1, -1.8369701987210294e-16, 6.162975822039154e-33], "zDir": [-1.778638746781469e-16, -0.9682458365518544, 0.25000000000000006]}, "obj": undefined});
        atStep(st, 104);
        cqRect(c, st, 104, 103, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 105);
        cqExtrude(c, st, 105, 104, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 105, "extrude");
        atStep(st, 106);
        cqCut(c, st, 106, 102, {"toCut": {"wp": 105}, "clean": true, "tol": undefined});
        stepVol(c, st, 106, "cut");
        atStep(st, 107);
        cqWorkplaneNew(c, st, 107, {"plane": {"origin": [5.809475019311126, -10.062305898749052, 3], "xDir": [0.8660254037844386, 0.5000000000000001, 1.3877787807814457e-17], "zDir": [0.4841229182759272, -0.8385254915624211, 0.25]}, "obj": undefined});
        atStep(st, 108);
        cqRect(c, st, 108, 107, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 109);
        cqExtrude(c, st, 109, 108, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 109, "extrude");
        atStep(st, 110);
        cqWorkplaneNew(c, st, 110, {"plane": {"origin": [5.809475019311126, -10.062305898749052, 3], "xDir": [0.8660254037844386, 0.5000000000000001, 1.3877787807814457e-17], "zDir": [0.4841229182759272, -0.8385254915624211, 0.25]}, "obj": undefined});
        atStep(st, 111);
        cqRect(c, st, 111, 110, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 112);
        cqExtrude(c, st, 112, 111, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 112, "extrude");
        atStep(st, 113);
        cqWorkplaneNew(c, st, 113, {"plane": {"origin": [5.809475019311126, -10.062305898749052, 3], "xDir": [0.8660254037844386, 0.5000000000000001, 1.3877787807814457e-17], "zDir": [0.4841229182759272, -0.8385254915624211, 0.25]}, "obj": undefined});
        atStep(st, 114);
        cqRect(c, st, 114, 113, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 115);
        cqExtrude(c, st, 115, 114, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 115, "extrude");
        atStep(st, 116);
        cqCut(c, st, 116, 112, {"toCut": {"wp": 115}, "clean": true, "tol": undefined});
        stepVol(c, st, 116, "cut");
        atStep(st, 117);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [10.06230589874905, -5.80947501931113, 3], "xDir": [0.5000000000000004, 0.8660254037844385, 0], "zDir": [0.8385254915624208, -0.4841229182759275, 0.25]}, "obj": undefined});
        atStep(st, 118);
        cqRect(c, st, 118, 117, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 119);
        cqExtrude(c, st, 119, 118, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 119, "extrude");
        atStep(st, 120);
        cqWorkplaneNew(c, st, 120, {"plane": {"origin": [10.06230589874905, -5.80947501931113, 3], "xDir": [0.5000000000000004, 0.8660254037844385, 0], "zDir": [0.8385254915624208, -0.4841229182759275, 0.25]}, "obj": undefined});
        atStep(st, 121);
        cqRect(c, st, 121, 120, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 122);
        cqExtrude(c, st, 122, 121, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 122, "extrude");
        atStep(st, 123);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [10.06230589874905, -5.80947501931113, 3], "xDir": [0.5000000000000004, 0.8660254037844385, 0], "zDir": [0.8385254915624208, -0.4841229182759275, 0.25]}, "obj": undefined});
        atStep(st, 124);
        cqRect(c, st, 124, 123, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 125);
        cqExtrude(c, st, 125, 124, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 125, "extrude");
        atStep(st, 126);
        cqCut(c, st, 126, 122, {"toCut": {"wp": 125}, "clean": true, "tol": undefined});
        stepVol(c, st, 126, "cut");
        atStep(st, 127);
        cqWorkplaneNew(c, st, 127, {"plane": {"origin": [10.392304845413264, 0, 6], "xDir": [0, 1, 0], "zDir": [0.8660254037844387, 0, 0.5]}, "obj": undefined});
        atStep(st, 128);
        cqRect(c, st, 128, 127, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 129);
        cqExtrude(c, st, 129, 128, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 129, "extrude");
        atStep(st, 130);
        cqWorkplaneNew(c, st, 130, {"plane": {"origin": [10.392304845413264, 0, 6], "xDir": [0, 1, 0], "zDir": [0.8660254037844387, 0, 0.5]}, "obj": undefined});
        atStep(st, 131);
        cqRect(c, st, 131, 130, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 132);
        cqExtrude(c, st, 132, 131, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 132, "extrude");
        atStep(st, 133);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [10.392304845413264, 0, 6], "xDir": [0, 1, 0], "zDir": [0.8660254037844387, 0, 0.5]}, "obj": undefined});
        atStep(st, 134);
        cqRect(c, st, 134, 133, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 135);
        cqExtrude(c, st, 135, 134, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 135, "extrude");
        atStep(st, 136);
        cqCut(c, st, 136, 132, {"toCut": {"wp": 135}, "clean": true, "tol": undefined});
        stepVol(c, st, 136, "cut");
        atStep(st, 137);
        cqWorkplaneNew(c, st, 137, {"plane": {"origin": [9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, 0.8660254037844387, -2.7755575615628907e-17], "zDir": [0.7500000000000001, 0.4330127018922193, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 138);
        cqRect(c, st, 138, 137, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 139);
        cqExtrude(c, st, 139, 138, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 139, "extrude");
        atStep(st, 140);
        cqWorkplaneNew(c, st, 140, {"plane": {"origin": [9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, 0.8660254037844387, -2.7755575615628907e-17], "zDir": [0.7500000000000001, 0.4330127018922193, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 141);
        cqRect(c, st, 141, 140, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 142);
        cqExtrude(c, st, 142, 141, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 142, "extrude");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 143, {"plane": {"origin": [9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, 0.8660254037844387, -2.7755575615628907e-17], "zDir": [0.7500000000000001, 0.4330127018922193, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 144);
        cqRect(c, st, 144, 143, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 145);
        cqExtrude(c, st, 145, 144, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 145, "extrude");
        atStep(st, 146);
        cqCut(c, st, 146, 142, {"toCut": {"wp": 145}, "clean": true, "tol": undefined});
        stepVol(c, st, 146, "cut");
        atStep(st, 147);
        cqWorkplaneNew(c, st, 147, {"plane": {"origin": [5.196152422706633, 9, 6], "xDir": [-0.8660254037844386, 0.5000000000000001, -2.7755575615628907e-17], "zDir": [0.4330127018922194, 0.75, 0.5]}, "obj": undefined});
        atStep(st, 148);
        cqRect(c, st, 148, 147, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 149);
        cqExtrude(c, st, 149, 148, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 149, "extrude");
        atStep(st, 150);
        cqWorkplaneNew(c, st, 150, {"plane": {"origin": [5.196152422706633, 9, 6], "xDir": [-0.8660254037844386, 0.5000000000000001, -2.7755575615628907e-17], "zDir": [0.4330127018922194, 0.75, 0.5]}, "obj": undefined});
        atStep(st, 151);
        cqRect(c, st, 151, 150, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 152);
        cqExtrude(c, st, 152, 151, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 152, "extrude");
        atStep(st, 153);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [5.196152422706633, 9, 6], "xDir": [-0.8660254037844386, 0.5000000000000001, -2.7755575615628907e-17], "zDir": [0.4330127018922194, 0.75, 0.5]}, "obj": undefined});
        atStep(st, 154);
        cqRect(c, st, 154, 153, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 155);
        cqExtrude(c, st, 155, 154, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 155, "extrude");
        atStep(st, 156);
        cqCut(c, st, 156, 152, {"toCut": {"wp": 155}, "clean": true, "tol": undefined});
        stepVol(c, st, 156, "cut");
        atStep(st, 157);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [6.363451432349442e-16, 10.392304845413264, 6], "xDir": [-1, 6.123233995736767e-17, 3.081487911019578e-33], "zDir": [5.3028761936245346e-17, 0.8660254037844387, 0.5]}, "obj": undefined});
        atStep(st, 158);
        cqRect(c, st, 158, 157, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 159);
        cqExtrude(c, st, 159, 158, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 159, "extrude");
        atStep(st, 160);
        cqWorkplaneNew(c, st, 160, {"plane": {"origin": [6.363451432349442e-16, 10.392304845413264, 6], "xDir": [-1, 6.123233995736767e-17, 3.081487911019578e-33], "zDir": [5.3028761936245346e-17, 0.8660254037844387, 0.5]}, "obj": undefined});
        atStep(st, 161);
        cqRect(c, st, 161, 160, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 162);
        cqExtrude(c, st, 162, 161, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 162, "extrude");
        atStep(st, 163);
        cqWorkplaneNew(c, st, 163, {"plane": {"origin": [6.363451432349442e-16, 10.392304845413264, 6], "xDir": [-1, 6.123233995736767e-17, 3.081487911019578e-33], "zDir": [5.3028761936245346e-17, 0.8660254037844387, 0.5]}, "obj": undefined});
        atStep(st, 164);
        cqRect(c, st, 164, 163, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 165);
        cqExtrude(c, st, 165, 164, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 165, "extrude");
        atStep(st, 166);
        cqCut(c, st, 166, 162, {"toCut": {"wp": 165}, "clean": true, "tol": undefined});
        stepVol(c, st, 166, "cut");
        atStep(st, 167);
        cqWorkplaneNew(c, st, 167, {"plane": {"origin": [-5.196152422706629, 9, 6], "xDir": [-0.8660254037844387, -0.4999999999999998, -2.7755575615628907e-17], "zDir": [-0.4330127018922192, 0.7500000000000001, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 168);
        cqRect(c, st, 168, 167, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 169);
        cqExtrude(c, st, 169, 168, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 169, "extrude");
        atStep(st, 170);
        cqWorkplaneNew(c, st, 170, {"plane": {"origin": [-5.196152422706629, 9, 6], "xDir": [-0.8660254037844387, -0.4999999999999998, -2.7755575615628907e-17], "zDir": [-0.4330127018922192, 0.7500000000000001, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 171);
        cqRect(c, st, 171, 170, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 172);
        cqExtrude(c, st, 172, 171, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 172, "extrude");
        atStep(st, 173);
        cqWorkplaneNew(c, st, 173, {"plane": {"origin": [-5.196152422706629, 9, 6], "xDir": [-0.8660254037844387, -0.4999999999999998, -2.7755575615628907e-17], "zDir": [-0.4330127018922192, 0.7500000000000001, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 174);
        cqRect(c, st, 174, 173, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 175);
        cqExtrude(c, st, 175, 174, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 175, "extrude");
        atStep(st, 176);
        cqCut(c, st, 176, 172, {"toCut": {"wp": 175}, "clean": true, "tol": undefined});
        stepVol(c, st, 176, "cut");
        atStep(st, 177);
        cqWorkplaneNew(c, st, 177, {"plane": {"origin": [-9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, -0.8660254037844387, 2.7755575615628907e-17], "zDir": [-0.7500000000000001, 0.4330127018922193, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 178);
        cqRect(c, st, 178, 177, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 179);
        cqExtrude(c, st, 179, 178, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 179, "extrude");
        atStep(st, 180);
        cqWorkplaneNew(c, st, 180, {"plane": {"origin": [-9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, -0.8660254037844387, 2.7755575615628907e-17], "zDir": [-0.7500000000000001, 0.4330127018922193, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 181);
        cqRect(c, st, 181, 180, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 182);
        cqExtrude(c, st, 182, 181, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 182, "extrude");
        atStep(st, 183);
        cqWorkplaneNew(c, st, 183, {"plane": {"origin": [-9, 5.196152422706631, 6], "xDir": [-0.4999999999999999, -0.8660254037844387, 2.7755575615628907e-17], "zDir": [-0.7500000000000001, 0.4330127018922193, 0.5000000000000001]}, "obj": undefined});
        atStep(st, 184);
        cqRect(c, st, 184, 183, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 185);
        cqExtrude(c, st, 185, 184, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 185, "extrude");
        atStep(st, 186);
        cqCut(c, st, 186, 182, {"toCut": {"wp": 185}, "clean": true, "tol": undefined});
        stepVol(c, st, 186, "cut");
        atStep(st, 187);
        cqWorkplaneNew(c, st, 187, {"plane": {"origin": [-10.392304845413264, 1.2726902864698884e-15, 6], "xDir": [-1.2246467991473535e-16, -1, 6.162975822039155e-33], "zDir": [-0.8660254037844387, 1.060575238724907e-16, 0.5]}, "obj": undefined});
        atStep(st, 188);
        cqRect(c, st, 188, 187, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 189);
        cqExtrude(c, st, 189, 188, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 189, "extrude");
        atStep(st, 190);
        cqWorkplaneNew(c, st, 190, {"plane": {"origin": [-10.392304845413264, 1.2726902864698884e-15, 6], "xDir": [-1.2246467991473535e-16, -1, 6.162975822039155e-33], "zDir": [-0.8660254037844387, 1.060575238724907e-16, 0.5]}, "obj": undefined});
        atStep(st, 191);
        cqRect(c, st, 191, 190, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 192);
        cqExtrude(c, st, 192, 191, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 192, "extrude");
        atStep(st, 193);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [-10.392304845413264, 1.2726902864698884e-15, 6], "xDir": [-1.2246467991473535e-16, -1, 6.162975822039155e-33], "zDir": [-0.8660254037844387, 1.060575238724907e-16, 0.5]}, "obj": undefined});
        atStep(st, 194);
        cqRect(c, st, 194, 193, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 195);
        cqExtrude(c, st, 195, 194, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 195, "extrude");
        atStep(st, 196);
        cqCut(c, st, 196, 192, {"toCut": {"wp": 195}, "clean": true, "tol": undefined});
        stepVol(c, st, 196, "cut");
        atStep(st, 197);
        cqWorkplaneNew(c, st, 197, {"plane": {"origin": [-9.000000000000002, -5.196152422706629, 6], "xDir": [0.4999999999999998, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [-0.7500000000000001, -0.43301270189221913, 0.5]}, "obj": undefined});
        atStep(st, 198);
        cqRect(c, st, 198, 197, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 199);
        cqExtrude(c, st, 199, 198, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 199, "extrude");
        atStep(st, 200);
        cqWorkplaneNew(c, st, 200, {"plane": {"origin": [-9.000000000000002, -5.196152422706629, 6], "xDir": [0.4999999999999998, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [-0.7500000000000001, -0.43301270189221913, 0.5]}, "obj": undefined});
        atStep(st, 201);
        cqRect(c, st, 201, 200, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 202);
        cqExtrude(c, st, 202, 201, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 202, "extrude");
        atStep(st, 203);
        cqWorkplaneNew(c, st, 203, {"plane": {"origin": [-9.000000000000002, -5.196152422706629, 6], "xDir": [0.4999999999999998, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [-0.7500000000000001, -0.43301270189221913, 0.5]}, "obj": undefined});
        atStep(st, 204);
        cqRect(c, st, 204, 203, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 205);
        cqExtrude(c, st, 205, 204, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 205, "extrude");
        atStep(st, 206);
        cqCut(c, st, 206, 202, {"toCut": {"wp": 205}, "clean": true, "tol": undefined});
        stepVol(c, st, 206, "cut");
        atStep(st, 207);
        cqWorkplaneNew(c, st, 207, {"plane": {"origin": [-5.1961524227066365, -8.999999999999998, 6], "xDir": [0.8660254037844384, -0.5000000000000003, -2.7755575615628907e-17], "zDir": [-0.4330127018922197, -0.7499999999999999, 0.5]}, "obj": undefined});
        atStep(st, 208);
        cqRect(c, st, 208, 207, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 209);
        cqExtrude(c, st, 209, 208, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 209, "extrude");
        atStep(st, 210);
        cqWorkplaneNew(c, st, 210, {"plane": {"origin": [-5.1961524227066365, -8.999999999999998, 6], "xDir": [0.8660254037844384, -0.5000000000000003, -2.7755575615628907e-17], "zDir": [-0.4330127018922197, -0.7499999999999999, 0.5]}, "obj": undefined});
        atStep(st, 211);
        cqRect(c, st, 211, 210, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 212);
        cqExtrude(c, st, 212, 211, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 212, "extrude");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [-5.1961524227066365, -8.999999999999998, 6], "xDir": [0.8660254037844384, -0.5000000000000003, -2.7755575615628907e-17], "zDir": [-0.4330127018922197, -0.7499999999999999, 0.5]}, "obj": undefined});
        atStep(st, 214);
        cqRect(c, st, 214, 213, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 215);
        cqExtrude(c, st, 215, 214, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 215, "extrude");
        atStep(st, 216);
        cqCut(c, st, 216, 212, {"toCut": {"wp": 215}, "clean": true, "tol": undefined});
        stepVol(c, st, 216, "cut");
        atStep(st, 217);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [-1.9090354297048323e-15, -10.392304845413264, 6], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [-1.59086285808736e-16, -0.8660254037844387, 0.5]}, "obj": undefined});
        atStep(st, 218);
        cqRect(c, st, 218, 217, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 219);
        cqExtrude(c, st, 219, 218, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 219, "extrude");
        atStep(st, 220);
        cqWorkplaneNew(c, st, 220, {"plane": {"origin": [-1.9090354297048323e-15, -10.392304845413264, 6], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [-1.59086285808736e-16, -0.8660254037844387, 0.5]}, "obj": undefined});
        atStep(st, 221);
        cqRect(c, st, 221, 220, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 222);
        cqExtrude(c, st, 222, 221, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 222, "extrude");
        atStep(st, 223);
        cqWorkplaneNew(c, st, 223, {"plane": {"origin": [-1.9090354297048323e-15, -10.392304845413264, 6], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [-1.59086285808736e-16, -0.8660254037844387, 0.5]}, "obj": undefined});
        atStep(st, 224);
        cqRect(c, st, 224, 223, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 225);
        cqExtrude(c, st, 225, 224, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 225, "extrude");
        atStep(st, 226);
        cqCut(c, st, 226, 222, {"toCut": {"wp": 225}, "clean": true, "tol": undefined});
        stepVol(c, st, 226, "cut");
        atStep(st, 227);
        cqWorkplaneNew(c, st, 227, {"plane": {"origin": [5.196152422706633, -9, 6], "xDir": [0.8660254037844386, 0.5000000000000001, 2.7755575615628907e-17], "zDir": [0.4330127018922194, -0.75, 0.5]}, "obj": undefined});
        atStep(st, 228);
        cqRect(c, st, 228, 227, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 229);
        cqExtrude(c, st, 229, 228, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 229, "extrude");
        atStep(st, 230);
        cqWorkplaneNew(c, st, 230, {"plane": {"origin": [5.196152422706633, -9, 6], "xDir": [0.8660254037844386, 0.5000000000000001, 2.7755575615628907e-17], "zDir": [0.4330127018922194, -0.75, 0.5]}, "obj": undefined});
        atStep(st, 231);
        cqRect(c, st, 231, 230, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 232);
        cqExtrude(c, st, 232, 231, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 232, "extrude");
        atStep(st, 233);
        cqWorkplaneNew(c, st, 233, {"plane": {"origin": [5.196152422706633, -9, 6], "xDir": [0.8660254037844386, 0.5000000000000001, 2.7755575615628907e-17], "zDir": [0.4330127018922194, -0.75, 0.5]}, "obj": undefined});
        atStep(st, 234);
        cqRect(c, st, 234, 233, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 235);
        cqExtrude(c, st, 235, 234, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 235, "extrude");
        atStep(st, 236);
        cqCut(c, st, 236, 232, {"toCut": {"wp": 235}, "clean": true, "tol": undefined});
        stepVol(c, st, 236, "cut");
        atStep(st, 237);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [8.999999999999998, -5.1961524227066365, 6], "xDir": [0.5000000000000003, 0.8660254037844384, -2.7755575615628907e-17], "zDir": [0.7499999999999999, -0.4330127018922197, 0.5]}, "obj": undefined});
        atStep(st, 238);
        cqRect(c, st, 238, 237, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 239);
        cqExtrude(c, st, 239, 238, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 239, "extrude");
        atStep(st, 240);
        cqWorkplaneNew(c, st, 240, {"plane": {"origin": [8.999999999999998, -5.1961524227066365, 6], "xDir": [0.5000000000000003, 0.8660254037844384, -2.7755575615628907e-17], "zDir": [0.7499999999999999, -0.4330127018922197, 0.5]}, "obj": undefined});
        atStep(st, 241);
        cqRect(c, st, 241, 240, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 242);
        cqExtrude(c, st, 242, 241, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 242, "extrude");
        atStep(st, 243);
        cqWorkplaneNew(c, st, 243, {"plane": {"origin": [8.999999999999998, -5.1961524227066365, 6], "xDir": [0.5000000000000003, 0.8660254037844384, -2.7755575615628907e-17], "zDir": [0.7499999999999999, -0.4330127018922197, 0.5]}, "obj": undefined});
        atStep(st, 244);
        cqRect(c, st, 244, 243, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 245);
        cqExtrude(c, st, 245, 244, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 245, "extrude");
        atStep(st, 246);
        cqCut(c, st, 246, 242, {"toCut": {"wp": 245}, "clean": true, "tol": undefined});
        stepVol(c, st, 246, "cut");
        atStep(st, 247);
        cqWorkplaneNew(c, st, 247, {"plane": {"origin": [7.937253933193772, 0, 9], "xDir": [0, 1, 0], "zDir": [0.6614378277661477, 0, 0.75]}, "obj": undefined});
        atStep(st, 248);
        cqRect(c, st, 248, 247, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 249);
        cqExtrude(c, st, 249, 248, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 249, "extrude");
        atStep(st, 250);
        cqWorkplaneNew(c, st, 250, {"plane": {"origin": [7.937253933193772, 0, 9], "xDir": [0, 1, 0], "zDir": [0.6614378277661477, 0, 0.75]}, "obj": undefined});
        atStep(st, 251);
        cqRect(c, st, 251, 250, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 252);
        cqExtrude(c, st, 252, 251, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 252, "extrude");
        atStep(st, 253);
        cqWorkplaneNew(c, st, 253, {"plane": {"origin": [7.937253933193772, 0, 9], "xDir": [0, 1, 0], "zDir": [0.6614378277661477, 0, 0.75]}, "obj": undefined});
        atStep(st, 254);
        cqRect(c, st, 254, 253, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 255);
        cqExtrude(c, st, 255, 254, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 255, "extrude");
        atStep(st, 256);
        cqCut(c, st, 256, 252, {"toCut": {"wp": 255}, "clean": true, "tol": undefined});
        stepVol(c, st, 256, "cut");
        atStep(st, 257);
        cqWorkplaneNew(c, st, 257, {"plane": {"origin": [6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, 0.8660254037844388, 2.7755575615628914e-17], "zDir": [0.5728219618694801, 0.3307189138830738, 0.75]}, "obj": undefined});
        atStep(st, 258);
        cqRect(c, st, 258, 257, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 259);
        cqExtrude(c, st, 259, 258, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 259, "extrude");
        atStep(st, 260);
        cqWorkplaneNew(c, st, 260, {"plane": {"origin": [6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, 0.8660254037844388, 2.7755575615628914e-17], "zDir": [0.5728219618694801, 0.3307189138830738, 0.75]}, "obj": undefined});
        atStep(st, 261);
        cqRect(c, st, 261, 260, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 262);
        cqExtrude(c, st, 262, 261, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 262, "extrude");
        atStep(st, 263);
        cqWorkplaneNew(c, st, 263, {"plane": {"origin": [6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, 0.8660254037844388, 2.7755575615628914e-17], "zDir": [0.5728219618694801, 0.3307189138830738, 0.75]}, "obj": undefined});
        atStep(st, 264);
        cqRect(c, st, 264, 263, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 265);
        cqExtrude(c, st, 265, 264, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 265, "extrude");
        atStep(st, 266);
        cqCut(c, st, 266, 262, {"toCut": {"wp": 265}, "clean": true, "tol": undefined});
        stepVol(c, st, 266, "cut");
        atStep(st, 267);
        cqWorkplaneNew(c, st, 267, {"plane": {"origin": [3.968626966596887, 6.87386354243376, 9], "xDir": [-0.8660254037844386, 0.5000000000000002, 0], "zDir": [0.3307189138830739, 0.57282196186948, 0.75]}, "obj": undefined});
        atStep(st, 268);
        cqRect(c, st, 268, 267, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 269);
        cqExtrude(c, st, 269, 268, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 269, "extrude");
        atStep(st, 270);
        cqWorkplaneNew(c, st, 270, {"plane": {"origin": [3.968626966596887, 6.87386354243376, 9], "xDir": [-0.8660254037844386, 0.5000000000000002, 0], "zDir": [0.3307189138830739, 0.57282196186948, 0.75]}, "obj": undefined});
        atStep(st, 271);
        cqRect(c, st, 271, 270, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 272);
        cqExtrude(c, st, 272, 271, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 272, "extrude");
        atStep(st, 273);
        cqWorkplaneNew(c, st, 273, {"plane": {"origin": [3.968626966596887, 6.87386354243376, 9], "xDir": [-0.8660254037844386, 0.5000000000000002, 0], "zDir": [0.3307189138830739, 0.57282196186948, 0.75]}, "obj": undefined});
        atStep(st, 274);
        cqRect(c, st, 274, 273, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 275);
        cqExtrude(c, st, 275, 274, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 275, "extrude");
        atStep(st, 276);
        cqCut(c, st, 276, 272, {"toCut": {"wp": 275}, "clean": true, "tol": undefined});
        stepVol(c, st, 276, "cut");
        atStep(st, 277);
        cqWorkplaneNew(c, st, 277, {"plane": {"origin": [4.860166311652746e-16, 7.937253933193772, 9], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [4.0501385930439555e-17, 0.6614378277661477, 0.75]}, "obj": undefined});
        atStep(st, 278);
        cqRect(c, st, 278, 277, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 279);
        cqExtrude(c, st, 279, 278, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 279, "extrude");
        atStep(st, 280);
        cqWorkplaneNew(c, st, 280, {"plane": {"origin": [4.860166311652746e-16, 7.937253933193772, 9], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [4.0501385930439555e-17, 0.6614378277661477, 0.75]}, "obj": undefined});
        atStep(st, 281);
        cqRect(c, st, 281, 280, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 282);
        cqExtrude(c, st, 282, 281, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 282, "extrude");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [4.860166311652746e-16, 7.937253933193772, 9], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [4.0501385930439555e-17, 0.6614378277661477, 0.75]}, "obj": undefined});
        atStep(st, 284);
        cqRect(c, st, 284, 283, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 285);
        cqExtrude(c, st, 285, 284, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 285, "extrude");
        atStep(st, 286);
        cqCut(c, st, 286, 282, {"toCut": {"wp": 285}, "clean": true, "tol": undefined});
        stepVol(c, st, 286, "cut");
        atStep(st, 287);
        cqWorkplaneNew(c, st, 287, {"plane": {"origin": [-3.968626966596884, 6.873863542433761, 9], "xDir": [-0.8660254037844388, -0.4999999999999998, 2.7755575615628914e-17], "zDir": [-0.3307189138830737, 0.5728219618694801, 0.75]}, "obj": undefined});
        atStep(st, 288);
        cqRect(c, st, 288, 287, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 289);
        cqExtrude(c, st, 289, 288, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 289, "extrude");
        atStep(st, 290);
        cqWorkplaneNew(c, st, 290, {"plane": {"origin": [-3.968626966596884, 6.873863542433761, 9], "xDir": [-0.8660254037844388, -0.4999999999999998, 2.7755575615628914e-17], "zDir": [-0.3307189138830737, 0.5728219618694801, 0.75]}, "obj": undefined});
        atStep(st, 291);
        cqRect(c, st, 291, 290, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 292);
        cqExtrude(c, st, 292, 291, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 292, "extrude");
        atStep(st, 293);
        cqWorkplaneNew(c, st, 293, {"plane": {"origin": [-3.968626966596884, 6.873863542433761, 9], "xDir": [-0.8660254037844388, -0.4999999999999998, 2.7755575615628914e-17], "zDir": [-0.3307189138830737, 0.5728219618694801, 0.75]}, "obj": undefined});
        atStep(st, 294);
        cqRect(c, st, 294, 293, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 295);
        cqExtrude(c, st, 295, 294, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 295, "extrude");
        atStep(st, 296);
        cqCut(c, st, 296, 292, {"toCut": {"wp": 295}, "clean": true, "tol": undefined});
        stepVol(c, st, 296, "cut");
        atStep(st, 297);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [-6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [-0.5728219618694801, 0.3307189138830738, 0.75]}, "obj": undefined});
        atStep(st, 298);
        cqRect(c, st, 298, 297, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 299);
        cqExtrude(c, st, 299, 298, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 299, "extrude");
        atStep(st, 300);
        cqWorkplaneNew(c, st, 300, {"plane": {"origin": [-6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [-0.5728219618694801, 0.3307189138830738, 0.75]}, "obj": undefined});
        atStep(st, 301);
        cqRect(c, st, 301, 300, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 302);
        cqExtrude(c, st, 302, 301, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 302, "extrude");
        atStep(st, 303);
        cqWorkplaneNew(c, st, 303, {"plane": {"origin": [-6.873863542433761, 3.9686269665968856, 9], "xDir": [-0.4999999999999999, -0.8660254037844388, -2.7755575615628914e-17], "zDir": [-0.5728219618694801, 0.3307189138830738, 0.75]}, "obj": undefined});
        atStep(st, 304);
        cqRect(c, st, 304, 303, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 305);
        cqExtrude(c, st, 305, 304, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 305, "extrude");
        atStep(st, 306);
        cqCut(c, st, 306, 302, {"toCut": {"wp": 305}, "clean": true, "tol": undefined});
        stepVol(c, st, 306, "cut");
        atStep(st, 307);
        cqWorkplaneNew(c, st, 307, {"plane": {"origin": [-7.937253933193772, 9.720332623305493e-16, 9], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.6614378277661477, 8.100277186087911e-17, 0.75]}, "obj": undefined});
        atStep(st, 308);
        cqRect(c, st, 308, 307, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 309);
        cqExtrude(c, st, 309, 308, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 309, "extrude");
        atStep(st, 310);
        cqWorkplaneNew(c, st, 310, {"plane": {"origin": [-7.937253933193772, 9.720332623305493e-16, 9], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.6614378277661477, 8.100277186087911e-17, 0.75]}, "obj": undefined});
        atStep(st, 311);
        cqRect(c, st, 311, 310, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 312);
        cqExtrude(c, st, 312, 311, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 312, "extrude");
        atStep(st, 313);
        cqWorkplaneNew(c, st, 313, {"plane": {"origin": [-7.937253933193772, 9.720332623305493e-16, 9], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.6614378277661477, 8.100277186087911e-17, 0.75]}, "obj": undefined});
        atStep(st, 314);
        cqRect(c, st, 314, 313, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 315);
        cqExtrude(c, st, 315, 314, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 315, "extrude");
        atStep(st, 316);
        cqCut(c, st, 316, 312, {"toCut": {"wp": 315}, "clean": true, "tol": undefined});
        stepVol(c, st, 316, "cut");
        atStep(st, 317);
        cqWorkplaneNew(c, st, 317, {"plane": {"origin": [-6.873863542433762, -3.968626966596884, 9], "xDir": [0.4999999999999998, -0.8660254037844389, 0], "zDir": [-0.5728219618694802, -0.3307189138830737, 0.75]}, "obj": undefined});
        atStep(st, 318);
        cqRect(c, st, 318, 317, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 319);
        cqExtrude(c, st, 319, 318, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 319, "extrude");
        atStep(st, 320);
        cqWorkplaneNew(c, st, 320, {"plane": {"origin": [-6.873863542433762, -3.968626966596884, 9], "xDir": [0.4999999999999998, -0.8660254037844389, 0], "zDir": [-0.5728219618694802, -0.3307189138830737, 0.75]}, "obj": undefined});
        atStep(st, 321);
        cqRect(c, st, 321, 320, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 322);
        cqExtrude(c, st, 322, 321, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 322, "extrude");
        atStep(st, 323);
        cqWorkplaneNew(c, st, 323, {"plane": {"origin": [-6.873863542433762, -3.968626966596884, 9], "xDir": [0.4999999999999998, -0.8660254037844389, 0], "zDir": [-0.5728219618694802, -0.3307189138830737, 0.75]}, "obj": undefined});
        atStep(st, 324);
        cqRect(c, st, 324, 323, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 325);
        cqExtrude(c, st, 325, 324, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 325, "extrude");
        atStep(st, 326);
        cqCut(c, st, 326, 322, {"toCut": {"wp": 325}, "clean": true, "tol": undefined});
        stepVol(c, st, 326, "cut");
        atStep(st, 327);
        cqWorkplaneNew(c, st, 327, {"plane": {"origin": [-3.9686269665968896, -6.873863542433758, 9], "xDir": [0.8660254037844384, -0.5000000000000004, -2.7755575615628914e-17], "zDir": [-0.3307189138830741, -0.5728219618694799, 0.75]}, "obj": undefined});
        atStep(st, 328);
        cqRect(c, st, 328, 327, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 329);
        cqExtrude(c, st, 329, 328, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 329, "extrude");
        atStep(st, 330);
        cqWorkplaneNew(c, st, 330, {"plane": {"origin": [-3.9686269665968896, -6.873863542433758, 9], "xDir": [0.8660254037844384, -0.5000000000000004, -2.7755575615628914e-17], "zDir": [-0.3307189138830741, -0.5728219618694799, 0.75]}, "obj": undefined});
        atStep(st, 331);
        cqRect(c, st, 331, 330, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 332);
        cqExtrude(c, st, 332, 331, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 332, "extrude");
        atStep(st, 333);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [-3.9686269665968896, -6.873863542433758, 9], "xDir": [0.8660254037844384, -0.5000000000000004, -2.7755575615628914e-17], "zDir": [-0.3307189138830741, -0.5728219618694799, 0.75]}, "obj": undefined});
        atStep(st, 334);
        cqRect(c, st, 334, 333, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 335);
        cqExtrude(c, st, 335, 334, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 335, "extrude");
        atStep(st, 336);
        cqCut(c, st, 336, 332, {"toCut": {"wp": 335}, "clean": true, "tol": undefined});
        stepVol(c, st, 336, "cut");
        atStep(st, 337);
        cqWorkplaneNew(c, st, 337, {"plane": {"origin": [-1.4580498934958238e-15, -7.937253933193772, 9], "xDir": [1, -1.83697019872103e-16, -1.232595164407831e-32], "zDir": [-1.2150415779131866e-16, -0.6614378277661477, 0.75]}, "obj": undefined});
        atStep(st, 338);
        cqRect(c, st, 338, 337, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 339);
        cqExtrude(c, st, 339, 338, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 339, "extrude");
        atStep(st, 340);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [-1.4580498934958238e-15, -7.937253933193772, 9], "xDir": [1, -1.83697019872103e-16, -1.232595164407831e-32], "zDir": [-1.2150415779131866e-16, -0.6614378277661477, 0.75]}, "obj": undefined});
        atStep(st, 341);
        cqRect(c, st, 341, 340, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 342);
        cqExtrude(c, st, 342, 341, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 342, "extrude");
        atStep(st, 343);
        cqWorkplaneNew(c, st, 343, {"plane": {"origin": [-1.4580498934958238e-15, -7.937253933193772, 9], "xDir": [1, -1.83697019872103e-16, -1.232595164407831e-32], "zDir": [-1.2150415779131866e-16, -0.6614378277661477, 0.75]}, "obj": undefined});
        atStep(st, 344);
        cqRect(c, st, 344, 343, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 345);
        cqExtrude(c, st, 345, 344, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 345, "extrude");
        atStep(st, 346);
        cqCut(c, st, 346, 342, {"toCut": {"wp": 345}, "clean": true, "tol": undefined});
        stepVol(c, st, 346, "cut");
        atStep(st, 347);
        cqWorkplaneNew(c, st, 347, {"plane": {"origin": [3.968626966596887, -6.87386354243376, 9], "xDir": [0.8660254037844386, 0.5000000000000002, 0], "zDir": [0.3307189138830739, -0.57282196186948, 0.75]}, "obj": undefined});
        atStep(st, 348);
        cqRect(c, st, 348, 347, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 349);
        cqExtrude(c, st, 349, 348, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 349, "extrude");
        atStep(st, 350);
        cqWorkplaneNew(c, st, 350, {"plane": {"origin": [3.968626966596887, -6.87386354243376, 9], "xDir": [0.8660254037844386, 0.5000000000000002, 0], "zDir": [0.3307189138830739, -0.57282196186948, 0.75]}, "obj": undefined});
        atStep(st, 351);
        cqRect(c, st, 351, 350, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 352);
        cqExtrude(c, st, 352, 351, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 352, "extrude");
        atStep(st, 353);
        cqWorkplaneNew(c, st, 353, {"plane": {"origin": [3.968626966596887, -6.87386354243376, 9], "xDir": [0.8660254037844386, 0.5000000000000002, 0], "zDir": [0.3307189138830739, -0.57282196186948, 0.75]}, "obj": undefined});
        atStep(st, 354);
        cqRect(c, st, 354, 353, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 355);
        cqExtrude(c, st, 355, 354, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 355, "extrude");
        atStep(st, 356);
        cqCut(c, st, 356, 352, {"toCut": {"wp": 355}, "clean": true, "tol": undefined});
        stepVol(c, st, 356, "cut");
        atStep(st, 357);
        cqWorkplaneNew(c, st, 357, {"plane": {"origin": [6.873863542433758, -3.9686269665968896, 9], "xDir": [0.5000000000000004, 0.8660254037844384, -2.7755575615628914e-17], "zDir": [0.5728219618694799, -0.3307189138830741, 0.75]}, "obj": undefined});
        atStep(st, 358);
        cqRect(c, st, 358, 357, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 359);
        cqExtrude(c, st, 359, 358, {"until": -12.201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 359, "extrude");
        atStep(st, 360);
        cqWorkplaneNew(c, st, 360, {"plane": {"origin": [6.873863542433758, -3.9686269665968896, 9], "xDir": [0.5000000000000004, 0.8660254037844384, -2.7755575615628914e-17], "zDir": [0.5728219618694799, -0.3307189138830741, 0.75]}, "obj": undefined});
        atStep(st, 361);
        cqRect(c, st, 361, 360, {"xLen": 1.2, "yLen": 1.7, "centered": true, "forConstruction": false});
        atStep(st, 362);
        cqExtrude(c, st, 362, 361, {"until": 0.05, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        atStep(st, 363);
        cqWorkplaneNew(c, st, 363, {"plane": {"origin": [6.873863542433758, -3.9686269665968896, 9], "xDir": [0.5000000000000004, 0.8660254037844384, -2.7755575615628914e-17], "zDir": [0.5728219618694799, -0.3307189138830741, 0.75]}, "obj": undefined});
        atStep(st, 364);
        cqRect(c, st, 364, 363, {"xLen": 1, "yLen": 1.5, "centered": true, "forConstruction": false});
        atStep(st, 365);
        cqExtrude(c, st, 365, 364, {"until": 0.051, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 365, "extrude");
        atStep(st, 366);
        cqCut(c, st, 366, 362, {"toCut": {"wp": 365}, "clean": true, "tol": undefined});
        stepVol(c, st, 366, "cut");
        atStep(st, 367);
        cqCut(c, st, 367, 6, {"toCut": {"wp": 9}, "clean": true, "tol": undefined});
        stepVol(c, st, 367, "cut");
        atStep(st, 368);
        cqCut(c, st, 368, 367, {"toCut": {"wp": 19}, "clean": true, "tol": undefined});
        stepVol(c, st, 368, "cut");
        atStep(st, 369);
        cqCut(c, st, 369, 368, {"toCut": {"wp": 29}, "clean": true, "tol": undefined});
        stepVol(c, st, 369, "cut");
        atStep(st, 370);
        cqCut(c, st, 370, 369, {"toCut": {"wp": 39}, "clean": true, "tol": undefined});
        stepVol(c, st, 370, "cut");
        atStep(st, 371);
        cqCut(c, st, 371, 370, {"toCut": {"wp": 49}, "clean": true, "tol": undefined});
        stepVol(c, st, 371, "cut");
        atStep(st, 372);
        cqCut(c, st, 372, 371, {"toCut": {"wp": 59}, "clean": true, "tol": undefined});
        stepVol(c, st, 372, "cut");
        atStep(st, 373);
        cqCut(c, st, 373, 372, {"toCut": {"wp": 69}, "clean": true, "tol": undefined});
        stepVol(c, st, 373, "cut");
        atStep(st, 374);
        cqCut(c, st, 374, 373, {"toCut": {"wp": 79}, "clean": true, "tol": undefined});
        stepVol(c, st, 374, "cut");
        atStep(st, 375);
        cqCut(c, st, 375, 374, {"toCut": {"wp": 89}, "clean": true, "tol": undefined});
        stepVol(c, st, 375, "cut");
        atStep(st, 376);
        cqCut(c, st, 376, 375, {"toCut": {"wp": 99}, "clean": true, "tol": undefined});
        stepVol(c, st, 376, "cut");
        atStep(st, 377);
        cqCut(c, st, 377, 376, {"toCut": {"wp": 109}, "clean": true, "tol": undefined});
        stepVol(c, st, 377, "cut");
        atStep(st, 378);
        cqCut(c, st, 378, 377, {"toCut": {"wp": 119}, "clean": true, "tol": undefined});
        stepVol(c, st, 378, "cut");
        atStep(st, 379);
        cqCut(c, st, 379, 378, {"toCut": {"wp": 129}, "clean": true, "tol": undefined});
        stepVol(c, st, 379, "cut");
        atStep(st, 380);
        cqCut(c, st, 380, 379, {"toCut": {"wp": 139}, "clean": true, "tol": undefined});
        stepVol(c, st, 380, "cut");
        atStep(st, 381);
        cqCut(c, st, 381, 380, {"toCut": {"wp": 149}, "clean": true, "tol": undefined});
        stepVol(c, st, 381, "cut");
        atStep(st, 382);
        cqCut(c, st, 382, 381, {"toCut": {"wp": 159}, "clean": true, "tol": undefined});
        stepVol(c, st, 382, "cut");
        atStep(st, 383);
        cqCut(c, st, 383, 382, {"toCut": {"wp": 169}, "clean": true, "tol": undefined});
        stepVol(c, st, 383, "cut");
        atStep(st, 384);
        cqCut(c, st, 384, 383, {"toCut": {"wp": 179}, "clean": true, "tol": undefined});
        stepVol(c, st, 384, "cut");
        atStep(st, 385);
        cqCut(c, st, 385, 384, {"toCut": {"wp": 189}, "clean": true, "tol": undefined});
        stepVol(c, st, 385, "cut");
        atStep(st, 386);
        cqCut(c, st, 386, 385, {"toCut": {"wp": 199}, "clean": true, "tol": undefined});
        stepVol(c, st, 386, "cut");
        atStep(st, 387);
        cqCut(c, st, 387, 386, {"toCut": {"wp": 209}, "clean": true, "tol": undefined});
        stepVol(c, st, 387, "cut");
        atStep(st, 388);
        cqCut(c, st, 388, 387, {"toCut": {"wp": 219}, "clean": true, "tol": undefined});
        stepVol(c, st, 388, "cut");
        atStep(st, 389);
        cqCut(c, st, 389, 388, {"toCut": {"wp": 229}, "clean": true, "tol": undefined});
        stepVol(c, st, 389, "cut");
        atStep(st, 390);
        cqCut(c, st, 390, 389, {"toCut": {"wp": 239}, "clean": true, "tol": undefined});
        stepVol(c, st, 390, "cut");
        atStep(st, 391);
        cqCut(c, st, 391, 390, {"toCut": {"wp": 249}, "clean": true, "tol": undefined});
        stepVol(c, st, 391, "cut");
        atStep(st, 392);
        cqCut(c, st, 392, 391, {"toCut": {"wp": 259}, "clean": true, "tol": undefined});
        stepVol(c, st, 392, "cut");
        atStep(st, 393);
        cqCut(c, st, 393, 392, {"toCut": {"wp": 269}, "clean": true, "tol": undefined});
        stepVol(c, st, 393, "cut");
        atStep(st, 394);
        cqCut(c, st, 394, 393, {"toCut": {"wp": 279}, "clean": true, "tol": undefined});
        stepVol(c, st, 394, "cut");
        atStep(st, 395);
        cqCut(c, st, 395, 394, {"toCut": {"wp": 289}, "clean": true, "tol": undefined});
        stepVol(c, st, 395, "cut");
        atStep(st, 396);
        cqCut(c, st, 396, 395, {"toCut": {"wp": 299}, "clean": true, "tol": undefined});
        stepVol(c, st, 396, "cut");
        atStep(st, 397);
        cqCut(c, st, 397, 396, {"toCut": {"wp": 309}, "clean": true, "tol": undefined});
        stepVol(c, st, 397, "cut");
        atStep(st, 398);
        cqCut(c, st, 398, 397, {"toCut": {"wp": 319}, "clean": true, "tol": undefined});
        stepVol(c, st, 398, "cut");
        atStep(st, 399);
        cqCut(c, st, 399, 398, {"toCut": {"wp": 329}, "clean": true, "tol": undefined});
        stepVol(c, st, 399, "cut");
        atStep(st, 400);
        cqCut(c, st, 400, 399, {"toCut": {"wp": 339}, "clean": true, "tol": undefined});
        stepVol(c, st, 400, "cut");
        atStep(st, 401);
        cqCut(c, st, 401, 400, {"toCut": {"wp": 349}, "clean": true, "tol": undefined});
        stepVol(c, st, 401, "cut");
        atStep(st, 402);
        cqCut(c, st, 402, 401, {"toCut": {"wp": 359}, "clean": true, "tol": undefined});
        stepVol(c, st, 402, "cut");
        atStep(st, 403);
        cqUnion(c, st, 403, 402, {"toUnion": {"wp": 16}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 403, "union");
        atStep(st, 404);
        cqUnion(c, st, 404, 403, {"toUnion": {"wp": 26}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 404, "union");
        atStep(st, 405);
        cqUnion(c, st, 405, 404, {"toUnion": {"wp": 36}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 405, "union");
        atStep(st, 406);
        cqUnion(c, st, 406, 405, {"toUnion": {"wp": 46}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 406, "union");
        atStep(st, 407);
        cqUnion(c, st, 407, 406, {"toUnion": {"wp": 56}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 407, "union");
        atStep(st, 408);
        cqUnion(c, st, 408, 407, {"toUnion": {"wp": 66}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 408, "union");
        atStep(st, 409);
        cqUnion(c, st, 409, 408, {"toUnion": {"wp": 76}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 409, "union");
        atStep(st, 410);
        cqUnion(c, st, 410, 409, {"toUnion": {"wp": 86}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 410, "union");
        atStep(st, 411);
        cqUnion(c, st, 411, 410, {"toUnion": {"wp": 96}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 411, "union");
        atStep(st, 412);
        cqUnion(c, st, 412, 411, {"toUnion": {"wp": 106}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 412, "union");
        atStep(st, 413);
        cqUnion(c, st, 413, 412, {"toUnion": {"wp": 116}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 413, "union");
        atStep(st, 414);
        cqUnion(c, st, 414, 413, {"toUnion": {"wp": 126}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 414, "union");
        atStep(st, 415);
        cqUnion(c, st, 415, 414, {"toUnion": {"wp": 136}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 415, "union");
        atStep(st, 416);
        cqUnion(c, st, 416, 415, {"toUnion": {"wp": 146}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        atStep(st, 417);
        cqUnion(c, st, 417, 416, {"toUnion": {"wp": 156}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 417, "union");
        atStep(st, 418);
        cqUnion(c, st, 418, 417, {"toUnion": {"wp": 166}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 418, "union");
        atStep(st, 419);
        cqUnion(c, st, 419, 418, {"toUnion": {"wp": 176}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 419, "union");
        atStep(st, 420);
        cqUnion(c, st, 420, 419, {"toUnion": {"wp": 186}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 420, "union");
        atStep(st, 421);
        cqUnion(c, st, 421, 420, {"toUnion": {"wp": 196}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 421, "union");
        atStep(st, 422);
        cqUnion(c, st, 422, 421, {"toUnion": {"wp": 206}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 422, "union");
        atStep(st, 423);
        cqUnion(c, st, 423, 422, {"toUnion": {"wp": 216}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 423, "union");
        atStep(st, 424);
        cqUnion(c, st, 424, 423, {"toUnion": {"wp": 226}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 424, "union");
        atStep(st, 425);
        cqUnion(c, st, 425, 424, {"toUnion": {"wp": 236}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 425, "union");
        atStep(st, 426);
        cqUnion(c, st, 426, 425, {"toUnion": {"wp": 246}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 426, "union");
        atStep(st, 427);
        cqUnion(c, st, 427, 426, {"toUnion": {"wp": 256}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 427, "union");
        atStep(st, 428);
        cqUnion(c, st, 428, 427, {"toUnion": {"wp": 266}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 428, "union");
        atStep(st, 429);
        cqUnion(c, st, 429, 428, {"toUnion": {"wp": 276}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 429, "union");
        atStep(st, 430);
        cqUnion(c, st, 430, 429, {"toUnion": {"wp": 286}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 430, "union");
        atStep(st, 431);
        cqUnion(c, st, 431, 430, {"toUnion": {"wp": 296}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 431, "union");
        atStep(st, 432);
        cqUnion(c, st, 432, 431, {"toUnion": {"wp": 306}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        atStep(st, 433);
        cqUnion(c, st, 433, 432, {"toUnion": {"wp": 316}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 433, "union");
        atStep(st, 434);
        cqUnion(c, st, 434, 433, {"toUnion": {"wp": 326}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 434, "union");
        atStep(st, 435);
        cqUnion(c, st, 435, 434, {"toUnion": {"wp": 336}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 435, "union");
        atStep(st, 436);
        cqUnion(c, st, 436, 435, {"toUnion": {"wp": 346}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 436, "union");
        atStep(st, 437);
        cqUnion(c, st, 437, 436, {"toUnion": {"wp": 356}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 437, "union");
        atStep(st, 438);
        cqUnion(c, st, 438, 437, {"toUnion": {"wp": 366}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 438, "union");
        atStep(st, 439);
        cqWorkplaneNew(c, st, 439, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 440);
        cqSphere(c, st, 440, 439, {"radius": 12.2, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 440, "sphere");
        atStep(st, 441);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 442);
        cqSphere(c, st, 442, 441, {"radius": 12, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 442, "sphere");
        atStep(st, 443);
        cqCut(c, st, 443, 440, {"toCut": {"wp": 442}, "clean": true, "tol": undefined});
        stepVol(c, st, 443, "cut");
        atStep(st, 444);
        cqWorkplaneNew(c, st, 444, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 445);
        cqTransformed(c, st, 445, 444, {"rotate": [0, 0, 0], "offset": [0, 0, -12.2]});
        atStep(st, 446);
        cqBox(c, st, 446, 445, {"length": 24.4, "width": 24.4, "height": 12.2, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 446, "box");
        atStep(st, 447);
        cqCut(c, st, 447, 443, {"toCut": {"wp": 446}, "clean": true, "tol": undefined});
        stepVol(c, st, 447, "cut");
        atStep(st, 448);
        cqWorkplaneNew(c, st, 448, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 449);
        cqCircle(c, st, 449, 448, {"radius": 11.86895003862225, "forConstruction": false});
        atStep(st, 450);
        cqExtrude(c, st, 450, 449, {"until": 12.2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 450, "extrude");
        atStep(st, 451);
        cqWorkplaneNew(c, st, 451, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 452);
        cqCircle(c, st, 452, 451, {"radius": 11.36895003862225, "forConstruction": false});
        atStep(st, 453);
        cqExtrude(c, st, 453, 452, {"until": 12.2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 453, "extrude");
        atStep(st, 454);
        cqCut(c, st, 454, 450, {"toCut": {"wp": 453}, "clean": true, "tol": undefined});
        stepVol(c, st, 454, "cut");
        atStep(st, 455);
        cqIntersect(c, st, 455, 454, {"toIntersect": {"wp": 447}, "clean": true, "tol": undefined});
        stepVol(c, st, 455, "intersect");
        atStep(st, 456);
        cqWorkplaneNew(c, st, 456, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 457);
        cqCircle(c, st, 457, 456, {"radius": 10.642304845413264, "forConstruction": false});
        atStep(st, 458);
        cqExtrude(c, st, 458, 457, {"until": 12.2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 458, "extrude");
        atStep(st, 459);
        cqWorkplaneNew(c, st, 459, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 460);
        cqCircle(c, st, 460, 459, {"radius": 10.142304845413264, "forConstruction": false});
        atStep(st, 461);
        cqExtrude(c, st, 461, 460, {"until": 12.2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 461, "extrude");
        atStep(st, 462);
        cqCut(c, st, 462, 458, {"toCut": {"wp": 461}, "clean": true, "tol": undefined});
        stepVol(c, st, 462, "cut");
        atStep(st, 463);
        cqIntersect(c, st, 463, 462, {"toIntersect": {"wp": 447}, "clean": true, "tol": undefined});
        stepVol(c, st, 463, "intersect");
        atStep(st, 464);
        cqWorkplaneNew(c, st, 464, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 465);
        cqCircle(c, st, 465, 464, {"radius": 8.187253933193773, "forConstruction": false});
        atStep(st, 466);
        cqExtrude(c, st, 466, 465, {"until": 12.2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 466, "extrude");
        atStep(st, 467);
        cqWorkplaneNew(c, st, 467, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 468);
        cqCircle(c, st, 468, 467, {"radius": 7.687253933193772, "forConstruction": false});
        atStep(st, 469);
        cqExtrude(c, st, 469, 468, {"until": 12.2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 469, "extrude");
        atStep(st, 470);
        cqCut(c, st, 470, 466, {"toCut": {"wp": 469}, "clean": true, "tol": undefined});
        stepVol(c, st, 470, "cut");
        atStep(st, 471);
        cqIntersect(c, st, 471, 470, {"toIntersect": {"wp": 447}, "clean": true, "tol": undefined});
        stepVol(c, st, 471, "intersect");
        atStep(st, 472);
        cqUnion(c, st, 472, 438, {"toUnion": {"wp": 455}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 472, "union");
        atStep(st, 473);
        cqUnion(c, st, 473, 472, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 473, "union");
        atStep(st, 474);
        cqUnion(c, st, 474, 473, {"toUnion": {"wp": 471}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 474, "union");
        atStep(st, 475);
        cqWorkplaneNew(c, st, 475, {"plane": {"origin": [12, 0, 0], "xDir": [0, 1, 0], "zDir": [1, 0, 0]}, "obj": undefined});
        atStep(st, 476);
        cqRect(c, st, 476, 475, {"xLen": 0.5, "yLen": 0.5, "centered": true, "forConstruction": false});
        atStep(st, 477);
        cqExtrude(c, st, 477, 476, {"until": 0.3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 477, "extrude");
        atStep(st, 478);
        cqWorkplaneNew(c, st, 478, {"plane": {"origin": [6.000000000000002, 10.392304845413264, 0], "xDir": [-0.8660254037844387, 0.5000000000000001, 0], "zDir": [0.5000000000000001, 0.8660254037844387, 0]}, "obj": undefined});
        atStep(st, 479);
        cqRect(c, st, 479, 478, {"xLen": 0.5, "yLen": 0.5, "centered": true, "forConstruction": false});
        atStep(st, 480);
        cqExtrude(c, st, 480, 479, {"until": 0.3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 480, "extrude");
        atStep(st, 481);
        cqWorkplaneNew(c, st, 481, {"plane": {"origin": [-5.999999999999997, 10.392304845413264, 0], "xDir": [-0.8660254037844388, -0.49999999999999983, 0], "zDir": [-0.49999999999999983, 0.8660254037844388, 0]}, "obj": undefined});
        atStep(st, 482);
        cqRect(c, st, 482, 481, {"xLen": 0.5, "yLen": 0.5, "centered": true, "forConstruction": false});
        atStep(st, 483);
        cqExtrude(c, st, 483, 482, {"until": 0.3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 483, "extrude");
        atStep(st, 484);
        cqWorkplaneNew(c, st, 484, {"plane": {"origin": [-12, 1.469576158976824e-15, 0], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-1, 1.2246467991473532e-16, 0]}, "obj": undefined});
        atStep(st, 485);
        cqRect(c, st, 485, 484, {"xLen": 0.5, "yLen": 0.5, "centered": true, "forConstruction": false});
        atStep(st, 486);
        cqExtrude(c, st, 486, 485, {"until": 0.3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 486, "extrude");
        atStep(st, 487);
        cqWorkplaneNew(c, st, 487, {"plane": {"origin": [-6.000000000000005, -10.39230484541326, 0], "xDir": [0.8660254037844384, -0.5000000000000004, 0], "zDir": [-0.5000000000000004, -0.8660254037844384, 0]}, "obj": undefined});
        atStep(st, 488);
        cqRect(c, st, 488, 487, {"xLen": 0.5, "yLen": 0.5, "centered": true, "forConstruction": false});
        atStep(st, 489);
        cqExtrude(c, st, 489, 488, {"until": 0.3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 489, "extrude");
        atStep(st, 490);
        cqWorkplaneNew(c, st, 490, {"plane": {"origin": [6.000000000000002, -10.392304845413264, 0], "xDir": [0.8660254037844387, 0.5000000000000001, 0], "zDir": [0.5000000000000001, -0.8660254037844387, 0]}, "obj": undefined});
        atStep(st, 491);
        cqRect(c, st, 491, 490, {"xLen": 0.5, "yLen": 0.5, "centered": true, "forConstruction": false});
        atStep(st, 492);
        cqExtrude(c, st, 492, 491, {"until": 0.3, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 492, "extrude");
        atStep(st, 493);
        cqUnion(c, st, 493, 474, {"toUnion": {"wp": 477}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 493, "union");
        atStep(st, 494);
        cqUnion(c, st, 494, 493, {"toUnion": {"wp": 480}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 494, "union");
        atStep(st, 495);
        cqUnion(c, st, 495, 494, {"toUnion": {"wp": 483}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 495, "union");
        atStep(st, 496);
        cqUnion(c, st, 496, 495, {"toUnion": {"wp": 486}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 496, "union");
        atStep(st, 497);
        cqUnion(c, st, 497, 496, {"toUnion": {"wp": 489}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 497, "union");
        atStep(st, 498);
        cqUnion(c, st, 498, 497, {"toUnion": {"wp": 492}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 498, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 498}), {"i": 3999});
    } catch (e) {
        return {"i": 3999, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_3999(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c3999"] = r; }
    return out;
}
