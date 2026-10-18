// Промежуточные кадры поворота по 3D-скелету (v2): копия ближайшего ключа, затем каждая часть
// подгоняется по суставам: нога — поворот+растяжение по оси (верх у плеча/бедра, низ у копыта),
// голова/шея/торс/крылья — сдвиг за суставом, грива/хвост — сдвиг за корнем и сжатие от корня.
// JOBD = { src, dst, sym, start, keys: [[кадр, угол]...], from, to, render }
function kindOf(layer, sym) {
    var L = layer.toUpperCase(), s = sym.toUpperCase(), far = L.indexOf("FAR") >= 0 || s.indexOf("FAR") >= 0;
    if (s.indexOf("WING") >= 0) return "wing";
    if (s.indexOf("TAIL") >= 0) return "tail";
    if (s.indexOf("MANE") >= 0 || s.indexOf("HAIR") >= 0) return "mane";
    if (s.indexOf("HEAD") >= 0) return "head";
    if (s.indexOf("NECK") >= 0 || s.indexOf("COLLAR") >= 0) return "neck";
    if (s.indexOf("FRONTLEG") >= 0) return far ? "fl_far" : "fl_near";
    if (s.indexOf("HINDLEG") >= 0) return far ? "hl_far" : "hl_near";
    if (s.indexOf("SHADOW") >= 0) return "shadow";
    return "torso";
}
function J(M, n) { return M.J[n]; }
function sideOf(M, base) {  // "Left"/"Right" ближней ноги по глубине
    var a = J(M, "Left" + base), b = J(M, "Right" + base);
    return a[2] <= b[2] ? ["Left", "Right"] : ["Right", "Left"];
}
// Сторона ноги (Left/Right) решается один раз — на угле ключа Mk — и держится в кадре Ma.
// Слой без near/far (вид спереди/сзади) — по тому, с какой стороны тела нога на картинке.
function legSide(Mk, kind, layerName, elCx, bodyCx) {
    var front = kind.charAt(0) == "f", top = front ? "Shoulder" : "Thigh";
    var L = layerName.toUpperCase();
    if (L.indexOf("NEAR") < 0 && L.indexOf("FAR") < 0) {
        var a = Mk.J["Left" + top], b = Mk.J["Right" + top];
        var leftIsL = a[0] <= b[0];
        return (elCx < bodyCx) == leftIsL ? "Left" : "Right";
    }
    var sd = sideOf(Mk, top);
    return kind.indexOf("near") >= 0 ? sd[0] : sd[1];
}
function legJoints(M, kind, side) {
    var front = kind.charAt(0) == "f", top = front ? "Shoulder" : "Thigh", bot = front ? "Ball" : "Foot";
    return [J(M, side + top), J(M, side + bot)];
}
function avg(a, b) { return [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2]; }
function anchorOf(M, kind) {
    if (kind == "head") return J(M, "Head");
    if (kind == "neck") return J(M, "Neck");
    if (kind == "mane") return J(M, "hair_jiggle1");
    if (kind == "tail") return J(M, "tail_jiggle1");
    return avg(J(M, "Chest2"), J(M, "Pelvis"));
}
function width3d(M, kind) { var c = { mane: "mane", tail: "tail", torso: "torso", head: "head" }[kind]; return c && M[c] ? M[c].x1 - M[c].x0 : 0; }
function height3d(M) { var t = 1e9, b = -1e9; for (var k in M) if (k != "J") { t = Math.min(t, M[k].y0); b = Math.max(b, M[k].y1); } return b - t; }
function mk(a) { return MARKS[a.toFixed(3)]; }
function setAffine(el, T, pk, pa) {   // x' = T (x - pk) + pa для матрицы элемента
    var m = el.matrix, A = [[m.a, m.c], [m.b, m.d]], t = [m.tx - pk[0], m.ty - pk[1]];
    m.a = T[0][0] * A[0][0] + T[0][1] * A[1][0]; m.c = T[0][0] * A[0][1] + T[0][1] * A[1][1];
    m.b = T[1][0] * A[0][0] + T[1][1] * A[1][0]; m.d = T[1][0] * A[0][1] + T[1][1] * A[1][1];
    m.tx = T[0][0] * t[0] + T[0][1] * t[1] + pa[0]; m.ty = T[1][0] * t[0] + T[1][1] * t[1] + pa[1];
    el.matrix = m;
}
function legT(vk, va) {  // поворот vk→va + растяжение вдоль оси, поперёк — как было
    var lk = Math.sqrt(vk[0] * vk[0] + vk[1] * vk[1]), la = Math.sqrt(va[0] * va[0] + va[1] * va[1]);
    var uk = [vk[0] / lk, vk[1] / lk], ua = [va[0] / la, va[1] / la], k = Math.max(0.8, Math.min(1.25, la / lk));
    // T = Ra * diag(k,1) * Rk^T, R = [u, n], n = (-u.y, u.x)
    var Rk = [[uk[0], -uk[1]], [uk[1], uk[0]]], Ra = [[ua[0], -ua[1]], [ua[1], ua[0]]];
    var D = [[k * Rk[0][0], k * Rk[1][0]], [Rk[0][1], Rk[1][1]]];  // diag * Rk^T
    return [[Ra[0][0] * D[0][0] + Ra[0][1] * D[1][0], Ra[0][0] * D[0][1] + Ra[0][1] * D[1][1]],
            [Ra[1][0] * D[0][0] + Ra[1][1] * D[1][0], Ra[1][0] * D[0][1] + Ra[1][1] * D[1][1]]];
}

