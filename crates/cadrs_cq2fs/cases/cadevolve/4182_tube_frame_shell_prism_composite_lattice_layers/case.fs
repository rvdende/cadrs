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

const case_4182 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqCircle(c, st, 2, 1, {"radius": 10, "forConstruction": false});
        atStep(st, 3);
        cqExtrude(c, st, 3, 2, {"until": 30, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 3, "extrude");
        atStep(st, 4);
        cqSelect("Faces")(c, st, 4, 3, {"selector": {"a": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "b": {"k": "nth", "key": "center", "max": false, "n": -1, "parallel": false, "v": [0, 0, 1]}, "k": "or"}, "tag": undefined});
        atStep(st, 5);
        cqShell(c, st, 5, 4, {"thickness": -2, "kind": "arc"});
        stepVol(c, st, 5, "shell");
        atStep(st, 6);
        cqWorkplaneNew(c, st, 6, {"plane": {"origin": [9, 0, 5], "xDir": [0, 1, 0], "zDir": [-1, 0, 0]}, "obj": undefined});
        atStep(st, 7);
        cqRect(c, st, 7, 6, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 8);
        cqExtrude(c, st, 8, 7, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 8, "extrude");
        atStep(st, 9);
        cqWorkplaneNew(c, st, 9, {"plane": {"origin": [7.64646806564575, 4.74673847194679, 5], "xDir": [-0.5274153857718655, 0.8496075628495278, 0], "zDir": [-0.8496075628495278, -0.5274153857718655, 0]}, "obj": undefined});
        atStep(st, 10);
        cqRect(c, st, 10, 9, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 11);
        cqExtrude(c, st, 11, 10, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 11, "extrude");
        atStep(st, 12);
        cqUnion(c, st, 12, 8, {"toUnion": {"wp": 11}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 12, "union");
        atStep(st, 13);
        cqWorkplaneNew(c, st, 13, {"plane": {"origin": [3.992994195320057, 8.065729809269607, 5], "xDir": [-0.8961922010299563, 0.4436660217022285, 0], "zDir": [-0.4436660217022285, -0.8961922010299563, 0]}, "obj": undefined});
        atStep(st, 14);
        cqRect(c, st, 14, 13, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 15);
        cqExtrude(c, st, 15, 14, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 15, "extrude");
        atStep(st, 16);
        cqUnion(c, st, 16, 12, {"toUnion": {"wp": 15}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 16, "union");
        atStep(st, 17);
        cqWorkplaneNew(c, st, 17, {"plane": {"origin": [-0.8615119321293809, 8.958671619765884, 5], "xDir": [-0.995407957751765, -0.09572354801437569, 0], "zDir": [0.09572354801437569, -0.995407957751765, 0]}, "obj": undefined});
        atStep(st, 18);
        cqRect(c, st, 18, 17, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 19);
        cqExtrude(c, st, 19, 18, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 19, "extrude");
        atStep(st, 20);
        cqUnion(c, st, 20, 16, {"toUnion": {"wp": 19}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 20, "union");
        atStep(st, 21);
        cqWorkplaneNew(c, st, 21, {"plane": {"origin": [-5.456888301364518, 7.156980513207442, 5], "xDir": [-0.7952200570230491, -0.6063209223738354, 0], "zDir": [0.6063209223738354, -0.7952200570230491, 0]}, "obj": undefined});
        atStep(st, 22);
        cqRect(c, st, 22, 21, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 23);
        cqExtrude(c, st, 23, 22, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 23, "extrude");
        atStep(st, 24);
        cqUnion(c, st, 24, 20, {"toUnion": {"wp": 23}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 24, "union");
        atStep(st, 25);
        cqWorkplaneNew(c, st, 25, {"plane": {"origin": [-8.410915208799434, 3.202577922609591, 5], "xDir": [-0.3558419914010657, -0.9345461343110484, 0], "zDir": [0.9345461343110484, -0.3558419914010657, 0]}, "obj": undefined});
        atStep(st, 26);
        cqRect(c, st, 26, 25, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 27);
        cqExtrude(c, st, 27, 26, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 27, "extrude");
        atStep(st, 28);
        cqUnion(c, st, 28, 24, {"toUnion": {"wp": 27}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 28, "union");
        atStep(st, 29);
        cqWorkplaneNew(c, st, 29, {"plane": {"origin": [-8.835066042399712, -1.7151116658793684, 5], "xDir": [0.1905679628754854, -0.981674004711079, 0], "zDir": [0.981674004711079, 0.1905679628754854, 0]}, "obj": undefined});
        atStep(st, 30);
        cqRect(c, st, 30, 29, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 31);
        cqExtrude(c, st, 31, 30, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 31, "extrude");
        atStep(st, 32);
        cqUnion(c, st, 32, 28, {"toUnion": {"wp": 31}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 32, "union");
        atStep(st, 33);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [-6.6017626469962485, -6.1169216075347155, 5], "xDir": [0.6796579563927462, -0.7335291829995831, 0], "zDir": [0.7335291829995831, 0.6796579563927462, 0]}, "obj": undefined});
        atStep(st, 34);
        cqRect(c, st, 34, 33, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 35);
        cqExtrude(c, st, 35, 34, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 35, "extrude");
        atStep(st, 36);
        cqUnion(c, st, 36, 32, {"toUnion": {"wp": 35}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 36, "union");
        atStep(st, 37);
        cqWorkplaneNew(c, st, 37, {"plane": {"origin": [-2.382748903651346, -8.678854052359004, 5], "xDir": [0.9643171169287782, -0.2647498781834829, 0], "zDir": [0.2647498781834829, 0.9643171169287782, 0]}, "obj": undefined});
        atStep(st, 38);
        cqRect(c, st, 38, 37, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 39);
        cqExtrude(c, st, 39, 38, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 39, "extrude");
        atStep(st, 40);
        cqUnion(c, st, 40, 36, {"toUnion": {"wp": 39}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 40, "union");
        atStep(st, 41);
        cqWorkplaneNew(c, st, 41, {"plane": {"origin": [2.552959669169036, -8.630318471968247, 5], "xDir": [0.9589242746631386, 0.2836621854632263, 0], "zDir": [-0.2836621854632263, 0.9589242746631386, 0]}, "obj": undefined});
        atStep(st, 42);
        cqRect(c, st, 42, 41, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 43);
        cqExtrude(c, st, 43, 42, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 43, "extrude");
        atStep(st, 44);
        cqUnion(c, st, 44, 40, {"toUnion": {"wp": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 44, "union");
        atStep(st, 45);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [6.72077658880303, -5.9859136348094015, 5], "xDir": [0.6651015149788224, 0.7467529543114478, 0], "zDir": [-0.7467529543114478, 0.6651015149788224, 0]}, "obj": undefined});
        atStep(st, 46);
        cqRect(c, st, 46, 45, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 47);
        cqExtrude(c, st, 47, 46, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 47, "extrude");
        atStep(st, 48);
        cqUnion(c, st, 48, 44, {"toUnion": {"wp": 47}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 48, "union");
        atStep(st, 49);
        cqWorkplaneNew(c, st, 49, {"plane": {"origin": [8.867085566969175, -1.541036517428103, 5], "xDir": [0.1712262797142337, 0.9852317296632416, 0], "zDir": [-0.9852317296632416, 0.1712262797142337, 0]}, "obj": undefined});
        atStep(st, 50);
        cqRect(c, st, 50, 49, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 51);
        cqExtrude(c, st, 51, 50, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 51, "extrude");
        atStep(st, 52);
        cqUnion(c, st, 52, 48, {"toUnion": {"wp": 51}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 52, "union");
        atStep(st, 53);
        cqWorkplaneNew(c, st, 53, {"plane": {"origin": [9, 0, 10], "xDir": [0, 1, 0], "zDir": [-1, 0, 0]}, "obj": undefined});
        atStep(st, 54);
        cqRect(c, st, 54, 53, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 55);
        cqExtrude(c, st, 55, 54, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 55, "extrude");
        atStep(st, 56);
        cqUnion(c, st, 56, 52, {"toUnion": {"wp": 55}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 56, "union");
        atStep(st, 57);
        cqWorkplaneNew(c, st, 57, {"plane": {"origin": [7.64646806564575, 4.74673847194679, 10], "xDir": [-0.5274153857718655, 0.8496075628495278, 0], "zDir": [-0.8496075628495278, -0.5274153857718655, 0]}, "obj": undefined});
        atStep(st, 58);
        cqRect(c, st, 58, 57, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 59);
        cqExtrude(c, st, 59, 58, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 59, "extrude");
        atStep(st, 60);
        cqUnion(c, st, 60, 56, {"toUnion": {"wp": 59}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 60, "union");
        atStep(st, 61);
        cqWorkplaneNew(c, st, 61, {"plane": {"origin": [3.992994195320057, 8.065729809269607, 10], "xDir": [-0.8961922010299563, 0.4436660217022285, 0], "zDir": [-0.4436660217022285, -0.8961922010299563, 0]}, "obj": undefined});
        atStep(st, 62);
        cqRect(c, st, 62, 61, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 63);
        cqExtrude(c, st, 63, 62, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 63, "extrude");
        atStep(st, 64);
        cqUnion(c, st, 64, 60, {"toUnion": {"wp": 63}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 64, "union");
        atStep(st, 65);
        cqWorkplaneNew(c, st, 65, {"plane": {"origin": [-0.8615119321293809, 8.958671619765884, 10], "xDir": [-0.995407957751765, -0.09572354801437569, 0], "zDir": [0.09572354801437569, -0.995407957751765, 0]}, "obj": undefined});
        atStep(st, 66);
        cqRect(c, st, 66, 65, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 67);
        cqExtrude(c, st, 67, 66, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 67, "extrude");
        atStep(st, 68);
        cqUnion(c, st, 68, 64, {"toUnion": {"wp": 67}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 68, "union");
        atStep(st, 69);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [-5.456888301364518, 7.156980513207442, 10], "xDir": [-0.7952200570230491, -0.6063209223738354, 0], "zDir": [0.6063209223738354, -0.7952200570230491, 0]}, "obj": undefined});
        atStep(st, 70);
        cqRect(c, st, 70, 69, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 71);
        cqExtrude(c, st, 71, 70, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 71, "extrude");
        atStep(st, 72);
        cqUnion(c, st, 72, 68, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [-8.410915208799434, 3.202577922609591, 10], "xDir": [-0.3558419914010657, -0.9345461343110484, 0], "zDir": [0.9345461343110484, -0.3558419914010657, 0]}, "obj": undefined});
        atStep(st, 74);
        cqRect(c, st, 74, 73, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 75);
        cqExtrude(c, st, 75, 74, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 75, "extrude");
        atStep(st, 76);
        cqUnion(c, st, 76, 72, {"toUnion": {"wp": 75}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 76, "union");
        atStep(st, 77);
        cqWorkplaneNew(c, st, 77, {"plane": {"origin": [-8.835066042399712, -1.7151116658793684, 10], "xDir": [0.1905679628754854, -0.981674004711079, 0], "zDir": [0.981674004711079, 0.1905679628754854, 0]}, "obj": undefined});
        atStep(st, 78);
        cqRect(c, st, 78, 77, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 79);
        cqExtrude(c, st, 79, 78, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 79, "extrude");
        atStep(st, 80);
        cqUnion(c, st, 80, 76, {"toUnion": {"wp": 79}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 80, "union");
        atStep(st, 81);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [-6.6017626469962485, -6.1169216075347155, 10], "xDir": [0.6796579563927462, -0.7335291829995831, 0], "zDir": [0.7335291829995831, 0.6796579563927462, 0]}, "obj": undefined});
        atStep(st, 82);
        cqRect(c, st, 82, 81, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 83);
        cqExtrude(c, st, 83, 82, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 83, "extrude");
        atStep(st, 84);
        cqUnion(c, st, 84, 80, {"toUnion": {"wp": 83}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 84, "union");
        atStep(st, 85);
        cqWorkplaneNew(c, st, 85, {"plane": {"origin": [-2.382748903651346, -8.678854052359004, 10], "xDir": [0.9643171169287782, -0.2647498781834829, 0], "zDir": [0.2647498781834829, 0.9643171169287782, 0]}, "obj": undefined});
        atStep(st, 86);
        cqRect(c, st, 86, 85, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 87);
        cqExtrude(c, st, 87, 86, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 87, "extrude");
        atStep(st, 88);
        cqUnion(c, st, 88, 84, {"toUnion": {"wp": 87}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 88, "union");
        atStep(st, 89);
        cqWorkplaneNew(c, st, 89, {"plane": {"origin": [2.552959669169036, -8.630318471968247, 10], "xDir": [0.9589242746631386, 0.2836621854632263, 0], "zDir": [-0.2836621854632263, 0.9589242746631386, 0]}, "obj": undefined});
        atStep(st, 90);
        cqRect(c, st, 90, 89, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 91);
        cqExtrude(c, st, 91, 90, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 91, "extrude");
        atStep(st, 92);
        cqUnion(c, st, 92, 88, {"toUnion": {"wp": 91}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 92, "union");
        atStep(st, 93);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [6.72077658880303, -5.9859136348094015, 10], "xDir": [0.6651015149788224, 0.7467529543114478, 0], "zDir": [-0.7467529543114478, 0.6651015149788224, 0]}, "obj": undefined});
        atStep(st, 94);
        cqRect(c, st, 94, 93, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 95);
        cqExtrude(c, st, 95, 94, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 95, "extrude");
        atStep(st, 96);
        cqUnion(c, st, 96, 92, {"toUnion": {"wp": 95}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 96, "union");
        atStep(st, 97);
        cqWorkplaneNew(c, st, 97, {"plane": {"origin": [8.867085566969175, -1.541036517428103, 10], "xDir": [0.1712262797142337, 0.9852317296632416, 0], "zDir": [-0.9852317296632416, 0.1712262797142337, 0]}, "obj": undefined});
        atStep(st, 98);
        cqRect(c, st, 98, 97, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 99);
        cqExtrude(c, st, 99, 98, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 99, "extrude");
        atStep(st, 100);
        cqUnion(c, st, 100, 96, {"toUnion": {"wp": 99}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 100, "union");
        atStep(st, 101);
        cqWorkplaneNew(c, st, 101, {"plane": {"origin": [9, 0, 15], "xDir": [0, 1, 0], "zDir": [-1, 0, 0]}, "obj": undefined});
        atStep(st, 102);
        cqRect(c, st, 102, 101, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 103);
        cqExtrude(c, st, 103, 102, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 103, "extrude");
        atStep(st, 104);
        cqUnion(c, st, 104, 100, {"toUnion": {"wp": 103}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 104, "union");
        atStep(st, 105);
        cqWorkplaneNew(c, st, 105, {"plane": {"origin": [7.64646806564575, 4.74673847194679, 15], "xDir": [-0.5274153857718655, 0.8496075628495278, 0], "zDir": [-0.8496075628495278, -0.5274153857718655, 0]}, "obj": undefined});
        atStep(st, 106);
        cqRect(c, st, 106, 105, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 107);
        cqExtrude(c, st, 107, 106, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 107, "extrude");
        atStep(st, 108);
        cqUnion(c, st, 108, 104, {"toUnion": {"wp": 107}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 108, "union");
        atStep(st, 109);
        cqWorkplaneNew(c, st, 109, {"plane": {"origin": [3.992994195320057, 8.065729809269607, 15], "xDir": [-0.8961922010299563, 0.4436660217022285, 0], "zDir": [-0.4436660217022285, -0.8961922010299563, 0]}, "obj": undefined});
        atStep(st, 110);
        cqRect(c, st, 110, 109, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 111);
        cqExtrude(c, st, 111, 110, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 111, "extrude");
        atStep(st, 112);
        cqUnion(c, st, 112, 108, {"toUnion": {"wp": 111}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 112, "union");
        atStep(st, 113);
        cqWorkplaneNew(c, st, 113, {"plane": {"origin": [-0.8615119321293809, 8.958671619765884, 15], "xDir": [-0.995407957751765, -0.09572354801437569, 0], "zDir": [0.09572354801437569, -0.995407957751765, 0]}, "obj": undefined});
        atStep(st, 114);
        cqRect(c, st, 114, 113, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 115);
        cqExtrude(c, st, 115, 114, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 115, "extrude");
        atStep(st, 116);
        cqUnion(c, st, 116, 112, {"toUnion": {"wp": 115}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 116, "union");
        atStep(st, 117);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [-5.456888301364518, 7.156980513207442, 15], "xDir": [-0.7952200570230491, -0.6063209223738354, 0], "zDir": [0.6063209223738354, -0.7952200570230491, 0]}, "obj": undefined});
        atStep(st, 118);
        cqRect(c, st, 118, 117, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 119);
        cqExtrude(c, st, 119, 118, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 119, "extrude");
        atStep(st, 120);
        cqUnion(c, st, 120, 116, {"toUnion": {"wp": 119}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 120, "union");
        atStep(st, 121);
        cqWorkplaneNew(c, st, 121, {"plane": {"origin": [-8.410915208799434, 3.202577922609591, 15], "xDir": [-0.3558419914010657, -0.9345461343110484, 0], "zDir": [0.9345461343110484, -0.3558419914010657, 0]}, "obj": undefined});
        atStep(st, 122);
        cqRect(c, st, 122, 121, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 123);
        cqExtrude(c, st, 123, 122, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 123, "extrude");
        atStep(st, 124);
        cqUnion(c, st, 124, 120, {"toUnion": {"wp": 123}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 124, "union");
        atStep(st, 125);
        cqWorkplaneNew(c, st, 125, {"plane": {"origin": [-8.835066042399712, -1.7151116658793684, 15], "xDir": [0.1905679628754854, -0.981674004711079, 0], "zDir": [0.981674004711079, 0.1905679628754854, 0]}, "obj": undefined});
        atStep(st, 126);
        cqRect(c, st, 126, 125, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 127);
        cqExtrude(c, st, 127, 126, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 127, "extrude");
        atStep(st, 128);
        cqUnion(c, st, 128, 124, {"toUnion": {"wp": 127}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 128, "union");
        atStep(st, 129);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [-6.6017626469962485, -6.1169216075347155, 15], "xDir": [0.6796579563927462, -0.7335291829995831, 0], "zDir": [0.7335291829995831, 0.6796579563927462, 0]}, "obj": undefined});
        atStep(st, 130);
        cqRect(c, st, 130, 129, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 131);
        cqExtrude(c, st, 131, 130, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 131, "extrude");
        atStep(st, 132);
        cqUnion(c, st, 132, 128, {"toUnion": {"wp": 131}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 132, "union");
        atStep(st, 133);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [-2.382748903651346, -8.678854052359004, 15], "xDir": [0.9643171169287782, -0.2647498781834829, 0], "zDir": [0.2647498781834829, 0.9643171169287782, 0]}, "obj": undefined});
        atStep(st, 134);
        cqRect(c, st, 134, 133, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 135);
        cqExtrude(c, st, 135, 134, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 135, "extrude");
        atStep(st, 136);
        cqUnion(c, st, 136, 132, {"toUnion": {"wp": 135}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 136, "union");
        atStep(st, 137);
        cqWorkplaneNew(c, st, 137, {"plane": {"origin": [2.552959669169036, -8.630318471968247, 15], "xDir": [0.9589242746631386, 0.2836621854632263, 0], "zDir": [-0.2836621854632263, 0.9589242746631386, 0]}, "obj": undefined});
        atStep(st, 138);
        cqRect(c, st, 138, 137, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 139);
        cqExtrude(c, st, 139, 138, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 139, "extrude");
        atStep(st, 140);
        cqUnion(c, st, 140, 136, {"toUnion": {"wp": 139}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 140, "union");
        atStep(st, 141);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [6.72077658880303, -5.9859136348094015, 15], "xDir": [0.6651015149788224, 0.7467529543114478, 0], "zDir": [-0.7467529543114478, 0.6651015149788224, 0]}, "obj": undefined});
        atStep(st, 142);
        cqRect(c, st, 142, 141, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 143);
        cqExtrude(c, st, 143, 142, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 143, "extrude");
        atStep(st, 144);
        cqUnion(c, st, 144, 140, {"toUnion": {"wp": 143}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 144, "union");
        atStep(st, 145);
        cqWorkplaneNew(c, st, 145, {"plane": {"origin": [8.867085566969175, -1.541036517428103, 15], "xDir": [0.1712262797142337, 0.9852317296632416, 0], "zDir": [-0.9852317296632416, 0.1712262797142337, 0]}, "obj": undefined});
        atStep(st, 146);
        cqRect(c, st, 146, 145, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 147);
        cqExtrude(c, st, 147, 146, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 147, "extrude");
        atStep(st, 148);
        cqUnion(c, st, 148, 144, {"toUnion": {"wp": 147}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 148, "union");
        atStep(st, 149);
        cqWorkplaneNew(c, st, 149, {"plane": {"origin": [9, 0, 20], "xDir": [0, 1, 0], "zDir": [-1, 0, 0]}, "obj": undefined});
        atStep(st, 150);
        cqRect(c, st, 150, 149, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 151);
        cqExtrude(c, st, 151, 150, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 151, "extrude");
        atStep(st, 152);
        cqUnion(c, st, 152, 148, {"toUnion": {"wp": 151}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 152, "union");
        atStep(st, 153);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [7.64646806564575, 4.74673847194679, 20], "xDir": [-0.5274153857718655, 0.8496075628495278, 0], "zDir": [-0.8496075628495278, -0.5274153857718655, 0]}, "obj": undefined});
        atStep(st, 154);
        cqRect(c, st, 154, 153, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 155);
        cqExtrude(c, st, 155, 154, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 155, "extrude");
        atStep(st, 156);
        cqUnion(c, st, 156, 152, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        atStep(st, 157);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [3.992994195320057, 8.065729809269607, 20], "xDir": [-0.8961922010299563, 0.4436660217022285, 0], "zDir": [-0.4436660217022285, -0.8961922010299563, 0]}, "obj": undefined});
        atStep(st, 158);
        cqRect(c, st, 158, 157, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 159);
        cqExtrude(c, st, 159, 158, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 159, "extrude");
        atStep(st, 160);
        cqUnion(c, st, 160, 156, {"toUnion": {"wp": 159}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 160, "union");
        atStep(st, 161);
        cqWorkplaneNew(c, st, 161, {"plane": {"origin": [-0.8615119321293809, 8.958671619765884, 20], "xDir": [-0.995407957751765, -0.09572354801437569, 0], "zDir": [0.09572354801437569, -0.995407957751765, 0]}, "obj": undefined});
        atStep(st, 162);
        cqRect(c, st, 162, 161, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 163);
        cqExtrude(c, st, 163, 162, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 163, "extrude");
        atStep(st, 164);
        cqUnion(c, st, 164, 160, {"toUnion": {"wp": 163}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 164, "union");
        atStep(st, 165);
        cqWorkplaneNew(c, st, 165, {"plane": {"origin": [-5.456888301364518, 7.156980513207442, 20], "xDir": [-0.7952200570230491, -0.6063209223738354, 0], "zDir": [0.6063209223738354, -0.7952200570230491, 0]}, "obj": undefined});
        atStep(st, 166);
        cqRect(c, st, 166, 165, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 167);
        cqExtrude(c, st, 167, 166, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 167, "extrude");
        atStep(st, 168);
        cqUnion(c, st, 168, 164, {"toUnion": {"wp": 167}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 168, "union");
        atStep(st, 169);
        cqWorkplaneNew(c, st, 169, {"plane": {"origin": [-8.410915208799434, 3.202577922609591, 20], "xDir": [-0.3558419914010657, -0.9345461343110484, 0], "zDir": [0.9345461343110484, -0.3558419914010657, 0]}, "obj": undefined});
        atStep(st, 170);
        cqRect(c, st, 170, 169, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 171);
        cqExtrude(c, st, 171, 170, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 171, "extrude");
        atStep(st, 172);
        cqUnion(c, st, 172, 168, {"toUnion": {"wp": 171}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 172, "union");
        atStep(st, 173);
        cqWorkplaneNew(c, st, 173, {"plane": {"origin": [-8.835066042399712, -1.7151116658793684, 20], "xDir": [0.1905679628754854, -0.981674004711079, 0], "zDir": [0.981674004711079, 0.1905679628754854, 0]}, "obj": undefined});
        atStep(st, 174);
        cqRect(c, st, 174, 173, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 175);
        cqExtrude(c, st, 175, 174, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 175, "extrude");
        atStep(st, 176);
        cqUnion(c, st, 176, 172, {"toUnion": {"wp": 175}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 176, "union");
        atStep(st, 177);
        cqWorkplaneNew(c, st, 177, {"plane": {"origin": [-6.6017626469962485, -6.1169216075347155, 20], "xDir": [0.6796579563927462, -0.7335291829995831, 0], "zDir": [0.7335291829995831, 0.6796579563927462, 0]}, "obj": undefined});
        atStep(st, 178);
        cqRect(c, st, 178, 177, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 179);
        cqExtrude(c, st, 179, 178, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 179, "extrude");
        atStep(st, 180);
        cqUnion(c, st, 180, 176, {"toUnion": {"wp": 179}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 180, "union");
        atStep(st, 181);
        cqWorkplaneNew(c, st, 181, {"plane": {"origin": [-2.382748903651346, -8.678854052359004, 20], "xDir": [0.9643171169287782, -0.2647498781834829, 0], "zDir": [0.2647498781834829, 0.9643171169287782, 0]}, "obj": undefined});
        atStep(st, 182);
        cqRect(c, st, 182, 181, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 183);
        cqExtrude(c, st, 183, 182, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 183, "extrude");
        atStep(st, 184);
        cqUnion(c, st, 184, 180, {"toUnion": {"wp": 183}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 184, "union");
        atStep(st, 185);
        cqWorkplaneNew(c, st, 185, {"plane": {"origin": [2.552959669169036, -8.630318471968247, 20], "xDir": [0.9589242746631386, 0.2836621854632263, 0], "zDir": [-0.2836621854632263, 0.9589242746631386, 0]}, "obj": undefined});
        atStep(st, 186);
        cqRect(c, st, 186, 185, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 187);
        cqExtrude(c, st, 187, 186, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 187, "extrude");
        atStep(st, 188);
        cqUnion(c, st, 188, 184, {"toUnion": {"wp": 187}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 188, "union");
        atStep(st, 189);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [6.72077658880303, -5.9859136348094015, 20], "xDir": [0.6651015149788224, 0.7467529543114478, 0], "zDir": [-0.7467529543114478, 0.6651015149788224, 0]}, "obj": undefined});
        atStep(st, 190);
        cqRect(c, st, 190, 189, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 191);
        cqExtrude(c, st, 191, 190, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 191, "extrude");
        atStep(st, 192);
        cqUnion(c, st, 192, 188, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        atStep(st, 193);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [8.867085566969175, -1.541036517428103, 20], "xDir": [0.1712262797142337, 0.9852317296632416, 0], "zDir": [-0.9852317296632416, 0.1712262797142337, 0]}, "obj": undefined});
        atStep(st, 194);
        cqRect(c, st, 194, 193, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 195);
        cqExtrude(c, st, 195, 194, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 195, "extrude");
        atStep(st, 196);
        cqUnion(c, st, 196, 192, {"toUnion": {"wp": 195}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 196, "union");
        atStep(st, 197);
        cqWorkplaneNew(c, st, 197, {"plane": {"origin": [9, 0, 25], "xDir": [0, 1, 0], "zDir": [-1, 0, 0]}, "obj": undefined});
        atStep(st, 198);
        cqRect(c, st, 198, 197, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 199);
        cqExtrude(c, st, 199, 198, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 199, "extrude");
        atStep(st, 200);
        cqUnion(c, st, 200, 196, {"toUnion": {"wp": 199}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 200, "union");
        atStep(st, 201);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [7.64646806564575, 4.74673847194679, 25], "xDir": [-0.5274153857718655, 0.8496075628495278, 0], "zDir": [-0.8496075628495278, -0.5274153857718655, 0]}, "obj": undefined});
        atStep(st, 202);
        cqRect(c, st, 202, 201, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 203);
        cqExtrude(c, st, 203, 202, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 203, "extrude");
        atStep(st, 204);
        cqUnion(c, st, 204, 200, {"toUnion": {"wp": 203}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 204, "union");
        atStep(st, 205);
        cqWorkplaneNew(c, st, 205, {"plane": {"origin": [3.992994195320057, 8.065729809269607, 25], "xDir": [-0.8961922010299563, 0.4436660217022285, 0], "zDir": [-0.4436660217022285, -0.8961922010299563, 0]}, "obj": undefined});
        atStep(st, 206);
        cqRect(c, st, 206, 205, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 207);
        cqExtrude(c, st, 207, 206, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 207, "extrude");
        atStep(st, 208);
        cqUnion(c, st, 208, 204, {"toUnion": {"wp": 207}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 208, "union");
        atStep(st, 209);
        cqWorkplaneNew(c, st, 209, {"plane": {"origin": [-0.8615119321293809, 8.958671619765884, 25], "xDir": [-0.995407957751765, -0.09572354801437569, 0], "zDir": [0.09572354801437569, -0.995407957751765, 0]}, "obj": undefined});
        atStep(st, 210);
        cqRect(c, st, 210, 209, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 211);
        cqExtrude(c, st, 211, 210, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 211, "extrude");
        atStep(st, 212);
        cqUnion(c, st, 212, 208, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [-5.456888301364518, 7.156980513207442, 25], "xDir": [-0.7952200570230491, -0.6063209223738354, 0], "zDir": [0.6063209223738354, -0.7952200570230491, 0]}, "obj": undefined});
        atStep(st, 214);
        cqRect(c, st, 214, 213, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 215);
        cqExtrude(c, st, 215, 214, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 215, "extrude");
        atStep(st, 216);
        cqUnion(c, st, 216, 212, {"toUnion": {"wp": 215}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 216, "union");
        atStep(st, 217);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [-8.410915208799434, 3.202577922609591, 25], "xDir": [-0.3558419914010657, -0.9345461343110484, 0], "zDir": [0.9345461343110484, -0.3558419914010657, 0]}, "obj": undefined});
        atStep(st, 218);
        cqRect(c, st, 218, 217, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 219);
        cqExtrude(c, st, 219, 218, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 219, "extrude");
        atStep(st, 220);
        cqUnion(c, st, 220, 216, {"toUnion": {"wp": 219}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 220, "union");
        atStep(st, 221);
        cqWorkplaneNew(c, st, 221, {"plane": {"origin": [-8.835066042399712, -1.7151116658793684, 25], "xDir": [0.1905679628754854, -0.981674004711079, 0], "zDir": [0.981674004711079, 0.1905679628754854, 0]}, "obj": undefined});
        atStep(st, 222);
        cqRect(c, st, 222, 221, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 223);
        cqExtrude(c, st, 223, 222, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 223, "extrude");
        atStep(st, 224);
        cqUnion(c, st, 224, 220, {"toUnion": {"wp": 223}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 224, "union");
        atStep(st, 225);
        cqWorkplaneNew(c, st, 225, {"plane": {"origin": [-6.6017626469962485, -6.1169216075347155, 25], "xDir": [0.6796579563927462, -0.7335291829995831, 0], "zDir": [0.7335291829995831, 0.6796579563927462, 0]}, "obj": undefined});
        atStep(st, 226);
        cqRect(c, st, 226, 225, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 227);
        cqExtrude(c, st, 227, 226, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 227, "extrude");
        atStep(st, 228);
        cqUnion(c, st, 228, 224, {"toUnion": {"wp": 227}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 228, "union");
        atStep(st, 229);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [-2.382748903651346, -8.678854052359004, 25], "xDir": [0.9643171169287782, -0.2647498781834829, 0], "zDir": [0.2647498781834829, 0.9643171169287782, 0]}, "obj": undefined});
        atStep(st, 230);
        cqRect(c, st, 230, 229, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 231);
        cqExtrude(c, st, 231, 230, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 231, "extrude");
        atStep(st, 232);
        cqUnion(c, st, 232, 228, {"toUnion": {"wp": 231}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 232, "union");
        atStep(st, 233);
        cqWorkplaneNew(c, st, 233, {"plane": {"origin": [2.552959669169036, -8.630318471968247, 25], "xDir": [0.9589242746631386, 0.2836621854632263, 0], "zDir": [-0.2836621854632263, 0.9589242746631386, 0]}, "obj": undefined});
        atStep(st, 234);
        cqRect(c, st, 234, 233, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 235);
        cqExtrude(c, st, 235, 234, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 235, "extrude");
        atStep(st, 236);
        cqUnion(c, st, 236, 232, {"toUnion": {"wp": 235}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 236, "union");
        atStep(st, 237);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [6.72077658880303, -5.9859136348094015, 25], "xDir": [0.6651015149788224, 0.7467529543114478, 0], "zDir": [-0.7467529543114478, 0.6651015149788224, 0]}, "obj": undefined});
        atStep(st, 238);
        cqRect(c, st, 238, 237, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 239);
        cqExtrude(c, st, 239, 238, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 239, "extrude");
        atStep(st, 240);
        cqUnion(c, st, 240, 236, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        atStep(st, 241);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [8.867085566969175, -1.541036517428103, 25], "xDir": [0.1712262797142337, 0.9852317296632416, 0], "zDir": [-0.9852317296632416, 0.1712262797142337, 0]}, "obj": undefined});
        atStep(st, 242);
        cqRect(c, st, 242, 241, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 243);
        cqExtrude(c, st, 243, 242, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 243, "extrude");
        atStep(st, 244);
        cqUnion(c, st, 244, 240, {"toUnion": {"wp": 243}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 244, "union");
        atStep(st, 245);
        cqWorkplaneNew(c, st, 245, {"plane": {"origin": [7.794228634059948, 4.499999999999999, 5], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 246);
        cqRect(c, st, 246, 245, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 247);
        cqExtrude(c, st, 247, 246, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 247, "extrude");
        atStep(st, 248);
        cqWorkplaneNew(c, st, 248, {"plane": {"origin": [4.24866635810228, 7.934030134649723, 5], "xDir": [-0.8815589038499693, 0.4720740397891423, 0], "zDir": [-0.4720740397891423, -0.8815589038499693, 0]}, "obj": undefined});
        atStep(st, 249);
        cqRect(c, st, 249, 248, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 250);
        cqExtrude(c, st, 250, 249, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 250, "extrude");
        atStep(st, 251);
        cqUnion(c, st, 251, 247, {"toUnion": {"wp": 250}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 251, "union");
        atStep(st, 252);
        cqWorkplaneNew(c, st, 252, {"plane": {"origin": [-0.5748304943238305, 8.981624012548924, 5], "xDir": [-0.9979582236165472, -0.06387005492487005, 0], "zDir": [0.06387005492487005, -0.9979582236165472, 0]}, "obj": undefined});
        atStep(st, 253);
        cqRect(c, st, 253, 252, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 254);
        cqExtrude(c, st, 254, 253, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 254, "extrude");
        atStep(st, 255);
        cqUnion(c, st, 255, 251, {"toUnion": {"wp": 254}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 255, "union");
        atStep(st, 256);
        cqWorkplaneNew(c, st, 256, {"plane": {"origin": [-5.225427028770399, 7.327681240815253, 5], "xDir": [-0.8141868045350281, -0.580603003196711, 0], "zDir": [0.580603003196711, -0.8141868045350281, 0]}, "obj": undefined});
        atStep(st, 257);
        cqRect(c, st, 257, 256, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 258);
        cqExtrude(c, st, 258, 257, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 258, "extrude");
        atStep(st, 259);
        cqUnion(c, st, 259, 255, {"toUnion": {"wp": 258}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 259, "union");
        atStep(st, 260);
        cqWorkplaneNew(c, st, 260, {"plane": {"origin": [-8.304294151199507, 3.4696827881455756, 5], "xDir": [-0.3855203097939529, -0.9226993501332786, 0], "zDir": [0.9226993501332786, -0.3855203097939529, 0]}, "obj": undefined});
        atStep(st, 261);
        cqRect(c, st, 261, 260, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 262);
        cqExtrude(c, st, 262, 261, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 262, "extrude");
        atStep(st, 263);
        cqUnion(c, st, 263, 259, {"toUnion": {"wp": 262}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 263, "union");
        atStep(st, 264);
        cqWorkplaneNew(c, st, 264, {"plane": {"origin": [-8.885355201202003, -1.4319437658206158, 5], "xDir": [0.1591048628689573, -0.9872616890224448, 0], "zDir": [0.9872616890224448, 0.1591048628689573, 0]}, "obj": undefined});
        atStep(st, 265);
        cqRect(c, st, 265, 264, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 266);
        cqExtrude(c, st, 266, 265, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 266, "extrude");
        atStep(st, 267);
        cqUnion(c, st, 267, 263, {"toUnion": {"wp": 266}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 267, "union");
        atStep(st, 268);
        cqWorkplaneNew(c, st, 268, {"plane": {"origin": [-6.793835803891709, -5.902863294178436, 5], "xDir": [0.6558736993531595, -0.7548706448768566, 0], "zDir": [0.7548706448768566, 0.6558736993531595, 0]}, "obj": undefined});
        atStep(st, 269);
        cqRect(c, st, 269, 268, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 270);
        cqExtrude(c, st, 270, 269, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 270, "extrude");
        atStep(st, 271);
        cqUnion(c, st, 271, 267, {"toUnion": {"wp": 270}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 271, "union");
        atStep(st, 272);
        cqWorkplaneNew(c, st, 272, {"plane": {"origin": [-2.658833358286594, -8.598290828581133, 5], "xDir": [0.955365647620126, -0.29542592869851053, 0], "zDir": [0.29542592869851053, 0.955365647620126, 0]}, "obj": undefined});
        atStep(st, 273);
        cqRect(c, st, 273, 272, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 274);
        cqExtrude(c, st, 274, 273, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 274, "extrude");
        atStep(st, 275);
        cqUnion(c, st, 275, 271, {"toUnion": {"wp": 274}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 275, "union");
        atStep(st, 276);
        cqWorkplaneNew(c, st, 276, {"plane": {"origin": [2.2759059447779113, -8.707482536906092, 5], "xDir": [0.9674980596562324, 0.2528784383086568, 0], "zDir": [-0.2528784383086568, 0.9674980596562324, 0]}, "obj": undefined});
        atStep(st, 277);
        cqRect(c, st, 277, 276, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 278);
        cqExtrude(c, st, 278, 277, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 278, "extrude");
        atStep(st, 279);
        cqUnion(c, st, 279, 275, {"toUnion": {"wp": 278}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 279, "union");
        atStep(st, 280);
        cqWorkplaneNew(c, st, 280, {"plane": {"origin": [6.526087164321621, -6.197595204890086, 5], "xDir": [0.6886216894322318, 0.7251207960357356, 0], "zDir": [-0.7251207960357356, 0.6886216894322318, 0]}, "obj": undefined});
        atStep(st, 281);
        cqRect(c, st, 281, 280, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 282);
        cqExtrude(c, st, 282, 281, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 282, "extrude");
        atStep(st, 283);
        cqUnion(c, st, 283, 279, {"toUnion": {"wp": 282}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 283, "union");
        atStep(st, 284);
        cqWorkplaneNew(c, st, 284, {"plane": {"origin": [8.813320076467848, -1.823564978203072, 5], "xDir": [0.20261833091145243, 0.9792577862742052, 0], "zDir": [-0.9792577862742052, 0.20261833091145243, 0]}, "obj": undefined});
        atStep(st, 285);
        cqRect(c, st, 285, 284, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 286);
        cqExtrude(c, st, 286, 285, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 286, "extrude");
        atStep(st, 287);
        cqUnion(c, st, 287, 283, {"toUnion": {"wp": 286}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 287, "union");
        atStep(st, 288);
        cqWorkplaneNew(c, st, 288, {"plane": {"origin": [7.794228634059948, 4.499999999999999, 10], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 289);
        cqRect(c, st, 289, 288, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 290);
        cqExtrude(c, st, 290, 289, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 290, "extrude");
        atStep(st, 291);
        cqUnion(c, st, 291, 287, {"toUnion": {"wp": 290}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 291, "union");
        atStep(st, 292);
        cqWorkplaneNew(c, st, 292, {"plane": {"origin": [4.24866635810228, 7.934030134649723, 10], "xDir": [-0.8815589038499693, 0.4720740397891423, 0], "zDir": [-0.4720740397891423, -0.8815589038499693, 0]}, "obj": undefined});
        atStep(st, 293);
        cqRect(c, st, 293, 292, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 294);
        cqExtrude(c, st, 294, 293, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 294, "extrude");
        atStep(st, 295);
        cqUnion(c, st, 295, 291, {"toUnion": {"wp": 294}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 295, "union");
        atStep(st, 296);
        cqWorkplaneNew(c, st, 296, {"plane": {"origin": [-0.5748304943238305, 8.981624012548924, 10], "xDir": [-0.9979582236165472, -0.06387005492487005, 0], "zDir": [0.06387005492487005, -0.9979582236165472, 0]}, "obj": undefined});
        atStep(st, 297);
        cqRect(c, st, 297, 296, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 298);
        cqExtrude(c, st, 298, 297, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 298, "extrude");
        atStep(st, 299);
        cqUnion(c, st, 299, 295, {"toUnion": {"wp": 298}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 299, "union");
        atStep(st, 300);
        cqWorkplaneNew(c, st, 300, {"plane": {"origin": [-5.225427028770399, 7.327681240815253, 10], "xDir": [-0.8141868045350281, -0.580603003196711, 0], "zDir": [0.580603003196711, -0.8141868045350281, 0]}, "obj": undefined});
        atStep(st, 301);
        cqRect(c, st, 301, 300, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 302);
        cqExtrude(c, st, 302, 301, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 302, "extrude");
        atStep(st, 303);
        cqUnion(c, st, 303, 299, {"toUnion": {"wp": 302}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 303, "union");
        atStep(st, 304);
        cqWorkplaneNew(c, st, 304, {"plane": {"origin": [-8.304294151199507, 3.4696827881455756, 10], "xDir": [-0.3855203097939529, -0.9226993501332786, 0], "zDir": [0.9226993501332786, -0.3855203097939529, 0]}, "obj": undefined});
        atStep(st, 305);
        cqRect(c, st, 305, 304, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 306);
        cqExtrude(c, st, 306, 305, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 306, "extrude");
        atStep(st, 307);
        cqUnion(c, st, 307, 303, {"toUnion": {"wp": 306}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 307, "union");
        atStep(st, 308);
        cqWorkplaneNew(c, st, 308, {"plane": {"origin": [-8.885355201202003, -1.4319437658206158, 10], "xDir": [0.1591048628689573, -0.9872616890224448, 0], "zDir": [0.9872616890224448, 0.1591048628689573, 0]}, "obj": undefined});
        atStep(st, 309);
        cqRect(c, st, 309, 308, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 310);
        cqExtrude(c, st, 310, 309, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 310, "extrude");
        atStep(st, 311);
        cqUnion(c, st, 311, 307, {"toUnion": {"wp": 310}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 311, "union");
        atStep(st, 312);
        cqWorkplaneNew(c, st, 312, {"plane": {"origin": [-6.793835803891709, -5.902863294178436, 10], "xDir": [0.6558736993531595, -0.7548706448768566, 0], "zDir": [0.7548706448768566, 0.6558736993531595, 0]}, "obj": undefined});
        atStep(st, 313);
        cqRect(c, st, 313, 312, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 314);
        cqExtrude(c, st, 314, 313, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 314, "extrude");
        atStep(st, 315);
        cqUnion(c, st, 315, 311, {"toUnion": {"wp": 314}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 315, "union");
        atStep(st, 316);
        cqWorkplaneNew(c, st, 316, {"plane": {"origin": [-2.658833358286594, -8.598290828581133, 10], "xDir": [0.955365647620126, -0.29542592869851053, 0], "zDir": [0.29542592869851053, 0.955365647620126, 0]}, "obj": undefined});
        atStep(st, 317);
        cqRect(c, st, 317, 316, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 318);
        cqExtrude(c, st, 318, 317, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 318, "extrude");
        atStep(st, 319);
        cqUnion(c, st, 319, 315, {"toUnion": {"wp": 318}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 319, "union");
        atStep(st, 320);
        cqWorkplaneNew(c, st, 320, {"plane": {"origin": [2.2759059447779113, -8.707482536906092, 10], "xDir": [0.9674980596562324, 0.2528784383086568, 0], "zDir": [-0.2528784383086568, 0.9674980596562324, 0]}, "obj": undefined});
        atStep(st, 321);
        cqRect(c, st, 321, 320, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 322);
        cqExtrude(c, st, 322, 321, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 322, "extrude");
        atStep(st, 323);
        cqUnion(c, st, 323, 319, {"toUnion": {"wp": 322}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 323, "union");
        atStep(st, 324);
        cqWorkplaneNew(c, st, 324, {"plane": {"origin": [6.526087164321621, -6.197595204890086, 10], "xDir": [0.6886216894322318, 0.7251207960357356, 0], "zDir": [-0.7251207960357356, 0.6886216894322318, 0]}, "obj": undefined});
        atStep(st, 325);
        cqRect(c, st, 325, 324, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 326);
        cqExtrude(c, st, 326, 325, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 326, "extrude");
        atStep(st, 327);
        cqUnion(c, st, 327, 323, {"toUnion": {"wp": 326}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 327, "union");
        atStep(st, 328);
        cqWorkplaneNew(c, st, 328, {"plane": {"origin": [8.813320076467848, -1.823564978203072, 10], "xDir": [0.20261833091145243, 0.9792577862742052, 0], "zDir": [-0.9792577862742052, 0.20261833091145243, 0]}, "obj": undefined});
        atStep(st, 329);
        cqRect(c, st, 329, 328, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 330);
        cqExtrude(c, st, 330, 329, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 330, "extrude");
        atStep(st, 331);
        cqUnion(c, st, 331, 327, {"toUnion": {"wp": 330}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 331, "union");
        atStep(st, 332);
        cqWorkplaneNew(c, st, 332, {"plane": {"origin": [7.794228634059948, 4.499999999999999, 15], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 333);
        cqRect(c, st, 333, 332, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 334);
        cqExtrude(c, st, 334, 333, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 334, "extrude");
        atStep(st, 335);
        cqUnion(c, st, 335, 331, {"toUnion": {"wp": 334}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 335, "union");
        atStep(st, 336);
        cqWorkplaneNew(c, st, 336, {"plane": {"origin": [4.24866635810228, 7.934030134649723, 15], "xDir": [-0.8815589038499693, 0.4720740397891423, 0], "zDir": [-0.4720740397891423, -0.8815589038499693, 0]}, "obj": undefined});
        atStep(st, 337);
        cqRect(c, st, 337, 336, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 338);
        cqExtrude(c, st, 338, 337, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 338, "extrude");
        atStep(st, 339);
        cqUnion(c, st, 339, 335, {"toUnion": {"wp": 338}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 339, "union");
        atStep(st, 340);
        cqWorkplaneNew(c, st, 340, {"plane": {"origin": [-0.5748304943238305, 8.981624012548924, 15], "xDir": [-0.9979582236165472, -0.06387005492487005, 0], "zDir": [0.06387005492487005, -0.9979582236165472, 0]}, "obj": undefined});
        atStep(st, 341);
        cqRect(c, st, 341, 340, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 342);
        cqExtrude(c, st, 342, 341, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 342, "extrude");
        atStep(st, 343);
        cqUnion(c, st, 343, 339, {"toUnion": {"wp": 342}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 343, "union");
        atStep(st, 344);
        cqWorkplaneNew(c, st, 344, {"plane": {"origin": [-5.225427028770399, 7.327681240815253, 15], "xDir": [-0.8141868045350281, -0.580603003196711, 0], "zDir": [0.580603003196711, -0.8141868045350281, 0]}, "obj": undefined});
        atStep(st, 345);
        cqRect(c, st, 345, 344, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 346);
        cqExtrude(c, st, 346, 345, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 346, "extrude");
        atStep(st, 347);
        cqUnion(c, st, 347, 343, {"toUnion": {"wp": 346}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 347, "union");
        atStep(st, 348);
        cqWorkplaneNew(c, st, 348, {"plane": {"origin": [-8.304294151199507, 3.4696827881455756, 15], "xDir": [-0.3855203097939529, -0.9226993501332786, 0], "zDir": [0.9226993501332786, -0.3855203097939529, 0]}, "obj": undefined});
        atStep(st, 349);
        cqRect(c, st, 349, 348, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 350);
        cqExtrude(c, st, 350, 349, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 350, "extrude");
        atStep(st, 351);
        cqUnion(c, st, 351, 347, {"toUnion": {"wp": 350}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 351, "union");
        atStep(st, 352);
        cqWorkplaneNew(c, st, 352, {"plane": {"origin": [-8.885355201202003, -1.4319437658206158, 15], "xDir": [0.1591048628689573, -0.9872616890224448, 0], "zDir": [0.9872616890224448, 0.1591048628689573, 0]}, "obj": undefined});
        atStep(st, 353);
        cqRect(c, st, 353, 352, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 354);
        cqExtrude(c, st, 354, 353, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 354, "extrude");
        atStep(st, 355);
        cqUnion(c, st, 355, 351, {"toUnion": {"wp": 354}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 355, "union");
        atStep(st, 356);
        cqWorkplaneNew(c, st, 356, {"plane": {"origin": [-6.793835803891709, -5.902863294178436, 15], "xDir": [0.6558736993531595, -0.7548706448768566, 0], "zDir": [0.7548706448768566, 0.6558736993531595, 0]}, "obj": undefined});
        atStep(st, 357);
        cqRect(c, st, 357, 356, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 358);
        cqExtrude(c, st, 358, 357, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 358, "extrude");
        atStep(st, 359);
        cqUnion(c, st, 359, 355, {"toUnion": {"wp": 358}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 359, "union");
        atStep(st, 360);
        cqWorkplaneNew(c, st, 360, {"plane": {"origin": [-2.658833358286594, -8.598290828581133, 15], "xDir": [0.955365647620126, -0.29542592869851053, 0], "zDir": [0.29542592869851053, 0.955365647620126, 0]}, "obj": undefined});
        atStep(st, 361);
        cqRect(c, st, 361, 360, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 362);
        cqExtrude(c, st, 362, 361, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 362, "extrude");
        atStep(st, 363);
        cqUnion(c, st, 363, 359, {"toUnion": {"wp": 362}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 363, "union");
        atStep(st, 364);
        cqWorkplaneNew(c, st, 364, {"plane": {"origin": [2.2759059447779113, -8.707482536906092, 15], "xDir": [0.9674980596562324, 0.2528784383086568, 0], "zDir": [-0.2528784383086568, 0.9674980596562324, 0]}, "obj": undefined});
        atStep(st, 365);
        cqRect(c, st, 365, 364, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 366);
        cqExtrude(c, st, 366, 365, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 366, "extrude");
        atStep(st, 367);
        cqUnion(c, st, 367, 363, {"toUnion": {"wp": 366}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 367, "union");
        atStep(st, 368);
        cqWorkplaneNew(c, st, 368, {"plane": {"origin": [6.526087164321621, -6.197595204890086, 15], "xDir": [0.6886216894322318, 0.7251207960357356, 0], "zDir": [-0.7251207960357356, 0.6886216894322318, 0]}, "obj": undefined});
        atStep(st, 369);
        cqRect(c, st, 369, 368, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 370);
        cqExtrude(c, st, 370, 369, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 370, "extrude");
        atStep(st, 371);
        cqUnion(c, st, 371, 367, {"toUnion": {"wp": 370}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 371, "union");
        atStep(st, 372);
        cqWorkplaneNew(c, st, 372, {"plane": {"origin": [8.813320076467848, -1.823564978203072, 15], "xDir": [0.20261833091145243, 0.9792577862742052, 0], "zDir": [-0.9792577862742052, 0.20261833091145243, 0]}, "obj": undefined});
        atStep(st, 373);
        cqRect(c, st, 373, 372, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 374);
        cqExtrude(c, st, 374, 373, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 374, "extrude");
        atStep(st, 375);
        cqUnion(c, st, 375, 371, {"toUnion": {"wp": 374}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 375, "union");
        atStep(st, 376);
        cqWorkplaneNew(c, st, 376, {"plane": {"origin": [7.794228634059948, 4.499999999999999, 20], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 377);
        cqRect(c, st, 377, 376, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 378);
        cqExtrude(c, st, 378, 377, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 378, "extrude");
        atStep(st, 379);
        cqUnion(c, st, 379, 375, {"toUnion": {"wp": 378}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 379, "union");
        atStep(st, 380);
        cqWorkplaneNew(c, st, 380, {"plane": {"origin": [4.24866635810228, 7.934030134649723, 20], "xDir": [-0.8815589038499693, 0.4720740397891423, 0], "zDir": [-0.4720740397891423, -0.8815589038499693, 0]}, "obj": undefined});
        atStep(st, 381);
        cqRect(c, st, 381, 380, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 382);
        cqExtrude(c, st, 382, 381, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 382, "extrude");
        atStep(st, 383);
        cqUnion(c, st, 383, 379, {"toUnion": {"wp": 382}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 383, "union");
        atStep(st, 384);
        cqWorkplaneNew(c, st, 384, {"plane": {"origin": [-0.5748304943238305, 8.981624012548924, 20], "xDir": [-0.9979582236165472, -0.06387005492487005, 0], "zDir": [0.06387005492487005, -0.9979582236165472, 0]}, "obj": undefined});
        atStep(st, 385);
        cqRect(c, st, 385, 384, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 386);
        cqExtrude(c, st, 386, 385, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 386, "extrude");
        atStep(st, 387);
        cqUnion(c, st, 387, 383, {"toUnion": {"wp": 386}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 387, "union");
        atStep(st, 388);
        cqWorkplaneNew(c, st, 388, {"plane": {"origin": [-5.225427028770399, 7.327681240815253, 20], "xDir": [-0.8141868045350281, -0.580603003196711, 0], "zDir": [0.580603003196711, -0.8141868045350281, 0]}, "obj": undefined});
        atStep(st, 389);
        cqRect(c, st, 389, 388, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 390);
        cqExtrude(c, st, 390, 389, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 390, "extrude");
        atStep(st, 391);
        cqUnion(c, st, 391, 387, {"toUnion": {"wp": 390}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 391, "union");
        atStep(st, 392);
        cqWorkplaneNew(c, st, 392, {"plane": {"origin": [-8.304294151199507, 3.4696827881455756, 20], "xDir": [-0.3855203097939529, -0.9226993501332786, 0], "zDir": [0.9226993501332786, -0.3855203097939529, 0]}, "obj": undefined});
        atStep(st, 393);
        cqRect(c, st, 393, 392, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 394);
        cqExtrude(c, st, 394, 393, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 394, "extrude");
        atStep(st, 395);
        cqUnion(c, st, 395, 391, {"toUnion": {"wp": 394}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 395, "union");
        atStep(st, 396);
        cqWorkplaneNew(c, st, 396, {"plane": {"origin": [-8.885355201202003, -1.4319437658206158, 20], "xDir": [0.1591048628689573, -0.9872616890224448, 0], "zDir": [0.9872616890224448, 0.1591048628689573, 0]}, "obj": undefined});
        atStep(st, 397);
        cqRect(c, st, 397, 396, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 398);
        cqExtrude(c, st, 398, 397, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 398, "extrude");
        atStep(st, 399);
        cqUnion(c, st, 399, 395, {"toUnion": {"wp": 398}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 399, "union");
        atStep(st, 400);
        cqWorkplaneNew(c, st, 400, {"plane": {"origin": [-6.793835803891709, -5.902863294178436, 20], "xDir": [0.6558736993531595, -0.7548706448768566, 0], "zDir": [0.7548706448768566, 0.6558736993531595, 0]}, "obj": undefined});
        atStep(st, 401);
        cqRect(c, st, 401, 400, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 402);
        cqExtrude(c, st, 402, 401, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 402, "extrude");
        atStep(st, 403);
        cqUnion(c, st, 403, 399, {"toUnion": {"wp": 402}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 403, "union");
        atStep(st, 404);
        cqWorkplaneNew(c, st, 404, {"plane": {"origin": [-2.658833358286594, -8.598290828581133, 20], "xDir": [0.955365647620126, -0.29542592869851053, 0], "zDir": [0.29542592869851053, 0.955365647620126, 0]}, "obj": undefined});
        atStep(st, 405);
        cqRect(c, st, 405, 404, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 406);
        cqExtrude(c, st, 406, 405, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 406, "extrude");
        atStep(st, 407);
        cqUnion(c, st, 407, 403, {"toUnion": {"wp": 406}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 407, "union");
        atStep(st, 408);
        cqWorkplaneNew(c, st, 408, {"plane": {"origin": [2.2759059447779113, -8.707482536906092, 20], "xDir": [0.9674980596562324, 0.2528784383086568, 0], "zDir": [-0.2528784383086568, 0.9674980596562324, 0]}, "obj": undefined});
        atStep(st, 409);
        cqRect(c, st, 409, 408, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 410);
        cqExtrude(c, st, 410, 409, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 410, "extrude");
        atStep(st, 411);
        cqUnion(c, st, 411, 407, {"toUnion": {"wp": 410}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 411, "union");
        atStep(st, 412);
        cqWorkplaneNew(c, st, 412, {"plane": {"origin": [6.526087164321621, -6.197595204890086, 20], "xDir": [0.6886216894322318, 0.7251207960357356, 0], "zDir": [-0.7251207960357356, 0.6886216894322318, 0]}, "obj": undefined});
        atStep(st, 413);
        cqRect(c, st, 413, 412, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 414);
        cqExtrude(c, st, 414, 413, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 414, "extrude");
        atStep(st, 415);
        cqUnion(c, st, 415, 411, {"toUnion": {"wp": 414}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 415, "union");
        atStep(st, 416);
        cqWorkplaneNew(c, st, 416, {"plane": {"origin": [8.813320076467848, -1.823564978203072, 20], "xDir": [0.20261833091145243, 0.9792577862742052, 0], "zDir": [-0.9792577862742052, 0.20261833091145243, 0]}, "obj": undefined});
        atStep(st, 417);
        cqRect(c, st, 417, 416, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 418);
        cqExtrude(c, st, 418, 417, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 418, "extrude");
        atStep(st, 419);
        cqUnion(c, st, 419, 415, {"toUnion": {"wp": 418}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 419, "union");
        atStep(st, 420);
        cqWorkplaneNew(c, st, 420, {"plane": {"origin": [7.794228634059948, 4.499999999999999, 25], "xDir": [-0.49999999999999994, 0.8660254037844387, 0], "zDir": [-0.8660254037844387, -0.49999999999999994, 0]}, "obj": undefined});
        atStep(st, 421);
        cqRect(c, st, 421, 420, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 422);
        cqExtrude(c, st, 422, 421, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 422, "extrude");
        atStep(st, 423);
        cqUnion(c, st, 423, 419, {"toUnion": {"wp": 422}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 423, "union");
        atStep(st, 424);
        cqWorkplaneNew(c, st, 424, {"plane": {"origin": [4.24866635810228, 7.934030134649723, 25], "xDir": [-0.8815589038499693, 0.4720740397891423, 0], "zDir": [-0.4720740397891423, -0.8815589038499693, 0]}, "obj": undefined});
        atStep(st, 425);
        cqRect(c, st, 425, 424, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 426);
        cqExtrude(c, st, 426, 425, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 427);
        cqUnion(c, st, 427, 423, {"toUnion": {"wp": 426}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 427, "union");
        atStep(st, 428);
        cqWorkplaneNew(c, st, 428, {"plane": {"origin": [-0.5748304943238305, 8.981624012548924, 25], "xDir": [-0.9979582236165472, -0.06387005492487005, 0], "zDir": [0.06387005492487005, -0.9979582236165472, 0]}, "obj": undefined});
        atStep(st, 429);
        cqRect(c, st, 429, 428, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 430);
        cqExtrude(c, st, 430, 429, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 430, "extrude");
        atStep(st, 431);
        cqUnion(c, st, 431, 427, {"toUnion": {"wp": 430}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 431, "union");
        atStep(st, 432);
        cqWorkplaneNew(c, st, 432, {"plane": {"origin": [-5.225427028770399, 7.327681240815253, 25], "xDir": [-0.8141868045350281, -0.580603003196711, 0], "zDir": [0.580603003196711, -0.8141868045350281, 0]}, "obj": undefined});
        atStep(st, 433);
        cqRect(c, st, 433, 432, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 434);
        cqExtrude(c, st, 434, 433, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 434, "extrude");
        atStep(st, 435);
        cqUnion(c, st, 435, 431, {"toUnion": {"wp": 434}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 435, "union");
        atStep(st, 436);
        cqWorkplaneNew(c, st, 436, {"plane": {"origin": [-8.304294151199507, 3.4696827881455756, 25], "xDir": [-0.3855203097939529, -0.9226993501332786, 0], "zDir": [0.9226993501332786, -0.3855203097939529, 0]}, "obj": undefined});
        atStep(st, 437);
        cqRect(c, st, 437, 436, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 438);
        cqExtrude(c, st, 438, 437, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 438, "extrude");
        atStep(st, 439);
        cqUnion(c, st, 439, 435, {"toUnion": {"wp": 438}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 439, "union");
        atStep(st, 440);
        cqWorkplaneNew(c, st, 440, {"plane": {"origin": [-8.885355201202003, -1.4319437658206158, 25], "xDir": [0.1591048628689573, -0.9872616890224448, 0], "zDir": [0.9872616890224448, 0.1591048628689573, 0]}, "obj": undefined});
        atStep(st, 441);
        cqRect(c, st, 441, 440, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 442);
        cqExtrude(c, st, 442, 441, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 442, "extrude");
        atStep(st, 443);
        cqUnion(c, st, 443, 439, {"toUnion": {"wp": 442}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 443, "union");
        atStep(st, 444);
        cqWorkplaneNew(c, st, 444, {"plane": {"origin": [-6.793835803891709, -5.902863294178436, 25], "xDir": [0.6558736993531595, -0.7548706448768566, 0], "zDir": [0.7548706448768566, 0.6558736993531595, 0]}, "obj": undefined});
        atStep(st, 445);
        cqRect(c, st, 445, 444, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 446);
        cqExtrude(c, st, 446, 445, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 446, "extrude");
        atStep(st, 447);
        cqUnion(c, st, 447, 443, {"toUnion": {"wp": 446}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 447, "union");
        atStep(st, 448);
        cqWorkplaneNew(c, st, 448, {"plane": {"origin": [-2.658833358286594, -8.598290828581133, 25], "xDir": [0.955365647620126, -0.29542592869851053, 0], "zDir": [0.29542592869851053, 0.955365647620126, 0]}, "obj": undefined});
        atStep(st, 449);
        cqRect(c, st, 449, 448, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 450);
        cqExtrude(c, st, 450, 449, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 450, "extrude");
        atStep(st, 451);
        cqUnion(c, st, 451, 447, {"toUnion": {"wp": 450}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 451, "union");
        atStep(st, 452);
        cqWorkplaneNew(c, st, 452, {"plane": {"origin": [2.2759059447779113, -8.707482536906092, 25], "xDir": [0.9674980596562324, 0.2528784383086568, 0], "zDir": [-0.2528784383086568, 0.9674980596562324, 0]}, "obj": undefined});
        atStep(st, 453);
        cqRect(c, st, 453, 452, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 454);
        cqExtrude(c, st, 454, 453, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 454, "extrude");
        atStep(st, 455);
        cqUnion(c, st, 455, 451, {"toUnion": {"wp": 454}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 455, "union");
        atStep(st, 456);
        cqWorkplaneNew(c, st, 456, {"plane": {"origin": [6.526087164321621, -6.197595204890086, 25], "xDir": [0.6886216894322318, 0.7251207960357356, 0], "zDir": [-0.7251207960357356, 0.6886216894322318, 0]}, "obj": undefined});
        atStep(st, 457);
        cqRect(c, st, 457, 456, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 458);
        cqExtrude(c, st, 458, 457, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 458, "extrude");
        atStep(st, 459);
        cqUnion(c, st, 459, 455, {"toUnion": {"wp": 458}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 459, "union");
        atStep(st, 460);
        cqWorkplaneNew(c, st, 460, {"plane": {"origin": [8.813320076467848, -1.823564978203072, 25], "xDir": [0.20261833091145243, 0.9792577862742052, 0], "zDir": [-0.9792577862742052, 0.20261833091145243, 0]}, "obj": undefined});
        atStep(st, 461);
        cqRect(c, st, 461, 460, {"xLen": 4, "yLen": 4, "centered": true, "forConstruction": false});
        atStep(st, 462);
        cqExtrude(c, st, 462, 461, {"until": 2.01, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 462, "extrude");
        atStep(st, 463);
        cqUnion(c, st, 463, 459, {"toUnion": {"wp": 462}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 463, "union");
        atStep(st, 464);
        cqUnion(c, st, 464, 244, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 464, "union");
        atStep(st, 465);
        cqCut(c, st, 465, 5, {"toCut": {"wp": 464}, "clean": true, "tol": undefined});
        stepVol(c, st, 465, "cut");
        // step 466: guard_begin {"before":{"wp":465},"block":1,"g":1,"op":"guard_begin","skippable":true}
        atStep(st, 467);
        cqSelect("Edges")(c, st, 466, 465, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        // step 468: injected_failure {"at":468,"op":"injected_failure","was":"fillet"}
        // step 469: guard_caught {"block":1,"g":1,"op":"guard_caught"}
        return mergeMaps(cqMeasure(c, st, {"wp": 465}), {"i": 4182});
    } catch (e) {
        return {"i": 4182, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_4182(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c4182"] = r; }
    return out;
}
