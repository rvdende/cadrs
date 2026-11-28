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

const case_5536 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqSphere(c, st, 2, 1, {"radius": 50, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "sphere");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqRect(c, st, 4, 3, {"xLen": 100, "yLen": 100, "centered": true, "forConstruction": false});
        atStep(st, 5);
        cqExtrude(c, st, 5, 4, {"until": -50, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 5, "extrude");
        atStep(st, 6);
        cqCut(c, st, 6, 2, {"toCut": {"wp": 5}, "clean": true, "tol": undefined});
        stepVol(c, st, 6, "cut");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 8);
        cqWorkplane(c, st, 8, 7, {"offset": 35, "invert": false, "centerOption": "ProjectedOrigin", "origin": undefined});
        atStep(st, 9);
        cqRect(c, st, 9, 8, {"xLen": 100, "yLen": 100, "centered": true, "forConstruction": false});
        atStep(st, 10);
        cqExtrude(c, st, 10, 9, {"until": 50, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 10, "extrude");
        atStep(st, 11);
        cqCut(c, st, 11, 6, {"toCut": {"wp": 10}, "clean": true, "tol": undefined});
        stepVol(c, st, 11, "cut");
        atStep(st, 12);
        cqSelect("Faces")(c, st, 12, 11, {"selector": {"k": "nth", "key": "center", "max": false, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        atStep(st, 13);
        cqSelect("Edges")(c, st, 13, 12, {"selector": undefined, "tag": undefined});
        atStep(st, 14);
        cqFillet(c, st, 14, 13, {"radius": 2});
        stepVol(c, st, 14, "fillet");
        atStep(st, 15);
        cqWorkplaneNew(c, st, 15, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 16);
        cqSphere(c, st, 16, 15, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 16, "sphere");
        atStep(st, 17);
        cqTranslate(c, st, 17, 16, {"vec": [-12.5, -47.63139720814412, 8.660254037844412]});
        stepVol(c, st, 17, "translate");
        atStep(st, 18);
        cqUnion(c, st, 18, 14, {"toUnion": {"wp": 17}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 18, "union");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 20);
        cqSphere(c, st, 20, 19, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 20, "sphere");
        atStep(st, 21);
        cqTranslate(c, st, 21, 20, {"vec": [-7.5, -47.63139720814412, 13.22875655532297]});
        stepVol(c, st, 21, "translate");
        atStep(st, 22);
        cqUnion(c, st, 22, 18, {"toUnion": {"wp": 21}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 22, "union");
        atStep(st, 23);
        cqWorkplaneNew(c, st, 23, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 24);
        cqSphere(c, st, 24, 23, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 24, "sphere");
        atStep(st, 25);
        cqTranslate(c, st, 25, 24, {"vec": [-2.5, -47.63139720814412, 15.000000000000016]});
        stepVol(c, st, 25, "translate");
        atStep(st, 26);
        cqUnion(c, st, 26, 22, {"toUnion": {"wp": 25}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 26, "union");
        atStep(st, 27);
        cqWorkplaneNew(c, st, 27, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 28);
        cqSphere(c, st, 28, 27, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 28, "sphere");
        atStep(st, 29);
        cqTranslate(c, st, 29, 28, {"vec": [2.5, -47.63139720814412, 15.000000000000016]});
        stepVol(c, st, 29, "translate");
        atStep(st, 30);
        cqUnion(c, st, 30, 26, {"toUnion": {"wp": 29}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 30, "union");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 31, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 32);
        cqSphere(c, st, 32, 31, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 32, "sphere");
        atStep(st, 33);
        cqTranslate(c, st, 33, 32, {"vec": [7.5, -47.63139720814412, 13.22875655532297]});
        stepVol(c, st, 33, "translate");
        atStep(st, 34);
        cqUnion(c, st, 34, 30, {"toUnion": {"wp": 33}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 34, "union");
        atStep(st, 35);
        cqWorkplaneNew(c, st, 35, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 36);
        cqSphere(c, st, 36, 35, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 36, "sphere");
        atStep(st, 37);
        cqTranslate(c, st, 37, 36, {"vec": [12.5, -47.63139720814412, 8.660254037844412]});
        stepVol(c, st, 37, "translate");
        atStep(st, 38);
        cqUnion(c, st, 38, 34, {"toUnion": {"wp": 37}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 38, "union");
        atStep(st, 39);
        cqWorkplaneNew(c, st, 39, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 40);
        cqSphere(c, st, 40, 39, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 40, "sphere");
        atStep(st, 41);
        cqTranslate(c, st, 41, 40, {"vec": [-25, -43.30127018922192, 8.259061849445711e-7]});
        stepVol(c, st, 41, "translate");
        atStep(st, 42);
        cqUnion(c, st, 42, 38, {"toUnion": {"wp": 41}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 42, "union");
        atStep(st, 43);
        cqWorkplaneNew(c, st, 43, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 44);
        cqSphere(c, st, 44, 43, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 44, "sphere");
        atStep(st, 45);
        cqTranslate(c, st, 45, 44, {"vec": [-20, -43.30127018922192, 15.000000000000025]});
        stepVol(c, st, 45, "translate");
        atStep(st, 46);
        cqUnion(c, st, 46, 42, {"toUnion": {"wp": 45}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 46, "union");
        atStep(st, 47);
        cqWorkplaneNew(c, st, 47, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 48);
        cqSphere(c, st, 48, 47, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 48, "sphere");
        atStep(st, 49);
        cqTranslate(c, st, 49, 48, {"vec": [-15, -43.30127018922192, 20.000000000000018]});
        stepVol(c, st, 49, "translate");
        atStep(st, 50);
        cqUnion(c, st, 50, 46, {"toUnion": {"wp": 49}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 50, "union");
        atStep(st, 51);
        cqWorkplaneNew(c, st, 51, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 52);
        cqSphere(c, st, 52, 51, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 52, "sphere");
        atStep(st, 53);
        cqTranslate(c, st, 53, 52, {"vec": [-10, -43.30127018922192, 22.912878474779216]});
        stepVol(c, st, 53, "translate");
        atStep(st, 54);
        cqUnion(c, st, 54, 50, {"toUnion": {"wp": 53}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 54, "union");
        atStep(st, 55);
        cqWorkplaneNew(c, st, 55, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 56);
        cqSphere(c, st, 56, 55, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 56, "sphere");
        atStep(st, 57);
        cqTranslate(c, st, 57, 56, {"vec": [-5, -43.30127018922192, 24.494897427831795]});
        stepVol(c, st, 57, "translate");
        atStep(st, 58);
        cqUnion(c, st, 58, 54, {"toUnion": {"wp": 57}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 58, "union");
        atStep(st, 59);
        cqWorkplaneNew(c, st, 59, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 60);
        cqSphere(c, st, 60, 59, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 60, "sphere");
        atStep(st, 61);
        cqTranslate(c, st, 61, 60, {"vec": [0, -43.30127018922192, 25.000000000000018]});
        stepVol(c, st, 61, "translate");
        atStep(st, 62);
        cqUnion(c, st, 62, 58, {"toUnion": {"wp": 61}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 62, "union");
        atStep(st, 63);
        cqWorkplaneNew(c, st, 63, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 64);
        cqSphere(c, st, 64, 63, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 64, "sphere");
        atStep(st, 65);
        cqTranslate(c, st, 65, 64, {"vec": [5, -43.30127018922192, 24.494897427831795]});
        stepVol(c, st, 65, "translate");
        atStep(st, 66);
        cqUnion(c, st, 66, 62, {"toUnion": {"wp": 65}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 66, "union");
        atStep(st, 67);
        cqWorkplaneNew(c, st, 67, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 68);
        cqSphere(c, st, 68, 67, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 68, "sphere");
        atStep(st, 69);
        cqTranslate(c, st, 69, 68, {"vec": [10, -43.30127018922192, 22.912878474779216]});
        stepVol(c, st, 69, "translate");
        atStep(st, 70);
        cqUnion(c, st, 70, 66, {"toUnion": {"wp": 69}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 70, "union");
        atStep(st, 71);
        cqWorkplaneNew(c, st, 71, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 72);
        cqSphere(c, st, 72, 71, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 72, "sphere");
        atStep(st, 73);
        cqTranslate(c, st, 73, 72, {"vec": [15, -43.30127018922192, 20.000000000000018]});
        stepVol(c, st, 73, "translate");
        atStep(st, 74);
        cqUnion(c, st, 74, 70, {"toUnion": {"wp": 73}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 74, "union");
        atStep(st, 75);
        cqWorkplaneNew(c, st, 75, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 76);
        cqSphere(c, st, 76, 75, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 76, "sphere");
        atStep(st, 77);
        cqTranslate(c, st, 77, 76, {"vec": [20, -43.30127018922192, 15.000000000000025]});
        stepVol(c, st, 77, "translate");
        atStep(st, 78);
        cqUnion(c, st, 78, 74, {"toUnion": {"wp": 77}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 78, "union");
        atStep(st, 79);
        cqWorkplaneNew(c, st, 79, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 80);
        cqSphere(c, st, 80, 79, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 80, "sphere");
        atStep(st, 81);
        cqTranslate(c, st, 81, 80, {"vec": [25, -43.30127018922192, 8.259061849445711e-7]});
        stepVol(c, st, 81, "translate");
        atStep(st, 82);
        cqUnion(c, st, 82, 78, {"toUnion": {"wp": 81}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 82, "union");
        atStep(st, 83);
        cqWorkplaneNew(c, st, 83, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 84);
        cqSphere(c, st, 84, 83, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 84, "sphere");
        atStep(st, 85);
        cqTranslate(c, st, 85, 84, {"vec": [-27.5, -38.97114317029973, 15.000000000000016]});
        stepVol(c, st, 85, "translate");
        atStep(st, 86);
        cqUnion(c, st, 86, 82, {"toUnion": {"wp": 85}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 86, "union");
        atStep(st, 87);
        cqWorkplaneNew(c, st, 87, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 88);
        cqSphere(c, st, 88, 87, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 88, "sphere");
        atStep(st, 89);
        cqTranslate(c, st, 89, 88, {"vec": [-22.5, -38.97114317029973, 21.79449471770338]});
        stepVol(c, st, 89, "translate");
        atStep(st, 90);
        cqUnion(c, st, 90, 86, {"toUnion": {"wp": 89}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 90, "union");
        atStep(st, 91);
        cqWorkplaneNew(c, st, 91, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 92);
        cqSphere(c, st, 92, 91, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 92, "sphere");
        atStep(st, 93);
        cqTranslate(c, st, 93, 92, {"vec": [-17.5, -38.97114317029973, 25.980762113533167]});
        stepVol(c, st, 93, "translate");
        atStep(st, 94);
        cqUnion(c, st, 94, 90, {"toUnion": {"wp": 93}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 94, "union");
        atStep(st, 95);
        cqWorkplaneNew(c, st, 95, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 96);
        cqSphere(c, st, 96, 95, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 96, "sphere");
        atStep(st, 97);
        cqTranslate(c, st, 97, 96, {"vec": [-12.5, -38.97114317029973, 28.72281323269015]});
        stepVol(c, st, 97, "translate");
        atStep(st, 98);
        cqUnion(c, st, 98, 94, {"toUnion": {"wp": 97}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 98, "union");
        atStep(st, 99);
        cqWorkplaneNew(c, st, 99, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 100);
        cqSphere(c, st, 100, 99, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 100, "sphere");
        atStep(st, 101);
        cqTranslate(c, st, 101, 100, {"vec": [-7.5, -38.97114317029973, 30.413812651491103]});
        stepVol(c, st, 101, "translate");
        atStep(st, 102);
        cqUnion(c, st, 102, 98, {"toUnion": {"wp": 101}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 102, "union");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 104);
        cqSphere(c, st, 104, 103, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 104, "sphere");
        atStep(st, 105);
        cqTranslate(c, st, 105, 104, {"vec": [-2.5, -38.97114317029973, 31.224989991991997]});
        stepVol(c, st, 105, "translate");
        atStep(st, 106);
        cqUnion(c, st, 106, 102, {"toUnion": {"wp": 105}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 106, "union");
        atStep(st, 107);
        cqWorkplaneNew(c, st, 107, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 108);
        cqSphere(c, st, 108, 107, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 108, "sphere");
        atStep(st, 109);
        cqTranslate(c, st, 109, 108, {"vec": [2.5, -38.97114317029973, 31.224989991991997]});
        stepVol(c, st, 109, "translate");
        atStep(st, 110);
        cqUnion(c, st, 110, 106, {"toUnion": {"wp": 109}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 110, "union");
        atStep(st, 111);
        cqWorkplaneNew(c, st, 111, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 112);
        cqSphere(c, st, 112, 111, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 112, "sphere");
        atStep(st, 113);
        cqTranslate(c, st, 113, 112, {"vec": [7.5, -38.97114317029973, 30.413812651491103]});
        stepVol(c, st, 113, "translate");
        atStep(st, 114);
        cqUnion(c, st, 114, 110, {"toUnion": {"wp": 113}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 114, "union");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 115, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 116);
        cqSphere(c, st, 116, 115, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 116, "sphere");
        atStep(st, 117);
        cqTranslate(c, st, 117, 116, {"vec": [12.5, -38.97114317029973, 28.72281323269015]});
        stepVol(c, st, 117, "translate");
        atStep(st, 118);
        cqUnion(c, st, 118, 114, {"toUnion": {"wp": 117}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 118, "union");
        atStep(st, 119);
        cqWorkplaneNew(c, st, 119, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 120);
        cqSphere(c, st, 120, 119, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 120, "sphere");
        atStep(st, 121);
        cqTranslate(c, st, 121, 120, {"vec": [17.5, -38.97114317029973, 25.980762113533167]});
        stepVol(c, st, 121, "translate");
        atStep(st, 122);
        cqUnion(c, st, 122, 118, {"toUnion": {"wp": 121}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 122, "union");
        atStep(st, 123);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 124);
        cqSphere(c, st, 124, 123, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 124, "sphere");
        atStep(st, 125);
        cqTranslate(c, st, 125, 124, {"vec": [22.5, -38.97114317029973, 21.79449471770338]});
        stepVol(c, st, 125, "translate");
        atStep(st, 126);
        cqUnion(c, st, 126, 122, {"toUnion": {"wp": 125}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 126, "union");
        atStep(st, 127);
        cqWorkplaneNew(c, st, 127, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 128);
        cqSphere(c, st, 128, 127, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 128, "sphere");
        atStep(st, 129);
        cqTranslate(c, st, 129, 128, {"vec": [27.5, -38.97114317029973, 15.000000000000016]});
        stepVol(c, st, 129, "translate");
        atStep(st, 130);
        cqUnion(c, st, 130, 126, {"toUnion": {"wp": 129}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 130, "union");
        atStep(st, 131);
        cqWorkplaneNew(c, st, 131, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 132);
        cqSphere(c, st, 132, 131, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 132, "sphere");
        atStep(st, 133);
        cqTranslate(c, st, 133, 132, {"vec": [-35, -34.64101615137754, 8.6602540378444]});
        stepVol(c, st, 133, "translate");
        atStep(st, 134);
        cqUnion(c, st, 134, 130, {"toUnion": {"wp": 133}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 134, "union");
        atStep(st, 135);
        cqWorkplaneNew(c, st, 135, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 136);
        cqSphere(c, st, 136, 135, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 136, "sphere");
        atStep(st, 137);
        cqTranslate(c, st, 137, 136, {"vec": [-30, -34.64101615137754, 20.000000000000007]});
        stepVol(c, st, 137, "translate");
        atStep(st, 138);
        cqUnion(c, st, 138, 134, {"toUnion": {"wp": 137}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 138, "union");
        atStep(st, 139);
        cqWorkplaneNew(c, st, 139, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 140);
        cqSphere(c, st, 140, 139, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 140, "sphere");
        atStep(st, 141);
        cqTranslate(c, st, 141, 140, {"vec": [-25, -34.64101615137754, 25.980762113533164]});
        stepVol(c, st, 141, "translate");
        atStep(st, 142);
        cqUnion(c, st, 142, 138, {"toUnion": {"wp": 141}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 142, "union");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 143, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 144);
        cqSphere(c, st, 144, 143, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 144, "sphere");
        atStep(st, 145);
        cqTranslate(c, st, 145, 144, {"vec": [-20, -34.64101615137754, 30.000000000000004]});
        stepVol(c, st, 145, "translate");
        atStep(st, 146);
        cqUnion(c, st, 146, 142, {"toUnion": {"wp": 145}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 146, "union");
        atStep(st, 147);
        cqWorkplaneNew(c, st, 147, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 148);
        cqSphere(c, st, 148, 147, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 148, "sphere");
        atStep(st, 149);
        cqTranslate(c, st, 149, 148, {"vec": [-15, -34.64101615137754, 32.78719262151001]});
        stepVol(c, st, 149, "translate");
        atStep(st, 150);
        cqUnion(c, st, 150, 146, {"toUnion": {"wp": 149}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 150, "union");
        atStep(st, 151);
        cqWorkplaneNew(c, st, 151, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 152);
        cqSphere(c, st, 152, 151, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 152, "sphere");
        atStep(st, 153);
        cqTranslate(c, st, 153, 152, {"vec": [-10, -34.64101615137754, 34.64101615137755]});
        stepVol(c, st, 153, "translate");
        atStep(st, 154);
        cqUnion(c, st, 154, 150, {"toUnion": {"wp": 153}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 154, "union");
        atStep(st, 155);
        cqWorkplaneNew(c, st, 155, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 156);
        cqSphere(c, st, 156, 155, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 156, "sphere");
        atStep(st, 157);
        cqTranslate(c, st, 157, 156, {"vec": [10, -34.64101615137754, 34.64101615137755]});
        stepVol(c, st, 157, "translate");
        atStep(st, 158);
        cqUnion(c, st, 158, 154, {"toUnion": {"wp": 157}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 158, "union");
        atStep(st, 159);
        cqWorkplaneNew(c, st, 159, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 160);
        cqSphere(c, st, 160, 159, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 160, "sphere");
        atStep(st, 161);
        cqTranslate(c, st, 161, 160, {"vec": [15, -34.64101615137754, 32.78719262151001]});
        stepVol(c, st, 161, "translate");
        atStep(st, 162);
        cqUnion(c, st, 162, 158, {"toUnion": {"wp": 161}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 162, "union");
        atStep(st, 163);
        cqWorkplaneNew(c, st, 163, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 164);
        cqSphere(c, st, 164, 163, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 164, "sphere");
        atStep(st, 165);
        cqTranslate(c, st, 165, 164, {"vec": [20, -34.64101615137754, 30.000000000000004]});
        stepVol(c, st, 165, "translate");
        atStep(st, 166);
        cqUnion(c, st, 166, 162, {"toUnion": {"wp": 165}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 166, "union");
        atStep(st, 167);
        cqWorkplaneNew(c, st, 167, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 168);
        cqSphere(c, st, 168, 167, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 168, "sphere");
        atStep(st, 169);
        cqTranslate(c, st, 169, 168, {"vec": [25, -34.64101615137754, 25.980762113533164]});
        stepVol(c, st, 169, "translate");
        atStep(st, 170);
        cqUnion(c, st, 170, 166, {"toUnion": {"wp": 169}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 170, "union");
        atStep(st, 171);
        cqWorkplaneNew(c, st, 171, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 172);
        cqSphere(c, st, 172, 171, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 172, "sphere");
        atStep(st, 173);
        cqTranslate(c, st, 173, 172, {"vec": [30, -34.64101615137754, 20.000000000000007]});
        stepVol(c, st, 173, "translate");
        atStep(st, 174);
        cqUnion(c, st, 174, 170, {"toUnion": {"wp": 173}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 174, "union");
        atStep(st, 175);
        cqWorkplaneNew(c, st, 175, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 176);
        cqSphere(c, st, 176, 175, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 176, "sphere");
        atStep(st, 177);
        cqTranslate(c, st, 177, 176, {"vec": [35, -34.64101615137754, 8.6602540378444]});
        stepVol(c, st, 177, "translate");
        atStep(st, 178);
        cqUnion(c, st, 178, 174, {"toUnion": {"wp": 177}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 178, "union");
        atStep(st, 179);
        cqWorkplaneNew(c, st, 179, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 180);
        cqSphere(c, st, 180, 179, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 180, "sphere");
        atStep(st, 181);
        cqTranslate(c, st, 181, 180, {"vec": [-37.5, -30.31088913245535, 13.228756555322958]});
        stepVol(c, st, 181, "translate");
        atStep(st, 182);
        cqUnion(c, st, 182, 178, {"toUnion": {"wp": 181}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 182, "union");
        atStep(st, 183);
        cqWorkplaneNew(c, st, 183, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 184);
        cqSphere(c, st, 184, 183, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 184, "sphere");
        atStep(st, 185);
        cqTranslate(c, st, 185, 184, {"vec": [-32.5, -30.31088913245535, 22.9128784747792]});
        stepVol(c, st, 185, "translate");
        atStep(st, 186);
        cqUnion(c, st, 186, 182, {"toUnion": {"wp": 185}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 186, "union");
        atStep(st, 187);
        cqWorkplaneNew(c, st, 187, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 188);
        cqSphere(c, st, 188, 187, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 188, "sphere");
        atStep(st, 189);
        cqTranslate(c, st, 189, 188, {"vec": [-27.5, -30.31088913245535, 28.722813232690147]});
        stepVol(c, st, 189, "translate");
        atStep(st, 190);
        cqUnion(c, st, 190, 186, {"toUnion": {"wp": 189}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 190, "union");
        atStep(st, 191);
        cqWorkplaneNew(c, st, 191, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 192);
        cqSphere(c, st, 192, 191, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 192, "sphere");
        atStep(st, 193);
        cqTranslate(c, st, 193, 192, {"vec": [-22.5, -30.31088913245535, 32.78719262151]});
        stepVol(c, st, 193, "translate");
        atStep(st, 194);
        cqUnion(c, st, 194, 190, {"toUnion": {"wp": 193}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 194, "union");
        atStep(st, 195);
        cqWorkplaneNew(c, st, 195, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 196);
        cqSphere(c, st, 196, 195, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 196, "sphere");
        atStep(st, 197);
        cqTranslate(c, st, 197, 196, {"vec": [22.5, -30.31088913245535, 32.78719262151]});
        stepVol(c, st, 197, "translate");
        atStep(st, 198);
        cqUnion(c, st, 198, 194, {"toUnion": {"wp": 197}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 198, "union");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 199, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 200);
        cqSphere(c, st, 200, 199, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 200, "sphere");
        atStep(st, 201);
        cqTranslate(c, st, 201, 200, {"vec": [27.5, -30.31088913245535, 28.722813232690147]});
        stepVol(c, st, 201, "translate");
        atStep(st, 202);
        cqUnion(c, st, 202, 198, {"toUnion": {"wp": 201}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 202, "union");
        atStep(st, 203);
        cqWorkplaneNew(c, st, 203, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 204);
        cqSphere(c, st, 204, 203, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 204, "sphere");
        atStep(st, 205);
        cqTranslate(c, st, 205, 204, {"vec": [32.5, -30.31088913245535, 22.9128784747792]});
        stepVol(c, st, 205, "translate");
        atStep(st, 206);
        cqUnion(c, st, 206, 202, {"toUnion": {"wp": 205}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 206, "union");
        atStep(st, 207);
        cqWorkplaneNew(c, st, 207, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 208);
        cqSphere(c, st, 208, 207, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 208, "sphere");
        atStep(st, 209);
        cqTranslate(c, st, 209, 208, {"vec": [37.5, -30.31088913245535, 13.228756555322958]});
        stepVol(c, st, 209, "translate");
        atStep(st, 210);
        cqUnion(c, st, 210, 206, {"toUnion": {"wp": 209}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 210, "union");
        atStep(st, 211);
        cqWorkplaneNew(c, st, 211, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 212);
        cqSphere(c, st, 212, 211, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 212, "sphere");
        atStep(st, 213);
        cqTranslate(c, st, 213, 212, {"vec": [-40, -25.980762113533157, 15.000000000000004]});
        stepVol(c, st, 213, "translate");
        atStep(st, 214);
        cqUnion(c, st, 214, 210, {"toUnion": {"wp": 213}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 214, "union");
        atStep(st, 215);
        cqWorkplaneNew(c, st, 215, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 216);
        cqSphere(c, st, 216, 215, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 216, "sphere");
        atStep(st, 217);
        cqTranslate(c, st, 217, 216, {"vec": [-35, -25.980762113533157, 24.494897427831784]});
        stepVol(c, st, 217, "translate");
        atStep(st, 218);
        cqUnion(c, st, 218, 214, {"toUnion": {"wp": 217}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 218, "union");
        atStep(st, 219);
        cqWorkplaneNew(c, st, 219, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 220);
        cqSphere(c, st, 220, 219, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 220, "sphere");
        atStep(st, 221);
        cqTranslate(c, st, 221, 220, {"vec": [-30, -25.980762113533157, 30.4138126514911]});
        stepVol(c, st, 221, "translate");
        atStep(st, 222);
        cqUnion(c, st, 222, 218, {"toUnion": {"wp": 221}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 222, "union");
        atStep(st, 223);
        cqWorkplaneNew(c, st, 223, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 224);
        cqSphere(c, st, 224, 223, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 224, "sphere");
        atStep(st, 225);
        cqTranslate(c, st, 225, 224, {"vec": [-25, -25.980762113533157, 34.64101615137755]});
        stepVol(c, st, 225, "translate");
        atStep(st, 226);
        cqUnion(c, st, 226, 222, {"toUnion": {"wp": 225}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 226, "union");
        atStep(st, 227);
        cqWorkplaneNew(c, st, 227, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 228);
        cqSphere(c, st, 228, 227, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 228, "sphere");
        atStep(st, 229);
        cqTranslate(c, st, 229, 228, {"vec": [25, -25.980762113533157, 34.64101615137755]});
        stepVol(c, st, 229, "translate");
        atStep(st, 230);
        cqUnion(c, st, 230, 226, {"toUnion": {"wp": 229}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 230, "union");
        atStep(st, 231);
        cqWorkplaneNew(c, st, 231, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 232);
        cqSphere(c, st, 232, 231, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 232, "sphere");
        atStep(st, 233);
        cqTranslate(c, st, 233, 232, {"vec": [30, -25.980762113533157, 30.4138126514911]});
        stepVol(c, st, 233, "translate");
        atStep(st, 234);
        cqUnion(c, st, 234, 230, {"toUnion": {"wp": 233}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 234, "union");
        atStep(st, 235);
        cqWorkplaneNew(c, st, 235, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 236);
        cqSphere(c, st, 236, 235, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 236, "sphere");
        atStep(st, 237);
        cqTranslate(c, st, 237, 236, {"vec": [35, -25.980762113533157, 24.494897427831784]});
        stepVol(c, st, 237, "translate");
        atStep(st, 238);
        cqUnion(c, st, 238, 234, {"toUnion": {"wp": 237}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 238, "union");
        atStep(st, 239);
        cqWorkplaneNew(c, st, 239, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 240);
        cqSphere(c, st, 240, 239, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 240, "sphere");
        atStep(st, 241);
        cqTranslate(c, st, 241, 240, {"vec": [40, -25.980762113533157, 15.000000000000004]});
        stepVol(c, st, 241, "translate");
        atStep(st, 242);
        cqUnion(c, st, 242, 238, {"toUnion": {"wp": 241}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 242, "union");
        atStep(st, 243);
        cqWorkplaneNew(c, st, 243, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 244);
        cqSphere(c, st, 244, 243, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 244, "sphere");
        atStep(st, 245);
        cqTranslate(c, st, 245, 244, {"vec": [-42.5, -21.65063509461096, 15.000000000000004]});
        stepVol(c, st, 245, "translate");
        atStep(st, 246);
        cqUnion(c, st, 246, 242, {"toUnion": {"wp": 245}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 246, "union");
        atStep(st, 247);
        cqWorkplaneNew(c, st, 247, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 248);
        cqSphere(c, st, 248, 247, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 248, "sphere");
        atStep(st, 249);
        cqTranslate(c, st, 249, 248, {"vec": [-37.5, -21.65063509461096, 25.000000000000004]});
        stepVol(c, st, 249, "translate");
        atStep(st, 250);
        cqUnion(c, st, 250, 246, {"toUnion": {"wp": 249}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 250, "union");
        atStep(st, 251);
        cqWorkplaneNew(c, st, 251, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 252);
        cqSphere(c, st, 252, 251, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 252, "sphere");
        atStep(st, 253);
        cqTranslate(c, st, 253, 252, {"vec": [-32.5, -21.65063509461096, 31.224989991991993]});
        stepVol(c, st, 253, "translate");
        atStep(st, 254);
        cqUnion(c, st, 254, 250, {"toUnion": {"wp": 253}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 254, "union");
        atStep(st, 255);
        cqWorkplaneNew(c, st, 255, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 256);
        cqSphere(c, st, 256, 255, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 256, "sphere");
        atStep(st, 257);
        cqTranslate(c, st, 257, 256, {"vec": [32.5, -21.65063509461096, 31.224989991991993]});
        stepVol(c, st, 257, "translate");
        atStep(st, 258);
        cqUnion(c, st, 258, 254, {"toUnion": {"wp": 257}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 258, "union");
        atStep(st, 259);
        cqWorkplaneNew(c, st, 259, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 260);
        cqSphere(c, st, 260, 259, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 260, "sphere");
        atStep(st, 261);
        cqTranslate(c, st, 261, 260, {"vec": [37.5, -21.65063509461096, 25.000000000000004]});
        stepVol(c, st, 261, "translate");
        atStep(st, 262);
        cqUnion(c, st, 262, 258, {"toUnion": {"wp": 261}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 262, "union");
        atStep(st, 263);
        cqWorkplaneNew(c, st, 263, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 264);
        cqSphere(c, st, 264, 263, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 264, "sphere");
        atStep(st, 265);
        cqTranslate(c, st, 265, 264, {"vec": [42.5, -21.65063509461096, 15.000000000000004]});
        stepVol(c, st, 265, "translate");
        atStep(st, 266);
        cqUnion(c, st, 266, 262, {"toUnion": {"wp": 265}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 266, "union");
        atStep(st, 267);
        cqWorkplaneNew(c, st, 267, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 268);
        cqSphere(c, st, 268, 267, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 268, "sphere");
        atStep(st, 269);
        cqTranslate(c, st, 269, 268, {"vec": [-45, -17.32050807568877, 13.228756555322954]});
        stepVol(c, st, 269, "translate");
        atStep(st, 270);
        cqUnion(c, st, 270, 266, {"toUnion": {"wp": 269}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 270, "union");
        atStep(st, 271);
        cqWorkplaneNew(c, st, 271, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 272);
        cqSphere(c, st, 272, 271, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 272, "sphere");
        atStep(st, 273);
        cqTranslate(c, st, 273, 272, {"vec": [-40, -17.32050807568877, 24.49489742783178]});
        stepVol(c, st, 273, "translate");
        atStep(st, 274);
        cqUnion(c, st, 274, 270, {"toUnion": {"wp": 273}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 274, "union");
        atStep(st, 275);
        cqWorkplaneNew(c, st, 275, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 276);
        cqSphere(c, st, 276, 275, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 276, "sphere");
        atStep(st, 277);
        cqTranslate(c, st, 277, 276, {"vec": [-35, -17.32050807568877, 31.22498999199199]});
        stepVol(c, st, 277, "translate");
        atStep(st, 278);
        cqUnion(c, st, 278, 274, {"toUnion": {"wp": 277}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 278, "union");
        atStep(st, 279);
        cqWorkplaneNew(c, st, 279, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 280);
        cqSphere(c, st, 280, 279, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 280, "sphere");
        atStep(st, 281);
        cqTranslate(c, st, 281, 280, {"vec": [35, -17.32050807568877, 31.22498999199199]});
        stepVol(c, st, 281, "translate");
        atStep(st, 282);
        cqUnion(c, st, 282, 278, {"toUnion": {"wp": 281}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 282, "union");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 284);
        cqSphere(c, st, 284, 283, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 284, "sphere");
        atStep(st, 285);
        cqTranslate(c, st, 285, 284, {"vec": [40, -17.32050807568877, 24.49489742783178]});
        stepVol(c, st, 285, "translate");
        atStep(st, 286);
        cqUnion(c, st, 286, 282, {"toUnion": {"wp": 285}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 286, "union");
        atStep(st, 287);
        cqWorkplaneNew(c, st, 287, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 288);
        cqSphere(c, st, 288, 287, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 288, "sphere");
        atStep(st, 289);
        cqTranslate(c, st, 289, 288, {"vec": [45, -17.32050807568877, 13.228756555322954]});
        stepVol(c, st, 289, "translate");
        atStep(st, 290);
        cqUnion(c, st, 290, 286, {"toUnion": {"wp": 289}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 290, "union");
        atStep(st, 291);
        cqWorkplaneNew(c, st, 291, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 292);
        cqSphere(c, st, 292, 291, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 292, "sphere");
        atStep(st, 293);
        cqTranslate(c, st, 293, 292, {"vec": [-47.5, -12.990381056766578, 8.660254037844387]});
        stepVol(c, st, 293, "translate");
        atStep(st, 294);
        cqUnion(c, st, 294, 290, {"toUnion": {"wp": 293}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 294, "union");
        atStep(st, 295);
        cqWorkplaneNew(c, st, 295, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 296);
        cqSphere(c, st, 296, 295, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 296, "sphere");
        atStep(st, 297);
        cqTranslate(c, st, 297, 296, {"vec": [-42.5, -12.990381056766578, 22.9128784747792]});
        stepVol(c, st, 297, "translate");
        atStep(st, 298);
        cqUnion(c, st, 298, 294, {"toUnion": {"wp": 297}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 298, "union");
        atStep(st, 299);
        cqWorkplaneNew(c, st, 299, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 300);
        cqSphere(c, st, 300, 299, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 300, "sphere");
        atStep(st, 301);
        cqTranslate(c, st, 301, 300, {"vec": [-37.5, -12.990381056766578, 30.4138126514911]});
        stepVol(c, st, 301, "translate");
        atStep(st, 302);
        cqUnion(c, st, 302, 298, {"toUnion": {"wp": 301}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 302, "union");
        atStep(st, 303);
        cqWorkplaneNew(c, st, 303, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 304);
        cqSphere(c, st, 304, 303, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 304, "sphere");
        atStep(st, 305);
        cqTranslate(c, st, 305, 304, {"vec": [37.5, -12.990381056766578, 30.4138126514911]});
        stepVol(c, st, 305, "translate");
        atStep(st, 306);
        cqUnion(c, st, 306, 302, {"toUnion": {"wp": 305}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 306, "union");
        atStep(st, 307);
        cqWorkplaneNew(c, st, 307, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 308);
        cqSphere(c, st, 308, 307, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 308, "sphere");
        atStep(st, 309);
        cqTranslate(c, st, 309, 308, {"vec": [42.5, -12.990381056766578, 22.9128784747792]});
        stepVol(c, st, 309, "translate");
        atStep(st, 310);
        cqUnion(c, st, 310, 306, {"toUnion": {"wp": 309}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 310, "union");
        atStep(st, 311);
        cqWorkplaneNew(c, st, 311, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 312);
        cqSphere(c, st, 312, 311, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 312, "sphere");
        atStep(st, 313);
        cqTranslate(c, st, 313, 312, {"vec": [47.5, -12.990381056766578, 8.660254037844387]});
        stepVol(c, st, 313, "translate");
        atStep(st, 314);
        cqUnion(c, st, 314, 310, {"toUnion": {"wp": 313}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 314, "union");
        atStep(st, 315);
        cqWorkplaneNew(c, st, 315, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 316);
        cqSphere(c, st, 316, 315, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 316, "sphere");
        atStep(st, 317);
        cqTranslate(c, st, 317, 316, {"vec": [-45, -8.660254037844386, 20]});
        stepVol(c, st, 317, "translate");
        atStep(st, 318);
        cqUnion(c, st, 318, 314, {"toUnion": {"wp": 317}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 318, "union");
        atStep(st, 319);
        cqWorkplaneNew(c, st, 319, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 320);
        cqSphere(c, st, 320, 319, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 320, "sphere");
        atStep(st, 321);
        cqTranslate(c, st, 321, 320, {"vec": [-40, -8.660254037844386, 28.722813232690143]});
        stepVol(c, st, 321, "translate");
        atStep(st, 322);
        cqUnion(c, st, 322, 318, {"toUnion": {"wp": 321}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 322, "union");
        atStep(st, 323);
        cqWorkplaneNew(c, st, 323, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 324);
        cqSphere(c, st, 324, 323, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 324, "sphere");
        atStep(st, 325);
        cqTranslate(c, st, 325, 324, {"vec": [-35, -8.660254037844386, 34.64101615137755]});
        stepVol(c, st, 325, "translate");
        atStep(st, 326);
        cqUnion(c, st, 326, 322, {"toUnion": {"wp": 325}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 326, "union");
        atStep(st, 327);
        cqWorkplaneNew(c, st, 327, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 328);
        cqSphere(c, st, 328, 327, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 328, "sphere");
        atStep(st, 329);
        cqTranslate(c, st, 329, 328, {"vec": [35, -8.660254037844386, 34.64101615137755]});
        stepVol(c, st, 329, "translate");
        atStep(st, 330);
        cqUnion(c, st, 330, 326, {"toUnion": {"wp": 329}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 330, "union");
        atStep(st, 331);
        cqWorkplaneNew(c, st, 331, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 332);
        cqSphere(c, st, 332, 331, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 332, "sphere");
        atStep(st, 333);
        cqTranslate(c, st, 333, 332, {"vec": [40, -8.660254037844386, 28.722813232690143]});
        stepVol(c, st, 333, "translate");
        atStep(st, 334);
        cqUnion(c, st, 334, 330, {"toUnion": {"wp": 333}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 334, "union");
        atStep(st, 335);
        cqWorkplaneNew(c, st, 335, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 336);
        cqSphere(c, st, 336, 335, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 336, "sphere");
        atStep(st, 337);
        cqTranslate(c, st, 337, 336, {"vec": [45, -8.660254037844386, 20]});
        stepVol(c, st, 337, "translate");
        atStep(st, 338);
        cqUnion(c, st, 338, 334, {"toUnion": {"wp": 337}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 338, "union");
        atStep(st, 339);
        cqWorkplaneNew(c, st, 339, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 340);
        cqSphere(c, st, 340, 339, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 340, "sphere");
        atStep(st, 341);
        cqTranslate(c, st, 341, 340, {"vec": [-47.5, -4.330127018922193, 15]});
        stepVol(c, st, 341, "translate");
        atStep(st, 342);
        cqUnion(c, st, 342, 338, {"toUnion": {"wp": 341}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 342, "union");
        atStep(st, 343);
        cqWorkplaneNew(c, st, 343, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 344);
        cqSphere(c, st, 344, 343, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 344, "sphere");
        atStep(st, 345);
        cqTranslate(c, st, 345, 344, {"vec": [-42.5, -4.330127018922193, 25.98076211353316]});
        stepVol(c, st, 345, "translate");
        atStep(st, 346);
        cqUnion(c, st, 346, 342, {"toUnion": {"wp": 345}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 346, "union");
        atStep(st, 347);
        cqWorkplaneNew(c, st, 347, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 348);
        cqSphere(c, st, 348, 347, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 348, "sphere");
        atStep(st, 349);
        cqTranslate(c, st, 349, 348, {"vec": [-37.5, -4.330127018922193, 32.78719262151]});
        stepVol(c, st, 349, "translate");
        atStep(st, 350);
        cqUnion(c, st, 350, 346, {"toUnion": {"wp": 349}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 350, "union");
        atStep(st, 351);
        cqWorkplaneNew(c, st, 351, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 352);
        cqSphere(c, st, 352, 351, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 352, "sphere");
        atStep(st, 353);
        cqTranslate(c, st, 353, 352, {"vec": [37.5, -4.330127018922193, 32.78719262151]});
        stepVol(c, st, 353, "translate");
        atStep(st, 354);
        cqUnion(c, st, 354, 350, {"toUnion": {"wp": 353}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 354, "union");
        atStep(st, 355);
        cqWorkplaneNew(c, st, 355, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 356);
        cqSphere(c, st, 356, 355, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 356, "sphere");
        atStep(st, 357);
        cqTranslate(c, st, 357, 356, {"vec": [42.5, -4.330127018922193, 25.98076211353316]});
        stepVol(c, st, 357, "translate");
        atStep(st, 358);
        cqUnion(c, st, 358, 354, {"toUnion": {"wp": 357}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 358, "union");
        atStep(st, 359);
        cqWorkplaneNew(c, st, 359, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 360);
        cqSphere(c, st, 360, 359, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 360, "sphere");
        atStep(st, 361);
        cqTranslate(c, st, 361, 360, {"vec": [47.5, -4.330127018922193, 15]});
        stepVol(c, st, 361, "translate");
        atStep(st, 362);
        cqUnion(c, st, 362, 358, {"toUnion": {"wp": 361}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 362, "union");
        atStep(st, 363);
        cqWorkplaneNew(c, st, 363, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 364);
        cqSphere(c, st, 364, 363, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 364, "sphere");
        atStep(st, 365);
        cqTranslate(c, st, 365, 364, {"vec": [-45, 0, 21.79449471770337]});
        stepVol(c, st, 365, "translate");
        atStep(st, 366);
        cqUnion(c, st, 366, 362, {"toUnion": {"wp": 365}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 366, "union");
        atStep(st, 367);
        cqWorkplaneNew(c, st, 367, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 368);
        cqSphere(c, st, 368, 367, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 368, "sphere");
        atStep(st, 369);
        cqTranslate(c, st, 369, 368, {"vec": [-40, 0, 30]});
        stepVol(c, st, 369, "translate");
        atStep(st, 370);
        cqUnion(c, st, 370, 366, {"toUnion": {"wp": 369}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 370, "union");
        atStep(st, 371);
        cqWorkplaneNew(c, st, 371, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 372);
        cqSphere(c, st, 372, 371, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 372, "sphere");
        atStep(st, 373);
        cqTranslate(c, st, 373, 372, {"vec": [40, 0, 30]});
        stepVol(c, st, 373, "translate");
        atStep(st, 374);
        cqUnion(c, st, 374, 370, {"toUnion": {"wp": 373}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 374, "union");
        atStep(st, 375);
        cqWorkplaneNew(c, st, 375, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 376);
        cqSphere(c, st, 376, 375, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 376, "sphere");
        atStep(st, 377);
        cqTranslate(c, st, 377, 376, {"vec": [45, 0, 21.79449471770337]});
        stepVol(c, st, 377, "translate");
        atStep(st, 378);
        cqUnion(c, st, 378, 374, {"toUnion": {"wp": 377}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 378, "union");
        atStep(st, 379);
        cqWorkplaneNew(c, st, 379, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 380);
        cqSphere(c, st, 380, 379, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 380, "sphere");
        atStep(st, 381);
        cqTranslate(c, st, 381, 380, {"vec": [-47.5, 4.330127018922193, 15]});
        stepVol(c, st, 381, "translate");
        atStep(st, 382);
        cqUnion(c, st, 382, 378, {"toUnion": {"wp": 381}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 382, "union");
        atStep(st, 383);
        cqWorkplaneNew(c, st, 383, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 384);
        cqSphere(c, st, 384, 383, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 384, "sphere");
        atStep(st, 385);
        cqTranslate(c, st, 385, 384, {"vec": [-42.5, 4.330127018922193, 25.98076211353316]});
        stepVol(c, st, 385, "translate");
        atStep(st, 386);
        cqUnion(c, st, 386, 382, {"toUnion": {"wp": 385}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 386, "union");
        atStep(st, 387);
        cqWorkplaneNew(c, st, 387, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 388);
        cqSphere(c, st, 388, 387, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 388, "sphere");
        atStep(st, 389);
        cqTranslate(c, st, 389, 388, {"vec": [-37.5, 4.330127018922193, 32.78719262151]});
        stepVol(c, st, 389, "translate");
        atStep(st, 390);
        cqUnion(c, st, 390, 386, {"toUnion": {"wp": 389}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 390, "union");
        atStep(st, 391);
        cqWorkplaneNew(c, st, 391, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 392);
        cqSphere(c, st, 392, 391, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 392, "sphere");
        atStep(st, 393);
        cqTranslate(c, st, 393, 392, {"vec": [37.5, 4.330127018922193, 32.78719262151]});
        stepVol(c, st, 393, "translate");
        atStep(st, 394);
        cqUnion(c, st, 394, 390, {"toUnion": {"wp": 393}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 394, "union");
        atStep(st, 395);
        cqWorkplaneNew(c, st, 395, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 396);
        cqSphere(c, st, 396, 395, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 396, "sphere");
        atStep(st, 397);
        cqTranslate(c, st, 397, 396, {"vec": [42.5, 4.330127018922193, 25.98076211353316]});
        stepVol(c, st, 397, "translate");
        atStep(st, 398);
        cqUnion(c, st, 398, 394, {"toUnion": {"wp": 397}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 398, "union");
        atStep(st, 399);
        cqWorkplaneNew(c, st, 399, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 400);
        cqSphere(c, st, 400, 399, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 400, "sphere");
        atStep(st, 401);
        cqTranslate(c, st, 401, 400, {"vec": [47.5, 4.330127018922193, 15]});
        stepVol(c, st, 401, "translate");
        atStep(st, 402);
        cqUnion(c, st, 402, 398, {"toUnion": {"wp": 401}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 402, "union");
        atStep(st, 403);
        cqWorkplaneNew(c, st, 403, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 404);
        cqSphere(c, st, 404, 403, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 404, "sphere");
        atStep(st, 405);
        cqTranslate(c, st, 405, 404, {"vec": [-45, 8.660254037844386, 20]});
        stepVol(c, st, 405, "translate");
        atStep(st, 406);
        cqUnion(c, st, 406, 402, {"toUnion": {"wp": 405}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 406, "union");
        atStep(st, 407);
        cqWorkplaneNew(c, st, 407, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 408);
        cqSphere(c, st, 408, 407, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 408, "sphere");
        atStep(st, 409);
        cqTranslate(c, st, 409, 408, {"vec": [-40, 8.660254037844386, 28.722813232690143]});
        stepVol(c, st, 409, "translate");
        atStep(st, 410);
        cqUnion(c, st, 410, 406, {"toUnion": {"wp": 409}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 410, "union");
        atStep(st, 411);
        cqWorkplaneNew(c, st, 411, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 412);
        cqSphere(c, st, 412, 411, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 412, "sphere");
        atStep(st, 413);
        cqTranslate(c, st, 413, 412, {"vec": [-35, 8.660254037844386, 34.64101615137755]});
        stepVol(c, st, 413, "translate");
        atStep(st, 414);
        cqUnion(c, st, 414, 410, {"toUnion": {"wp": 413}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 414, "union");
        atStep(st, 415);
        cqWorkplaneNew(c, st, 415, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 416);
        cqSphere(c, st, 416, 415, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 416, "sphere");
        atStep(st, 417);
        cqTranslate(c, st, 417, 416, {"vec": [35, 8.660254037844386, 34.64101615137755]});
        stepVol(c, st, 417, "translate");
        atStep(st, 418);
        cqUnion(c, st, 418, 414, {"toUnion": {"wp": 417}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 418, "union");
        atStep(st, 419);
        cqWorkplaneNew(c, st, 419, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 420);
        cqSphere(c, st, 420, 419, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 420, "sphere");
        atStep(st, 421);
        cqTranslate(c, st, 421, 420, {"vec": [40, 8.660254037844386, 28.722813232690143]});
        stepVol(c, st, 421, "translate");
        atStep(st, 422);
        cqUnion(c, st, 422, 418, {"toUnion": {"wp": 421}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 422, "union");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 424);
        cqSphere(c, st, 424, 423, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 424, "sphere");
        atStep(st, 425);
        cqTranslate(c, st, 425, 424, {"vec": [45, 8.660254037844386, 20]});
        stepVol(c, st, 425, "translate");
        atStep(st, 426);
        cqUnion(c, st, 426, 422, {"toUnion": {"wp": 425}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 426, "union");
        atStep(st, 427);
        cqWorkplaneNew(c, st, 427, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 428);
        cqSphere(c, st, 428, 427, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 428, "sphere");
        atStep(st, 429);
        cqTranslate(c, st, 429, 428, {"vec": [-47.5, 12.990381056766578, 8.660254037844387]});
        stepVol(c, st, 429, "translate");
        atStep(st, 430);
        cqUnion(c, st, 430, 426, {"toUnion": {"wp": 429}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 430, "union");
        atStep(st, 431);
        cqWorkplaneNew(c, st, 431, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 432);
        cqSphere(c, st, 432, 431, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 432, "sphere");
        atStep(st, 433);
        cqTranslate(c, st, 433, 432, {"vec": [-42.5, 12.990381056766578, 22.9128784747792]});
        stepVol(c, st, 433, "translate");
        atStep(st, 434);
        cqUnion(c, st, 434, 430, {"toUnion": {"wp": 433}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 434, "union");
        atStep(st, 435);
        cqWorkplaneNew(c, st, 435, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 436);
        cqSphere(c, st, 436, 435, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 436, "sphere");
        atStep(st, 437);
        cqTranslate(c, st, 437, 436, {"vec": [-37.5, 12.990381056766578, 30.4138126514911]});
        stepVol(c, st, 437, "translate");
        atStep(st, 438);
        cqUnion(c, st, 438, 434, {"toUnion": {"wp": 437}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 438, "union");
        atStep(st, 439);
        cqWorkplaneNew(c, st, 439, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 440);
        cqSphere(c, st, 440, 439, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 440, "sphere");
        atStep(st, 441);
        cqTranslate(c, st, 441, 440, {"vec": [37.5, 12.990381056766578, 30.4138126514911]});
        stepVol(c, st, 441, "translate");
        atStep(st, 442);
        cqUnion(c, st, 442, 438, {"toUnion": {"wp": 441}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 442, "union");
        atStep(st, 443);
        cqWorkplaneNew(c, st, 443, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 444);
        cqSphere(c, st, 444, 443, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 444, "sphere");
        atStep(st, 445);
        cqTranslate(c, st, 445, 444, {"vec": [42.5, 12.990381056766578, 22.9128784747792]});
        stepVol(c, st, 445, "translate");
        atStep(st, 446);
        cqUnion(c, st, 446, 442, {"toUnion": {"wp": 445}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 446, "union");
        atStep(st, 447);
        cqWorkplaneNew(c, st, 447, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 448);
        cqSphere(c, st, 448, 447, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 448, "sphere");
        atStep(st, 449);
        cqTranslate(c, st, 449, 448, {"vec": [47.5, 12.990381056766578, 8.660254037844387]});
        stepVol(c, st, 449, "translate");
        atStep(st, 450);
        cqUnion(c, st, 450, 446, {"toUnion": {"wp": 449}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 450, "union");
        atStep(st, 451);
        cqWorkplaneNew(c, st, 451, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 452);
        cqSphere(c, st, 452, 451, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 452, "sphere");
        atStep(st, 453);
        cqTranslate(c, st, 453, 452, {"vec": [-45, 17.32050807568877, 13.228756555322954]});
        stepVol(c, st, 453, "translate");
        atStep(st, 454);
        cqUnion(c, st, 454, 450, {"toUnion": {"wp": 453}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 454, "union");
        atStep(st, 455);
        cqWorkplaneNew(c, st, 455, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 456);
        cqSphere(c, st, 456, 455, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 456, "sphere");
        atStep(st, 457);
        cqTranslate(c, st, 457, 456, {"vec": [-40, 17.32050807568877, 24.49489742783178]});
        stepVol(c, st, 457, "translate");
        atStep(st, 458);
        cqUnion(c, st, 458, 454, {"toUnion": {"wp": 457}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 458, "union");
        atStep(st, 459);
        cqWorkplaneNew(c, st, 459, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 460);
        cqSphere(c, st, 460, 459, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 460, "sphere");
        atStep(st, 461);
        cqTranslate(c, st, 461, 460, {"vec": [-35, 17.32050807568877, 31.22498999199199]});
        stepVol(c, st, 461, "translate");
        atStep(st, 462);
        cqUnion(c, st, 462, 458, {"toUnion": {"wp": 461}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 462, "union");
        atStep(st, 463);
        cqWorkplaneNew(c, st, 463, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 464);
        cqSphere(c, st, 464, 463, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 464, "sphere");
        atStep(st, 465);
        cqTranslate(c, st, 465, 464, {"vec": [35, 17.32050807568877, 31.22498999199199]});
        stepVol(c, st, 465, "translate");
        atStep(st, 466);
        cqUnion(c, st, 466, 462, {"toUnion": {"wp": 465}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 466, "union");
        atStep(st, 467);
        cqWorkplaneNew(c, st, 467, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 468);
        cqSphere(c, st, 468, 467, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 468, "sphere");
        atStep(st, 469);
        cqTranslate(c, st, 469, 468, {"vec": [40, 17.32050807568877, 24.49489742783178]});
        stepVol(c, st, 469, "translate");
        atStep(st, 470);
        cqUnion(c, st, 470, 466, {"toUnion": {"wp": 469}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 470, "union");
        atStep(st, 471);
        cqWorkplaneNew(c, st, 471, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 472);
        cqSphere(c, st, 472, 471, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 472, "sphere");
        atStep(st, 473);
        cqTranslate(c, st, 473, 472, {"vec": [45, 17.32050807568877, 13.228756555322954]});
        stepVol(c, st, 473, "translate");
        atStep(st, 474);
        cqUnion(c, st, 474, 470, {"toUnion": {"wp": 473}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 474, "union");
        atStep(st, 475);
        cqWorkplaneNew(c, st, 475, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 476);
        cqSphere(c, st, 476, 475, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 476, "sphere");
        atStep(st, 477);
        cqTranslate(c, st, 477, 476, {"vec": [-42.5, 21.65063509461096, 15.000000000000004]});
        stepVol(c, st, 477, "translate");
        atStep(st, 478);
        cqUnion(c, st, 478, 474, {"toUnion": {"wp": 477}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 478, "union");
        atStep(st, 479);
        cqWorkplaneNew(c, st, 479, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 480);
        cqSphere(c, st, 480, 479, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 480, "sphere");
        atStep(st, 481);
        cqTranslate(c, st, 481, 480, {"vec": [-37.5, 21.65063509461096, 25.000000000000004]});
        stepVol(c, st, 481, "translate");
        atStep(st, 482);
        cqUnion(c, st, 482, 478, {"toUnion": {"wp": 481}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 482, "union");
        atStep(st, 483);
        cqWorkplaneNew(c, st, 483, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 484);
        cqSphere(c, st, 484, 483, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 484, "sphere");
        atStep(st, 485);
        cqTranslate(c, st, 485, 484, {"vec": [-32.5, 21.65063509461096, 31.224989991991993]});
        stepVol(c, st, 485, "translate");
        atStep(st, 486);
        cqUnion(c, st, 486, 482, {"toUnion": {"wp": 485}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 486, "union");
        atStep(st, 487);
        cqWorkplaneNew(c, st, 487, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 488);
        cqSphere(c, st, 488, 487, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 488, "sphere");
        atStep(st, 489);
        cqTranslate(c, st, 489, 488, {"vec": [32.5, 21.65063509461096, 31.224989991991993]});
        stepVol(c, st, 489, "translate");
        atStep(st, 490);
        cqUnion(c, st, 490, 486, {"toUnion": {"wp": 489}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 490, "union");
        atStep(st, 491);
        cqWorkplaneNew(c, st, 491, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 492);
        cqSphere(c, st, 492, 491, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 492, "sphere");
        atStep(st, 493);
        cqTranslate(c, st, 493, 492, {"vec": [37.5, 21.65063509461096, 25.000000000000004]});
        stepVol(c, st, 493, "translate");
        atStep(st, 494);
        cqUnion(c, st, 494, 490, {"toUnion": {"wp": 493}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 494, "union");
        atStep(st, 495);
        cqWorkplaneNew(c, st, 495, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 496);
        cqSphere(c, st, 496, 495, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 496, "sphere");
        atStep(st, 497);
        cqTranslate(c, st, 497, 496, {"vec": [42.5, 21.65063509461096, 15.000000000000004]});
        stepVol(c, st, 497, "translate");
        atStep(st, 498);
        cqUnion(c, st, 498, 494, {"toUnion": {"wp": 497}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 498, "union");
        atStep(st, 499);
        cqWorkplaneNew(c, st, 499, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 500);
        cqSphere(c, st, 500, 499, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 500, "sphere");
        atStep(st, 501);
        cqTranslate(c, st, 501, 500, {"vec": [-40, 25.980762113533157, 15.000000000000004]});
        stepVol(c, st, 501, "translate");
        atStep(st, 502);
        cqUnion(c, st, 502, 498, {"toUnion": {"wp": 501}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 502, "union");
        atStep(st, 503);
        cqWorkplaneNew(c, st, 503, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 504);
        cqSphere(c, st, 504, 503, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 504, "sphere");
        atStep(st, 505);
        cqTranslate(c, st, 505, 504, {"vec": [-35, 25.980762113533157, 24.494897427831784]});
        stepVol(c, st, 505, "translate");
        atStep(st, 506);
        cqUnion(c, st, 506, 502, {"toUnion": {"wp": 505}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 506, "union");
        atStep(st, 507);
        cqWorkplaneNew(c, st, 507, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 508);
        cqSphere(c, st, 508, 507, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 508, "sphere");
        atStep(st, 509);
        cqTranslate(c, st, 509, 508, {"vec": [-30, 25.980762113533157, 30.4138126514911]});
        stepVol(c, st, 509, "translate");
        atStep(st, 510);
        cqUnion(c, st, 510, 506, {"toUnion": {"wp": 509}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 510, "union");
        atStep(st, 511);
        cqWorkplaneNew(c, st, 511, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 512);
        cqSphere(c, st, 512, 511, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 512, "sphere");
        atStep(st, 513);
        cqTranslate(c, st, 513, 512, {"vec": [-25, 25.980762113533157, 34.64101615137755]});
        stepVol(c, st, 513, "translate");
        atStep(st, 514);
        cqUnion(c, st, 514, 510, {"toUnion": {"wp": 513}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 514, "union");
        atStep(st, 515);
        cqWorkplaneNew(c, st, 515, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 516);
        cqSphere(c, st, 516, 515, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 516, "sphere");
        atStep(st, 517);
        cqTranslate(c, st, 517, 516, {"vec": [25, 25.980762113533157, 34.64101615137755]});
        stepVol(c, st, 517, "translate");
        atStep(st, 518);
        cqUnion(c, st, 518, 514, {"toUnion": {"wp": 517}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 518, "union");
        atStep(st, 519);
        cqWorkplaneNew(c, st, 519, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 520);
        cqSphere(c, st, 520, 519, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 520, "sphere");
        atStep(st, 521);
        cqTranslate(c, st, 521, 520, {"vec": [30, 25.980762113533157, 30.4138126514911]});
        stepVol(c, st, 521, "translate");
        atStep(st, 522);
        cqUnion(c, st, 522, 518, {"toUnion": {"wp": 521}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 522, "union");
        atStep(st, 523);
        cqWorkplaneNew(c, st, 523, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 524);
        cqSphere(c, st, 524, 523, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 524, "sphere");
        atStep(st, 525);
        cqTranslate(c, st, 525, 524, {"vec": [35, 25.980762113533157, 24.494897427831784]});
        stepVol(c, st, 525, "translate");
        atStep(st, 526);
        cqUnion(c, st, 526, 522, {"toUnion": {"wp": 525}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 526, "union");
        atStep(st, 527);
        cqWorkplaneNew(c, st, 527, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 528);
        cqSphere(c, st, 528, 527, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 528, "sphere");
        atStep(st, 529);
        cqTranslate(c, st, 529, 528, {"vec": [40, 25.980762113533157, 15.000000000000004]});
        stepVol(c, st, 529, "translate");
        atStep(st, 530);
        cqUnion(c, st, 530, 526, {"toUnion": {"wp": 529}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 530, "union");
        atStep(st, 531);
        cqWorkplaneNew(c, st, 531, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 532);
        cqSphere(c, st, 532, 531, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 532, "sphere");
        atStep(st, 533);
        cqTranslate(c, st, 533, 532, {"vec": [-37.5, 30.31088913245535, 13.228756555322958]});
        stepVol(c, st, 533, "translate");
        atStep(st, 534);
        cqUnion(c, st, 534, 530, {"toUnion": {"wp": 533}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 534, "union");
        atStep(st, 535);
        cqWorkplaneNew(c, st, 535, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 536);
        cqSphere(c, st, 536, 535, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 536, "sphere");
        atStep(st, 537);
        cqTranslate(c, st, 537, 536, {"vec": [-32.5, 30.31088913245535, 22.9128784747792]});
        stepVol(c, st, 537, "translate");
        atStep(st, 538);
        cqUnion(c, st, 538, 534, {"toUnion": {"wp": 537}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 538, "union");
        atStep(st, 539);
        cqWorkplaneNew(c, st, 539, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 540);
        cqSphere(c, st, 540, 539, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 540, "sphere");
        atStep(st, 541);
        cqTranslate(c, st, 541, 540, {"vec": [-27.5, 30.31088913245535, 28.722813232690147]});
        stepVol(c, st, 541, "translate");
        atStep(st, 542);
        cqUnion(c, st, 542, 538, {"toUnion": {"wp": 541}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 542, "union");
        atStep(st, 543);
        cqWorkplaneNew(c, st, 543, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 544);
        cqSphere(c, st, 544, 543, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 544, "sphere");
        atStep(st, 545);
        cqTranslate(c, st, 545, 544, {"vec": [-22.5, 30.31088913245535, 32.78719262151]});
        stepVol(c, st, 545, "translate");
        atStep(st, 546);
        cqUnion(c, st, 546, 542, {"toUnion": {"wp": 545}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 546, "union");
        atStep(st, 547);
        cqWorkplaneNew(c, st, 547, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 548);
        cqSphere(c, st, 548, 547, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 548, "sphere");
        atStep(st, 549);
        cqTranslate(c, st, 549, 548, {"vec": [22.5, 30.31088913245535, 32.78719262151]});
        stepVol(c, st, 549, "translate");
        atStep(st, 550);
        cqUnion(c, st, 550, 546, {"toUnion": {"wp": 549}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 550, "union");
        atStep(st, 551);
        cqWorkplaneNew(c, st, 551, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 552);
        cqSphere(c, st, 552, 551, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 552, "sphere");
        atStep(st, 553);
        cqTranslate(c, st, 553, 552, {"vec": [27.5, 30.31088913245535, 28.722813232690147]});
        stepVol(c, st, 553, "translate");
        atStep(st, 554);
        cqUnion(c, st, 554, 550, {"toUnion": {"wp": 553}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 554, "union");
        atStep(st, 555);
        cqWorkplaneNew(c, st, 555, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 556);
        cqSphere(c, st, 556, 555, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 556, "sphere");
        atStep(st, 557);
        cqTranslate(c, st, 557, 556, {"vec": [32.5, 30.31088913245535, 22.9128784747792]});
        stepVol(c, st, 557, "translate");
        atStep(st, 558);
        cqUnion(c, st, 558, 554, {"toUnion": {"wp": 557}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 558, "union");
        atStep(st, 559);
        cqWorkplaneNew(c, st, 559, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 560);
        cqSphere(c, st, 560, 559, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 560, "sphere");
        atStep(st, 561);
        cqTranslate(c, st, 561, 560, {"vec": [37.5, 30.31088913245535, 13.228756555322958]});
        stepVol(c, st, 561, "translate");
        atStep(st, 562);
        cqUnion(c, st, 562, 558, {"toUnion": {"wp": 561}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 562, "union");
        atStep(st, 563);
        cqWorkplaneNew(c, st, 563, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 564);
        cqSphere(c, st, 564, 563, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 564, "sphere");
        atStep(st, 565);
        cqTranslate(c, st, 565, 564, {"vec": [-35, 34.64101615137754, 8.6602540378444]});
        stepVol(c, st, 565, "translate");
        atStep(st, 566);
        cqUnion(c, st, 566, 562, {"toUnion": {"wp": 565}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 566, "union");
        atStep(st, 567);
        cqWorkplaneNew(c, st, 567, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 568);
        cqSphere(c, st, 568, 567, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 568, "sphere");
        atStep(st, 569);
        cqTranslate(c, st, 569, 568, {"vec": [-30, 34.64101615137754, 20.000000000000007]});
        stepVol(c, st, 569, "translate");
        atStep(st, 570);
        cqUnion(c, st, 570, 566, {"toUnion": {"wp": 569}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 570, "union");
        atStep(st, 571);
        cqWorkplaneNew(c, st, 571, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 572);
        cqSphere(c, st, 572, 571, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 572, "sphere");
        atStep(st, 573);
        cqTranslate(c, st, 573, 572, {"vec": [-25, 34.64101615137754, 25.980762113533164]});
        stepVol(c, st, 573, "translate");
        atStep(st, 574);
        cqUnion(c, st, 574, 570, {"toUnion": {"wp": 573}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 574, "union");
        atStep(st, 575);
        cqWorkplaneNew(c, st, 575, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 576);
        cqSphere(c, st, 576, 575, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 576, "sphere");
        atStep(st, 577);
        cqTranslate(c, st, 577, 576, {"vec": [-20, 34.64101615137754, 30.000000000000004]});
        stepVol(c, st, 577, "translate");
        atStep(st, 578);
        cqUnion(c, st, 578, 574, {"toUnion": {"wp": 577}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 578, "union");
        atStep(st, 579);
        cqWorkplaneNew(c, st, 579, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 580);
        cqSphere(c, st, 580, 579, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 580, "sphere");
        atStep(st, 581);
        cqTranslate(c, st, 581, 580, {"vec": [-15, 34.64101615137754, 32.78719262151001]});
        stepVol(c, st, 581, "translate");
        atStep(st, 582);
        cqUnion(c, st, 582, 578, {"toUnion": {"wp": 581}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 582, "union");
        atStep(st, 583);
        cqWorkplaneNew(c, st, 583, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 584);
        cqSphere(c, st, 584, 583, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 584, "sphere");
        atStep(st, 585);
        cqTranslate(c, st, 585, 584, {"vec": [-10, 34.64101615137754, 34.64101615137755]});
        stepVol(c, st, 585, "translate");
        atStep(st, 586);
        cqUnion(c, st, 586, 582, {"toUnion": {"wp": 585}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 586, "union");
        atStep(st, 587);
        cqWorkplaneNew(c, st, 587, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 588);
        cqSphere(c, st, 588, 587, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 588, "sphere");
        atStep(st, 589);
        cqTranslate(c, st, 589, 588, {"vec": [10, 34.64101615137754, 34.64101615137755]});
        stepVol(c, st, 589, "translate");
        atStep(st, 590);
        cqUnion(c, st, 590, 586, {"toUnion": {"wp": 589}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 590, "union");
        atStep(st, 591);
        cqWorkplaneNew(c, st, 591, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 592);
        cqSphere(c, st, 592, 591, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 592, "sphere");
        atStep(st, 593);
        cqTranslate(c, st, 593, 592, {"vec": [15, 34.64101615137754, 32.78719262151001]});
        stepVol(c, st, 593, "translate");
        atStep(st, 594);
        cqUnion(c, st, 594, 590, {"toUnion": {"wp": 593}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 594, "union");
        atStep(st, 595);
        cqWorkplaneNew(c, st, 595, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 596);
        cqSphere(c, st, 596, 595, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 596, "sphere");
        atStep(st, 597);
        cqTranslate(c, st, 597, 596, {"vec": [20, 34.64101615137754, 30.000000000000004]});
        stepVol(c, st, 597, "translate");
        atStep(st, 598);
        cqUnion(c, st, 598, 594, {"toUnion": {"wp": 597}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 598, "union");
        atStep(st, 599);
        cqWorkplaneNew(c, st, 599, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 600);
        cqSphere(c, st, 600, 599, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 600, "sphere");
        atStep(st, 601);
        cqTranslate(c, st, 601, 600, {"vec": [25, 34.64101615137754, 25.980762113533164]});
        stepVol(c, st, 601, "translate");
        atStep(st, 602);
        cqUnion(c, st, 602, 598, {"toUnion": {"wp": 601}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 602, "union");
        atStep(st, 603);
        cqWorkplaneNew(c, st, 603, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 604);
        cqSphere(c, st, 604, 603, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 604, "sphere");
        atStep(st, 605);
        cqTranslate(c, st, 605, 604, {"vec": [30, 34.64101615137754, 20.000000000000007]});
        stepVol(c, st, 605, "translate");
        atStep(st, 606);
        cqUnion(c, st, 606, 602, {"toUnion": {"wp": 605}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 606, "union");
        atStep(st, 607);
        cqWorkplaneNew(c, st, 607, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 608);
        cqSphere(c, st, 608, 607, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 608, "sphere");
        atStep(st, 609);
        cqTranslate(c, st, 609, 608, {"vec": [35, 34.64101615137754, 8.6602540378444]});
        stepVol(c, st, 609, "translate");
        atStep(st, 610);
        cqUnion(c, st, 610, 606, {"toUnion": {"wp": 609}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 610, "union");
        atStep(st, 611);
        cqWorkplaneNew(c, st, 611, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 612);
        cqSphere(c, st, 612, 611, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 612, "sphere");
        atStep(st, 613);
        cqTranslate(c, st, 613, 612, {"vec": [-27.5, 38.97114317029973, 15.000000000000016]});
        stepVol(c, st, 613, "translate");
        atStep(st, 614);
        cqUnion(c, st, 614, 610, {"toUnion": {"wp": 613}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 614, "union");
        atStep(st, 615);
        cqWorkplaneNew(c, st, 615, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 616);
        cqSphere(c, st, 616, 615, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 616, "sphere");
        atStep(st, 617);
        cqTranslate(c, st, 617, 616, {"vec": [-22.5, 38.97114317029973, 21.79449471770338]});
        stepVol(c, st, 617, "translate");
        atStep(st, 618);
        cqUnion(c, st, 618, 614, {"toUnion": {"wp": 617}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 618, "union");
        atStep(st, 619);
        cqWorkplaneNew(c, st, 619, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 620);
        cqSphere(c, st, 620, 619, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 620, "sphere");
        atStep(st, 621);
        cqTranslate(c, st, 621, 620, {"vec": [-17.5, 38.97114317029973, 25.980762113533167]});
        stepVol(c, st, 621, "translate");
        atStep(st, 622);
        cqUnion(c, st, 622, 618, {"toUnion": {"wp": 621}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 622, "union");
        atStep(st, 623);
        cqWorkplaneNew(c, st, 623, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 624);
        cqSphere(c, st, 624, 623, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 624, "sphere");
        atStep(st, 625);
        cqTranslate(c, st, 625, 624, {"vec": [-12.5, 38.97114317029973, 28.72281323269015]});
        stepVol(c, st, 625, "translate");
        atStep(st, 626);
        cqUnion(c, st, 626, 622, {"toUnion": {"wp": 625}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 626, "union");
        atStep(st, 627);
        cqWorkplaneNew(c, st, 627, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 628);
        cqSphere(c, st, 628, 627, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 628, "sphere");
        atStep(st, 629);
        cqTranslate(c, st, 629, 628, {"vec": [-7.5, 38.97114317029973, 30.413812651491103]});
        stepVol(c, st, 629, "translate");
        atStep(st, 630);
        cqUnion(c, st, 630, 626, {"toUnion": {"wp": 629}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 630, "union");
        atStep(st, 631);
        cqWorkplaneNew(c, st, 631, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 632);
        cqSphere(c, st, 632, 631, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 632, "sphere");
        atStep(st, 633);
        cqTranslate(c, st, 633, 632, {"vec": [-2.5, 38.97114317029973, 31.224989991991997]});
        stepVol(c, st, 633, "translate");
        atStep(st, 634);
        cqUnion(c, st, 634, 630, {"toUnion": {"wp": 633}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 634, "union");
        atStep(st, 635);
        cqWorkplaneNew(c, st, 635, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 636);
        cqSphere(c, st, 636, 635, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 636, "sphere");
        atStep(st, 637);
        cqTranslate(c, st, 637, 636, {"vec": [2.5, 38.97114317029973, 31.224989991991997]});
        stepVol(c, st, 637, "translate");
        atStep(st, 638);
        cqUnion(c, st, 638, 634, {"toUnion": {"wp": 637}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 638, "union");
        atStep(st, 639);
        cqWorkplaneNew(c, st, 639, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 640);
        cqSphere(c, st, 640, 639, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 640, "sphere");
        atStep(st, 641);
        cqTranslate(c, st, 641, 640, {"vec": [7.5, 38.97114317029973, 30.413812651491103]});
        stepVol(c, st, 641, "translate");
        atStep(st, 642);
        cqUnion(c, st, 642, 638, {"toUnion": {"wp": 641}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 642, "union");
        atStep(st, 643);
        cqWorkplaneNew(c, st, 643, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 644);
        cqSphere(c, st, 644, 643, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 644, "sphere");
        atStep(st, 645);
        cqTranslate(c, st, 645, 644, {"vec": [12.5, 38.97114317029973, 28.72281323269015]});
        stepVol(c, st, 645, "translate");
        atStep(st, 646);
        cqUnion(c, st, 646, 642, {"toUnion": {"wp": 645}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 646, "union");
        atStep(st, 647);
        cqWorkplaneNew(c, st, 647, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 648);
        cqSphere(c, st, 648, 647, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 648, "sphere");
        atStep(st, 649);
        cqTranslate(c, st, 649, 648, {"vec": [17.5, 38.97114317029973, 25.980762113533167]});
        stepVol(c, st, 649, "translate");
        atStep(st, 650);
        cqUnion(c, st, 650, 646, {"toUnion": {"wp": 649}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 650, "union");
        atStep(st, 651);
        cqWorkplaneNew(c, st, 651, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 652);
        cqSphere(c, st, 652, 651, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 652, "sphere");
        atStep(st, 653);
        cqTranslate(c, st, 653, 652, {"vec": [22.5, 38.97114317029973, 21.79449471770338]});
        stepVol(c, st, 653, "translate");
        atStep(st, 654);
        cqUnion(c, st, 654, 650, {"toUnion": {"wp": 653}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 654, "union");
        atStep(st, 655);
        cqWorkplaneNew(c, st, 655, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 656);
        cqSphere(c, st, 656, 655, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 656, "sphere");
        atStep(st, 657);
        cqTranslate(c, st, 657, 656, {"vec": [27.5, 38.97114317029973, 15.000000000000016]});
        stepVol(c, st, 657, "translate");
        atStep(st, 658);
        cqUnion(c, st, 658, 654, {"toUnion": {"wp": 657}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 658, "union");
        atStep(st, 659);
        cqWorkplaneNew(c, st, 659, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 660);
        cqSphere(c, st, 660, 659, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 660, "sphere");
        atStep(st, 661);
        cqTranslate(c, st, 661, 660, {"vec": [-25, 43.30127018922192, 8.259061849445711e-7]});
        stepVol(c, st, 661, "translate");
        atStep(st, 662);
        cqUnion(c, st, 662, 658, {"toUnion": {"wp": 661}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 662, "union");
        atStep(st, 663);
        cqWorkplaneNew(c, st, 663, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 664);
        cqSphere(c, st, 664, 663, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 664, "sphere");
        atStep(st, 665);
        cqTranslate(c, st, 665, 664, {"vec": [-20, 43.30127018922192, 15.000000000000025]});
        stepVol(c, st, 665, "translate");
        atStep(st, 666);
        cqUnion(c, st, 666, 662, {"toUnion": {"wp": 665}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 666, "union");
        atStep(st, 667);
        cqWorkplaneNew(c, st, 667, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 668);
        cqSphere(c, st, 668, 667, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 668, "sphere");
        atStep(st, 669);
        cqTranslate(c, st, 669, 668, {"vec": [-15, 43.30127018922192, 20.000000000000018]});
        stepVol(c, st, 669, "translate");
        atStep(st, 670);
        cqUnion(c, st, 670, 666, {"toUnion": {"wp": 669}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 670, "union");
        atStep(st, 671);
        cqWorkplaneNew(c, st, 671, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 672);
        cqSphere(c, st, 672, 671, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 672, "sphere");
        atStep(st, 673);
        cqTranslate(c, st, 673, 672, {"vec": [-10, 43.30127018922192, 22.912878474779216]});
        stepVol(c, st, 673, "translate");
        atStep(st, 674);
        cqUnion(c, st, 674, 670, {"toUnion": {"wp": 673}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 674, "union");
        atStep(st, 675);
        cqWorkplaneNew(c, st, 675, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 676);
        cqSphere(c, st, 676, 675, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 676, "sphere");
        atStep(st, 677);
        cqTranslate(c, st, 677, 676, {"vec": [-5, 43.30127018922192, 24.494897427831795]});
        stepVol(c, st, 677, "translate");
        atStep(st, 678);
        cqUnion(c, st, 678, 674, {"toUnion": {"wp": 677}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 678, "union");
        atStep(st, 679);
        cqWorkplaneNew(c, st, 679, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 680);
        cqSphere(c, st, 680, 679, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 680, "sphere");
        atStep(st, 681);
        cqTranslate(c, st, 681, 680, {"vec": [0, 43.30127018922192, 25.000000000000018]});
        stepVol(c, st, 681, "translate");
        atStep(st, 682);
        cqUnion(c, st, 682, 678, {"toUnion": {"wp": 681}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 682, "union");
        atStep(st, 683);
        cqWorkplaneNew(c, st, 683, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 684);
        cqSphere(c, st, 684, 683, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 684, "sphere");
        atStep(st, 685);
        cqTranslate(c, st, 685, 684, {"vec": [5, 43.30127018922192, 24.494897427831795]});
        stepVol(c, st, 685, "translate");
        atStep(st, 686);
        cqUnion(c, st, 686, 682, {"toUnion": {"wp": 685}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 686, "union");
        atStep(st, 687);
        cqWorkplaneNew(c, st, 687, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 688);
        cqSphere(c, st, 688, 687, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 688, "sphere");
        atStep(st, 689);
        cqTranslate(c, st, 689, 688, {"vec": [10, 43.30127018922192, 22.912878474779216]});
        stepVol(c, st, 689, "translate");
        atStep(st, 690);
        cqUnion(c, st, 690, 686, {"toUnion": {"wp": 689}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 690, "union");
        atStep(st, 691);
        cqWorkplaneNew(c, st, 691, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 692);
        cqSphere(c, st, 692, 691, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 692, "sphere");
        atStep(st, 693);
        cqTranslate(c, st, 693, 692, {"vec": [15, 43.30127018922192, 20.000000000000018]});
        stepVol(c, st, 693, "translate");
        atStep(st, 694);
        cqUnion(c, st, 694, 690, {"toUnion": {"wp": 693}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 694, "union");
        atStep(st, 695);
        cqWorkplaneNew(c, st, 695, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 696);
        cqSphere(c, st, 696, 695, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 696, "sphere");
        atStep(st, 697);
        cqTranslate(c, st, 697, 696, {"vec": [20, 43.30127018922192, 15.000000000000025]});
        stepVol(c, st, 697, "translate");
        atStep(st, 698);
        cqUnion(c, st, 698, 694, {"toUnion": {"wp": 697}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 698, "union");
        atStep(st, 699);
        cqWorkplaneNew(c, st, 699, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 700);
        cqSphere(c, st, 700, 699, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 700, "sphere");
        atStep(st, 701);
        cqTranslate(c, st, 701, 700, {"vec": [25, 43.30127018922192, 8.259061849445711e-7]});
        stepVol(c, st, 701, "translate");
        atStep(st, 702);
        cqUnion(c, st, 702, 698, {"toUnion": {"wp": 701}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 702, "union");
        atStep(st, 703);
        cqWorkplaneNew(c, st, 703, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 704);
        cqSphere(c, st, 704, 703, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 704, "sphere");
        atStep(st, 705);
        cqTranslate(c, st, 705, 704, {"vec": [-12.5, 47.63139720814412, 8.660254037844412]});
        stepVol(c, st, 705, "translate");
        atStep(st, 706);
        cqUnion(c, st, 706, 702, {"toUnion": {"wp": 705}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 706, "union");
        atStep(st, 707);
        cqWorkplaneNew(c, st, 707, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 708);
        cqSphere(c, st, 708, 707, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 708, "sphere");
        atStep(st, 709);
        cqTranslate(c, st, 709, 708, {"vec": [-7.5, 47.63139720814412, 13.22875655532297]});
        stepVol(c, st, 709, "translate");
        atStep(st, 710);
        cqUnion(c, st, 710, 706, {"toUnion": {"wp": 709}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 710, "union");
        atStep(st, 711);
        cqWorkplaneNew(c, st, 711, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 712);
        cqSphere(c, st, 712, 711, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 712, "sphere");
        atStep(st, 713);
        cqTranslate(c, st, 713, 712, {"vec": [-2.5, 47.63139720814412, 15.000000000000016]});
        stepVol(c, st, 713, "translate");
        atStep(st, 714);
        cqUnion(c, st, 714, 710, {"toUnion": {"wp": 713}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 714, "union");
        atStep(st, 715);
        cqWorkplaneNew(c, st, 715, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 716);
        cqSphere(c, st, 716, 715, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 716, "sphere");
        atStep(st, 717);
        cqTranslate(c, st, 717, 716, {"vec": [2.5, 47.63139720814412, 15.000000000000016]});
        stepVol(c, st, 717, "translate");
        atStep(st, 718);
        cqUnion(c, st, 718, 714, {"toUnion": {"wp": 717}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 718, "union");
        atStep(st, 719);
        cqWorkplaneNew(c, st, 719, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 720);
        cqSphere(c, st, 720, 719, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 720, "sphere");
        atStep(st, 721);
        cqTranslate(c, st, 721, 720, {"vec": [7.5, 47.63139720814412, 13.22875655532297]});
        stepVol(c, st, 721, "translate");
        atStep(st, 722);
        cqUnion(c, st, 722, 718, {"toUnion": {"wp": 721}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 722, "union");
        atStep(st, 723);
        cqWorkplaneNew(c, st, 723, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 724);
        cqSphere(c, st, 724, 723, {"radius": 1, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 724, "sphere");
        atStep(st, 725);
        cqTranslate(c, st, 725, 724, {"vec": [12.5, 47.63139720814412, 8.660254037844412]});
        stepVol(c, st, 725, "translate");
        atStep(st, 726);
        cqUnion(c, st, 726, 722, {"toUnion": {"wp": 725}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 726, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 726}), {"i": 5536});
    } catch (e) {
        return {"i": 5536, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_5536(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c5536"] = r; }
    return out;
}