// Ключ группы части: вид + сторона для ног (сторона — по 3D на угле своего ключа).
function groupKey(M, layerName, el, bodyCx) {
    var kind = kindOf(layerName, el.libraryItem.name.split("/").pop());
    if (kind.indexOf("l_") == 1) return kind.charAt(0) + "l_" + legSide(M, kind, layerName, el.left + el.width / 2, bodyCx);
    if (kind == "neck") return "neck";
    return kind;
}
function shadowCx(tl, f) {
    var tC = frameBounds(tl, f), cx = tC.left + tC.width / 2;
    for (var li = 0; li < tl.layers.length; li++) { var fr = tl.layers[li].frames[f]; if (!fr || tl.layers[li].layerType == "guide") continue;
        for (var e = 0; e < fr.elements.length; e++) { var x = fr.elements[e]; if (x.elementType == "instance" && x.libraryItem.name.toUpperCase().indexOf("SHADOW") >= 0) cx = x.left + x.width / 2; } }
    return cx;
}
// Рамки групп частей кадра f: {ключ: {l,t,r,b, els:[...]}}
function groups(tl, f, M) {
    var g = {}, bc = shadowCx(tl, f);
    for (var li = 0; li < tl.layers.length; li++) {
        var L = tl.layers[li]; if (L.layerType == "folder" || L.layerType == "guide" || !L.frames[f]) continue;
        var els = L.frames[f].elements;
        for (var e = 0; e < els.length; e++) {
            var el = els[e]; if (el.elementType != "instance") continue;
            var k = groupKey(M, L.name, el, bc), G = g[k] || (g[k] = { l: 1e9, t: 1e9, r: -1e9, b: -1e9, els: [] });
            G.l = Math.min(G.l, el.left); G.t = Math.min(G.t, el.top); G.r = Math.max(G.r, el.left + el.width); G.b = Math.max(G.b, el.top + el.height);
            G.els.push(el); (G.syms || (G.syms = {}))[el.libraryItem.name + ":" + el.firstFrame] = 1;  // рисунок = символ + внутренний кадр
        }
    }
    return g;
}
// Подтянуть группы кадра F к группам соседнего ключа of (сдвинутым по 3D к углу a) с весом u (0..0.5).
function blendToward(tl, F, Ma, of, Mo, u, s) {
    if (u <= 0) return 0;
    var gF = groups(tl, F, Ma), gO = groups(tl, of, Mo), n = 0;
    for (var k in gF) {
        var A = gF[k], B = gO[k]; if (!B || k == "shadow") continue;
        // грива из разных кусков (бок ↔ спина) — рамки несравнимы, не тянем
        if (k == "mane") { var same = true; for (var q1 in A.syms) if (!B.syms[q1]) same = false; for (var q2 in B.syms) if (!A.syms[q2]) same = false; if (!same) continue; }
        var kind = k.indexOf("l_") == 1 ? k.substring(0, 2) + "_near" : k;
        var d3 = kind.indexOf("l_") == 1 ? [0, 0] : (function () { var p = anchorOf(Mo, k), q = anchorOf(Ma, k); return [(q[0] - p[0]) * s, -(q[1] - p[1]) * s]; })();
        var acx = (A.l + A.r) / 2, acy = (A.t + A.b) / 2, aw = A.r - A.l, ah = A.b - A.t;
        var bcx = (B.l + B.r) / 2 + d3[0], bcy = (B.t + B.b) / 2 + d3[1], bw = B.r - B.l, bh = B.b - B.t;
        var sx = 1 + u * (bw / aw - 1), sy = 1 + u * (bh / ah - 1);
        sx = Math.max(0.8, Math.min(1.25, sx)); sy = Math.max(0.85, Math.min(1.15, sy));
        var tx = acx + u * (bcx - acx), ty = acy + u * (bcy - acy);
        // ноги: низ (копыто) остаётся на земле — опора масштаба по низу
        var ay = kind.indexOf("l_") == 1 ? A.b : acy, ty2 = kind.indexOf("l_") == 1 ? A.b : ty;
        for (var e = 0; e < A.els.length; e++) setAffine(A.els[e], [[sx, 0], [0, sy]], [acx, ay], [tx, ty2]);
        n++;
    }
    return n;
}

