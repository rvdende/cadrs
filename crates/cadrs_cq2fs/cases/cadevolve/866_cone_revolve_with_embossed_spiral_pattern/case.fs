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

const case_866 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 2);
        cqMoveTo(c, st, 2, 1, {"x": 0, "y": 0});
        atStep(st, 3);
        cqLineTo(c, st, 3, 2, {"x": 10, "y": 0, "forConstruction": false});
        atStep(st, 4);
        cqLineTo(c, st, 4, 3, {"x": 2, "y": 20, "forConstruction": false});
        atStep(st, 5);
        cqClose(c, st, 5, 4, {});
        atStep(st, 6);
        cqRevolve(c, st, 6, 5, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 6, "revolve");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 8);
        cqPolyline(c, st, 8, 7, {"listOfXYTuple": [[10, 0], [10.5, 0], [10.425925925925926, 0.18518518518518515], [9.925925925925926, 0.18518518518518515]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 9);
        cqClose(c, st, 9, 8, {});
        atStep(st, 10);
        cqRevolve(c, st, 10, 9, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 10, "revolve");
        atStep(st, 11);
        cqRotate(c, st, 11, 10, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 0});
        stepVol(c, st, 11, "rotate");
        atStep(st, 12);
        cqUnion(c, st, 12, 6, {"toUnion": {"wp": 11}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 12, "union");
        atStep(st, 13);
        cqWorkplaneNew(c, st, 13, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 14);
        cqPolyline(c, st, 14, 13, {"listOfXYTuple": [[9.925925925925926, 0.18518518518518515], [10.425925925925926, 0.18518518518518515], [10.351851851851851, 0.3703703703703703], [9.851851851851851, 0.3703703703703703]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 15);
        cqClose(c, st, 15, 14, {});
        atStep(st, 16);
        cqRevolve(c, st, 16, 15, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 16, "revolve");
        atStep(st, 17);
        cqRotate(c, st, 17, 16, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 10});
        stepVol(c, st, 17, "rotate");
        atStep(st, 18);
        cqUnion(c, st, 18, 12, {"toUnion": {"wp": 17}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 18, "union");
        atStep(st, 19);
        cqWorkplaneNew(c, st, 19, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 20);
        cqPolyline(c, st, 20, 19, {"listOfXYTuple": [[9.851851851851851, 0.3703703703703703], [10.351851851851851, 0.3703703703703703], [10.27777777777778, 0.5555555555555556], [9.77777777777778, 0.5555555555555556]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 21);
        cqClose(c, st, 21, 20, {});
        atStep(st, 22);
        cqRevolve(c, st, 22, 21, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 22, "revolve");
        atStep(st, 23);
        cqRotate(c, st, 23, 22, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 20});
        stepVol(c, st, 23, "rotate");
        atStep(st, 24);
        cqUnion(c, st, 24, 18, {"toUnion": {"wp": 23}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 24, "union");
        atStep(st, 25);
        cqWorkplaneNew(c, st, 25, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 26);
        cqPolyline(c, st, 26, 25, {"listOfXYTuple": [[9.77777777777778, 0.5555555555555556], [10.27777777777778, 0.5555555555555556], [10.203703703703704, 0.7407407407407407], [9.703703703703704, 0.7407407407407407]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 27);
        cqClose(c, st, 27, 26, {});
        atStep(st, 28);
        cqRevolve(c, st, 28, 27, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 28, "revolve");
        atStep(st, 29);
        cqRotate(c, st, 29, 28, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 30});
        stepVol(c, st, 29, "rotate");
        atStep(st, 30);
        cqUnion(c, st, 30, 24, {"toUnion": {"wp": 29}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 30, "union");
        atStep(st, 31);
        cqWorkplaneNew(c, st, 31, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 32);
        cqPolyline(c, st, 32, 31, {"listOfXYTuple": [[9.703703703703704, 0.7407407407407407], [10.203703703703704, 0.7407407407407407], [10.12962962962963, 0.9259259259259258], [9.62962962962963, 0.9259259259259258]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 33);
        cqClose(c, st, 33, 32, {});
        atStep(st, 34);
        cqRevolve(c, st, 34, 33, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 34, "revolve");
        atStep(st, 35);
        cqRotate(c, st, 35, 34, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 40});
        stepVol(c, st, 35, "rotate");
        atStep(st, 36);
        cqUnion(c, st, 36, 30, {"toUnion": {"wp": 35}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 36, "union");
        atStep(st, 37);
        cqWorkplaneNew(c, st, 37, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 38);
        cqPolyline(c, st, 38, 37, {"listOfXYTuple": [[9.62962962962963, 0.9259259259259258], [10.12962962962963, 0.9259259259259258], [10.055555555555555, 1.1111111111111112], [9.555555555555555, 1.1111111111111112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 39);
        cqClose(c, st, 39, 38, {});
        atStep(st, 40);
        cqRevolve(c, st, 40, 39, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 40, "revolve");
        atStep(st, 41);
        cqRotate(c, st, 41, 40, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 50});
        stepVol(c, st, 41, "rotate");
        atStep(st, 42);
        cqUnion(c, st, 42, 36, {"toUnion": {"wp": 41}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 42, "union");
        atStep(st, 43);
        cqWorkplaneNew(c, st, 43, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 44);
        cqPolyline(c, st, 44, 43, {"listOfXYTuple": [[9.555555555555555, 1.1111111111111112], [10.055555555555555, 1.1111111111111112], [9.98148148148148, 1.2962962962962965], [9.48148148148148, 1.2962962962962965]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 45);
        cqClose(c, st, 45, 44, {});
        atStep(st, 46);
        cqRevolve(c, st, 46, 45, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 46, "revolve");
        atStep(st, 47);
        cqRotate(c, st, 47, 46, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 60});
        stepVol(c, st, 47, "rotate");
        atStep(st, 48);
        cqUnion(c, st, 48, 42, {"toUnion": {"wp": 47}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 48, "union");
        atStep(st, 49);
        cqWorkplaneNew(c, st, 49, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 50);
        cqPolyline(c, st, 50, 49, {"listOfXYTuple": [[9.48148148148148, 1.2962962962962965], [9.98148148148148, 1.2962962962962965], [9.907407407407408, 1.4814814814814814], [9.407407407407408, 1.4814814814814814]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 51);
        cqClose(c, st, 51, 50, {});
        atStep(st, 52);
        cqRevolve(c, st, 52, 51, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 52, "revolve");
        atStep(st, 53);
        cqRotate(c, st, 53, 52, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 70});
        stepVol(c, st, 53, "rotate");
        atStep(st, 54);
        cqUnion(c, st, 54, 48, {"toUnion": {"wp": 53}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 54, "union");
        atStep(st, 55);
        cqWorkplaneNew(c, st, 55, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 56);
        cqPolyline(c, st, 56, 55, {"listOfXYTuple": [[9.407407407407408, 1.4814814814814814], [9.907407407407408, 1.4814814814814814], [9.833333333333334, 1.6666666666666663], [9.333333333333334, 1.6666666666666663]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 57);
        cqClose(c, st, 57, 56, {});
        atStep(st, 58);
        cqRevolve(c, st, 58, 57, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 58, "revolve");
        atStep(st, 59);
        cqRotate(c, st, 59, 58, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 80});
        stepVol(c, st, 59, "rotate");
        atStep(st, 60);
        cqUnion(c, st, 60, 54, {"toUnion": {"wp": 59}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 60, "union");
        atStep(st, 61);
        cqWorkplaneNew(c, st, 61, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 62);
        cqPolyline(c, st, 62, 61, {"listOfXYTuple": [[9.333333333333334, 1.6666666666666663], [9.833333333333334, 1.6666666666666663], [9.75925925925926, 1.8518518518518516], [9.25925925925926, 1.8518518518518516]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 63);
        cqClose(c, st, 63, 62, {});
        atStep(st, 64);
        cqRevolve(c, st, 64, 63, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 64, "revolve");
        atStep(st, 65);
        cqRotate(c, st, 65, 64, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 90});
        stepVol(c, st, 65, "rotate");
        atStep(st, 66);
        cqUnion(c, st, 66, 60, {"toUnion": {"wp": 65}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 66, "union");
        atStep(st, 67);
        cqWorkplaneNew(c, st, 67, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 68);
        cqPolyline(c, st, 68, 67, {"listOfXYTuple": [[9.25925925925926, 1.8518518518518516], [9.75925925925926, 1.8518518518518516], [9.685185185185183, 2.0370370370370368], [9.185185185185183, 2.0370370370370368]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 69);
        cqClose(c, st, 69, 68, {});
        atStep(st, 70);
        cqRevolve(c, st, 70, 69, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 70, "revolve");
        atStep(st, 71);
        cqRotate(c, st, 71, 70, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 100});
        stepVol(c, st, 71, "rotate");
        atStep(st, 72);
        cqUnion(c, st, 72, 66, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 74);
        cqPolyline(c, st, 74, 73, {"listOfXYTuple": [[9.185185185185183, 2.0370370370370368], [9.685185185185183, 2.0370370370370368], [9.61111111111111, 2.2222222222222223], [9.11111111111111, 2.2222222222222223]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 75);
        cqClose(c, st, 75, 74, {});
        atStep(st, 76);
        cqRevolve(c, st, 76, 75, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 76, "revolve");
        atStep(st, 77);
        cqRotate(c, st, 77, 76, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 110});
        stepVol(c, st, 77, "rotate");
        atStep(st, 78);
        cqUnion(c, st, 78, 72, {"toUnion": {"wp": 77}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 78, "union");
        atStep(st, 79);
        cqWorkplaneNew(c, st, 79, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 80);
        cqPolyline(c, st, 80, 79, {"listOfXYTuple": [[9.11111111111111, 2.2222222222222223], [9.61111111111111, 2.2222222222222223], [9.537037037037036, 2.407407407407407], [9.037037037037036, 2.407407407407407]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 81);
        cqClose(c, st, 81, 80, {});
        atStep(st, 82);
        cqRevolve(c, st, 82, 81, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 82, "revolve");
        atStep(st, 83);
        cqRotate(c, st, 83, 82, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 120});
        stepVol(c, st, 83, "rotate");
        atStep(st, 84);
        cqUnion(c, st, 84, 78, {"toUnion": {"wp": 83}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 84, "union");
        atStep(st, 85);
        cqWorkplaneNew(c, st, 85, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 86);
        cqPolyline(c, st, 86, 85, {"listOfXYTuple": [[9.037037037037036, 2.407407407407407], [9.537037037037036, 2.407407407407407], [9.462962962962964, 2.592592592592593], [8.962962962962964, 2.592592592592593]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 87);
        cqClose(c, st, 87, 86, {});
        atStep(st, 88);
        cqRevolve(c, st, 88, 87, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 88, "revolve");
        atStep(st, 89);
        cqRotate(c, st, 89, 88, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 130});
        stepVol(c, st, 89, "rotate");
        atStep(st, 90);
        cqUnion(c, st, 90, 84, {"toUnion": {"wp": 89}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 90, "union");
        atStep(st, 91);
        cqWorkplaneNew(c, st, 91, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 92);
        cqPolyline(c, st, 92, 91, {"listOfXYTuple": [[8.962962962962964, 2.592592592592593], [9.462962962962964, 2.592592592592593], [9.38888888888889, 2.7777777777777777], [8.88888888888889, 2.7777777777777777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 93);
        cqClose(c, st, 93, 92, {});
        atStep(st, 94);
        cqRevolve(c, st, 94, 93, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 94, "revolve");
        atStep(st, 95);
        cqRotate(c, st, 95, 94, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 140});
        stepVol(c, st, 95, "rotate");
        atStep(st, 96);
        cqUnion(c, st, 96, 90, {"toUnion": {"wp": 95}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 96, "union");
        atStep(st, 97);
        cqWorkplaneNew(c, st, 97, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 98);
        cqPolyline(c, st, 98, 97, {"listOfXYTuple": [[8.88888888888889, 2.7777777777777777], [9.38888888888889, 2.7777777777777777], [9.314814814814817, 2.962962962962963], [8.814814814814815, 2.962962962962963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 99);
        cqClose(c, st, 99, 98, {});
        atStep(st, 100);
        cqRevolve(c, st, 100, 99, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 100, "revolve");
        atStep(st, 101);
        cqRotate(c, st, 101, 100, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 150});
        stepVol(c, st, 101, "rotate");
        atStep(st, 102);
        cqUnion(c, st, 102, 96, {"toUnion": {"wp": 101}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 102, "union");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 104);
        cqPolyline(c, st, 104, 103, {"listOfXYTuple": [[8.814814814814815, 2.962962962962963], [9.314814814814817, 2.962962962962963], [9.24074074074074, 3.1481481481481484], [8.74074074074074, 3.1481481481481484]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 105);
        cqClose(c, st, 105, 104, {});
        atStep(st, 106);
        cqRevolve(c, st, 106, 105, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 106, "revolve");
        atStep(st, 107);
        cqRotate(c, st, 107, 106, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 160});
        stepVol(c, st, 107, "rotate");
        atStep(st, 108);
        cqUnion(c, st, 108, 102, {"toUnion": {"wp": 107}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 108, "union");
        atStep(st, 109);
        cqWorkplaneNew(c, st, 109, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 110);
        cqPolyline(c, st, 110, 109, {"listOfXYTuple": [[8.74074074074074, 3.1481481481481484], [9.24074074074074, 3.1481481481481484], [9.166666666666666, 3.333333333333333], [8.666666666666666, 3.333333333333333]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 111);
        cqClose(c, st, 111, 110, {});
        atStep(st, 112);
        cqRevolve(c, st, 112, 111, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 112, "revolve");
        atStep(st, 113);
        cqRotate(c, st, 113, 112, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 170});
        stepVol(c, st, 113, "rotate");
        atStep(st, 114);
        cqUnion(c, st, 114, 108, {"toUnion": {"wp": 113}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 114, "union");
        atStep(st, 115);
        cqWorkplaneNew(c, st, 115, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 116);
        cqPolyline(c, st, 116, 115, {"listOfXYTuple": [[8.666666666666666, 3.333333333333333], [9.166666666666666, 3.333333333333333], [9.092592592592592, 3.5185185185185186], [8.592592592592592, 3.5185185185185186]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 117);
        cqClose(c, st, 117, 116, {});
        atStep(st, 118);
        cqRevolve(c, st, 118, 117, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 118, "revolve");
        atStep(st, 119);
        cqRotate(c, st, 119, 118, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 180});
        stepVol(c, st, 119, "rotate");
        atStep(st, 120);
        cqUnion(c, st, 120, 114, {"toUnion": {"wp": 119}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 120, "union");
        atStep(st, 121);
        cqWorkplaneNew(c, st, 121, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 122);
        cqPolyline(c, st, 122, 121, {"listOfXYTuple": [[8.592592592592592, 3.5185185185185186], [9.092592592592592, 3.5185185185185186], [9.01851851851852, 3.7037037037037033], [8.518518518518519, 3.7037037037037033]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 123);
        cqClose(c, st, 123, 122, {});
        atStep(st, 124);
        cqRevolve(c, st, 124, 123, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 124, "revolve");
        atStep(st, 125);
        cqRotate(c, st, 125, 124, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 190});
        stepVol(c, st, 125, "rotate");
        atStep(st, 126);
        cqUnion(c, st, 126, 120, {"toUnion": {"wp": 125}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 126, "union");
        atStep(st, 127);
        cqWorkplaneNew(c, st, 127, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 128);
        cqPolyline(c, st, 128, 127, {"listOfXYTuple": [[8.518518518518519, 3.7037037037037033], [9.01851851851852, 3.7037037037037033], [8.944444444444445, 3.888888888888889], [8.444444444444445, 3.888888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 129);
        cqClose(c, st, 129, 128, {});
        atStep(st, 130);
        cqRevolve(c, st, 130, 129, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 130, "revolve");
        atStep(st, 131);
        cqRotate(c, st, 131, 130, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 200});
        stepVol(c, st, 131, "rotate");
        atStep(st, 132);
        cqUnion(c, st, 132, 126, {"toUnion": {"wp": 131}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 132, "union");
        atStep(st, 133);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 134);
        cqPolyline(c, st, 134, 133, {"listOfXYTuple": [[8.444444444444445, 3.888888888888889], [8.944444444444445, 3.888888888888889], [8.87037037037037, 4.0740740740740735], [8.37037037037037, 4.0740740740740735]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 135);
        cqClose(c, st, 135, 134, {});
        atStep(st, 136);
        cqRevolve(c, st, 136, 135, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 136, "revolve");
        atStep(st, 137);
        cqRotate(c, st, 137, 136, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 210});
        stepVol(c, st, 137, "rotate");
        atStep(st, 138);
        cqUnion(c, st, 138, 132, {"toUnion": {"wp": 137}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 138, "union");
        atStep(st, 139);
        cqWorkplaneNew(c, st, 139, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 140);
        cqPolyline(c, st, 140, 139, {"listOfXYTuple": [[8.37037037037037, 4.0740740740740735], [8.87037037037037, 4.0740740740740735], [8.796296296296296, 4.2592592592592595], [8.296296296296296, 4.2592592592592595]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 141);
        cqClose(c, st, 141, 140, {});
        atStep(st, 142);
        cqRevolve(c, st, 142, 141, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 142, "revolve");
        atStep(st, 143);
        cqRotate(c, st, 143, 142, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 220});
        stepVol(c, st, 143, "rotate");
        atStep(st, 144);
        cqUnion(c, st, 144, 138, {"toUnion": {"wp": 143}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 144, "union");
        atStep(st, 145);
        cqWorkplaneNew(c, st, 145, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 146);
        cqPolyline(c, st, 146, 145, {"listOfXYTuple": [[8.296296296296296, 4.2592592592592595], [8.796296296296296, 4.2592592592592595], [8.722222222222221, 4.444444444444445], [8.222222222222221, 4.444444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 147);
        cqClose(c, st, 147, 146, {});
        atStep(st, 148);
        cqRevolve(c, st, 148, 147, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 148, "revolve");
        atStep(st, 149);
        cqRotate(c, st, 149, 148, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 230});
        stepVol(c, st, 149, "rotate");
        atStep(st, 150);
        cqUnion(c, st, 150, 144, {"toUnion": {"wp": 149}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 150, "union");
        atStep(st, 151);
        cqWorkplaneNew(c, st, 151, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 152);
        cqPolyline(c, st, 152, 151, {"listOfXYTuple": [[8.222222222222221, 4.444444444444445], [8.722222222222221, 4.444444444444445], [8.648148148148149, 4.62962962962963], [8.148148148148149, 4.62962962962963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 153);
        cqClose(c, st, 153, 152, {});
        atStep(st, 154);
        cqRevolve(c, st, 154, 153, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 154, "revolve");
        atStep(st, 155);
        cqRotate(c, st, 155, 154, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 240});
        stepVol(c, st, 155, "rotate");
        atStep(st, 156);
        cqUnion(c, st, 156, 150, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        atStep(st, 157);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 158);
        cqPolyline(c, st, 158, 157, {"listOfXYTuple": [[8.148148148148149, 4.62962962962963], [8.648148148148149, 4.62962962962963], [8.574074074074074, 4.814814814814815], [8.074074074074074, 4.814814814814815]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 159);
        cqClose(c, st, 159, 158, {});
        atStep(st, 160);
        cqRevolve(c, st, 160, 159, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 160, "revolve");
        atStep(st, 161);
        cqRotate(c, st, 161, 160, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 250});
        stepVol(c, st, 161, "rotate");
        atStep(st, 162);
        cqUnion(c, st, 162, 156, {"toUnion": {"wp": 161}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 162, "union");
        atStep(st, 163);
        cqWorkplaneNew(c, st, 163, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 164);
        cqPolyline(c, st, 164, 163, {"listOfXYTuple": [[8.074074074074074, 4.814814814814815], [8.574074074074074, 4.814814814814815], [8.5, 5], [8, 5]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 165);
        cqClose(c, st, 165, 164, {});
        atStep(st, 166);
        cqRevolve(c, st, 166, 165, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 166, "revolve");
        atStep(st, 167);
        cqRotate(c, st, 167, 166, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 260});
        stepVol(c, st, 167, "rotate");
        atStep(st, 168);
        cqUnion(c, st, 168, 162, {"toUnion": {"wp": 167}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 168, "union");
        atStep(st, 169);
        cqWorkplaneNew(c, st, 169, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 170);
        cqPolyline(c, st, 170, 169, {"listOfXYTuple": [[8, 5], [8.5, 5], [8.425925925925926, 5.185185185185185], [7.925925925925926, 5.185185185185185]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 171);
        cqClose(c, st, 171, 170, {});
        atStep(st, 172);
        cqRevolve(c, st, 172, 171, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 172, "revolve");
        atStep(st, 173);
        cqRotate(c, st, 173, 172, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 270});
        stepVol(c, st, 173, "rotate");
        atStep(st, 174);
        cqUnion(c, st, 174, 168, {"toUnion": {"wp": 173}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 174, "union");
        atStep(st, 175);
        cqWorkplaneNew(c, st, 175, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 176);
        cqPolyline(c, st, 176, 175, {"listOfXYTuple": [[7.925925925925926, 5.185185185185185], [8.425925925925926, 5.185185185185185], [8.351851851851851, 5.370370370370371], [7.851851851851851, 5.370370370370371]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 177);
        cqClose(c, st, 177, 176, {});
        atStep(st, 178);
        cqRevolve(c, st, 178, 177, {"angleDegrees": 10.000000000000057, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 178, "revolve");
        atStep(st, 179);
        cqRotate(c, st, 179, 178, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 280});
        stepVol(c, st, 179, "rotate");
        atStep(st, 180);
        cqUnion(c, st, 180, 174, {"toUnion": {"wp": 179}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 180, "union");
        atStep(st, 181);
        cqWorkplaneNew(c, st, 181, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 182);
        cqPolyline(c, st, 182, 181, {"listOfXYTuple": [[7.851851851851851, 5.370370370370371], [8.351851851851851, 5.370370370370371], [8.277777777777779, 5.555555555555555], [7.777777777777778, 5.555555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 183);
        cqClose(c, st, 183, 182, {});
        atStep(st, 184);
        cqRevolve(c, st, 184, 183, {"angleDegrees": 9.999999999999943, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 184, "revolve");
        atStep(st, 185);
        cqRotate(c, st, 185, 184, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 290.00000000000006});
        stepVol(c, st, 185, "rotate");
        atStep(st, 186);
        cqUnion(c, st, 186, 180, {"toUnion": {"wp": 185}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 186, "union");
        atStep(st, 187);
        cqWorkplaneNew(c, st, 187, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 188);
        cqPolyline(c, st, 188, 187, {"listOfXYTuple": [[7.777777777777778, 5.555555555555555], [8.277777777777779, 5.555555555555555], [8.203703703703704, 5.7407407407407405], [7.703703703703704, 5.7407407407407405]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 189);
        cqClose(c, st, 189, 188, {});
        atStep(st, 190);
        cqRevolve(c, st, 190, 189, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 190, "revolve");
        atStep(st, 191);
        cqRotate(c, st, 191, 190, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 300});
        stepVol(c, st, 191, "rotate");
        atStep(st, 192);
        cqUnion(c, st, 192, 186, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        atStep(st, 193);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 194);
        cqPolyline(c, st, 194, 193, {"listOfXYTuple": [[7.703703703703704, 5.7407407407407405], [8.203703703703704, 5.7407407407407405], [8.12962962962963, 5.925925925925926], [7.62962962962963, 5.925925925925926]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 195);
        cqClose(c, st, 195, 194, {});
        atStep(st, 196);
        cqRevolve(c, st, 196, 195, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 196, "revolve");
        atStep(st, 197);
        cqRotate(c, st, 197, 196, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 310});
        stepVol(c, st, 197, "rotate");
        atStep(st, 198);
        cqUnion(c, st, 198, 192, {"toUnion": {"wp": 197}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 198, "union");
        atStep(st, 199);
        cqWorkplaneNew(c, st, 199, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 200);
        cqPolyline(c, st, 200, 199, {"listOfXYTuple": [[7.62962962962963, 5.925925925925926], [8.12962962962963, 5.925925925925926], [8.055555555555555, 6.111111111111112], [7.555555555555555, 6.111111111111112]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 201);
        cqClose(c, st, 201, 200, {});
        atStep(st, 202);
        cqRevolve(c, st, 202, 201, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 202, "revolve");
        atStep(st, 203);
        cqRotate(c, st, 203, 202, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 320});
        stepVol(c, st, 203, "rotate");
        atStep(st, 204);
        cqUnion(c, st, 204, 198, {"toUnion": {"wp": 203}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 204, "union");
        atStep(st, 205);
        cqWorkplaneNew(c, st, 205, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 206);
        cqPolyline(c, st, 206, 205, {"listOfXYTuple": [[7.555555555555555, 6.111111111111112], [8.055555555555555, 6.111111111111112], [7.981481481481481, 6.296296296296297], [7.481481481481481, 6.296296296296297]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 207);
        cqClose(c, st, 207, 206, {});
        atStep(st, 208);
        cqRevolve(c, st, 208, 207, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 208, "revolve");
        atStep(st, 209);
        cqRotate(c, st, 209, 208, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 330});
        stepVol(c, st, 209, "rotate");
        atStep(st, 210);
        cqUnion(c, st, 210, 204, {"toUnion": {"wp": 209}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 210, "union");
        atStep(st, 211);
        cqWorkplaneNew(c, st, 211, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 212);
        cqPolyline(c, st, 212, 211, {"listOfXYTuple": [[7.481481481481481, 6.296296296296297], [7.981481481481481, 6.296296296296297], [7.907407407407407, 6.481481481481481], [7.407407407407407, 6.481481481481481]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 213);
        cqClose(c, st, 213, 212, {});
        atStep(st, 214);
        cqRevolve(c, st, 214, 213, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 214, "revolve");
        atStep(st, 215);
        cqRotate(c, st, 215, 214, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 340});
        stepVol(c, st, 215, "rotate");
        atStep(st, 216);
        cqUnion(c, st, 216, 210, {"toUnion": {"wp": 215}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 216, "union");
        atStep(st, 217);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 218);
        cqPolyline(c, st, 218, 217, {"listOfXYTuple": [[7.407407407407407, 6.481481481481481], [7.907407407407407, 6.481481481481481], [7.833333333333334, 6.666666666666666], [7.333333333333334, 6.666666666666666]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 219);
        cqClose(c, st, 219, 218, {});
        atStep(st, 220);
        cqRevolve(c, st, 220, 219, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 220, "revolve");
        atStep(st, 221);
        cqRotate(c, st, 221, 220, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 350});
        stepVol(c, st, 221, "rotate");
        atStep(st, 222);
        cqUnion(c, st, 222, 216, {"toUnion": {"wp": 221}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 222, "union");
        atStep(st, 223);
        cqWorkplaneNew(c, st, 223, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 224);
        cqPolyline(c, st, 224, 223, {"listOfXYTuple": [[7.333333333333334, 6.666666666666666], [7.833333333333334, 6.666666666666666], [7.7592592592592595, 6.851851851851852], [7.2592592592592595, 6.851851851851852]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 225);
        cqClose(c, st, 225, 224, {});
        atStep(st, 226);
        cqRevolve(c, st, 226, 225, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 226, "revolve");
        atStep(st, 227);
        cqRotate(c, st, 227, 226, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 360});
        stepVol(c, st, 227, "rotate");
        atStep(st, 228);
        cqUnion(c, st, 228, 222, {"toUnion": {"wp": 227}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 228, "union");
        atStep(st, 229);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 230);
        cqPolyline(c, st, 230, 229, {"listOfXYTuple": [[7.2592592592592595, 6.851851851851852], [7.7592592592592595, 6.851851851851852], [7.685185185185185, 7.037037037037037], [7.185185185185185, 7.037037037037037]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 231);
        cqClose(c, st, 231, 230, {});
        atStep(st, 232);
        cqRevolve(c, st, 232, 231, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 232, "revolve");
        atStep(st, 233);
        cqRotate(c, st, 233, 232, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 370});
        stepVol(c, st, 233, "rotate");
        atStep(st, 234);
        cqUnion(c, st, 234, 228, {"toUnion": {"wp": 233}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 234, "union");
        atStep(st, 235);
        cqWorkplaneNew(c, st, 235, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 236);
        cqPolyline(c, st, 236, 235, {"listOfXYTuple": [[7.185185185185185, 7.037037037037037], [7.685185185185185, 7.037037037037037], [7.611111111111111, 7.222222222222222], [7.111111111111111, 7.222222222222222]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 237);
        cqClose(c, st, 237, 236, {});
        atStep(st, 238);
        cqRevolve(c, st, 238, 237, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 238, "revolve");
        atStep(st, 239);
        cqRotate(c, st, 239, 238, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 380});
        stepVol(c, st, 239, "rotate");
        atStep(st, 240);
        cqUnion(c, st, 240, 234, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        atStep(st, 241);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 242);
        cqPolyline(c, st, 242, 241, {"listOfXYTuple": [[7.111111111111111, 7.222222222222222], [7.611111111111111, 7.222222222222222], [7.537037037037037, 7.4074074074074066], [7.037037037037037, 7.4074074074074066]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 243);
        cqClose(c, st, 243, 242, {});
        atStep(st, 244);
        cqRevolve(c, st, 244, 243, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 244, "revolve");
        atStep(st, 245);
        cqRotate(c, st, 245, 244, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 390});
        stepVol(c, st, 245, "rotate");
        atStep(st, 246);
        cqUnion(c, st, 246, 240, {"toUnion": {"wp": 245}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 246, "union");
        atStep(st, 247);
        cqWorkplaneNew(c, st, 247, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 248);
        cqPolyline(c, st, 248, 247, {"listOfXYTuple": [[7.037037037037037, 7.4074074074074066], [7.537037037037037, 7.4074074074074066], [7.462962962962962, 7.5925925925925934], [6.962962962962962, 7.5925925925925934]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 249);
        cqClose(c, st, 249, 248, {});
        atStep(st, 250);
        cqRevolve(c, st, 250, 249, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 250, "revolve");
        atStep(st, 251);
        cqRotate(c, st, 251, 250, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 400});
        stepVol(c, st, 251, "rotate");
        atStep(st, 252);
        cqUnion(c, st, 252, 246, {"toUnion": {"wp": 251}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 252, "union");
        atStep(st, 253);
        cqWorkplaneNew(c, st, 253, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 254);
        cqPolyline(c, st, 254, 253, {"listOfXYTuple": [[6.962962962962962, 7.5925925925925934], [7.462962962962962, 7.5925925925925934], [7.388888888888889, 7.777777777777778], [6.888888888888889, 7.777777777777778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 255);
        cqClose(c, st, 255, 254, {});
        atStep(st, 256);
        cqRevolve(c, st, 256, 255, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 256, "revolve");
        atStep(st, 257);
        cqRotate(c, st, 257, 256, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 410});
        stepVol(c, st, 257, "rotate");
        atStep(st, 258);
        cqUnion(c, st, 258, 252, {"toUnion": {"wp": 257}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 258, "union");
        atStep(st, 259);
        cqWorkplaneNew(c, st, 259, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 260);
        cqPolyline(c, st, 260, 259, {"listOfXYTuple": [[6.888888888888889, 7.777777777777778], [7.388888888888889, 7.777777777777778], [7.314814814814815, 7.962962962962963], [6.814814814814815, 7.962962962962963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 261);
        cqClose(c, st, 261, 260, {});
        atStep(st, 262);
        cqRevolve(c, st, 262, 261, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 262, "revolve");
        atStep(st, 263);
        cqRotate(c, st, 263, 262, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 420});
        stepVol(c, st, 263, "rotate");
        atStep(st, 264);
        cqUnion(c, st, 264, 258, {"toUnion": {"wp": 263}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 264, "union");
        atStep(st, 265);
        cqWorkplaneNew(c, st, 265, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 266);
        cqPolyline(c, st, 266, 265, {"listOfXYTuple": [[6.814814814814815, 7.962962962962963], [7.314814814814815, 7.962962962962963], [7.2407407407407405, 8.148148148148147], [6.7407407407407405, 8.148148148148147]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 267);
        cqClose(c, st, 267, 266, {});
        atStep(st, 268);
        cqRevolve(c, st, 268, 267, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 268, "revolve");
        atStep(st, 269);
        cqRotate(c, st, 269, 268, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 430});
        stepVol(c, st, 269, "rotate");
        atStep(st, 270);
        cqUnion(c, st, 270, 264, {"toUnion": {"wp": 269}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 270, "union");
        atStep(st, 271);
        cqWorkplaneNew(c, st, 271, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 272);
        cqPolyline(c, st, 272, 271, {"listOfXYTuple": [[6.7407407407407405, 8.148148148148147], [7.2407407407407405, 8.148148148148147], [7.166666666666666, 8.333333333333334], [6.666666666666666, 8.333333333333334]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 273);
        cqClose(c, st, 273, 272, {});
        atStep(st, 274);
        cqRevolve(c, st, 274, 273, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 274, "revolve");
        atStep(st, 275);
        cqRotate(c, st, 275, 274, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 440});
        stepVol(c, st, 275, "rotate");
        atStep(st, 276);
        cqUnion(c, st, 276, 270, {"toUnion": {"wp": 275}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 276, "union");
        atStep(st, 277);
        cqWorkplaneNew(c, st, 277, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 278);
        cqPolyline(c, st, 278, 277, {"listOfXYTuple": [[6.666666666666666, 8.333333333333334], [7.166666666666666, 8.333333333333334], [7.092592592592592, 8.518518518518519], [6.592592592592592, 8.518518518518519]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 279);
        cqClose(c, st, 279, 278, {});
        atStep(st, 280);
        cqRevolve(c, st, 280, 279, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 280, "revolve");
        atStep(st, 281);
        cqRotate(c, st, 281, 280, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 450});
        stepVol(c, st, 281, "rotate");
        atStep(st, 282);
        cqUnion(c, st, 282, 276, {"toUnion": {"wp": 281}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 282, "union");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 284);
        cqPolyline(c, st, 284, 283, {"listOfXYTuple": [[6.592592592592592, 8.518518518518519], [7.092592592592592, 8.518518518518519], [7.018518518518518, 8.703703703703704], [6.518518518518518, 8.703703703703704]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 285);
        cqClose(c, st, 285, 284, {});
        atStep(st, 286);
        cqRevolve(c, st, 286, 285, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 286, "revolve");
        atStep(st, 287);
        cqRotate(c, st, 287, 286, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 460});
        stepVol(c, st, 287, "rotate");
        atStep(st, 288);
        cqUnion(c, st, 288, 282, {"toUnion": {"wp": 287}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 288, "union");
        atStep(st, 289);
        cqWorkplaneNew(c, st, 289, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 290);
        cqPolyline(c, st, 290, 289, {"listOfXYTuple": [[6.518518518518518, 8.703703703703704], [7.018518518518518, 8.703703703703704], [6.944444444444445, 8.88888888888889], [6.444444444444445, 8.88888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 291);
        cqClose(c, st, 291, 290, {});
        atStep(st, 292);
        cqRevolve(c, st, 292, 291, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 292, "revolve");
        atStep(st, 293);
        cqRotate(c, st, 293, 292, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 470});
        stepVol(c, st, 293, "rotate");
        atStep(st, 294);
        cqUnion(c, st, 294, 288, {"toUnion": {"wp": 293}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 294, "union");
        atStep(st, 295);
        cqWorkplaneNew(c, st, 295, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 296);
        cqPolyline(c, st, 296, 295, {"listOfXYTuple": [[6.444444444444445, 8.88888888888889], [6.944444444444445, 8.88888888888889], [6.87037037037037, 9.074074074074074], [6.37037037037037, 9.074074074074074]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 297);
        cqClose(c, st, 297, 296, {});
        atStep(st, 298);
        cqRevolve(c, st, 298, 297, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 298, "revolve");
        atStep(st, 299);
        cqRotate(c, st, 299, 298, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 480});
        stepVol(c, st, 299, "rotate");
        atStep(st, 300);
        cqUnion(c, st, 300, 294, {"toUnion": {"wp": 299}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 300, "union");
        atStep(st, 301);
        cqWorkplaneNew(c, st, 301, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 302);
        cqPolyline(c, st, 302, 301, {"listOfXYTuple": [[6.37037037037037, 9.074074074074074], [6.87037037037037, 9.074074074074074], [6.796296296296296, 9.25925925925926], [6.296296296296296, 9.25925925925926]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 303);
        cqClose(c, st, 303, 302, {});
        atStep(st, 304);
        cqRevolve(c, st, 304, 303, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 304, "revolve");
        atStep(st, 305);
        cqRotate(c, st, 305, 304, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 490});
        stepVol(c, st, 305, "rotate");
        atStep(st, 306);
        cqUnion(c, st, 306, 300, {"toUnion": {"wp": 305}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 306, "union");
        atStep(st, 307);
        cqWorkplaneNew(c, st, 307, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 308);
        cqPolyline(c, st, 308, 307, {"listOfXYTuple": [[6.296296296296296, 9.25925925925926], [6.796296296296296, 9.25925925925926], [6.722222222222221, 9.444444444444445], [6.222222222222221, 9.444444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 309);
        cqClose(c, st, 309, 308, {});
        atStep(st, 310);
        cqRevolve(c, st, 310, 309, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 310, "revolve");
        atStep(st, 311);
        cqRotate(c, st, 311, 310, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 500});
        stepVol(c, st, 311, "rotate");
        atStep(st, 312);
        cqUnion(c, st, 312, 306, {"toUnion": {"wp": 311}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 312, "union");
        atStep(st, 313);
        cqWorkplaneNew(c, st, 313, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 314);
        cqPolyline(c, st, 314, 313, {"listOfXYTuple": [[6.222222222222221, 9.444444444444445], [6.722222222222221, 9.444444444444445], [6.648148148148148, 9.62962962962963], [6.148148148148148, 9.62962962962963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 315);
        cqClose(c, st, 315, 314, {});
        atStep(st, 316);
        cqRevolve(c, st, 316, 315, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 316, "revolve");
        atStep(st, 317);
        cqRotate(c, st, 317, 316, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 510});
        stepVol(c, st, 317, "rotate");
        atStep(st, 318);
        cqUnion(c, st, 318, 312, {"toUnion": {"wp": 317}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 318, "union");
        atStep(st, 319);
        cqWorkplaneNew(c, st, 319, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 320);
        cqPolyline(c, st, 320, 319, {"listOfXYTuple": [[6.148148148148148, 9.62962962962963], [6.648148148148148, 9.62962962962963], [6.574074074074074, 9.814814814814817], [6.074074074074074, 9.814814814814817]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 321);
        cqClose(c, st, 321, 320, {});
        atStep(st, 322);
        cqRevolve(c, st, 322, 321, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 322, "revolve");
        atStep(st, 323);
        cqRotate(c, st, 323, 322, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 520});
        stepVol(c, st, 323, "rotate");
        atStep(st, 324);
        cqUnion(c, st, 324, 318, {"toUnion": {"wp": 323}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 324, "union");
        atStep(st, 325);
        cqWorkplaneNew(c, st, 325, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 326);
        cqPolyline(c, st, 326, 325, {"listOfXYTuple": [[6.074074074074074, 9.814814814814817], [6.574074074074074, 9.814814814814817], [6.5, 10], [6, 10]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 327);
        cqClose(c, st, 327, 326, {});
        atStep(st, 328);
        cqRevolve(c, st, 328, 327, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 328, "revolve");
        atStep(st, 329);
        cqRotate(c, st, 329, 328, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 530});
        stepVol(c, st, 329, "rotate");
        atStep(st, 330);
        cqUnion(c, st, 330, 324, {"toUnion": {"wp": 329}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 330, "union");
        atStep(st, 331);
        cqWorkplaneNew(c, st, 331, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 332);
        cqPolyline(c, st, 332, 331, {"listOfXYTuple": [[6, 10], [6.5, 10], [6.425925925925925, 10.185185185185189], [5.925925925925925, 10.185185185185189]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 333);
        cqClose(c, st, 333, 332, {});
        atStep(st, 334);
        cqRevolve(c, st, 334, 333, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 334, "revolve");
        atStep(st, 335);
        cqRotate(c, st, 335, 334, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 540});
        stepVol(c, st, 335, "rotate");
        atStep(st, 336);
        cqUnion(c, st, 336, 330, {"toUnion": {"wp": 335}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 336, "union");
        atStep(st, 337);
        cqWorkplaneNew(c, st, 337, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 338);
        cqPolyline(c, st, 338, 337, {"listOfXYTuple": [[5.925925925925925, 10.185185185185189], [6.425925925925925, 10.185185185185189], [6.351851851851852, 10.37037037037037], [5.851851851851852, 10.37037037037037]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 339);
        cqClose(c, st, 339, 338, {});
        atStep(st, 340);
        cqRevolve(c, st, 340, 339, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 340, "revolve");
        atStep(st, 341);
        cqRotate(c, st, 341, 340, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 550});
        stepVol(c, st, 341, "rotate");
        atStep(st, 342);
        cqUnion(c, st, 342, 336, {"toUnion": {"wp": 341}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 342, "union");
        atStep(st, 343);
        cqWorkplaneNew(c, st, 343, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 344);
        cqPolyline(c, st, 344, 343, {"listOfXYTuple": [[5.851851851851852, 10.37037037037037], [6.351851851851852, 10.37037037037037], [6.277777777777778, 10.555555555555555], [5.777777777777778, 10.555555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 345);
        cqClose(c, st, 345, 344, {});
        atStep(st, 346);
        cqRevolve(c, st, 346, 345, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 346, "revolve");
        atStep(st, 347);
        cqRotate(c, st, 347, 346, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 560});
        stepVol(c, st, 347, "rotate");
        atStep(st, 348);
        cqUnion(c, st, 348, 342, {"toUnion": {"wp": 347}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 348, "union");
        atStep(st, 349);
        cqWorkplaneNew(c, st, 349, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 350);
        cqPolyline(c, st, 350, 349, {"listOfXYTuple": [[5.777777777777778, 10.555555555555555], [6.277777777777778, 10.555555555555555], [6.203703703703703, 10.740740740740742], [5.703703703703703, 10.740740740740742]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 351);
        cqClose(c, st, 351, 350, {});
        atStep(st, 352);
        cqRevolve(c, st, 352, 351, {"angleDegrees": 10.000000000000114, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 352, "revolve");
        atStep(st, 353);
        cqRotate(c, st, 353, 352, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 570});
        stepVol(c, st, 353, "rotate");
        atStep(st, 354);
        cqUnion(c, st, 354, 348, {"toUnion": {"wp": 353}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 354, "union");
        atStep(st, 355);
        cqWorkplaneNew(c, st, 355, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 356);
        cqPolyline(c, st, 356, 355, {"listOfXYTuple": [[5.703703703703703, 10.740740740740742], [6.203703703703703, 10.740740740740742], [6.12962962962963, 10.925925925925926], [5.62962962962963, 10.925925925925926]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 357);
        cqClose(c, st, 357, 356, {});
        atStep(st, 358);
        cqRevolve(c, st, 358, 357, {"angleDegrees": 9.999999999999886, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 358, "revolve");
        atStep(st, 359);
        cqRotate(c, st, 359, 358, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 580.0000000000001});
        stepVol(c, st, 359, "rotate");
        atStep(st, 360);
        cqUnion(c, st, 360, 354, {"toUnion": {"wp": 359}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 360, "union");
        atStep(st, 361);
        cqWorkplaneNew(c, st, 361, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 362);
        cqPolyline(c, st, 362, 361, {"listOfXYTuple": [[5.62962962962963, 10.925925925925926], [6.12962962962963, 10.925925925925926], [6.055555555555555, 11.11111111111111], [5.555555555555555, 11.11111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 363);
        cqClose(c, st, 363, 362, {});
        atStep(st, 364);
        cqRevolve(c, st, 364, 363, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 364, "revolve");
        atStep(st, 365);
        cqRotate(c, st, 365, 364, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 590});
        stepVol(c, st, 365, "rotate");
        atStep(st, 366);
        cqUnion(c, st, 366, 360, {"toUnion": {"wp": 365}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 366, "union");
        atStep(st, 367);
        cqWorkplaneNew(c, st, 367, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 368);
        cqPolyline(c, st, 368, 367, {"listOfXYTuple": [[5.555555555555555, 11.11111111111111], [6.055555555555555, 11.11111111111111], [5.981481481481482, 11.296296296296296], [5.481481481481482, 11.296296296296296]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 369);
        cqClose(c, st, 369, 368, {});
        atStep(st, 370);
        cqRevolve(c, st, 370, 369, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 370, "revolve");
        atStep(st, 371);
        cqRotate(c, st, 371, 370, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 600});
        stepVol(c, st, 371, "rotate");
        atStep(st, 372);
        cqUnion(c, st, 372, 366, {"toUnion": {"wp": 371}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 372, "union");
        atStep(st, 373);
        cqWorkplaneNew(c, st, 373, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 374);
        cqPolyline(c, st, 374, 373, {"listOfXYTuple": [[5.481481481481482, 11.296296296296296], [5.981481481481482, 11.296296296296296], [5.907407407407407, 11.48148148148148], [5.407407407407407, 11.48148148148148]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 375);
        cqClose(c, st, 375, 374, {});
        atStep(st, 376);
        cqRevolve(c, st, 376, 375, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 376, "revolve");
        atStep(st, 377);
        cqRotate(c, st, 377, 376, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 610});
        stepVol(c, st, 377, "rotate");
        atStep(st, 378);
        cqUnion(c, st, 378, 372, {"toUnion": {"wp": 377}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 378, "union");
        atStep(st, 379);
        cqWorkplaneNew(c, st, 379, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 380);
        cqPolyline(c, st, 380, 379, {"listOfXYTuple": [[5.407407407407407, 11.48148148148148], [5.907407407407407, 11.48148148148148], [5.833333333333333, 11.666666666666668], [5.333333333333333, 11.666666666666668]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 381);
        cqClose(c, st, 381, 380, {});
        atStep(st, 382);
        cqRevolve(c, st, 382, 381, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 382, "revolve");
        atStep(st, 383);
        cqRotate(c, st, 383, 382, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 620});
        stepVol(c, st, 383, "rotate");
        atStep(st, 384);
        cqUnion(c, st, 384, 378, {"toUnion": {"wp": 383}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 384, "union");
        atStep(st, 385);
        cqWorkplaneNew(c, st, 385, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 386);
        cqPolyline(c, st, 386, 385, {"listOfXYTuple": [[5.333333333333333, 11.666666666666668], [5.833333333333333, 11.666666666666668], [5.7592592592592595, 11.851851851851851], [5.2592592592592595, 11.851851851851851]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 387);
        cqClose(c, st, 387, 386, {});
        atStep(st, 388);
        cqRevolve(c, st, 388, 387, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 388, "revolve");
        atStep(st, 389);
        cqRotate(c, st, 389, 388, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 630});
        stepVol(c, st, 389, "rotate");
        atStep(st, 390);
        cqUnion(c, st, 390, 384, {"toUnion": {"wp": 389}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 390, "union");
        atStep(st, 391);
        cqWorkplaneNew(c, st, 391, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 392);
        cqPolyline(c, st, 392, 391, {"listOfXYTuple": [[5.2592592592592595, 11.851851851851851], [5.7592592592592595, 11.851851851851851], [5.685185185185184, 12.037037037037038], [5.185185185185184, 12.037037037037038]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 393);
        cqClose(c, st, 393, 392, {});
        atStep(st, 394);
        cqRevolve(c, st, 394, 393, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 394, "revolve");
        atStep(st, 395);
        cqRotate(c, st, 395, 394, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 640});
        stepVol(c, st, 395, "rotate");
        atStep(st, 396);
        cqUnion(c, st, 396, 390, {"toUnion": {"wp": 395}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 396, "union");
        atStep(st, 397);
        cqWorkplaneNew(c, st, 397, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 398);
        cqPolyline(c, st, 398, 397, {"listOfXYTuple": [[5.185185185185184, 12.037037037037038], [5.685185185185184, 12.037037037037038], [5.611111111111111, 12.222222222222223], [5.111111111111111, 12.222222222222223]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 399);
        cqClose(c, st, 399, 398, {});
        atStep(st, 400);
        cqRevolve(c, st, 400, 399, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 400, "revolve");
        atStep(st, 401);
        cqRotate(c, st, 401, 400, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 650});
        stepVol(c, st, 401, "rotate");
        atStep(st, 402);
        cqUnion(c, st, 402, 396, {"toUnion": {"wp": 401}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 402, "union");
        atStep(st, 403);
        cqWorkplaneNew(c, st, 403, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 404);
        cqPolyline(c, st, 404, 403, {"listOfXYTuple": [[5.111111111111111, 12.222222222222223], [5.611111111111111, 12.222222222222223], [5.537037037037037, 12.407407407407408], [5.037037037037037, 12.407407407407408]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 405);
        cqClose(c, st, 405, 404, {});
        atStep(st, 406);
        cqRevolve(c, st, 406, 405, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 406, "revolve");
        atStep(st, 407);
        cqRotate(c, st, 407, 406, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 660});
        stepVol(c, st, 407, "rotate");
        atStep(st, 408);
        cqUnion(c, st, 408, 402, {"toUnion": {"wp": 407}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 408, "union");
        atStep(st, 409);
        cqWorkplaneNew(c, st, 409, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 410);
        cqPolyline(c, st, 410, 409, {"listOfXYTuple": [[5.037037037037037, 12.407407407407408], [5.537037037037037, 12.407407407407408], [5.462962962962962, 12.592592592592592], [4.962962962962962, 12.592592592592592]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 411);
        cqClose(c, st, 411, 410, {});
        atStep(st, 412);
        cqRevolve(c, st, 412, 411, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 412, "revolve");
        atStep(st, 413);
        cqRotate(c, st, 413, 412, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 670});
        stepVol(c, st, 413, "rotate");
        atStep(st, 414);
        cqUnion(c, st, 414, 408, {"toUnion": {"wp": 413}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 414, "union");
        atStep(st, 415);
        cqWorkplaneNew(c, st, 415, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 416);
        cqPolyline(c, st, 416, 415, {"listOfXYTuple": [[4.962962962962962, 12.592592592592592], [5.462962962962962, 12.592592592592592], [5.388888888888889, 12.777777777777777], [4.888888888888889, 12.777777777777777]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 417);
        cqClose(c, st, 417, 416, {});
        atStep(st, 418);
        cqRevolve(c, st, 418, 417, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 418, "revolve");
        atStep(st, 419);
        cqRotate(c, st, 419, 418, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 680});
        stepVol(c, st, 419, "rotate");
        atStep(st, 420);
        cqUnion(c, st, 420, 414, {"toUnion": {"wp": 419}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 420, "union");
        atStep(st, 421);
        cqWorkplaneNew(c, st, 421, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 422);
        cqPolyline(c, st, 422, 421, {"listOfXYTuple": [[4.888888888888889, 12.777777777777777], [5.388888888888889, 12.777777777777777], [5.314814814814815, 12.962962962962962], [4.814814814814815, 12.962962962962962]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 423);
        cqClose(c, st, 423, 422, {});
        atStep(st, 424);
        cqRevolve(c, st, 424, 423, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 424, "revolve");
        atStep(st, 425);
        cqRotate(c, st, 425, 424, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 690});
        stepVol(c, st, 425, "rotate");
        atStep(st, 426);
        cqUnion(c, st, 426, 420, {"toUnion": {"wp": 425}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 426, "union");
        atStep(st, 427);
        cqWorkplaneNew(c, st, 427, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 428);
        cqPolyline(c, st, 428, 427, {"listOfXYTuple": [[4.814814814814815, 12.962962962962962], [5.314814814814815, 12.962962962962962], [5.2407407407407405, 13.148148148148149], [4.7407407407407405, 13.148148148148149]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 429);
        cqClose(c, st, 429, 428, {});
        atStep(st, 430);
        cqRevolve(c, st, 430, 429, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 430, "revolve");
        atStep(st, 431);
        cqRotate(c, st, 431, 430, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 700});
        stepVol(c, st, 431, "rotate");
        atStep(st, 432);
        cqUnion(c, st, 432, 426, {"toUnion": {"wp": 431}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        atStep(st, 433);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 434);
        cqPolyline(c, st, 434, 433, {"listOfXYTuple": [[4.7407407407407405, 13.148148148148149], [5.2407407407407405, 13.148148148148149], [5.166666666666667, 13.333333333333332], [4.666666666666667, 13.333333333333332]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 435);
        cqClose(c, st, 435, 434, {});
        atStep(st, 436);
        cqRevolve(c, st, 436, 435, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 436, "revolve");
        atStep(st, 437);
        cqRotate(c, st, 437, 436, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 710});
        stepVol(c, st, 437, "rotate");
        atStep(st, 438);
        cqUnion(c, st, 438, 432, {"toUnion": {"wp": 437}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 438, "union");
        atStep(st, 439);
        cqWorkplaneNew(c, st, 439, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 440);
        cqPolyline(c, st, 440, 439, {"listOfXYTuple": [[4.666666666666667, 13.333333333333332], [5.166666666666667, 13.333333333333332], [5.092592592592592, 13.51851851851852], [4.592592592592592, 13.51851851851852]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 441);
        cqClose(c, st, 441, 440, {});
        atStep(st, 442);
        cqRevolve(c, st, 442, 441, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 442, "revolve");
        atStep(st, 443);
        cqRotate(c, st, 443, 442, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 720});
        stepVol(c, st, 443, "rotate");
        atStep(st, 444);
        cqUnion(c, st, 444, 438, {"toUnion": {"wp": 443}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 444, "union");
        atStep(st, 445);
        cqWorkplaneNew(c, st, 445, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 446);
        cqPolyline(c, st, 446, 445, {"listOfXYTuple": [[4.592592592592592, 13.51851851851852], [5.092592592592592, 13.51851851851852], [5.018518518518518, 13.703703703703704], [4.518518518518518, 13.703703703703704]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 447);
        cqClose(c, st, 447, 446, {});
        atStep(st, 448);
        cqRevolve(c, st, 448, 447, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 448, "revolve");
        atStep(st, 449);
        cqRotate(c, st, 449, 448, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 730});
        stepVol(c, st, 449, "rotate");
        atStep(st, 450);
        cqUnion(c, st, 450, 444, {"toUnion": {"wp": 449}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 450, "union");
        atStep(st, 451);
        cqWorkplaneNew(c, st, 451, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 452);
        cqPolyline(c, st, 452, 451, {"listOfXYTuple": [[4.518518518518518, 13.703703703703704], [5.018518518518518, 13.703703703703704], [4.944444444444444, 13.88888888888889], [4.444444444444444, 13.88888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 453);
        cqClose(c, st, 453, 452, {});
        atStep(st, 454);
        cqRevolve(c, st, 454, 453, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 454, "revolve");
        atStep(st, 455);
        cqRotate(c, st, 455, 454, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 740});
        stepVol(c, st, 455, "rotate");
        atStep(st, 456);
        cqUnion(c, st, 456, 450, {"toUnion": {"wp": 455}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 456, "union");
        atStep(st, 457);
        cqWorkplaneNew(c, st, 457, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 458);
        cqPolyline(c, st, 458, 457, {"listOfXYTuple": [[4.444444444444444, 13.88888888888889], [4.944444444444444, 13.88888888888889], [4.87037037037037, 14.074074074074074], [4.37037037037037, 14.074074074074074]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 459);
        cqClose(c, st, 459, 458, {});
        atStep(st, 460);
        cqRevolve(c, st, 460, 459, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 460, "revolve");
        atStep(st, 461);
        cqRotate(c, st, 461, 460, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 750});
        stepVol(c, st, 461, "rotate");
        atStep(st, 462);
        cqUnion(c, st, 462, 456, {"toUnion": {"wp": 461}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 462, "union");
        atStep(st, 463);
        cqWorkplaneNew(c, st, 463, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 464);
        cqPolyline(c, st, 464, 463, {"listOfXYTuple": [[4.37037037037037, 14.074074074074074], [4.87037037037037, 14.074074074074074], [4.796296296296297, 14.259259259259258], [4.296296296296297, 14.259259259259258]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 465);
        cqClose(c, st, 465, 464, {});
        atStep(st, 466);
        cqRevolve(c, st, 466, 465, {"angleDegrees": 9.999999999999886, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 466, "revolve");
        atStep(st, 467);
        cqRotate(c, st, 467, 466, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 760});
        stepVol(c, st, 467, "rotate");
        atStep(st, 468);
        cqUnion(c, st, 468, 462, {"toUnion": {"wp": 467}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 468, "union");
        atStep(st, 469);
        cqWorkplaneNew(c, st, 469, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 470);
        cqPolyline(c, st, 470, 469, {"listOfXYTuple": [[4.296296296296297, 14.259259259259258], [4.796296296296297, 14.259259259259258], [4.722222222222221, 14.444444444444445], [4.222222222222221, 14.444444444444445]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 471);
        cqClose(c, st, 471, 470, {});
        atStep(st, 472);
        cqRevolve(c, st, 472, 471, {"angleDegrees": 10.000000000000114, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 472, "revolve");
        atStep(st, 473);
        cqRotate(c, st, 473, 472, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 769.9999999999999});
        stepVol(c, st, 473, "rotate");
        atStep(st, 474);
        cqUnion(c, st, 474, 468, {"toUnion": {"wp": 473}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 474, "union");
        atStep(st, 475);
        cqWorkplaneNew(c, st, 475, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 476);
        cqPolyline(c, st, 476, 475, {"listOfXYTuple": [[4.222222222222221, 14.444444444444445], [4.722222222222221, 14.444444444444445], [4.648148148148148, 14.62962962962963], [4.148148148148148, 14.62962962962963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 477);
        cqClose(c, st, 477, 476, {});
        atStep(st, 478);
        cqRevolve(c, st, 478, 477, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 478, "revolve");
        atStep(st, 479);
        cqRotate(c, st, 479, 478, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 780});
        stepVol(c, st, 479, "rotate");
        atStep(st, 480);
        cqUnion(c, st, 480, 474, {"toUnion": {"wp": 479}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 480, "union");
        atStep(st, 481);
        cqWorkplaneNew(c, st, 481, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 482);
        cqPolyline(c, st, 482, 481, {"listOfXYTuple": [[4.148148148148148, 14.62962962962963], [4.648148148148148, 14.62962962962963], [4.574074074074074, 14.814814814814811], [4.074074074074074, 14.814814814814811]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 483);
        cqClose(c, st, 483, 482, {});
        atStep(st, 484);
        cqRevolve(c, st, 484, 483, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 484, "revolve");
        atStep(st, 485);
        cqRotate(c, st, 485, 484, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 790});
        stepVol(c, st, 485, "rotate");
        atStep(st, 486);
        cqUnion(c, st, 486, 480, {"toUnion": {"wp": 485}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 486, "union");
        atStep(st, 487);
        cqWorkplaneNew(c, st, 487, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 488);
        cqPolyline(c, st, 488, 487, {"listOfXYTuple": [[4.074074074074074, 14.814814814814811], [4.574074074074074, 14.814814814814811], [4.5, 15], [4, 15]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 489);
        cqClose(c, st, 489, 488, {});
        atStep(st, 490);
        cqRevolve(c, st, 490, 489, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 490, "revolve");
        atStep(st, 491);
        cqRotate(c, st, 491, 490, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 800});
        stepVol(c, st, 491, "rotate");
        atStep(st, 492);
        cqUnion(c, st, 492, 486, {"toUnion": {"wp": 491}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 492, "union");
        atStep(st, 493);
        cqWorkplaneNew(c, st, 493, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 494);
        cqPolyline(c, st, 494, 493, {"listOfXYTuple": [[4, 15], [4.5, 15], [4.425925925925925, 15.185185185185189], [3.9259259259259247, 15.185185185185189]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 495);
        cqClose(c, st, 495, 494, {});
        atStep(st, 496);
        cqRevolve(c, st, 496, 495, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 496, "revolve");
        atStep(st, 497);
        cqRotate(c, st, 497, 496, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 810});
        stepVol(c, st, 497, "rotate");
        atStep(st, 498);
        cqUnion(c, st, 498, 492, {"toUnion": {"wp": 497}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 498, "union");
        atStep(st, 499);
        cqWorkplaneNew(c, st, 499, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 500);
        cqPolyline(c, st, 500, 499, {"listOfXYTuple": [[3.9259259259259247, 15.185185185185189], [4.425925925925925, 15.185185185185189], [4.351851851851851, 15.37037037037037], [3.851851851851851, 15.37037037037037]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 501);
        cqClose(c, st, 501, 500, {});
        atStep(st, 502);
        cqRevolve(c, st, 502, 501, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 502, "revolve");
        atStep(st, 503);
        cqRotate(c, st, 503, 502, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 820});
        stepVol(c, st, 503, "rotate");
        atStep(st, 504);
        cqUnion(c, st, 504, 498, {"toUnion": {"wp": 503}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 504, "union");
        atStep(st, 505);
        cqWorkplaneNew(c, st, 505, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 506);
        cqPolyline(c, st, 506, 505, {"listOfXYTuple": [[3.851851851851851, 15.37037037037037], [4.351851851851851, 15.37037037037037], [4.277777777777778, 15.555555555555555], [3.7777777777777777, 15.555555555555555]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 507);
        cqClose(c, st, 507, 506, {});
        atStep(st, 508);
        cqRevolve(c, st, 508, 507, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 508, "revolve");
        atStep(st, 509);
        cqRotate(c, st, 509, 508, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 830});
        stepVol(c, st, 509, "rotate");
        atStep(st, 510);
        cqUnion(c, st, 510, 504, {"toUnion": {"wp": 509}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 510, "union");
        atStep(st, 511);
        cqWorkplaneNew(c, st, 511, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 512);
        cqPolyline(c, st, 512, 511, {"listOfXYTuple": [[3.7777777777777777, 15.555555555555555], [4.277777777777778, 15.555555555555555], [4.203703703703702, 15.740740740740742], [3.7037037037037024, 15.740740740740742]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 513);
        cqClose(c, st, 513, 512, {});
        atStep(st, 514);
        cqRevolve(c, st, 514, 513, {"angleDegrees": 10.000000000000114, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 514, "revolve");
        atStep(st, 515);
        cqRotate(c, st, 515, 514, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 840});
        stepVol(c, st, 515, "rotate");
        atStep(st, 516);
        cqUnion(c, st, 516, 510, {"toUnion": {"wp": 515}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 516, "union");
        atStep(st, 517);
        cqWorkplaneNew(c, st, 517, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 518);
        cqPolyline(c, st, 518, 517, {"listOfXYTuple": [[3.7037037037037024, 15.740740740740742], [4.203703703703702, 15.740740740740742], [4.12962962962963, 15.925925925925926], [3.6296296296296298, 15.925925925925926]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 519);
        cqClose(c, st, 519, 518, {});
        atStep(st, 520);
        cqRevolve(c, st, 520, 519, {"angleDegrees": 9.999999999999886, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 520, "revolve");
        atStep(st, 521);
        cqRotate(c, st, 521, 520, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 850.0000000000001});
        stepVol(c, st, 521, "rotate");
        atStep(st, 522);
        cqUnion(c, st, 522, 516, {"toUnion": {"wp": 521}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 522, "union");
        atStep(st, 523);
        cqWorkplaneNew(c, st, 523, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 524);
        cqPolyline(c, st, 524, 523, {"listOfXYTuple": [[3.6296296296296298, 15.925925925925926], [4.12962962962963, 15.925925925925926], [4.055555555555555, 16.11111111111111], [3.5555555555555554, 16.11111111111111]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 525);
        cqClose(c, st, 525, 524, {});
        atStep(st, 526);
        cqRevolve(c, st, 526, 525, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 526, "revolve");
        atStep(st, 527);
        cqRotate(c, st, 527, 526, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 860});
        stepVol(c, st, 527, "rotate");
        atStep(st, 528);
        cqUnion(c, st, 528, 522, {"toUnion": {"wp": 527}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 528, "union");
        atStep(st, 529);
        cqWorkplaneNew(c, st, 529, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 530);
        cqPolyline(c, st, 530, 529, {"listOfXYTuple": [[3.5555555555555554, 16.11111111111111], [4.055555555555555, 16.11111111111111], [3.981481481481482, 16.296296296296294], [3.481481481481482, 16.296296296296294]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 531);
        cqClose(c, st, 531, 530, {});
        atStep(st, 532);
        cqRevolve(c, st, 532, 531, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 532, "revolve");
        atStep(st, 533);
        cqRotate(c, st, 533, 532, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 870});
        stepVol(c, st, 533, "rotate");
        atStep(st, 534);
        cqUnion(c, st, 534, 528, {"toUnion": {"wp": 533}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 534, "union");
        atStep(st, 535);
        cqWorkplaneNew(c, st, 535, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 536);
        cqPolyline(c, st, 536, 535, {"listOfXYTuple": [[3.481481481481482, 16.296296296296294], [3.981481481481482, 16.296296296296294], [3.907407407407407, 16.48148148148148], [3.407407407407407, 16.48148148148148]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 537);
        cqClose(c, st, 537, 536, {});
        atStep(st, 538);
        cqRevolve(c, st, 538, 537, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 538, "revolve");
        atStep(st, 539);
        cqRotate(c, st, 539, 538, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 880});
        stepVol(c, st, 539, "rotate");
        atStep(st, 540);
        cqUnion(c, st, 540, 534, {"toUnion": {"wp": 539}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 540, "union");
        atStep(st, 541);
        cqWorkplaneNew(c, st, 541, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 542);
        cqPolyline(c, st, 542, 541, {"listOfXYTuple": [[3.407407407407407, 16.48148148148148], [3.907407407407407, 16.48148148148148], [3.833333333333332, 16.666666666666668], [3.333333333333332, 16.666666666666668]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 543);
        cqClose(c, st, 543, 542, {});
        atStep(st, 544);
        cqRevolve(c, st, 544, 543, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 544, "revolve");
        atStep(st, 545);
        cqRotate(c, st, 545, 544, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 890});
        stepVol(c, st, 545, "rotate");
        atStep(st, 546);
        cqUnion(c, st, 546, 540, {"toUnion": {"wp": 545}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 546, "union");
        atStep(st, 547);
        cqWorkplaneNew(c, st, 547, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 548);
        cqPolyline(c, st, 548, 547, {"listOfXYTuple": [[3.333333333333332, 16.666666666666668], [3.833333333333332, 16.666666666666668], [3.759259259259259, 16.85185185185185], [3.2592592592592595, 16.85185185185185]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 549);
        cqClose(c, st, 549, 548, {});
        atStep(st, 550);
        cqRevolve(c, st, 550, 549, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 550, "revolve");
        atStep(st, 551);
        cqRotate(c, st, 551, 550, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 900});
        stepVol(c, st, 551, "rotate");
        atStep(st, 552);
        cqUnion(c, st, 552, 546, {"toUnion": {"wp": 551}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 552, "union");
        atStep(st, 553);
        cqWorkplaneNew(c, st, 553, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 554);
        cqPolyline(c, st, 554, 553, {"listOfXYTuple": [[3.2592592592592595, 16.85185185185185], [3.759259259259259, 16.85185185185185], [3.685185185185184, 17.037037037037038], [3.185185185185184, 17.037037037037038]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 555);
        cqClose(c, st, 555, 554, {});
        atStep(st, 556);
        cqRevolve(c, st, 556, 555, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 556, "revolve");
        atStep(st, 557);
        cqRotate(c, st, 557, 556, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 910});
        stepVol(c, st, 557, "rotate");
        atStep(st, 558);
        cqUnion(c, st, 558, 552, {"toUnion": {"wp": 557}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 558, "union");
        atStep(st, 559);
        cqWorkplaneNew(c, st, 559, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 560);
        cqPolyline(c, st, 560, 559, {"listOfXYTuple": [[3.185185185185184, 17.037037037037038], [3.685185185185184, 17.037037037037038], [3.6111111111111103, 17.22222222222222], [3.1111111111111107, 17.22222222222222]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 561);
        cqClose(c, st, 561, 560, {});
        atStep(st, 562);
        cqRevolve(c, st, 562, 561, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 562, "revolve");
        atStep(st, 563);
        cqRotate(c, st, 563, 562, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 920});
        stepVol(c, st, 563, "rotate");
        atStep(st, 564);
        cqUnion(c, st, 564, 558, {"toUnion": {"wp": 563}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 564, "union");
        atStep(st, 565);
        cqWorkplaneNew(c, st, 565, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 566);
        cqPolyline(c, st, 566, 565, {"listOfXYTuple": [[3.1111111111111107, 17.22222222222222], [3.6111111111111103, 17.22222222222222], [3.5370370370370363, 17.40740740740741], [3.0370370370370363, 17.40740740740741]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 567);
        cqClose(c, st, 567, 566, {});
        atStep(st, 568);
        cqRevolve(c, st, 568, 567, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 568, "revolve");
        atStep(st, 569);
        cqRotate(c, st, 569, 568, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 930});
        stepVol(c, st, 569, "rotate");
        atStep(st, 570);
        cqUnion(c, st, 570, 564, {"toUnion": {"wp": 569}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 570, "union");
        atStep(st, 571);
        cqWorkplaneNew(c, st, 571, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 572);
        cqPolyline(c, st, 572, 571, {"listOfXYTuple": [[3.0370370370370363, 17.40740740740741], [3.5370370370370363, 17.40740740740741], [3.462962962962963, 17.59259259259259], [2.962962962962963, 17.59259259259259]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 573);
        cqClose(c, st, 573, 572, {});
        atStep(st, 574);
        cqRevolve(c, st, 574, 573, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 574, "revolve");
        atStep(st, 575);
        cqRotate(c, st, 575, 574, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 940});
        stepVol(c, st, 575, "rotate");
        atStep(st, 576);
        cqUnion(c, st, 576, 570, {"toUnion": {"wp": 575}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 576, "union");
        atStep(st, 577);
        cqWorkplaneNew(c, st, 577, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 578);
        cqPolyline(c, st, 578, 577, {"listOfXYTuple": [[2.962962962962963, 17.59259259259259], [3.462962962962963, 17.59259259259259], [3.3888888888888884, 17.77777777777778], [2.8888888888888884, 17.77777777777778]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 579);
        cqClose(c, st, 579, 578, {});
        atStep(st, 580);
        cqRevolve(c, st, 580, 579, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 580, "revolve");
        atStep(st, 581);
        cqRotate(c, st, 581, 580, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 950});
        stepVol(c, st, 581, "rotate");
        atStep(st, 582);
        cqUnion(c, st, 582, 576, {"toUnion": {"wp": 581}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 582, "union");
        atStep(st, 583);
        cqWorkplaneNew(c, st, 583, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 584);
        cqPolyline(c, st, 584, 583, {"listOfXYTuple": [[2.8888888888888884, 17.77777777777778], [3.3888888888888884, 17.77777777777778], [3.314814814814815, 17.962962962962962], [2.814814814814815, 17.962962962962962]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 585);
        cqClose(c, st, 585, 584, {});
        atStep(st, 586);
        cqRevolve(c, st, 586, 585, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 586, "revolve");
        atStep(st, 587);
        cqRotate(c, st, 587, 586, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 960});
        stepVol(c, st, 587, "rotate");
        atStep(st, 588);
        cqUnion(c, st, 588, 582, {"toUnion": {"wp": 587}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 588, "union");
        atStep(st, 589);
        cqWorkplaneNew(c, st, 589, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 590);
        cqPolyline(c, st, 590, 589, {"listOfXYTuple": [[2.814814814814815, 17.962962962962962], [3.314814814814815, 17.962962962962962], [3.2407407407407405, 18.14814814814815], [2.7407407407407405, 18.14814814814815]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 591);
        cqClose(c, st, 591, 590, {});
        atStep(st, 592);
        cqRevolve(c, st, 592, 591, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 592, "revolve");
        atStep(st, 593);
        cqRotate(c, st, 593, 592, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 970});
        stepVol(c, st, 593, "rotate");
        atStep(st, 594);
        cqUnion(c, st, 594, 588, {"toUnion": {"wp": 593}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 594, "union");
        atStep(st, 595);
        cqWorkplaneNew(c, st, 595, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 596);
        cqPolyline(c, st, 596, 595, {"listOfXYTuple": [[2.7407407407407405, 18.14814814814815], [3.2407407407407405, 18.14814814814815], [3.166666666666667, 18.333333333333332], [2.666666666666667, 18.333333333333332]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 597);
        cqClose(c, st, 597, 596, {});
        atStep(st, 598);
        cqRevolve(c, st, 598, 597, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 598, "revolve");
        atStep(st, 599);
        cqRotate(c, st, 599, 598, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 980});
        stepVol(c, st, 599, "rotate");
        atStep(st, 600);
        cqUnion(c, st, 600, 594, {"toUnion": {"wp": 599}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 600, "union");
        atStep(st, 601);
        cqWorkplaneNew(c, st, 601, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 602);
        cqPolyline(c, st, 602, 601, {"listOfXYTuple": [[2.666666666666667, 18.333333333333332], [3.166666666666667, 18.333333333333332], [3.0925925925925917, 18.51851851851852], [2.5925925925925917, 18.51851851851852]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 603);
        cqClose(c, st, 603, 602, {});
        atStep(st, 604);
        cqRevolve(c, st, 604, 603, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 604, "revolve");
        atStep(st, 605);
        cqRotate(c, st, 605, 604, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 990});
        stepVol(c, st, 605, "rotate");
        atStep(st, 606);
        cqUnion(c, st, 606, 600, {"toUnion": {"wp": 605}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 606, "union");
        atStep(st, 607);
        cqWorkplaneNew(c, st, 607, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 608);
        cqPolyline(c, st, 608, 607, {"listOfXYTuple": [[2.5925925925925917, 18.51851851851852], [3.0925925925925917, 18.51851851851852], [3.0185185185185173, 18.703703703703702], [2.5185185185185173, 18.703703703703702]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 609);
        cqClose(c, st, 609, 608, {});
        atStep(st, 610);
        cqRevolve(c, st, 610, 609, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 610, "revolve");
        atStep(st, 611);
        cqRotate(c, st, 611, 610, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1000});
        stepVol(c, st, 611, "rotate");
        atStep(st, 612);
        cqUnion(c, st, 612, 606, {"toUnion": {"wp": 611}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 612, "union");
        atStep(st, 613);
        cqWorkplaneNew(c, st, 613, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 614);
        cqPolyline(c, st, 614, 613, {"listOfXYTuple": [[2.5185185185185173, 18.703703703703702], [3.0185185185185173, 18.703703703703702], [2.944444444444444, 18.88888888888889], [2.444444444444444, 18.88888888888889]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 615);
        cqClose(c, st, 615, 614, {});
        atStep(st, 616);
        cqRevolve(c, st, 616, 615, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 616, "revolve");
        atStep(st, 617);
        cqRotate(c, st, 617, 616, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1010});
        stepVol(c, st, 617, "rotate");
        atStep(st, 618);
        cqUnion(c, st, 618, 612, {"toUnion": {"wp": 617}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 618, "union");
        atStep(st, 619);
        cqWorkplaneNew(c, st, 619, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 620);
        cqPolyline(c, st, 620, 619, {"listOfXYTuple": [[2.444444444444444, 18.88888888888889], [2.944444444444444, 18.88888888888889], [2.87037037037037, 19.074074074074076], [2.37037037037037, 19.074074074074076]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 621);
        cqClose(c, st, 621, 620, {});
        atStep(st, 622);
        cqRevolve(c, st, 622, 621, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 622, "revolve");
        atStep(st, 623);
        cqRotate(c, st, 623, 622, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1020});
        stepVol(c, st, 623, "rotate");
        atStep(st, 624);
        cqUnion(c, st, 624, 618, {"toUnion": {"wp": 623}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 624, "union");
        atStep(st, 625);
        cqWorkplaneNew(c, st, 625, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 626);
        cqPolyline(c, st, 626, 625, {"listOfXYTuple": [[2.37037037037037, 19.074074074074076], [2.87037037037037, 19.074074074074076], [2.796296296296296, 19.25925925925926], [2.296296296296296, 19.25925925925926]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 627);
        cqClose(c, st, 627, 626, {});
        atStep(st, 628);
        cqRevolve(c, st, 628, 627, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 628, "revolve");
        atStep(st, 629);
        cqRotate(c, st, 629, 628, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1030});
        stepVol(c, st, 629, "rotate");
        atStep(st, 630);
        cqUnion(c, st, 630, 624, {"toUnion": {"wp": 629}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 630, "union");
        atStep(st, 631);
        cqWorkplaneNew(c, st, 631, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 632);
        cqPolyline(c, st, 632, 631, {"listOfXYTuple": [[2.296296296296296, 19.25925925925926], [2.796296296296296, 19.25925925925926], [2.7222222222222223, 19.444444444444443], [2.2222222222222223, 19.444444444444443]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 633);
        cqClose(c, st, 633, 632, {});
        atStep(st, 634);
        cqRevolve(c, st, 634, 633, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 634, "revolve");
        atStep(st, 635);
        cqRotate(c, st, 635, 634, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1040});
        stepVol(c, st, 635, "rotate");
        atStep(st, 636);
        cqUnion(c, st, 636, 630, {"toUnion": {"wp": 635}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 636, "union");
        atStep(st, 637);
        cqWorkplaneNew(c, st, 637, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 638);
        cqPolyline(c, st, 638, 637, {"listOfXYTuple": [[2.2222222222222223, 19.444444444444443], [2.7222222222222223, 19.444444444444443], [2.648148148148148, 19.62962962962963], [2.148148148148148, 19.62962962962963]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 639);
        cqClose(c, st, 639, 638, {});
        atStep(st, 640);
        cqRevolve(c, st, 640, 639, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 640, "revolve");
        atStep(st, 641);
        cqRotate(c, st, 641, 640, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1050});
        stepVol(c, st, 641, "rotate");
        atStep(st, 642);
        cqUnion(c, st, 642, 636, {"toUnion": {"wp": 641}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 642, "union");
        atStep(st, 643);
        cqWorkplaneNew(c, st, 643, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 644);
        cqPolyline(c, st, 644, 643, {"listOfXYTuple": [[2.148148148148148, 19.62962962962963], [2.648148148148148, 19.62962962962963], [2.5740740740740744, 19.814814814814813], [2.0740740740740744, 19.814814814814813]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 645);
        cqClose(c, st, 645, 644, {});
        atStep(st, 646);
        cqRevolve(c, st, 646, 645, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 646, "revolve");
        atStep(st, 647);
        cqRotate(c, st, 647, 646, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1060});
        stepVol(c, st, 647, "rotate");
        atStep(st, 648);
        cqUnion(c, st, 648, 642, {"toUnion": {"wp": 647}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 648, "union");
        atStep(st, 649);
        cqWorkplaneNew(c, st, 649, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 650);
        cqPolyline(c, st, 650, 649, {"listOfXYTuple": [[2.0740740740740744, 19.814814814814813], [2.5740740740740744, 19.814814814814813], [2.5, 20], [2, 20]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 651);
        cqClose(c, st, 651, 650, {});
        atStep(st, 652);
        cqRevolve(c, st, 652, 651, {"angleDegrees": 10, "axisStart": [0, 0, 0], "axisEnd": [0, 1, 0], "combine": true, "clean": true});
        stepVol(c, st, 652, "revolve");
        atStep(st, 653);
        cqRotate(c, st, 653, 652, {"axisStartPoint": [0, 0, 0], "axisEndPoint": [0, 0, 1], "angleDegrees": 1070});
        stepVol(c, st, 653, "rotate");
        atStep(st, 654);
        cqUnion(c, st, 654, 648, {"toUnion": {"wp": 653}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 654, "union");
        return mergeMaps(cqMeasure(c, st, {"wp": 654}), {"i": 866});
    } catch (e) {
        return {"i": 866, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_866(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c866"] = r; }
    return out;
}
