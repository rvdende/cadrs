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

const case_5637 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 2);
        cqMoveTo(c, st, 2, 1, {"x": 40, "y": 0});
        atStep(st, 3);
        cqLineTo(c, st, 3, 2, {"x": 10, "y": 80, "forConstruction": false});
        atStep(st, 4);
        cqLineTo(c, st, 4, 3, {"x": 0, "y": 80, "forConstruction": false});
        atStep(st, 5);
        cqLineTo(c, st, 5, 4, {"x": 0, "y": 0, "forConstruction": false});
        atStep(st, 6);
        cqClose(c, st, 6, 5, {});
        atStep(st, 7);
        cqRevolve(c, st, 7, 6, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 7, "revolve");
        atStep(st, 8);
        cqWorkplaneNew(c, st, 8, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 9);
        cqMoveTo(c, st, 9, 8, {"x": 38, "y": 0});
        atStep(st, 10);
        cqLineTo(c, st, 10, 9, {"x": 8, "y": 80, "forConstruction": false});
        atStep(st, 11);
        cqLineTo(c, st, 11, 10, {"x": 0, "y": 80, "forConstruction": false});
        atStep(st, 12);
        cqLineTo(c, st, 12, 11, {"x": 0, "y": 0, "forConstruction": false});
        atStep(st, 13);
        cqClose(c, st, 13, 12, {});
        atStep(st, 14);
        cqRevolve(c, st, 14, 13, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 14, "revolve");
        atStep(st, 15);
        cqCut(c, st, 15, 7, {"toCut": {"wp": 14}, "clean": true, "tol": undefined});
        stepVol(c, st, 15, "cut");
        atStep(st, 16);
        cqWorkplaneNew(c, st, 16, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 17);
        cqMoveTo(c, st, 17, 16, {"x": 36.125, "y": 5});
        atStep(st, 18);
        cqLineTo(c, st, 18, 17, {"x": 33.875, "y": 11, "forConstruction": false});
        atStep(st, 19);
        cqLineTo(c, st, 19, 18, {"x": 35.275, "y": 11, "forConstruction": false});
        atStep(st, 20);
        cqLineTo(c, st, 20, 19, {"x": 37.525, "y": 5, "forConstruction": false});
        atStep(st, 21);
        cqClose(c, st, 21, 20, {});
        atStep(st, 22);
        cqRevolve(c, st, 22, 21, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 22, "revolve");
        atStep(st, 23);
        cqCut(c, st, 23, 15, {"toCut": {"wp": 22}, "clean": true, "tol": undefined});
        stepVol(c, st, 23, "cut");
        atStep(st, 24);
        cqWorkplaneNew(c, st, 24, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 25);
        cqPushPoints(c, st, 25, 24, {"pntList": [[38.125, 5]]});
        atStep(st, 26);
        cqCircle(c, st, 26, 25, {"radius": 0.8, "forConstruction": false});
        atStep(st, 27);
        cqRevolve(c, st, 27, 26, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 27, "revolve");
        atStep(st, 28);
        cqCut(c, st, 28, 23, {"toCut": {"wp": 27}, "clean": true, "tol": undefined});
        stepVol(c, st, 28, "cut");
        atStep(st, 29);
        cqWorkplaneNew(c, st, 29, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 30);
        cqPushPoints(c, st, 30, 29, {"pntList": [[35.875, 11]]});
        atStep(st, 31);
        cqCircle(c, st, 31, 30, {"radius": 0.8, "forConstruction": false});
        atStep(st, 32);
        cqRevolve(c, st, 32, 31, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 32, "revolve");
        atStep(st, 33);
        cqCut(c, st, 33, 28, {"toCut": {"wp": 32}, "clean": true, "tol": undefined});
        stepVol(c, st, 33, "cut");
        // step 34: guard_begin {"block":1,"g":1,"op":"guard_begin","skippable":false}
        atStep(st, 35);
        cqWorkplaneNew(c, st, 34, {"plane": {"origin": [37.68, 0, 0], "xDir": [-0.9850546784467606, -0.1722419242523406, -3.469446951953615e-18], "zDir": [-0.1711433781415989, 0.9787720733808172, 0.11276157363258855]}, "obj": undefined});
        atStep(st, 36);
        cqCircle(c, st, 35, 34, {"radius": 0.8, "forConstruction": false});
        atStep(st, 37);
        cqExtrude(c, st, 36, 35, {"until": 9.853632539143597, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 36, "extrude");
        // step 38: guard_end {"block":1,"g":1,"op":"guard_end"}
        // step 39: guard_begin {"block":1,"g":2,"op":"guard_begin","skippable":false}
        atStep(st, 40);
        cqWorkplaneNew(c, st, 37, {"plane": {"origin": [35.993616040284984, 9.644460350670265, 1.1111111111111112], "xDir": [-0.9067104509495664, -0.4217536699767222, 0], "zDir": [-0.41900362617366016, 0.9007982476083528, 0.11401086947762634]}, "obj": undefined});
        atStep(st, 41);
        cqCircle(c, st, 38, 37, {"radius": 0.8, "forConstruction": false});
        atStep(st, 42);
        cqExtrude(c, st, 39, 38, {"until": 9.745659481433542, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 39, "extrude");
        // step 43: guard_end {"block":1,"g":2,"op":"guard_end"}
        atStep(st, 44);
        cqUnion(c, st, 40, 36, {"toUnion": {"wp": 39}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 40, "union");
        // step 45: guard_begin {"block":1,"g":3,"op":"guard_begin","skippable":false}
        atStep(st, 46);
        cqWorkplaneNew(c, st, 41, {"plane": {"origin": [31.910149378110617, 18.42333333333333, 2.2222222222222223], "xDir": [-0.7663458196502252, -0.6424282720309128, -6.9388939039072276e-18], "zDir": [-0.63814463405623, 0.7612359136922958, 0.11528794269261612]}, "obj": undefined});
        atStep(st, 47);
        cqCircle(c, st, 42, 41, {"radius": 0.8, "forConstruction": false});
        atStep(st, 48);
        cqExtrude(c, st, 43, 42, {"until": 9.63770438747082, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 43, "extrude");
        // step 49: guard_end {"block":1,"g":3,"op":"guard_end"}
        atStep(st, 50);
        cqUnion(c, st, 44, 40, {"toUnion": {"wp": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 44, "union");
        // step 51: guard_begin {"block":1,"g":4,"op":"guard_begin","skippable":false}
        atStep(st, 52);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [25.75990003862593, 25.75990003862593, 3.333333333333333], "xDir": [-0.5735543912087622, -0.8191674800217268, 6.9388939039072276e-18], "zDir": [-0.8135805068453373, 0.5696425745448502, 0.11659372258916371]}, "obj": undefined});
        atStep(st, 53);
        cqCircle(c, st, 46, 45, {"radius": 0.8, "forConstruction": false});
        atStep(st, 54);
        cqExtrude(c, st, 47, 46, {"until": 9.529767867746072, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 47, "extrude");
        // step 55: guard_end {"block":1,"g":4,"op":"guard_end"}
        atStep(st, 56);
        cqUnion(c, st, 48, 44, {"toUnion": {"wp": 47}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 48, "union");
        // step 57: guard_begin {"block":1,"g":5,"op":"guard_begin","skippable":false}
        atStep(st, 58);
        cqWorkplaneNew(c, st, 49, {"plane": {"origin": [18.00666666666667, 31.188461541623585, 4.444444444444445], "xDir": [-0.3415179857458632, -0.93987523928024, 0], "zDir": [-0.9333167974371795, 0.33913487599441505, 0.11792918004404036]}, "obj": undefined});
        atStep(st, 59);
        cqCircle(c, st, 50, 49, {"radius": 0.8, "forConstruction": false});
        atStep(st, 60);
        cqExtrude(c, st, 51, 50, {"until": 9.42185056061756, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 51, "extrude");
        // step 61: guard_end {"block":1,"g":5,"op":"guard_end"}
        atStep(st, 62);
        cqUnion(c, st, 52, 48, {"toUnion": {"wp": 51}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 52, "union");
        // step 63: guard_begin {"block":1,"g":6,"op":"guard_begin","skippable":false}
        atStep(st, 64);
        cqWorkplaneNew(c, st, 53, {"plane": {"origin": [9.213095275499397, 34.38373966313653, 5.555555555555555], "xDir": [-0.08610602471275829, -0.9962859792791254, 0], "zDir": [-0.9891713156633498, 0.08549112556345358, 0.11929532982783232]}, "obj": undefined});
        atStep(st, 65);
        cqCircle(c, st, 54, 53, {"radius": 0.8, "forConstruction": false});
        atStep(st, 66);
        cqExtrude(c, st, 55, 54, {"until": 9.313953133912891, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 55, "extrude");
        // step 67: guard_end {"block":1,"g":6,"op":"guard_end"}
        atStep(st, 68);
        cqUnion(c, st, 56, 52, {"toUnion": {"wp": 55}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 56, "union");
        // step 69: guard_begin {"block":1,"g":7,"op":"guard_begin","skippable":false}
        atStep(st, 70);
        cqWorkplaneNew(c, st, 57, {"plane": {"origin": [2.1541537197001944e-15, 35.18, 6.666666666666666], "xDir": [0.17520924960681375, -0.9845312178149646, -3.4694469519536138e-18], "zDir": [-0.9773341495083036, -0.17392844416909725, 0.12069323309033562]}, "obj": undefined});
        atStep(st, 71);
        cqCircle(c, st, 58, 57, {"radius": 0.8, "forConstruction": false});
        atStep(st, 72);
        cqExtrude(c, st, 59, 58, {"until": 9.206076286642144, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 59, "extrude");
        // step 73: guard_end {"block":1,"g":7,"op":"guard_end"}
        atStep(st, 74);
        cqUnion(c, st, 60, 56, {"toUnion": {"wp": 59}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 60, "union");
        // step 75: guard_begin {"block":1,"g":8,"op":"guard_begin","skippable":false}
        atStep(st, 76);
        cqWorkplaneNew(c, st, 61, {"plane": {"origin": [-8.99741273791396, 33.57880147456231, 7.777777777777778], "xDir": [0.42454761860412527, -0.9054056105070072, 0], "zDir": [-0.8986285142259686, -0.421369816242646, 0.12212400001501345]}, "obj": undefined});
        atStep(st, 77);
        cqCircle(c, st, 62, 61, {"radius": 0.8, "forConstruction": false});
        atStep(st, 78);
        cqExtrude(c, st, 63, 62, {"until": 9.098220750831254, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 63, "extrude");
        // step 79: guard_end {"block":1,"g":8,"op":"guard_end"}
        atStep(st, 80);
        cqUnion(c, st, 64, 60, {"toUnion": {"wp": 63}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 64, "union");
        // step 81: guard_begin {"block":1,"g":9,"op":"guard_begin","skippable":false}
        atStep(st, 82);
        cqWorkplaneNew(c, st, 65, {"plane": {"origin": [-17.173333333333325, 29.74508586864952, 8.88888888888889], "xDir": [0.6448441258747676, -0.7643141064541509, 0], "zDir": [-0.7584545058772901, -0.6399004397906645, 0.12358879265592863]}, "obj": undefined});
        atStep(st, 83);
        cqCircle(c, st, 66, 65, {"radius": 0.8, "forConstruction": false});
        atStep(st, 84);
        cqExtrude(c, st, 67, 66, {"until": 8.99038729348579, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 67, "extrude");
        // step 85: guard_end {"block":1,"g":9,"op":"guard_end"}
        atStep(st, 86);
        cqUnion(c, st, 68, 64, {"toUnion": {"wp": 67}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 68, "union");
        // step 87: guard_begin {"block":1,"g":10,"op":"guard_begin","skippable":false}
        atStep(st, 88);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [-23.992133085659557, 23.99213308565956, 10], "xDir": [0.8210170531633941, -0.5709036682443867, -6.9388939039072276e-18], "zDir": [-0.5664195318362111, -0.8145684127629143, 0.12508882797178306]}, "obj": undefined});
        atStep(st, 89);
        cqCircle(c, st, 70, 69, {"radius": 0.8, "forConstruction": false});
        atStep(st, 90);
        cqExtrude(c, st, 71, 70, {"until": 8.882576718695855, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 71, "extrude");
        // step 91: guard_end {"block":1,"g":10,"op":"guard_end"}
        atStep(st, 92);
        cqUnion(c, st, 72, 68, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        // step 93: guard_begin {"block":1,"g":11,"op":"guard_begin","skippable":false}
        atStep(st, 94);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [-29.02339803216249, 16.756666666666664, 11.11111111111111], "xDir": [0.941000708943282, -0.3384045888699512, 0], "zDir": [-0.3356806383885413, -0.9334262273362428, 0.1266253810730303]}, "obj": undefined});
        atStep(st, 95);
        cqCircle(c, st, 74, 73, {"radius": 0.8, "forConstruction": false});
        atStep(st, 96);
        cqExtrude(c, st, 75, 74, {"until": 8.774789869894148, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 75, "extrude");
        // step 97: guard_end {"block":1,"g":11,"op":"guard_end"}
        atStep(st, 98);
        cqUnion(c, st, 76, 72, {"toUnion": {"wp": 75}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 76, "union");
        // step 99: guard_begin {"block":1,"g":12,"op":"guard_begin","skippable":false}
        atStep(st, 100);
        cqWorkplaneNew(c, st, 77, {"plane": {"origin": [-31.968925097413862, 8.566047662743088, 12.222222222222223], "xDir": [0.9965723038852649, -0.08272631460917318, 0], "zDir": [-0.08204368703495878, -0.988348950318212, 0.12819978869950674]}, "obj": undefined});
        atStep(st, 101);
        cqCircle(c, st, 78, 77, {"radius": 0.8, "forConstruction": false});
        atStep(st, 102);
        cqExtrude(c, st, 79, 78, {"until": 8.667027632280211, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 79, "extrude");
        // step 103: guard_end {"block":1,"g":12,"op":"guard_end"}
        atStep(st, 104);
        cqUnion(c, st, 80, 76, {"toUnion": {"wp": 79}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 80, "union");
        // step 105: guard_begin {"block":1,"g":13,"op":"guard_begin","skippable":false}
        atStep(st, 106);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [-32.68, 4.00214573961355e-15, 13.333333333333332], "xDir": [0.9839162286886788, 0.17863049830039615, 0], "zDir": [0.17711900466963498, -0.975590757243346, 0.129813452947654]}, "obj": undefined});
        atStep(st, 107);
        cqCircle(c, st, 82, 81, {"radius": 0.8, "forConstruction": false});
        atStep(st, 108);
        cqExtrude(c, st, 83, 82, {"until": 8.559290935425292, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 83, "extrude");
        // step 109: guard_end {"block":1,"g":13,"op":"guard_end"}
        atStep(st, 110);
        cqUnion(c, st, 84, 80, {"toUnion": {"wp": 83}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 84, "union");
        // step 111: guard_begin {"block":1,"g":14,"op":"guard_begin","skippable":false}
        atStep(st, 112);
        cqWorkplaneNew(c, st, 85, {"plane": {"origin": [-31.163986908839643, -8.350365125157664, 14.444444444444445], "xDir": [0.903886976749236, 0.42777135629109897, -6.938893903907228e-18], "zDir": [0.42405848717421696, -0.8960416313520553, 0.1314678452681946]}, "obj": undefined});
        atStep(st, 113);
        cqCircle(c, st, 86, 85, {"radius": 0.8, "forConstruction": false});
        atStep(st, 114);
        cqExtrude(c, st, 87, 86, {"until": 8.451580756073415, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 87, "extrude");
        // step 115: guard_end {"block":1,"g":14,"op":"guard_end"}
        atStep(st, 116);
        cqUnion(c, st, 88, 84, {"toUnion": {"wp": 87}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 88, "union");
        // step 117: guard_begin {"block":1,"g":15,"op":"guard_begin","skippable":false}
        atStep(st, 118);
        cqWorkplaneNew(c, st, 89, {"plane": {"origin": [-27.580022359188423, -15.923333333333323, 15.555555555555555], "xDir": [0.7619525442369653, 0.6476328592117724, 0], "zDir": [0.6418650070661284, -0.7551665549922181, 0.13316451075712504]}, "obj": undefined});
        atStep(st, 119);
        cqCircle(c, st, 90, 89, {"radius": 0.8, "forConstruction": false});
        atStep(st, 120);
        cqExtrude(c, st, 91, 90, {"until": 8.343898121156593, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 91, "extrude");
        // step 121: guard_end {"block":1,"g":15,"op":"guard_end"}
        atStep(st, 122);
        cqUnion(c, st, 92, 88, {"toUnion": {"wp": 91}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 92, "union");
        // step 123: guard_begin {"block":1,"g":16,"op":"guard_begin","skippable":false}
        atStep(st, 124);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [-22.224366132693195, -22.224366132693188, 16.666666666666668], "xDir": [0.5678219876968056, 0.8231513775047995, -6.93889390390723e-18], "zDir": [0.8156265617503905, -0.5626312585606015, 0.13490507276507488]}, "obj": undefined});
        atStep(st, 125);
        cqCircle(c, st, 94, 93, {"radius": 0.8, "forConstruction": false});
        atStep(st, 126);
        cqExtrude(c, st, 95, 94, {"until": 8.236244111042522, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 95, "extrude");
        // step 127: guard_end {"block":1,"g":16,"op":"guard_end"}
        atStep(st, 128);
        cqUnion(c, st, 96, 92, {"toUnion": {"wp": 95}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 96, "union");
        // step 129: guard_begin {"block":1,"g":17,"op":"guard_begin","skippable":false}
        atStep(st, 130);
        cqWorkplaneNew(c, st, 97, {"plane": {"origin": [-15.506666666666682, -26.858334522701384, 17.77777777777778], "xDir": [0.3347824315304279, 0.9422954544826028, 0], "zDir": [0.933450788021608, -0.33164006367775345, 0.13669123785252182]}, "obj": undefined});
        atStep(st, 131);
        cqCircle(c, st, 98, 97, {"radius": 0.8, "forConstruction": false});
        atStep(st, 132);
        cqExtrude(c, st, 99, 98, {"until": 8.128619863036903, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 99, "extrude");
        // step 133: guard_end {"block":1,"g":17,"op":"guard_end"}
        atStep(st, 134);
        cqUnion(c, st, 100, 96, {"toUnion": {"wp": 99}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 100, "union");
        // step 135: guard_begin {"block":1,"g":18,"op":"guard_begin","skippable":false}
        atStep(st, 136);
        cqWorkplaneNew(c, st, 101, {"plane": {"origin": [-7.919000049986789, -29.55411053169119, 18.88888888888889], "xDir": [0.07879071677041759, 0.996891179091682, 1.7347234759768069e-18], "zDir": [0.9872801163018492, -0.07803109270911696, 0.1385248011210563]}, "obj": undefined});
        atStep(st, 137);
        cqCircle(c, st, 102, 101, {"radius": 0.8, "forConstruction": false});
        atStep(st, 138);
        cqExtrude(c, st, 103, 102, {"until": 8.02102657516263, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 103, "extrude");
        // step 139: guard_end {"block":1,"g":18,"op":"guard_end"}
        atStep(st, 140);
        cqUnion(c, st, 104, 100, {"toUnion": {"wp": 103}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 104, "union");
        // step 141: guard_begin {"block":1,"g":19,"op":"guard_begin","skippable":false}
        atStep(st, 142);
        cqWorkplaneNew(c, st, 105, {"plane": {"origin": [-5.543976059740068e-15, -30.18, 20], "xDir": [-0.18261829365848384, 0.9831838886094828, 3.469446951953614e-18], "zDir": [0.9734442517542252, 0.1808092364882367, 0.14040765195386487]}, "obj": undefined});
        atStep(st, 143);
        cqCircle(c, st, 106, 105, {"radius": 0.8, "forConstruction": false});
        atStep(st, 144);
        cqExtrude(c, st, 107, 106, {"until": 7.913465510242986, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 107, "extrude");
        // step 145: guard_end {"block":1,"g":19,"op":"guard_end"}
        atStep(st, 146);
        cqUnion(c, st, 108, 104, {"toUnion": {"wp": 107}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 108, "union");
        // step 147: guard_begin {"block":1,"g":20,"op":"guard_begin","skippable":false}
        atStep(st, 148);
        cqWorkplaneNew(c, st, 109, {"plane": {"origin": [7.7033175124013455, -28.74917234311697, 21.11111111111111], "xDir": [-0.4315321425658394, 0.9020975612052924, 0], "zDir": [0.892912013988552, 0.4271380958002284, 0.14234178020195923]}, "obj": undefined});
        atStep(st, 149);
        cqCircle(c, st, 110, 109, {"radius": 0.8, "forConstruction": false});
        atStep(st, 150);
        cqExtrude(c, st, 111, 110, {"until": 7.805938000316067, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 111, "extrude");
        // step 151: guard_end {"block":1,"g":20,"op":"guard_end"}
        atStep(st, 152);
        cqUnion(c, st, 112, 108, {"toUnion": {"wp": 111}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 112, "union");
        // step 153: guard_begin {"block":1,"g":21,"op":"guard_begin","skippable":false}
        atStep(st, 154);
        cqWorkplaneNew(c, st, 113, {"plane": {"origin": [14.673333333333336, -25.414958849727324, 22.22222222222222], "xDir": [-0.6508878264205878, 0.7591739177668599, -1.3877787807814457e-17], "zDir": [0.7512251509597838, 0.6440728457834753, 0.14432928285634083]}, "obj": undefined});
        atStep(st, 155);
        cqCircle(c, st, 114, 113, {"radius": 0.8, "forConstruction": false});
        atStep(st, 156);
        cqExtrude(c, st, 115, 114, {"until": 7.698445451413117, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 115, "extrude");
        // step 157: guard_end {"block":1,"g":21,"op":"guard_end"}
        atStep(st, 158);
        cqUnion(c, st, 116, 112, {"toUnion": {"wp": 115}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 116, "union");
        // step 159: guard_begin {"block":1,"g":22,"op":"guard_begin","skippable":false}
        atStep(st, 160);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [20.456599179726815, -20.456599179726823, 23.333333333333336], "xDir": [-0.825641460249141, 0.5641951604876334, 0], "zDir": [0.5581185322330889, 0.8167489411762426, 0.14637237125041447]}, "obj": undefined});
        atStep(st, 161);
        cqCircle(c, st, 118, 117, {"radius": 0.8, "forConstruction": false});
        atStep(st, 162);
        cqExtrude(c, st, 119, 118, {"until": 7.590989348735885, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 119, "extrude");
        // step 163: guard_end {"block":1,"g":22,"op":"guard_end"}
        atStep(st, 164);
        cqUnion(c, st, 120, 116, {"toUnion": {"wp": 119}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 120, "union");
        // step 165: guard_begin {"block":1,"g":23,"op":"guard_begin","skippable":false}
        atStep(st, 166);
        cqWorkplaneNew(c, st, 121, {"plane": {"origin": [24.6932710132403, -14.256666666666655, 24.444444444444446], "xDir": [-0.9438004113633444, 0.33051593533501833, 0], "zDir": [0.32685263051612495, 0.933339709698487, 0.14847337884157388]}, "obj": undefined});
        atStep(st, 167);
        cqCircle(c, st, 122, 121, {"radius": 0.8, "forConstruction": false});
        atStep(st, 168);
        cqExtrude(c, st, 123, 122, {"until": 7.483571262271201, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 123, "extrude");
        // step 169: guard_end {"block":1,"g":23,"op":"guard_end"}
        atStep(st, 170);
        cqUnion(c, st, 124, 120, {"toUnion": {"wp": 123}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 124, "union");
        // step 171: guard_begin {"block":1,"g":24,"op":"guard_begin","skippable":false}
        atStep(st, 172);
        cqWorkplaneNew(c, st, 125, {"plane": {"origin": [27.13929596596852, -7.271952437230515, 25.555555555555554], "xDir": [-0.997247096912072, 0.07415003506704715, 3.469446951953614e-18], "zDir": [0.07330394484192258, 0.9858679921015588, 0.15063476962591696]}, "obj": undefined});
        atStep(st, 173);
        cqCircle(c, st, 126, 125, {"radius": 0.8, "forConstruction": false});
        atStep(st, 174);
        cqExtrude(c, st, 127, 126, {"until": 7.376192852887946, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 127, "extrude");
        // step 175: guard_end {"block":1,"g":24,"op":"guard_end"}
        atStep(st, 176);
        cqUnion(c, st, 128, 124, {"toUnion": {"wp": 127}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 128, "union");
        // step 177: guard_begin {"block":1,"g":25,"op":"guard_begin","skippable":false}
        atStep(st, 178);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [27.68, -6.7796446800797465e-15, 26.666666666666664], "xDir": [-0.982297837942944, -0.18732580594412995, -3.469446951953614e-18], "zDir": [-0.18512435038094707, 0.9707538596365756, 0.15285914724583527]}, "obj": undefined});
        atStep(st, 179);
        cqCircle(c, st, 130, 129, {"radius": 0.8, "forConstruction": false});
        atStep(st, 180);
        cqExtrude(c, st, 131, 130, {"until": 7.268855878962697, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 131, "extrude");
        // step 181: guard_end {"block":1,"g":25,"op":"guard_end"}
        atStep(st, 182);
        cqUnion(c, st, 132, 128, {"toUnion": {"wp": 131}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 132, "union");
        // step 183: guard_begin {"block":1,"g":26,"op":"guard_begin","skippable":false}
        atStep(st, 184);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [26.334357777394303, 7.056269899645044, 27.77777777777778], "xDir": [-0.8999582489576635, -0.4359760889464654, 6.938893903907228e-18], "zDir": [-0.4306968716380858, 0.8890606899282285, 0.15514926485648808]}, "obj": undefined});
        atStep(st, 185);
        cqCircle(c, st, 134, 133, {"radius": 0.8, "forConstruction": false});
        atStep(st, 186);
        cqExtrude(c, st, 135, 134, {"until": 7.1615622035907185, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 135, "extrude");
        // step 187: guard_end {"block":1,"g":26,"op":"guard_end"}
        atStep(st, 188);
        cqUnion(c, st, 136, 132, {"toUnion": {"wp": 135}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 136, "union");
        // step 189: guard_begin {"block":1,"g":27,"op":"guard_begin","skippable":false}
        atStep(st, 190);
        cqWorkplaneNew(c, st, 137, {"plane": {"origin": [23.249895340266225, 13.423333333333332, 28.88888888888889], "xDir": [-0.7558573839817038, -0.6547362943050702, 0], "zDir": [-0.6465636830513498, 0.7464225495051866, 0.1575080358243695]}, "obj": undefined});
        atStep(st, 191);
        cqCircle(c, st, 138, 137, {"radius": 0.8, "forConstruction": false});
        atStep(st, 192);
        cqExtrude(c, st, 139, 138, {"until": 7.054313802440299, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 139, "extrude");
        // step 193: guard_end {"block":1,"g":27,"op":"guard_end"}
        atStep(st, 194);
        cqUnion(c, st, 140, 136, {"toUnion": {"wp": 139}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 140, "union");
        // step 195: guard_begin {"block":1,"g":28,"op":"guard_begin","skippable":false}
        atStep(st, 196);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [18.688832226760457, 18.688832226760447, 30], "xDir": [-0.5598648421115998, -0.8285839478093656, 1.3877787807814457e-17], "zDir": [-0.8179175646512672, 0.5526576871353311, 0.15993854533903543]}, "obj": undefined});
        atStep(st, 197);
        cqCircle(c, st, 142, 141, {"radius": 0.8, "forConstruction": false});
        atStep(st, 198);
        cqExtrude(c, st, 143, 142, {"until": 6.947112772320104, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 143, "extrude");
        // step 199: guard_end {"block":1,"g":28,"op":"guard_end"}
        atStep(st, 200);
        cqUnion(c, st, 144, 140, {"toUnion": {"wp": 143}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 144, "union");
        // step 201: guard_begin {"block":1,"g":29,"op":"guard_begin","skippable":false}
        atStep(st, 202);
        cqWorkplaneNew(c, st, 145, {"plane": {"origin": [13.006666666666682, 22.528207503779193, 31.11111111111111], "xDir": [-0.32541675507738704, -0.9455706930287676, 0], "zDir": [-0.933011390630795, 0.32109448973803645, 0.16244406302804737]}, "obj": undefined});
        atStep(st, 203);
        cqCircle(c, st, 146, 145, {"radius": 0.8, "forConstruction": false});
        atStep(st, 204);
        cqExtrude(c, st, 147, 146, {"until": 6.839961340533989, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 147, "extrude");
        // step 205: guard_end {"block":1,"g":29,"op":"guard_end"}
        atStep(st, 206);
        cqUnion(c, st, 148, 144, {"toUnion": {"wp": 147}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 148, "union");
        // step 207: guard_begin {"block":1,"g":30,"op":"guard_begin","skippable":false}
        atStep(st, 208);
        cqWorkplaneNew(c, st, 149, {"plane": {"origin": [6.624904824474188, 24.72448140024585, 32.22222222222222], "xDir": [-0.06859655039855042, -0.9976444824051398, 0], "zDir": [-0.9839656519564076, 0.06765601436711008, 0.1650280566750981]}, "obj": undefined});
        atStep(st, 209);
        cqCircle(c, st, 150, 149, {"radius": 0.8, "forConstruction": false});
        atStep(st, 210);
        cqExtrude(c, st, 151, 150, {"until": 6.732861875109115, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 151, "extrude");
        // step 211: guard_end {"block":1,"g":30,"op":"guard_end"}
        atStep(st, 212);
        cqUnion(c, st, 152, 148, {"toUnion": {"wp": 151}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 152, "union");
        // step 213: guard_begin {"block":1,"g":31,"op":"guard_begin","skippable":false}
        atStep(st, 214);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [7.709151600632588e-15, 25.18, 33.333333333333336], "xDir": [0.19296676226914725, -0.9812052938398582, 6.938893903907228e-18], "zDir": [-0.9673105048774532, -0.1902341715918226, 0.16769420715248304]}, "obj": undefined});
        atStep(st, 215);
        cqCircle(c, st, 154, 153, {"radius": 0.8, "forConstruction": false});
        atStep(st, 216);
        cqExtrude(c, st, 155, 154, {"until": 6.625816895993209, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 155, "extrude");
        // step 217: guard_end {"block":1,"g":31,"op":"guard_end"}
        atStep(st, 218);
        cqUnion(c, st, 156, 152, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        // step 219: guard_begin {"block":1,"g":32,"op":"guard_begin","skippable":false}
        atStep(st, 220);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [-6.409222286888741, 23.91954321167163, 34.44444444444444], "xDir": [0.44130745415742023, -0.8973559666626709, 0], "zDir": [-0.8842249074530149, -0.43484977791133134, 0.1704464246916706]}, "obj": undefined});
        atStep(st, 221);
        cqCircle(c, st, 158, 157, {"radius": 0.8, "forConstruction": false});
        atStep(st, 222);
        cqExtrude(c, st, 159, 158, {"until": 6.518829087327945, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 159, "extrude");
        // step 223: guard_end {"block":1,"g":32,"op":"guard_end"}
        atStep(st, 224);
        cqUnion(c, st, 160, 156, {"toUnion": {"wp": 159}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 160, "union");
        // step 225: guard_begin {"block":1,"g":33,"op":"guard_begin","skippable":false}
        atStep(st, 226);
        cqWorkplaneNew(c, st, 161, {"plane": {"origin": [-12.173333333333314, 21.084831830805147, 35.55555555555556], "xDir": [0.6593563231614078, -0.7518305920266009, 0], "zDir": [-0.7404561783217546, -0.6493809488176635, 0.17328886662972093]}, "obj": undefined});
        atStep(st, 227);
        cqCircle(c, st, 162, 161, {"radius": 0.8, "forConstruction": false});
        atStep(st, 228);
        cqExtrude(c, st, 163, 162, {"until": 6.411901310920915, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 163, "extrude");
        // step 229: guard_end {"block":1,"g":33,"op":"guard_end"}
        atStep(st, 230);
        cqUnion(c, st, 164, 160, {"toUnion": {"wp": 163}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 164, "union");
        // step 231: guard_begin {"block":1,"g":34,"op":"guard_begin","skippable":false}
        atStep(st, 232);
        cqWorkplaneNew(c, st, 165, {"plane": {"origin": [-16.921065273794063, 16.921065273794103, 36.666666666666664], "xDir": [0.83211377344814, -0.5546049657530104, 0], "zDir": [-0.5459252542690561, -0.8190909771853117, 0.1762259567852528]}, "obj": undefined});
        atStep(st, 233);
        cqCircle(c, st, 166, 165, {"radius": 0.8, "forConstruction": false});
        atStep(st, 234);
        cqExtrude(c, st, 167, 166, {"until": 6.305036621052955, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 167, "extrude");
        // step 235: guard_end {"block":1,"g":34,"op":"guard_end"}
        atStep(st, 236);
        cqUnion(c, st, 168, 164, {"toUnion": {"wp": 167}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 168, "union");
        // step 237: guard_begin {"block":1,"g":35,"op":"guard_begin","skippable":false}
        atStep(st, 238);
        cqWorkplaneNew(c, st, 169, {"plane": {"origin": [-20.363143994318108, 11.756666666666662, 37.77777777777778], "xDir": [0.9476821847536804, -0.3192154079935539, 0], "zDir": [-0.3140445320152, -0.9323309613429492, 0.1792624066353186]}, "obj": undefined});
        atStep(st, 239);
        cqCircle(c, st, 170, 169, {"radius": 0.8, "forConstruction": false});
        atStep(st, 240);
        cqExtrude(c, st, 171, 170, {"until": 6.198238280776233, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 171, "extrude");
        // step 241: guard_end {"block":1,"g":35,"op":"guard_end"}
        atStep(st, 242);
        cqUnion(c, st, 172, 168, {"toUnion": {"wp": 171}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 172, "union");
        // step 243: guard_begin {"block":1,"g":36,"op":"guard_begin","skippable":false}
        atStep(st, 244);
        cqWorkplaneNew(c, st, 173, {"plane": {"origin": [-22.309666834523178, 5.977857211717888, 38.888888888888886], "xDir": [0.998086553812946, -0.06183228200380188, 1.734723475976807e-18], "zDir": [-0.060794971831123826, -0.9813424631546116, 0.1824032384846629]}, "obj": undefined});
        atStep(st, 245);
        cqCircle(c, st, 174, 173, {"radius": 0.8, "forConstruction": false});
        atStep(st, 246);
        cqExtrude(c, st, 175, 174, {"until": 6.091509779880036, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 175, "extrude");
        // step 247: guard_end {"block":1,"g":36,"op":"guard_end"}
        atStep(st, 248);
        cqUnion(c, st, 176, 172, {"toUnion": {"wp": 175}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 176, "union");
        // step 249: guard_begin {"block":1,"g":37,"op":"guard_begin","skippable":false}
        atStep(st, 250);
        cqWorkplaneNew(c, st, 177, {"plane": {"origin": [-22.68, 8.332496821398591e-15, 40], "xDir": [0.979826795582744, 0.19984856931700007, -6.938893903907228e-18], "zDir": [0.19637424508692491, -0.962792718287098, 0.18565381084125587]}, "obj": undefined});
        atStep(st, 251);
        cqCircle(c, st, 178, 177, {"radius": 0.8, "forConstruction": false});
        atStep(st, 252);
        cqExtrude(c, st, 179, 178, {"until": 5.984854854722954, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 179, "extrude");
        // step 253: guard_end {"block":1,"g":37,"op":"guard_end"}
        atStep(st, 254);
        cqUnion(c, st, 180, 176, {"toUnion": {"wp": 179}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 180, "union");
        // step 255: guard_begin {"block":1,"g":38,"op":"guard_begin","skippable":false}
        atStep(st, 256);
        cqWorkplaneNew(c, st, 181, {"plane": {"origin": [-21.50472864594896, -5.76217467413244, 41.11111111111111], "xDir": [0.894123398032403, 0.4478206662169456, 0], "zDir": [0.4397479227778546, -0.8780052745519047, 0.1890198462373634]}, "obj": undefined});
        atStep(st, 257);
        cqCircle(c, st, 182, 181, {"radius": 0.8, "forConstruction": false});
        atStep(st, 258);
        cqExtrude(c, st, 183, 182, {"until": 5.8782775101606335, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 183, "extrude");
        // step 259: guard_end {"block":1,"g":38,"op":"guard_end"}
        atStep(st, 260);
        cqUnion(c, st, 184, 180, {"toUnion": {"wp": 183}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 184, "union");
        // step 261: guard_begin {"block":1,"g":39,"op":"guard_begin","skippable":false}
        atStep(st, 262);
        cqWorkplaneNew(c, st, 185, {"plane": {"origin": [-18.919768321344044, -10.923333333333314, 42.22222222222222], "xDir": [0.7468390852824655, 0.6650047974973189, 1.3877787807814449e-17], "zDir": [0.6525662216479788, -0.7328698407830297, 0.19250746176379183]}, "obj": undefined});
        atStep(st, 263);
        cqCircle(c, st, 186, 185, {"radius": 0.8, "forConstruction": false});
        atStep(st, 264);
        cqExtrude(c, st, 187, 186, {"until": 5.771782043827731, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 187, "extrude");
        // step 265: guard_end {"block":1,"g":39,"op":"guard_end"}
        atStep(st, 266);
        cqUnion(c, st, 188, 184, {"toUnion": {"wp": 187}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 188, "union");
        // step 267: guard_begin {"block":1,"g":40,"op":"guard_begin","skippable":false}
        atStep(st, 268);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [-15.153298320827734, -15.153298320827693, 43.33333333333333], "xDir": [0.5480813301762568, 0.8364250447662509, -1.3877787807814457e-17], "zDir": [0.8201810532562767, -0.5374371863526503, 0.19612320261705765]}, "obj": undefined});
        atStep(st, 269);
        cqCircle(c, st, 190, 189, {"radius": 0.8, "forConstruction": false});
        atStep(st, 270);
        cqExtrude(c, st, 191, 190, {"until": 5.665373073070938, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 191, "extrude");
        // step 271: guard_end {"block":1,"g":40,"op":"guard_end"}
        atStep(st, 272);
        cqUnion(c, st, 192, 188, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        // step 273: guard_begin {"block":1,"g":41,"op":"guard_begin","skippable":false}
        atStep(st, 274);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [-10.506666666666662, -18.198080484857005, 44.44444444444444], "xDir": [0.31151245313588494, 0.9502420699702068, 0], "zDir": [0.9310676979758312, -0.3052266278224517, 0.19987407899505413]}, "obj": undefined});
        atStep(st, 275);
        cqCircle(c, st, 194, 193, {"radius": 0.8, "forConstruction": false});
        atStep(st, 276);
        cqExtrude(c, st, 195, 194, {"until": 5.559055564871964, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 195, "extrude");
        // step 277: guard_end {"block":1,"g":41,"op":"guard_end"}
        atStep(st, 278);
        cqUnion(c, st, 196, 192, {"toUnion": {"wp": 195}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 196, "union");
        // step 279: guard_begin {"block":1,"g":42,"op":"guard_begin","skippable":false}
        atStep(st, 280);
        cqWorkplaneNew(c, st, 197, {"plane": {"origin": [-5.330809598961589, -19.89485226880051, 45.55555555555556], "xDir": [0.05341415068926724, 0.998572445296857, 0], "zDir": [0.9776216824611186, -0.05229348367263376, 0.20376760671723987]}, "obj": undefined});
        atStep(st, 281);
        cqCircle(c, st, 198, 197, {"radius": 0.8, "forConstruction": false});
        atStep(st, 282);
        cqExtrude(c, st, 199, 198, {"until": 5.452834869150515, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 199, "extrude");
        // step 283: guard_end {"block":1,"g":42,"op":"guard_end"}
        atStep(st, 284);
        cqUnion(c, st, 200, 196, {"toUnion": {"wp": 199}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 200, "union");
        // step 285: guard_begin {"block":1,"g":43,"op":"guard_begin","skippable":false}
        atStep(st, 286);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [-8.649680342377755e-15, -20.18, 46.66666666666667], "xDir": [-0.20842976481398853, 0.9780373372932064, 0], "zDir": [0.9566856250124868, 0.20387949643531597, 0.20781185199056784]}, "obj": undefined});
        atStep(st, 287);
        cqCircle(c, st, 202, 201, {"radius": 0.8, "forConstruction": false});
        atStep(st, 288);
        cqExtrude(c, st, 203, 202, {"until": 5.346716755892913, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 203, "extrude");
        // step 289: guard_end {"block":1,"g":43,"op":"guard_end"}
        atStep(st, 290);
        cqUnion(c, st, 204, 200, {"toUnion": {"wp": 203}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 204, "union");
        // step 291: guard_begin {"block":1,"g":44,"op":"guard_begin","skippable":false}
        atStep(st, 292);
        cqWorkplaneNew(c, st, 205, {"plane": {"origin": [5.115127061376138, -19.089914080226286, 47.77777777777778], "xDir": [-0.45595602831298, 0.8900023035054757, 0], "zDir": [0.8697692648722344, 0.4455904642019888, 0.21201548079295837]}, "obj": undefined});
        atStep(st, 293);
        cqCircle(c, st, 206, 205, {"radius": 0.8, "forConstruction": false});
        atStep(st, 294);
        cqExtrude(c, st, 207, 206, {"until": 5.24070745662275, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 207, "extrude");
        // step 295: guard_end {"block":1,"g":44,"op":"guard_end"}
        atStep(st, 296);
        cqUnion(c, st, 208, 204, {"toUnion": {"wp": 207}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 208, "union");
        // step 297: guard_begin {"block":1,"g":45,"op":"guard_begin","skippable":false}
        atStep(st, 298);
        cqWorkplaneNew(c, st, 209, {"plane": {"origin": [9.673333333333344, -16.75470481188293, 48.88888888888889], "xDir": [-0.6720662025806016, 0.7404910663531937, 1.3877787807814457e-17], "zDir": [0.7229469740480234, 0.6561432670733209, 0.2163878134025927]}, "obj": undefined});
        atStep(st, 299);
        cqCircle(c, st, 210, 209, {"radius": 0.8, "forConstruction": false});
        atStep(st, 300);
        cqExtrude(c, st, 211, 210, {"until": 5.134813710806665, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 211, "extrude");
        // step 301: guard_end {"block":1,"g":45,"op":"guard_end"}
        atStep(st, 302);
        cqUnion(c, st, 212, 208, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        // step 303: guard_begin {"block":1,"g":46,"op":"guard_begin","skippable":false}
        atStep(st, 304);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [13.385531367861326, -13.385531367861365, 50], "xDir": [-0.8418071139578506, 0.5397784572303294, 1.387778780781446e-17], "zDir": [0.5264392655635854, 0.8210040858097288, 0.2209388846640719]}, "obj": undefined});
        atStep(st, 305);
        cqCircle(c, st, 214, 213, {"radius": 0.8, "forConstruction": false});
        atStep(st, 306);
        cqExtrude(c, st, 215, 214, {"until": 5.029042817883796, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 215, "extrude");
        // step 307: guard_end {"block":1,"g":46,"op":"guard_end"}
        atStep(st, 308);
        cqUnion(c, st, 216, 212, {"toUnion": {"wp": 215}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 216, "union");
        // step 309: guard_begin {"block":1,"g":47,"op":"guard_begin","skippable":false}
        atStep(st, 310);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [16.033016975395896, -9.256666666666694, 51.11111111111111], "xDir": [-0.9534061543179052, 0.3016897494260333, 1.3877787807814457e-17], "zDir": [0.2939066367538235, 0.9288098014900348, 0.22567951065193775]}, "obj": undefined});
        atStep(st, 311);
        cqCircle(c, st, 218, 217, {"radius": 0.8, "forConstruction": false});
        atStep(st, 312);
        cqExtrude(c, st, 219, 218, {"until": 4.923402695713767, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 219, "extrude");
        // step 313: guard_end {"block":1,"g":47,"op":"guard_end"}
        atStep(st, 314);
        cqUnion(c, st, 220, 216, {"toUnion": {"wp": 219}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 220, "union");
        // step 315: guard_begin {"block":1,"g":48,"op":"guard_begin","skippable":false}
        atStep(st, 316);
        cqWorkplaneNew(c, st, 221, {"plane": {"origin": [17.480037703077837, -4.683761986205288, 52.22222222222222], "xDir": [-0.9990899119359898, 0.042653814222601234, 0], "zDir": [0.04150401962264425, 0.9721580136625922, 0.2306213624683678]}, "obj": undefined});
        atStep(st, 317);
        cqCircle(c, st, 222, 221, {"radius": 0.8, "forConstruction": false});
        atStep(st, 318);
        cqExtrude(c, st, 223, 222, {"until": 4.817901946371114, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 223, "extrude");
        // step 319: guard_end {"block":1,"g":48,"op":"guard_end"}
        atStep(st, 320);
        cqUnion(c, st, 224, 220, {"toUnion": {"wp": 223}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 224, "union");
        // step 321: guard_begin {"block":1,"g":49,"op":"guard_begin","skippable":false}
        atStep(st, 322);
        cqWorkplaneNew(c, st, 225, {"plane": {"origin": [17.68, -8.660702163570082e-15, 53.33333333333333], "xDir": [-0.975629270506468, -0.21942544640724135, 0], "zDir": [-0.21323922299937792, 0.9481235243433616, 0.2357770479952291]}, "obj": undefined});
        atStep(st, 323);
        cqCircle(c, st, 226, 225, {"radius": 0.8, "forConstruction": false});
        atStep(st, 324);
        cqExtrude(c, st, 227, 226, {"until": 4.712549930363015, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 227, "extrude");
        // step 325: guard_end {"block":1,"g":49,"op":"guard_end"}
        atStep(st, 326);
        cqUnion(c, st, 228, 224, {"toUnion": {"wp": 227}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 228, "union");
        // step 327: guard_begin {"block":1,"g":50,"op":"guard_begin","skippable":false}
        atStep(st, 328);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [16.675099514503618, 4.468079448619837, 54.44444444444444], "xDir": [-0.8845727257967579, -0.4664022864185956, 0], "zDir": [-0.45263657227731285, 0.8584648450357173, 0.241160202510379]}, "obj": undefined});
        atStep(st, 329);
        cqCircle(c, st, 230, 229, {"radius": 0.8, "forConstruction": false});
        atStep(st, 330);
        cqExtrude(c, st, 231, 230, {"until": 4.607356850528829, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 231, "extrude");
        // step 331: guard_end {"block":1,"g":50,"op":"guard_end"}
        atStep(st, 332);
        cqUnion(c, st, 232, 228, {"toUnion": {"wp": 231}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 232, "union");
        // step 333: guard_begin {"block":1,"g":51,"op":"guard_begin","skippable":false}
        atStep(st, 334);
        cqWorkplaneNew(c, st, 233, {"plane": {"origin": [14.589641302421851, 8.423333333333318, 55.55555555555556], "xDir": [-0.7321509512014238, -0.6811424114345329, -1.3877787807814457e-17], "zDir": [-0.6600747497754585, 0.7095056008835724, 0.2467855891734904]}, "obj": undefined});
        atStep(st, 335);
        cqCircle(c, st, 234, 233, {"radius": 0.8, "forConstruction": false});
        atStep(st, 336);
        cqExtrude(c, st, 235, 234, {"until": 4.50233384709511, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 235, "extrude");
        // step 337: guard_end {"block":1,"g":51,"op":"guard_end"}
        atStep(st, 338);
        cqUnion(c, st, 236, 232, {"toUnion": {"wp": 235}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 236, "union");
        // step 339: guard_begin {"block":1,"g":52,"op":"guard_begin","skippable":false}
        atStep(st, 340);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [11.617764414894973, 11.61776441489498, 56.66666666666667], "xDir": [-0.5288581492013884, -0.8487102320717489, 1.3877787807814457e-17], "zDir": [-0.8211718953280654, 0.5116981418726234, 0.25266921048560176]}, "obj": undefined});
        atStep(st, 341);
        cqCircle(c, st, 238, 237, {"radius": 0.8, "forConstruction": false});
        atStep(st, 342);
        cqExtrude(c, st, 239, 238, {"until": 4.397493105612974, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 239, "extrude");
        // step 343: guard_end {"block":1,"g":52,"op":"guard_end"}
        atStep(st, 344);
        cqUnion(c, st, 240, 236, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        // step 345: guard_begin {"block":1,"g":53,"op":"guard_begin","skippable":false}
        atStep(st, 346);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [8.006666666666664, 13.86795346593481, 57.77777777777778], "xDir": [-0.2887379207972205, -0.9574081747581322, 0], "zDir": [-0.924782874187186, 0.27889868848168364, 0.2588284319260238]}, "obj": undefined});
        atStep(st, 347);
        cqCircle(c, st, 242, 241, {"radius": 0.8, "forConstruction": false});
        atStep(st, 348);
        cqExtrude(c, st, 243, 242, {"until": 4.292847979810359, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 243, "extrude");
        // step 349: guard_end {"block":1,"g":53,"op":"guard_end"}
        atStep(st, 350);
        cqUnion(c, st, 244, 240, {"toUnion": {"wp": 243}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 244, "union");
        // step 351: guard_begin {"block":1,"g":54,"op":"guard_begin","skippable":false}
        atStep(st, 352);
        cqWorkplaneNew(c, st, 245, {"plane": {"origin": [4.036714373448985, 15.065223137355163, 58.88888888888889], "xDir": [-0.02842175207860449, -0.9995960204046346, 0], "zDir": [-0.9637813287437736, 0.027403424407847453, 0.2652821190649864]}, "obj": undefined});
        atStep(st, 353);
        cqCircle(c, st, 246, 245, {"radius": 0.8, "forConstruction": false});
        atStep(st, 354);
        cqExtrude(c, st, 247, 246, {"until": 4.188413131753209, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 247, "extrude");
        // step 355: guard_end {"block":1,"g":54,"op":"guard_end"}
        atStep(st, 356);
        cqUnion(c, st, 248, 244, {"toUnion": {"wp": 247}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 248, "union");
        // step 357: guard_begin {"block":1,"g":55,"op":"guard_begin","skippable":false}
        atStep(st, 358);
        cqWorkplaneNew(c, st, 249, {"plane": {"origin": [8.36556228497557e-15, 15.18, 60], "xDir": [0.2340146402591025, -0.9722330729533956, 0], "zDir": [-0.93556325499808, -0.2251882852461916, 0.2720507895325381]}, "obj": undefined});
        atStep(st, 359);
        cqCircle(c, st, 250, 249, {"radius": 0.8, "forConstruction": false});
        atStep(st, 360);
        cqExtrude(c, st, 251, 250, {"until": 4.084204692147071, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 251, "extrude");
        // step 361: guard_end {"block":1,"g":55,"op":"guard_end"}
        atStep(st, 362);
        cqUnion(c, st, 252, 248, {"toUnion": {"wp": 251}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 252, "union");
        // step 363: guard_begin {"block":1,"g":56,"op":"guard_begin","skippable":false}
        atStep(st, 364);
        cqWorkplaneNew(c, st, 253, {"plane": {"origin": [-3.821031835863536, 14.260284948780953, 61.11111111111111], "xDir": [0.48029912464055213, -0.877104754786747, 2.7755575615628914e-17], "zDir": [-0.8422359263275728, -0.46120509089511336, 0.279156781281712]}, "obj": undefined});
        atStep(st, 365);
        cqCircle(c, st, 254, 253, {"radius": 0.8, "forConstruction": false});
        atStep(st, 366);
        cqExtrude(c, st, 255, 254, {"until": 3.980240444131761, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 255, "extrude");
        // step 367: guard_end {"block":1,"g":56,"op":"guard_end"}
        atStep(st, 368);
        cqUnion(c, st, 256, 252, {"toUnion": {"wp": 255}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 256, "union");
        // step 369: guard_begin {"block":1,"g":57,"op":"guard_begin","skippable":false}
        atStep(st, 370);
        cqWorkplaneNew(c, st, 257, {"plane": {"origin": [-7.17333333333332, 12.424577792960756, 62.22222222222222], "xDir": [0.6932297014331558, -0.7207167134532804, -2.775557561562892e-17], "zDir": [-0.690477617859796, -0.6641438778098373, 0.28662443859931647]}, "obj": undefined});
        atStep(st, 371);
        cqCircle(c, st, 258, 257, {"radius": 0.8, "forConstruction": false});
        atStep(st, 372);
        cqExtrude(c, st, 259, 258, {"until": 3.8765400345515304, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 259, "extrude");
        // step 373: guard_end {"block":1,"g":57,"op":"guard_end"}
        atStep(st, 374);
        cqUnion(c, st, 260, 256, {"toUnion": {"wp": 259}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 260, "union");
        // step 375: guard_begin {"block":1,"g":58,"op":"guard_begin","skippable":false}
        atStep(st, 376);
        cqWorkplaneNew(c, st, 261, {"plane": {"origin": [-9.849997461928591, 9.849997461928622, 63.33333333333333], "xDir": [0.8578727130232702, -0.5138622463755184, -1.3877787807814457e-17], "zDir": [-0.4910763326857174, -0.8198325305937192, 0.294480317264576]}, "obj": undefined});
        atStep(st, 377);
        cqCircle(c, st, 262, 261, {"radius": 0.8, "forConstruction": false});
        atStep(st, 378);
        cqExtrude(c, st, 263, 262, {"until": 3.7731252174413954, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 263, "extrude");
        // step 379: guard_end {"block":1,"g":58,"op":"guard_end"}
        atStep(st, 380);
        cqUnion(c, st, 264, 260, {"toUnion": {"wp": 263}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 264, "union");
        // step 381: guard_begin {"block":1,"g":59,"op":"guard_begin","skippable":false}
        atStep(st, 382);
        cqWorkplaneNew(c, st, 265, {"plane": {"origin": [-11.702889956473712, 6.756666666666666, 64.44444444444444], "xDir": [0.962609954546752, -0.27089126122394597, 0], "zDir": [-0.2581780426823814, -0.9174336329956434, 0.3027534100965825]}, "obj": undefined});
        atStep(st, 383);
        cqCircle(c, st, 266, 265, {"radius": 0.8, "forConstruction": false});
        atStep(st, 384);
        cqExtrude(c, st, 267, 266, {"until": 3.670020135385608, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 267, "extrude");
        // step 385: guard_end {"block":1,"g":59,"op":"guard_end"}
        atStep(st, 386);
        cqUnion(c, st, 268, 264, {"toUnion": {"wp": 267}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 268, "union");
        // step 387: guard_begin {"block":1,"g":60,"op":"guard_begin","skippable":false}
        atStep(st, 388);
        cqWorkplaneNew(c, st, 269, {"plane": {"origin": [-12.650408571632497, 3.3896667606926845, 65.55555555555556], "xDir": [0.9999618966613908, -0.008729560433026575, 4.33680868994202e-19], "zDir": [-0.008295301623794243, -0.9502180102591604, 0.3114753938116017]}, "obj": undefined});
        atStep(st, 389);
        cqCircle(c, st, 270, 269, {"radius": 0.8, "forConstruction": false});
        atStep(st, 390);
        cqExtrude(c, st, 271, 270, {"until": 3.5672516455125773, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 271, "extrude");
        // step 391: guard_end {"block":1,"g":60,"op":"guard_end"}
        atStep(st, 392);
        cqUnion(c, st, 272, 268, {"toUnion": {"wp": 271}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 272, "union");
        // step 393: guard_begin {"block":1,"g":61,"op":"guard_begin","skippable":false}
        atStep(st, 394);
        cqWorkplaneNew(c, st, 273, {"plane": {"origin": [-12.68, 7.764260706594219e-15, 66.66666666666667], "xDir": [0.9671292134146908, 0.25428543914247514, 0], "zDir": [0.2408559359351597, -0.9160524985338048, 0.32068089755202434]}, "obj": undefined});
        atStep(st, 395);
        cqCircle(c, st, 274, 273, {"radius": 0.8, "forConstruction": false});
        atStep(st, 396);
        cqExtrude(c, st, 275, 274, {"until": 3.4648496982295103, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 275, "extrude");
        // step 397: guard_end {"block":1,"g":61,"op":"guard_end"}
        atStep(st, 398);
        cqUnion(c, st, 276, 272, {"toUnion": {"wp": 275}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 276, "union");
        // step 399: guard_begin {"block":1,"g":62,"op":"guard_begin","skippable":false}
        atStep(st, 400);
        cqWorkplaneNew(c, st, 277, {"plane": {"origin": [-11.845470383058275, -3.173984223107235, 67.77777777777777], "xDir": [0.8662137936697628, 0.4996735570912651, 0], "zDir": [0.4716110285256721, -0.8175657333836177, 0.33040779253804703]}, "obj": undefined});
        atStep(st, 401);
        cqCircle(c, st, 278, 277, {"radius": 0.8, "forConstruction": false});
        atStep(st, 402);
        cqExtrude(c, st, 279, 278, {"until": 3.3628477784257096, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 279, "extrude");
        // step 403: guard_end {"block":1,"g":62,"op":"guard_end"}
        atStep(st, 404);
        cqUnion(c, st, 280, 276, {"toUnion": {"wp": 279}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 280, "union");
        // step 405: guard_begin {"block":1,"g":63,"op":"guard_begin","skippable":false}
        atStep(st, 406);
        cqWorkplaneNew(c, st, 281, {"plane": {"origin": [-10.259514283499657, -5.92333333333332, 68.88888888888889], "xDir": [0.7041021242803096, 0.7100987245333957, -2.7755575615628907e-17], "zDir": [0.6676156266043035, -0.6619777851364285, 0.34069750087080447]}, "obj": undefined});
        atStep(st, 407);
        cqCircle(c, st, 282, 281, {"radius": 0.8, "forConstruction": false});
        atStep(st, 408);
        cqExtrude(c, st, 283, 282, {"until": 3.2612834208386445, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 283, "extrude");
        // step 409: guard_end {"block":1,"g":63,"op":"guard_end"}
        atStep(st, 410);
        cqUnion(c, st, 284, 280, {"toUnion": {"wp": 283}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 284, "union");
        // step 411: guard_begin {"block":1,"g":64,"op":"guard_begin","skippable":false}
        atStep(st, 412);
        cqWorkplaneNew(c, st, 285, {"plane": {"origin": [-8.082230508962237, -8.08223050896224, 70], "xDir": [0.4920168985845589, 0.8705856485764235, 0], "zDir": [0.8150005724844016, -0.46060264682069874, 0.35159531935431754]}, "obj": undefined});
        atStep(st, 413);
        cqCircle(c, st, 286, 285, {"radius": 0.8, "forConstruction": false});
        atStep(st, 414);
        cqExtrude(c, st, 287, 286, {"until": 3.1601988136577, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 287, "extrude");
        // step 415: guard_end {"block":1,"g":64,"op":"guard_end"}
        atStep(st, 416);
        cqUnion(c, st, 288, 284, {"toUnion": {"wp": 287}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 288, "union");
        // step 417: guard_begin {"block":1,"g":65,"op":"guard_begin","skippable":false}
        atStep(st, 418);
        cqWorkplaneNew(c, st, 289, {"plane": {"origin": [-5.506666666666685, -9.537826447012607, 71.11111111111111], "xDir": [0.24476536618097208, 0.9695823407624, 1.3877787807814457e-17], "zDir": [0.9033893389543388, -0.2280553317207445, 0.3631507509800635]}, "obj": undefined});
        atStep(st, 419);
        cqCircle(c, st, 290, 289, {"radius": 0.8, "forConstruction": false});
        atStep(st, 420);
        cqExtrude(c, st, 291, 290, {"until": 3.0596415073147205, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 291, "extrude");
        // step 421: guard_end {"block":1,"g":65,"op":"guard_end"}
        atStep(st, 422);
        cqUnion(c, st, 292, 288, {"toUnion": {"wp": 291}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 292, "union");
        // step 423: guard_begin {"block":1,"g":66,"op":"guard_begin","skippable":false}
        atStep(st, 424);
        cqWorkplaneNew(c, st, 293, {"plane": {"origin": [-2.7426191479363826, -10.235594005909826, 72.22222222222223], "xDir": [-0.02026624366583986, 0.9997946185930774, 0], "zDir": [0.926665321014148, 0.01878388305268527, 0.3754178319781959]}, "obj": undefined});
        atStep(st, 425);
        cqCircle(c, st, 294, 293, {"radius": 0.8, "forConstruction": false});
        atStep(st, 426);
        cqExtrude(c, st, 295, 294, {"until": 2.9596652488676485, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 295, "extrude");
        // step 427: guard_end {"block":1,"g":66,"op":"guard_end"}
        atStep(st, 428);
        cqUnion(c, st, 296, 292, {"toUnion": {"wp": 295}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 296, "union");
        // step 429: guard_begin {"block":1,"g":67,"op":"guard_begin","skippable":false}
        atStep(st, 430);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [-2.4940110053520585e-14, -10.18, 73.33333333333333], "xDir": [-0.2843053449394034, 0.9587337851765144, 1.3877787807814457e-17], "zDir": [0.8834420351786338, 0.26197813869591247, 0.388455435442547]}, "obj": undefined});
        atStep(st, 431);
        cqCircle(c, st, 298, 297, {"radius": 0.8, "forConstruction": false});
        atStep(st, 432);
        cqExtrude(c, st, 299, 298, {"until": 2.860330966524598, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 299, "extrude");
        // step 433: guard_end {"block":1,"g":67,"op":"guard_end"}
        atStep(st, 434);
        cqUnion(c, st, 300, 296, {"toUnion": {"wp": 299}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 300, "union");
        // step 435: guard_begin {"block":1,"g":68,"op":"guard_begin","skippable":false}
        atStep(st, 436);
        cqWorkplaneNew(c, st, 301, {"plane": {"origin": [2.5269366103509348, -9.430655817335603, 74.44444444444444], "xDir": [-0.5284927106155372, 0.8489378392003988, 2.7755575615628907e-17], "zDir": [0.7771990284503439, 0.4838328582695261, 0.4023275225914607]}, "obj": undefined});
        atStep(st, 437);
        cqCircle(c, st, 302, 301, {"radius": 0.8, "forConstruction": false});
        atStep(st, 438);
        cqExtrude(c, st, 303, 302, {"until": 2.7617079337606754, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 303, "extrude");
        // step 439: guard_end {"block":1,"g":68,"op":"guard_end"}
        atStep(st, 440);
        cqUnion(c, st, 304, 300, {"toUnion": {"wp": 303}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 304, "union");
        // step 441: guard_begin {"block":1,"g":69,"op":"guard_begin","skippable":false}
        atStep(st, 442);
        cqWorkplaneNew(c, st, 305, {"plane": {"origin": [4.673333333333338, -8.09445077403855, 75.55555555555556], "xDir": [-0.7352016348361236, 0.677848475792556, 2.7755575615628914e-17], "zDir": [0.6160687462218136, 0.6681946859349894, 0.4171032984968153]}, "obj": undefined});
        atStep(st, 443);
        cqCircle(c, st, 306, 305, {"radius": 0.8, "forConstruction": false});
        atStep(st, 444);
        cqExtrude(c, st, 307, 306, {"until": 2.6638751482316505, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 307, "extrude");
        // step 445: guard_end {"block":1,"g":69,"op":"guard_end"}
        atStep(st, 446);
        cqUnion(c, st, 308, 304, {"toUnion": {"wp": 307}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 308, "union");
        // step 447: guard_begin {"block":1,"g":70,"op":"guard_begin","skippable":false}
        atStep(st, 448);
        cqWorkplaneNew(c, st, 309, {"plane": {"origin": [6.314463555995859, -6.3144635559958795, 76.66666666666667], "xDir": [-0.8892871276539893, 0.45734932446535587, 0], "zDir": [0.41228326484434386, 0.8016589962209691, 0.4328572089122194]}, "obj": undefined});
        atStep(st, 449);
        cqCircle(c, st, 310, 309, {"radius": 0.8, "forConstruction": false});
        atStep(st, 450);
        cqExtrude(c, st, 311, 310, {"until": 2.5669229672837126, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 311, "extrude");
        // step 451: guard_end {"block":1,"g":70,"op":"guard_end"}
        atStep(st, 452);
        cqUnion(c, st, 312, 308, {"toUnion": {"wp": 311}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 312, "union");
        // step 453: guard_begin {"block":1,"g":71,"op":"guard_begin","skippable":false}
        atStep(st, 454);
        cqWorkplaneNew(c, st, 313, {"plane": {"origin": [7.372762937551519, -4.256666666666667, 77.77777777777777], "xDir": [-0.9791800323863158, 0.202993754031826, 0], "zDir": [0.1813130929999504, 0.8745991280496221, 0.44966868639213914]}, "obj": undefined});
        atStep(st, 455);
        cqCircle(c, st, 314, 313, {"radius": 0.8, "forConstruction": false});
        atStep(st, 456);
        cqExtrude(c, st, 315, 314, {"until": 2.470955049207398, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 315, "extrude");
        // step 457: guard_end {"block":1,"g":71,"op":"guard_end"}
        atStep(st, 458);
        cqUnion(c, st, 316, 312, {"toUnion": {"wp": 315}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 316, "union");
        // step 459: guard_begin {"block":1,"g":72,"op":"guard_begin","skippable":false}
        atStep(st, 460);
        cqWorkplaneNew(c, st, 317, {"plane": {"origin": [7.820779440187157, -2.0955715351800652, 78.88888888888889], "xDir": [-0.9977510670010884, -0.06702841411065591, 6.9388939039072276e-18], "zDir": [-0.059248345496663496, 0.881940901059559, 0.4676215142550236]}, "obj": undefined});
        atStep(st, 461);
        cqCircle(c, st, 318, 317, {"radius": 0.8, "forConstruction": false});
        atStep(st, 462);
        cqExtrude(c, st, 319, 318, {"until": 2.3760906571658618, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 319, "extrude");
        // step 463: guard_end {"block":1,"g":72,"op":"guard_end"}
        atStep(st, 464);
        cqUnion(c, st, 320, 316, {"toUnion": {"wp": 319}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 320, "union");
        atStep(st, 465);
        cqUnion(c, st, 321, 33, {"toUnion": {"wp": 320}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 321, "union");
        // step 466: guard_begin {"block":2,"g":73,"op":"guard_begin","skippable":false}
        atStep(st, 467);
        cqWorkplaneNew(c, st, 322, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 468);
        cqPolyline(c, st, 323, 322, {"listOfXYTuple": [[37.375, -1.3333333333333337], [37.375, 4.666666666666666], [37.055, 1.6666666666666663]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 469);
        cqClose(c, st, 324, 323, {});
        atStep(st, 470);
        cqExtrude(c, st, 325, 324, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 325, "extrude");
        atStep(st, 471);
        cqRotate(c, st, 326, 325, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 22.5});
        stepVol(c, st, 326, "rotate");
        // step 472: guard_end {"block":2,"g":73,"op":"guard_end"}
        // step 473: guard_begin {"block":2,"g":74,"op":"guard_begin","skippable":false}
        atStep(st, 474);
        cqWorkplaneNew(c, st, 327, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 475);
        cqPolyline(c, st, 328, 327, {"listOfXYTuple": [[36.125, 2], [36.125, 8], [35.805, 5]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 476);
        cqClose(c, st, 329, 328, {});
        atStep(st, 477);
        cqExtrude(c, st, 330, 329, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 330, "extrude");
        atStep(st, 478);
        cqRotate(c, st, 331, 330, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 67.5});
        stepVol(c, st, 331, "rotate");
        atStep(st, 479);
        cqUnion(c, st, 332, 326, {"toUnion": {"wp": 331}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 332, "union");
        // step 480: guard_end {"block":2,"g":74,"op":"guard_end"}
        // step 481: guard_begin {"block":2,"g":75,"op":"guard_begin","skippable":false}
        atStep(st, 482);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 483);
        cqPolyline(c, st, 334, 333, {"listOfXYTuple": [[34.875, 5.333333333333334], [34.875, 11.333333333333334], [34.555, 8.333333333333334]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 484);
        cqClose(c, st, 335, 334, {});
        atStep(st, 485);
        cqExtrude(c, st, 336, 335, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 336, "extrude");
        atStep(st, 486);
        cqRotate(c, st, 337, 336, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 112.5});
        stepVol(c, st, 337, "rotate");
        atStep(st, 487);
        cqUnion(c, st, 338, 332, {"toUnion": {"wp": 337}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 338, "union");
        // step 488: guard_end {"block":2,"g":75,"op":"guard_end"}
        // step 489: guard_begin {"block":2,"g":76,"op":"guard_begin","skippable":false}
        atStep(st, 490);
        cqWorkplaneNew(c, st, 339, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 491);
        cqPolyline(c, st, 340, 339, {"listOfXYTuple": [[33.625, 8.666666666666668], [33.625, 14.666666666666668], [33.305, 11.666666666666668]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 492);
        cqClose(c, st, 341, 340, {});
        atStep(st, 493);
        cqExtrude(c, st, 342, 341, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 342, "extrude");
        atStep(st, 494);
        cqRotate(c, st, 343, 342, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 157.5});
        stepVol(c, st, 343, "rotate");
        atStep(st, 495);
        cqUnion(c, st, 344, 338, {"toUnion": {"wp": 343}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 344, "union");
        // step 496: guard_end {"block":2,"g":76,"op":"guard_end"}
        // step 497: guard_begin {"block":2,"g":77,"op":"guard_begin","skippable":false}
        atStep(st, 498);
        cqWorkplaneNew(c, st, 345, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 499);
        cqPolyline(c, st, 346, 345, {"listOfXYTuple": [[32.375, 12], [32.375, 18], [32.055, 15]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 500);
        cqClose(c, st, 347, 346, {});
        atStep(st, 501);
        cqExtrude(c, st, 348, 347, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 348, "extrude");
        atStep(st, 502);
        cqRotate(c, st, 349, 348, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 202.5});
        stepVol(c, st, 349, "rotate");
        atStep(st, 503);
        cqUnion(c, st, 350, 344, {"toUnion": {"wp": 349}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 350, "union");
        // step 504: guard_end {"block":2,"g":77,"op":"guard_end"}
        // step 505: guard_begin {"block":2,"g":78,"op":"guard_begin","skippable":false}
        atStep(st, 506);
        cqWorkplaneNew(c, st, 351, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 507);
        cqPolyline(c, st, 352, 351, {"listOfXYTuple": [[31.125, 15.333333333333332], [31.125, 21.333333333333332], [30.805, 18.333333333333332]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 508);
        cqClose(c, st, 353, 352, {});
        atStep(st, 509);
        cqExtrude(c, st, 354, 353, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 354, "extrude");
        atStep(st, 510);
        cqRotate(c, st, 355, 354, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 247.49999999999997});
        stepVol(c, st, 355, "rotate");
        atStep(st, 511);
        cqUnion(c, st, 356, 350, {"toUnion": {"wp": 355}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 356, "union");
        // step 512: guard_end {"block":2,"g":78,"op":"guard_end"}
        // step 513: guard_begin {"block":2,"g":79,"op":"guard_begin","skippable":false}
        atStep(st, 514);
        cqWorkplaneNew(c, st, 357, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 515);
        cqPolyline(c, st, 358, 357, {"listOfXYTuple": [[29.875, 18.666666666666664], [29.875, 24.666666666666664], [29.555, 21.666666666666664]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 516);
        cqClose(c, st, 359, 358, {});
        atStep(st, 517);
        cqExtrude(c, st, 360, 359, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 360, "extrude");
        atStep(st, 518);
        cqRotate(c, st, 361, 360, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 292.5});
        stepVol(c, st, 361, "rotate");
        atStep(st, 519);
        cqUnion(c, st, 362, 356, {"toUnion": {"wp": 361}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 362, "union");
        // step 520: guard_end {"block":2,"g":79,"op":"guard_end"}
        // step 521: guard_begin {"block":2,"g":80,"op":"guard_begin","skippable":false}
        atStep(st, 522);
        cqWorkplaneNew(c, st, 363, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 523);
        cqPolyline(c, st, 364, 363, {"listOfXYTuple": [[28.625, 22], [28.625, 28], [28.305, 25]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 524);
        cqClose(c, st, 365, 364, {});
        atStep(st, 525);
        cqExtrude(c, st, 366, 365, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 366, "extrude");
        atStep(st, 526);
        cqRotate(c, st, 367, 366, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 337.5});
        stepVol(c, st, 367, "rotate");
        atStep(st, 527);
        cqUnion(c, st, 368, 362, {"toUnion": {"wp": 367}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 368, "union");
        // step 528: guard_end {"block":2,"g":80,"op":"guard_end"}
        // step 529: guard_begin {"block":2,"g":81,"op":"guard_begin","skippable":false}
        atStep(st, 530);
        cqWorkplaneNew(c, st, 369, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 531);
        cqPolyline(c, st, 370, 369, {"listOfXYTuple": [[27.375, 25.333333333333336], [27.375, 31.333333333333336], [27.055, 28.333333333333336]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 532);
        cqClose(c, st, 371, 370, {});
        atStep(st, 533);
        cqExtrude(c, st, 372, 371, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 372, "extrude");
        atStep(st, 534);
        cqRotate(c, st, 373, 372, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 382.5});
        stepVol(c, st, 373, "rotate");
        atStep(st, 535);
        cqUnion(c, st, 374, 368, {"toUnion": {"wp": 373}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 374, "union");
        // step 536: guard_end {"block":2,"g":81,"op":"guard_end"}
        // step 537: guard_begin {"block":2,"g":82,"op":"guard_begin","skippable":false}
        atStep(st, 538);
        cqWorkplaneNew(c, st, 375, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 539);
        cqPolyline(c, st, 376, 375, {"listOfXYTuple": [[26.125, 28.666666666666664], [26.125, 34.666666666666664], [25.805, 31.666666666666664]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 540);
        cqClose(c, st, 377, 376, {});
        atStep(st, 541);
        cqExtrude(c, st, 378, 377, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 378, "extrude");
        atStep(st, 542);
        cqRotate(c, st, 379, 378, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 427.5});
        stepVol(c, st, 379, "rotate");
        atStep(st, 543);
        cqUnion(c, st, 380, 374, {"toUnion": {"wp": 379}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 380, "union");
        // step 544: guard_end {"block":2,"g":82,"op":"guard_end"}
        // step 545: guard_begin {"block":2,"g":83,"op":"guard_begin","skippable":false}
        atStep(st, 546);
        cqWorkplaneNew(c, st, 381, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 547);
        cqPolyline(c, st, 382, 381, {"listOfXYTuple": [[24.875, 32], [24.875, 38], [24.555, 35]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 548);
        cqClose(c, st, 383, 382, {});
        atStep(st, 549);
        cqExtrude(c, st, 384, 383, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 384, "extrude");
        atStep(st, 550);
        cqRotate(c, st, 385, 384, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 472.5});
        stepVol(c, st, 385, "rotate");
        atStep(st, 551);
        cqUnion(c, st, 386, 380, {"toUnion": {"wp": 385}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 386, "union");
        // step 552: guard_end {"block":2,"g":83,"op":"guard_end"}
        // step 553: guard_begin {"block":2,"g":84,"op":"guard_begin","skippable":false}
        atStep(st, 554);
        cqWorkplaneNew(c, st, 387, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 555);
        cqPolyline(c, st, 388, 387, {"listOfXYTuple": [[23.625, 35.333333333333336], [23.625, 41.333333333333336], [23.305, 38.333333333333336]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 556);
        cqClose(c, st, 389, 388, {});
        atStep(st, 557);
        cqExtrude(c, st, 390, 389, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 390, "extrude");
        atStep(st, 558);
        cqRotate(c, st, 391, 390, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 517.5});
        stepVol(c, st, 391, "rotate");
        atStep(st, 559);
        cqUnion(c, st, 392, 386, {"toUnion": {"wp": 391}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 392, "union");
        // step 560: guard_end {"block":2,"g":84,"op":"guard_end"}
        // step 561: guard_begin {"block":2,"g":85,"op":"guard_begin","skippable":false}
        atStep(st, 562);
        cqWorkplaneNew(c, st, 393, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 563);
        cqPolyline(c, st, 394, 393, {"listOfXYTuple": [[22.375, 38.66666666666667], [22.375, 44.66666666666667], [22.055, 41.66666666666667]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 564);
        cqClose(c, st, 395, 394, {});
        atStep(st, 565);
        cqExtrude(c, st, 396, 395, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 396, "extrude");
        atStep(st, 566);
        cqRotate(c, st, 397, 396, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 562.5});
        stepVol(c, st, 397, "rotate");
        atStep(st, 567);
        cqUnion(c, st, 398, 392, {"toUnion": {"wp": 397}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 398, "union");
        // step 568: guard_end {"block":2,"g":85,"op":"guard_end"}
        // step 569: guard_begin {"block":2,"g":86,"op":"guard_begin","skippable":false}
        atStep(st, 570);
        cqWorkplaneNew(c, st, 399, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 571);
        cqPolyline(c, st, 400, 399, {"listOfXYTuple": [[21.125, 42], [21.125, 48], [20.805, 45]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 572);
        cqClose(c, st, 401, 400, {});
        atStep(st, 573);
        cqExtrude(c, st, 402, 401, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 402, "extrude");
        atStep(st, 574);
        cqRotate(c, st, 403, 402, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 607.5});
        stepVol(c, st, 403, "rotate");
        atStep(st, 575);
        cqUnion(c, st, 404, 398, {"toUnion": {"wp": 403}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 404, "union");
        // step 576: guard_end {"block":2,"g":86,"op":"guard_end"}
        // step 577: guard_begin {"block":2,"g":87,"op":"guard_begin","skippable":false}
        atStep(st, 578);
        cqWorkplaneNew(c, st, 405, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 579);
        cqPolyline(c, st, 406, 405, {"listOfXYTuple": [[19.875, 45.33333333333333], [19.875, 51.33333333333333], [19.555, 48.33333333333333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 580);
        cqClose(c, st, 407, 406, {});
        atStep(st, 581);
        cqExtrude(c, st, 408, 407, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 408, "extrude");
        atStep(st, 582);
        cqRotate(c, st, 409, 408, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 652.4999999999999});
        stepVol(c, st, 409, "rotate");
        atStep(st, 583);
        cqUnion(c, st, 410, 404, {"toUnion": {"wp": 409}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 410, "union");
        // step 584: guard_end {"block":2,"g":87,"op":"guard_end"}
        // step 585: guard_begin {"block":2,"g":88,"op":"guard_begin","skippable":false}
        atStep(st, 586);
        cqWorkplaneNew(c, st, 411, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 587);
        cqPolyline(c, st, 412, 411, {"listOfXYTuple": [[18.625, 48.66666666666667], [18.625, 54.66666666666667], [18.305, 51.66666666666667]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 588);
        cqClose(c, st, 413, 412, {});
        atStep(st, 589);
        cqExtrude(c, st, 414, 413, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 414, "extrude");
        atStep(st, 590);
        cqRotate(c, st, 415, 414, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 697.5000000000001});
        stepVol(c, st, 415, "rotate");
        atStep(st, 591);
        cqUnion(c, st, 416, 410, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        // step 592: guard_end {"block":2,"g":88,"op":"guard_end"}
        // step 593: guard_begin {"block":2,"g":89,"op":"guard_begin","skippable":false}
        atStep(st, 594);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 595);
        cqPolyline(c, st, 418, 417, {"listOfXYTuple": [[17.375, 52], [17.375, 58], [17.055, 55]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 596);
        cqClose(c, st, 419, 418, {});
        atStep(st, 597);
        cqExtrude(c, st, 420, 419, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 420, "extrude");
        atStep(st, 598);
        cqRotate(c, st, 421, 420, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 742.5});
        stepVol(c, st, 421, "rotate");
        atStep(st, 599);
        cqUnion(c, st, 422, 416, {"toUnion": {"wp": 421}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 422, "union");
        // step 600: guard_end {"block":2,"g":89,"op":"guard_end"}
        // step 601: guard_begin {"block":2,"g":90,"op":"guard_begin","skippable":false}
        atStep(st, 602);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 603);
        cqPolyline(c, st, 424, 423, {"listOfXYTuple": [[16.125, 55.33333333333333], [16.125, 61.33333333333333], [15.805, 58.33333333333333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 604);
        cqClose(c, st, 425, 424, {});
        atStep(st, 605);
        cqExtrude(c, st, 426, 425, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 606);
        cqRotate(c, st, 427, 426, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 787.5});
        stepVol(c, st, 427, "rotate");
        atStep(st, 607);
        cqUnion(c, st, 428, 422, {"toUnion": {"wp": 427}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 428, "union");
        // step 608: guard_end {"block":2,"g":90,"op":"guard_end"}
        // step 609: guard_begin {"block":2,"g":91,"op":"guard_begin","skippable":false}
        atStep(st, 610);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 611);
        cqPolyline(c, st, 430, 429, {"listOfXYTuple": [[14.875, 58.66666666666667], [14.875, 64.66666666666667], [14.555, 61.66666666666667]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 612);
        cqClose(c, st, 431, 430, {});
        atStep(st, 613);
        cqExtrude(c, st, 432, 431, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 432, "extrude");
        atStep(st, 614);
        cqRotate(c, st, 433, 432, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 832.5});
        stepVol(c, st, 433, "rotate");
        atStep(st, 615);
        cqUnion(c, st, 434, 428, {"toUnion": {"wp": 433}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 434, "union");
        // step 616: guard_end {"block":2,"g":91,"op":"guard_end"}
        // step 617: guard_begin {"block":2,"g":92,"op":"guard_begin","skippable":false}
        atStep(st, 618);
        cqWorkplaneNew(c, st, 435, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 619);
        cqPolyline(c, st, 436, 435, {"listOfXYTuple": [[13.625, 62], [13.625, 68], [13.305, 65]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 620);
        cqClose(c, st, 437, 436, {});
        atStep(st, 621);
        cqExtrude(c, st, 438, 437, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 438, "extrude");
        atStep(st, 622);
        cqRotate(c, st, 439, 438, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 877.5});
        stepVol(c, st, 439, "rotate");
        atStep(st, 623);
        cqUnion(c, st, 440, 434, {"toUnion": {"wp": 439}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 440, "union");
        // step 624: guard_end {"block":2,"g":92,"op":"guard_end"}
        // step 625: guard_begin {"block":2,"g":93,"op":"guard_begin","skippable":false}
        atStep(st, 626);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 627);
        cqPolyline(c, st, 442, 441, {"listOfXYTuple": [[12.375, 65.33333333333333], [12.375, 71.33333333333333], [12.055, 68.33333333333333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 628);
        cqClose(c, st, 443, 442, {});
        atStep(st, 629);
        cqExtrude(c, st, 444, 443, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 444, "extrude");
        atStep(st, 630);
        cqRotate(c, st, 445, 444, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 922.5});
        stepVol(c, st, 445, "rotate");
        atStep(st, 631);
        cqUnion(c, st, 446, 440, {"toUnion": {"wp": 445}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 446, "union");
        // step 632: guard_end {"block":2,"g":93,"op":"guard_end"}
        // step 633: guard_begin {"block":2,"g":94,"op":"guard_begin","skippable":false}
        atStep(st, 634);
        cqWorkplaneNew(c, st, 447, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 635);
        cqPolyline(c, st, 448, 447, {"listOfXYTuple": [[11.125, 68.66666666666667], [11.125, 74.66666666666667], [10.805, 71.66666666666667]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 636);
        cqClose(c, st, 449, 448, {});
        atStep(st, 637);
        cqExtrude(c, st, 450, 449, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 450, "extrude");
        atStep(st, 638);
        cqRotate(c, st, 451, 450, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 967.5});
        stepVol(c, st, 451, "rotate");
        atStep(st, 639);
        cqUnion(c, st, 452, 446, {"toUnion": {"wp": 451}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 452, "union");
        // step 640: guard_end {"block":2,"g":94,"op":"guard_end"}
        // step 641: guard_begin {"block":2,"g":95,"op":"guard_begin","skippable":false}
        atStep(st, 642);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 643);
        cqPolyline(c, st, 454, 453, {"listOfXYTuple": [[9.875, 72], [9.875, 78], [9.555, 75]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 644);
        cqClose(c, st, 455, 454, {});
        atStep(st, 645);
        cqExtrude(c, st, 456, 455, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 456, "extrude");
        atStep(st, 646);
        cqRotate(c, st, 457, 456, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1012.5});
        stepVol(c, st, 457, "rotate");
        atStep(st, 647);
        cqUnion(c, st, 458, 452, {"toUnion": {"wp": 457}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 458, "union");
        // step 648: guard_end {"block":2,"g":95,"op":"guard_end"}
        // step 649: guard_begin {"block":2,"g":96,"op":"guard_begin","skippable":false}
        atStep(st, 650);
        cqWorkplaneNew(c, st, 459, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 651);
        cqPolyline(c, st, 460, 459, {"listOfXYTuple": [[8.625, 75.33333333333333], [8.625, 81.33333333333333], [8.305, 78.33333333333333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 652);
        cqClose(c, st, 461, 460, {});
        atStep(st, 653);
        cqExtrude(c, st, 462, 461, {"until": 1.2, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 462, "extrude");
        atStep(st, 654);
        cqRotate(c, st, 463, 462, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1057.4999999999998});
        stepVol(c, st, 463, "rotate");
        atStep(st, 655);
        cqUnion(c, st, 464, 458, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 464, "union");
        // step 656: guard_end {"block":2,"g":96,"op":"guard_end"}
        atStep(st, 657);
        cqUnion(c, st, 465, 321, {"toUnion": {"wp": 464}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 465, "union");
        atStep(st, 658);
        cqWorkplaneNew(c, st, 466, {"plane": {"origin": [39, 0, 0], "xDir": [0, 0, 1], "zDir": [1, 0, 0]}, "obj": undefined});
        atStep(st, 659);
        cqCircle(c, st, 467, 466, {"radius": 2, "forConstruction": false});
        atStep(st, 660);
        cqExtrude(c, st, 468, 467, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 468, "extrude");
        atStep(st, 661);
        cqWorkplaneNew(c, st, 469, {"plane": {"origin": [27.577164466275356, 27.577164466275352, 0], "xDir": [0, 0, 1], "zDir": [0.7071067811865476, 0.7071067811865475, 0]}, "obj": undefined});
        atStep(st, 662);
        cqCircle(c, st, 470, 469, {"radius": 2, "forConstruction": false});
        atStep(st, 663);
        cqExtrude(c, st, 471, 470, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 471, "extrude");
        atStep(st, 664);
        cqUnion(c, st, 472, 468, {"toUnion": {"wp": 471}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 472, "union");
        atStep(st, 665);
        cqWorkplaneNew(c, st, 473, {"plane": {"origin": [2.388061258337339e-15, 39, 0], "xDir": [0, 0, 1], "zDir": [6.123233995736766e-17, 1, 0]}, "obj": undefined});
        atStep(st, 666);
        cqCircle(c, st, 474, 473, {"radius": 2, "forConstruction": false});
        atStep(st, 667);
        cqExtrude(c, st, 475, 474, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 475, "extrude");
        atStep(st, 668);
        cqUnion(c, st, 476, 472, {"toUnion": {"wp": 475}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 476, "union");
        atStep(st, 669);
        cqWorkplaneNew(c, st, 477, {"plane": {"origin": [-27.577164466275352, 27.577164466275356, 0], "xDir": [0, 0, 1], "zDir": [-0.7071067811865475, 0.7071067811865476, 0]}, "obj": undefined});
        atStep(st, 670);
        cqCircle(c, st, 478, 477, {"radius": 2, "forConstruction": false});
        atStep(st, 671);
        cqExtrude(c, st, 479, 478, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 479, "extrude");
        atStep(st, 672);
        cqUnion(c, st, 480, 476, {"toUnion": {"wp": 479}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 480, "union");
        atStep(st, 673);
        cqWorkplaneNew(c, st, 481, {"plane": {"origin": [-39, 4.776122516674678e-15, 0], "xDir": [0, 0, 1], "zDir": [-1, 1.2246467991473532e-16, 0]}, "obj": undefined});
        atStep(st, 674);
        cqCircle(c, st, 482, 481, {"radius": 2, "forConstruction": false});
        atStep(st, 675);
        cqExtrude(c, st, 483, 482, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 483, "extrude");
        atStep(st, 676);
        cqUnion(c, st, 484, 480, {"toUnion": {"wp": 483}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 484, "union");
        atStep(st, 677);
        cqWorkplaneNew(c, st, 485, {"plane": {"origin": [-27.57716446627536, -27.577164466275352, 0], "xDir": [0, 0, 1], "zDir": [-0.7071067811865477, -0.7071067811865475, 0]}, "obj": undefined});
        atStep(st, 678);
        cqCircle(c, st, 486, 485, {"radius": 2, "forConstruction": false});
        atStep(st, 679);
        cqExtrude(c, st, 487, 486, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 487, "extrude");
        atStep(st, 680);
        cqUnion(c, st, 488, 484, {"toUnion": {"wp": 487}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 488, "union");
        atStep(st, 681);
        cqWorkplaneNew(c, st, 489, {"plane": {"origin": [-7.164183775012016e-15, -39, 0], "xDir": [0, 0, 1], "zDir": [-1.8369701987210294e-16, -1, 0]}, "obj": undefined});
        atStep(st, 682);
        cqCircle(c, st, 490, 489, {"radius": 2, "forConstruction": false});
        atStep(st, 683);
        cqExtrude(c, st, 491, 490, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 491, "extrude");
        atStep(st, 684);
        cqUnion(c, st, 492, 488, {"toUnion": {"wp": 491}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 492, "union");
        atStep(st, 685);
        cqWorkplaneNew(c, st, 493, {"plane": {"origin": [27.577164466275345, -27.57716446627536, 0], "xDir": [0, 0, 1], "zDir": [0.7071067811865474, -0.7071067811865477, 0]}, "obj": undefined});
        atStep(st, 686);
        cqCircle(c, st, 494, 493, {"radius": 2, "forConstruction": false});
        atStep(st, 687);
        cqExtrude(c, st, 495, 494, {"until": 2, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 495, "extrude");
        atStep(st, 688);
        cqUnion(c, st, 496, 492, {"toUnion": {"wp": 495}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 496, "union");
        atStep(st, 689);
        cqUnion(c, st, 497, 465, {"toUnion": {"wp": 496}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 497, "union");
        // step 690: guard_begin {"block":5,"g":97,"op":"guard_begin","skippable":false}
        atStep(st, 691);
        cqSelect("Edges")(c, st, 498, 497, {"selector": {"k": "par", "v": [0, 0, 1]}, "tag": undefined});
        // step 692: injected_failure {"at":692,"op":"injected_failure","was":"fillet"}
        // step 693: guard_caught {"block":5,"g":97,"op":"guard_caught"}
        // step 694: guard_begin {"block":4,"g":98,"op":"guard_begin","skippable":false}
        atStep(st, 695);
        cqSelect("Edges")(c, st, 500, 497, {"selector": {"a": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "b": {"k": "nth", "key": "center", "max": false, "n": -1, "parallel": false, "v": [0, 0, 1]}, "k": "or"}, "tag": undefined});
        // step 696: injected_failure {"at":696,"op":"injected_failure","was":"fillet"}
        // step 697: guard_caught {"block":4,"g":98,"op":"guard_caught"}
        // step 698: guard_begin {"before":{"wp":497},"block":3,"g":99,"op":"guard_begin","skippable":true}
        atStep(st, 699);
        cqSelect("Edges")(c, st, 502, 497, {"selector": {"k": "nth", "key": "center", "max": true, "n": -1, "parallel": false, "v": [0, 0, 1]}, "tag": undefined});
        // step 700: injected_failure {"at":700,"op":"injected_failure","was":"fillet"}
        // step 701: guard_caught {"block":3,"g":99,"op":"guard_caught"}
        // step 702: guard_begin {"block":6,"g":100,"op":"guard_begin","skippable":false}
        atStep(st, 703);
        cqCombine(c, st, 504, 497, {"clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 504, "combine");
        // step 704: guard_end {"block":6,"g":100,"op":"guard_end"}
        return mergeMaps(cqMeasure(c, st, {"wp": 504}), {"i": 5637});
    } catch (e) {
        return {"i": 5637, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_5637(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c5637"] = r; }
    return out;
}