var doc = openCopy(JOBD.src, JOBD.dst);
doc.library.editItem(findItem(doc, JOBD.sym).name);
var tl = doc.getTimeline(), out = [];
for (var i = JOBD.from; i <= JOBD.to; i++) {
    var a = i * 5.625, best = null;
    for (var q = 0; q < JOBD.keys.length; q++) { var dd = Math.abs(JOBD.keys[q][1] - a); if (!best || dd < best.d - 1e-6) best = { d: dd, k: JOBD.keys[q] }; }
    if (best.d < 1e-6) continue;
    var kf = best.k[0], ka = best.k[1], F = JOBD.start + i;
    for (var li = 0; li < tl.layers.length; li++) {
        var L = tl.layers[li]; if (L.layerType == "folder" || !L.frames[F] || L.name == "ANGLE") continue;  // подпись угла — своя у кадра
        var lk = L.locked; L.locked = false; tl.setSelectedLayers(li); tl.currentLayer = li;
        if (L.frames[F].startFrame != F) tl.convertToBlankKeyframes(F);
        if (L.frames[F + 1] && L.frames[F + 1].startFrame != F + 1) tl.convertToBlankKeyframes(F + 1);
        tl.copyFrames(kf, kf); tl.pasteFrames(F, F);
        L.locked = lk;
    }
    var Mk = mk(ka), Ma = mk(a), s = frameBounds(tl, kf).height / height3d(Mk), n = 0;
    var D = function (p, q) { return [(q[0] - p[0]) * s, -(q[1] - p[1]) * s]; };  // 3D-сдвиг → пиксели DHX
    // центр тела — по тени (грива/хвост сдвигают общую рамку)
    var tC = frameBounds(tl, F), bodyCx = tC.left + tC.width / 2;
    for (var li = 0; li < tl.layers.length; li++) { var fr0 = tl.layers[li].frames[F]; if (!fr0 || tl.layers[li].layerType == "guide") continue;
        for (var e0 = 0; e0 < fr0.elements.length; e0++) { var x0 = fr0.elements[e0]; if (x0.elementType == "instance" && x0.libraryItem.name.toUpperCase().indexOf("SHADOW") >= 0) bodyCx = x0.left + x0.width / 2; } }
    for (var li = 0; li < tl.layers.length; li++) {
        var L = tl.layers[li]; if (L.layerType == "folder" || L.layerType == "guide" || !L.frames[F]) continue;
        var els = L.frames[F].elements;
        for (var e = 0; e < els.length; e++) {
            var el = els[e]; if (el.elementType != "instance") continue;
            var kind = kindOf(L.name, el.libraryItem.name.split("/").pop());
            var cx = el.left + el.width / 2, cy = el.top + el.height / 2;
            if (kind.indexOf("l_") == 1) {  // нога
                var side = legSide(Mk, kind, L.name, cx, bodyCx), jk = legJoints(Mk, kind, side), ja = legJoints(Ma, kind, side);
                var pk = [cx, el.top], qk = [cx, el.top + el.height];
                var dp = D(jk[0], ja[0]), dq = D(jk[1], ja[1]);
                var pa = [pk[0] + dp[0], pk[1] + dp[1]], qa = [qk[0] + dq[0], qk[1] + dq[1]];
                setAffine(el, legT([qk[0] - pk[0], qk[1] - pk[1]], [qa[0] - pa[0], qa[1] - pa[1]]), pk, pa);
            } else {
                var d = D(anchorOf(Mk, kind), anchorOf(Ma, kind)), sx = 1;
                if (kind == "shadow") d[1] = 0;
                var w0 = width3d(Mk, kind), w1 = width3d(Ma, kind);
                if (w0 > 0 && kind == "torso") sx = Math.max(0.8, Math.min(1.25, w1 / w0));
                // опора сжатия: торс — центр; грива/хвост — край ближе к телу
                var ax = kind == "torso" ? cx : (cx > bodyCx ? el.left : el.left + el.width);
                setAffine(el, [[sx, 0], [0, 1]], [ax, cy], [ax + d[0], cy + d[1]]);
            }
            n++;
        }
    }
    if (JOBD.blend !== false) {  // подтяжка к соседнему ключу (по другую сторону от a)
        var oth = null;
        for (var q = 0; q < JOBD.keys.length; q++) { var kq = JOBD.keys[q]; if ((kq[1] - ka) * (a - ka) > 0 && (!oth || Math.abs(kq[1] - ka) < Math.abs(oth[1] - ka))) oth = kq; }
        if (oth) { var u = Math.abs(a - ka) / Math.abs(oth[1] - ka); n += blendToward(tl, F, Ma, oth[0], mk(oth[1]), Math.min(0.5, u), s); }
    }
    if (JOBD.head) {  // голова: свой кадр JOBD.headStart + i
        var hel = null;
        for (var li = 0; li < tl.layers.length && !hel; li++) { var fr = tl.layers[li].frames[F]; if (!fr || tl.layers[li].layerType == "guide") continue;
            for (var e = 0; e < fr.elements.length; e++) if (fr.elements[e].elementType == "instance" && fr.elements[e].libraryItem.name.split("/").pop() == JOBD.head) hel = fr.elements[e]; }
        if (hel) {
            var hk = hel.firstFrame, HF = JOBD.headStart + 2 * i, hw = hel.width;
            var nh = driveHead(doc, JOBD.head, hk, HF, Mk, Ma, hw);
            doc.library.editItem(findItem(doc, JOBD.sym).name); tl = doc.getTimeline();
            for (var li = 0; li < tl.layers.length; li++) { var fr = tl.layers[li].frames[F]; if (!fr || tl.layers[li].layerType == "guide") continue;
                for (var e = 0; e < fr.elements.length; e++) if (fr.elements[e].elementType == "instance" && fr.elements[e].libraryItem.name.split("/").pop() == JOBD.head) fr.elements[e].firstFrame = HF; }
            n += nh;
        }
    }
    out.push(i + ":" + a.toFixed(1) + "<-" + ka + " n=" + n);
}
doc.exitEditMode();
saveAs(doc, JOBD.dst);
if (JOBD.render) renderSymbol(doc, JOBD.sym, JOBD.start + JOBD.from + 1, JOBD.start + JOBD.to + 1, JOBD.render);
log(out.join("; "));
